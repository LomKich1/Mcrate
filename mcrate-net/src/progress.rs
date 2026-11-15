//! Состояние идущей загрузки: общая «доска объявлений» между тем, кто качает, и тем, кто читает.
//!
//! Зачем: чтобы играть трек, пока он ещё скачивается, плееру нужно знать, сколько байт уже
//! лежит в файле, закончилась ли загрузка, не сломалась ли она и не отменил ли её пользователь.
//! Загрузчик (`StreamDownloader::download_progressive`) пишет сюда, а читатель файла
//! (в mcrate-audio) ждёт здесь новых данных.
//!
//! Аналогия из UE5: это как общая структура с флагами между потоком загрузки ассета и игровым
//! потоком, только с «звонком»: читатель спит и просыпается, когда пришло новое (Condvar).

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// Всё, что меняется во время загрузки. Лежит под замком (Mutex), чтобы два потока
/// не писали и не читали это одновременно.
#[derive(Debug, Default)]
struct State {
    /// Сколько байт уже записано в файл (и их можно читать).
    written: u64,
    /// Загрузка закончилась успешно.
    finished: bool,
    /// Загрузка оборвалась с ошибкой (текст без секретов: адреса уже обрезаны до `?`).
    error: Option<String>,
    /// Загрузку отменили (читатель должен немедленно перестать ждать).
    cancelled: bool,
    /// Доля скачанного, 0.0..=1.0, если её удалось посчитать.
    fraction: Option<f32>,
}

#[derive(Debug, Default)]
struct Inner {
    state: Mutex<State>,
    /// «Звонок»: загрузчик звонит при каждом изменении, читатель просыпается.
    changed: Condvar,
}

/// Ответ на вопрос «есть ли данные?» (см. `DownloadProgress::wait_for_bytes`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Available {
    /// Записано не меньше нужного (внутри число записанных байт).
    Data(u64),
    /// Загрузка успешно закончилась, а нужных байт нет: это конец файла.
    Eof,
    /// Загрузка оборвалась с ошибкой (внутри текст).
    Failed(String),
    /// Загрузку отменили.
    Cancelled,
    /// Время ожидания вышло, ничего нового (можно спросить ещё раз).
    Timeout,
}

/// Прогресс одной загрузки. `Clone` дешёвый: копия указывает на те же данные (Arc = общий счётчик ссылок),
/// поэтому один экземпляр отдаём загрузчику, другой читателю.
#[derive(Debug, Clone, Default)]
pub struct DownloadProgress {
    inner: Arc<Inner>,
}

impl DownloadProgress {
    pub fn new() -> Self {
        Self::default()
    }

    /// Берём замок. Если другой поток упал с паникой, держа замок (Mutex «отравлен»),
    /// данные всё равно годятся: это простые числа и флаги.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---------- что умеет читатель ----------

    /// Сколько байт уже записано.
    pub fn written(&self) -> u64 {
        self.lock().written
    }

    /// Загрузка закончилась успешно?
    pub fn is_finished(&self) -> bool {
        self.lock().finished
    }

    /// Текст ошибки, если загрузка оборвалась.
    pub fn error(&self) -> Option<String> {
        self.lock().error.clone()
    }

    /// Загрузку отменили?
    pub fn is_cancelled(&self) -> bool {
        self.lock().cancelled
    }

    /// Доля скачанного 0.0..=1.0 (None, если неизвестна). Для HLS считается по длительности
    /// кусков, для прямой ссылки по заголовку Content-Length.
    pub fn fraction(&self) -> Option<f32> {
        self.lock().fraction
    }

    /// Отменить загрузку: загрузчик остановится на ближайшем куске, читатели проснутся.
    pub fn cancel(&self) {
        self.lock().cancelled = true;
        self.inner.changed.notify_all();
    }

    /// Ждёт, пока в файле будет записано не меньше `needed` байт (но не дольше `timeout`).
    /// Читатель, которому нужен байт с номером `pos`, спрашивает `needed = pos + 1`.
    ///
    /// Порядок проверки важен: отмена главнее всего (читатель должен проснуться сразу),
    /// потом данные, потом ошибка, потом «конец файла».
    pub fn wait_for_bytes(&self, needed: u64, timeout: Duration) -> Available {
        let guard = self.lock();
        // wait_timeout_while: спать, пока условие в замыкании истинно, но не дольше timeout.
        let (state, _) = self
            .inner
            .changed
            .wait_timeout_while(guard, timeout, |s| {
                !(s.cancelled || s.written >= needed || s.error.is_some() || s.finished)
            })
            .unwrap_or_else(|e| e.into_inner());
        if state.cancelled {
            Available::Cancelled
        } else if state.written >= needed {
            Available::Data(state.written)
        } else if let Some(e) = &state.error {
            Available::Failed(e.clone())
        } else if state.finished {
            Available::Eof
        } else {
            Available::Timeout
        }
    }

    // ---------- что делает загрузчик ----------

    /// «Записал ещё n байт».
    pub fn add_bytes(&self, n: u64) {
        self.lock().written += n;
        self.inner.changed.notify_all();
    }

    /// Обновить долю скачанного (значение обрезается до 0..=1).
    pub fn set_fraction(&self, fraction: f32) {
        self.lock().fraction = Some(fraction.clamp(0.0, 1.0));
    }

    /// «Загрузка закончена успешно» (доля становится 1.0).
    pub fn finish(&self) {
        {
            let mut s = self.lock();
            s.finished = true;
            s.fraction = Some(1.0);
        }
        self.inner.changed.notify_all();
    }

    /// «Загрузка оборвалась». Если её отменили, ошибку не запоминаем: отмена и так всё объясняет.
    pub fn fail(&self, message: String) {
        {
            let mut s = self.lock();
            if !s.cancelled {
                s.error = Some(message);
            }
        }
        self.inner.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Instant;

    const SHORT: Duration = Duration::from_millis(30);
    const LONG: Duration = Duration::from_secs(5);

    #[test]
    fn data_already_there_returns_at_once() {
        let p = DownloadProgress::new();
        p.add_bytes(100);
        assert_eq!(p.wait_for_bytes(100, LONG), Available::Data(100));
        assert_eq!(p.wait_for_bytes(1, LONG), Available::Data(100));
    }

    #[test]
    fn times_out_when_nothing_comes() {
        let p = DownloadProgress::new();
        p.add_bytes(10);
        assert_eq!(p.wait_for_bytes(11, SHORT), Available::Timeout);
    }

    #[test]
    fn wakes_up_when_data_arrives() {
        let p = DownloadProgress::new();
        let writer = p.clone(); // вторая «ссылка» на те же данные
        let t = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            writer.add_bytes(500);
        });
        let started = Instant::now();
        assert_eq!(p.wait_for_bytes(1, LONG), Available::Data(500));
        assert!(started.elapsed() < Duration::from_secs(3), "проснулись не по звонку, а по таймауту");
        t.join().unwrap();
    }

    #[test]
    fn finish_means_eof_but_data_comes_first() {
        let p = DownloadProgress::new();
        p.add_bytes(10);
        p.finish();
        assert_eq!(p.wait_for_bytes(10, LONG), Available::Data(10)); // байты ещё можно дочитать
        assert_eq!(p.wait_for_bytes(11, LONG), Available::Eof); // а дальше конец
        assert!(p.is_finished());
        assert_eq!(p.fraction(), Some(1.0));
    }

    #[test]
    fn failure_is_reported_after_available_data() {
        let p = DownloadProgress::new();
        p.add_bytes(10);
        p.fail("обрыв".to_string());
        assert_eq!(p.wait_for_bytes(10, LONG), Available::Data(10));
        assert_eq!(p.wait_for_bytes(11, LONG), Available::Failed("обрыв".to_string()));
        assert_eq!(p.error().as_deref(), Some("обрыв"));
    }

    #[test]
    fn cancel_wakes_waiter_and_wins_over_data() {
        let p = DownloadProgress::new();
        let canceller = p.clone();
        let t = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            canceller.cancel();
        });
        let started = Instant::now();
        assert_eq!(p.wait_for_bytes(1, LONG), Available::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(3));
        t.join().unwrap();
        p.add_bytes(10);
        assert_eq!(p.wait_for_bytes(1, LONG), Available::Cancelled); // отмена главнее данных
        p.fail("после отмены".to_string());
        assert_eq!(p.error(), None); // после отмены ошибку не запоминаем
    }

    #[test]
    fn fraction_is_clamped() {
        let p = DownloadProgress::new();
        assert_eq!(p.fraction(), None);
        p.set_fraction(0.25);
        assert_eq!(p.fraction(), Some(0.25));
        p.set_fraction(7.0);
        assert_eq!(p.fraction(), Some(1.0));
        p.set_fraction(-1.0);
        assert_eq!(p.fraction(), Some(0.0));
    }
}

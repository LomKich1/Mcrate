//! Controller = мост между UI и ядром (mcrate-core / mcrate-audio / mcrate-providers).
//! UI не трогает провайдеры и плеер напрямую: все действия идут через этот тип.
//! Ядро (его делает LomKich) мы здесь НЕ меняем, только используем.
//!
//! Шаг 5c плана: Controller стал тонкой обёрткой над `PlaybackEngine`. Звук, очередь, смена трека,
//! «назад с порогом 3 с», перемотка и загрузка онлайн-треков живут в движке; здесь только:
//!   * команды (`play_list`, `next`, `seek_to`...) пересылаются движку;
//!   * каждый кадр (`tick`) читаем события и снимок состояния движка и копируем в поля для UI;
//!   * библиотека своих файлов (`LocalProvider`) и поиск по SoundCloud (в фоне, UI не ждёт).

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use mcrate_audio::{PlaybackEngine, PlaybackEvent, PlaybackSettings, RepeatMode, VisualizerFrame};
use mcrate_core::{MusicProvider, ProviderKind, Track};
use mcrate_providers::{LocalProvider, SoundCloudProvider};

/// Где лежит файл со списком сохранённых треков: %APPDATA%\Mcrate\library.txt
/// (APPDATA = переменная окружения Windows, обычно C:\Users\<имя>\AppData\Roaming).
/// Option, потому что переменной теоретически может не быть.
fn library_file() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("library.txt"))
}

/// Папка приложения: %APPDATA%\Mcrate (туда же ядро кладёт `soundcloud_client_id.txt`).
fn app_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("Mcrate"))
}

/// Как назвать сервис в интерфейсе («Свои файлы · в очереди: 3»).
pub fn provider_label(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Local => "Свои файлы",
        ProviderKind::SoundCloud => "SoundCloud",
        ProviderKind::Yandex => "Яндекс Музыка",
        ProviderKind::Vk => "VK Музыка",
        ProviderKind::Spotify => "Spotify",
    }
}

/// Сколько после перемотки показываем выбранную позицию, не дожидаясь снимка движка
/// (снимок обновляется раз в ~100 мс; без этого ползунок на миг прыгал бы назад).
const SEEK_HOLD: Duration = Duration::from_millis(300);

/// Сколько треков помним в истории прослушивания.
const HISTORY_MAX: usize = 100;

/// Поднять трек наверх истории (убрав прежнюю запись о нём) и обрезать список до `HISTORY_MAX`.
fn push_history(history: &mut Vec<Track>, track: Track) {
    history.retain(|t| !(t.id == track.id && t.provider == track.provider));
    history.insert(0, track);
    history.truncate(HISTORY_MAX);
}

/// Снимок очереди движка. Читается методами, как раньше читали `PlayQueue`
/// (`ctl.queue.shuffle()`, `ctl.queue.len()`), поэтому экраны менять не пришлось.
#[derive(Debug, Clone, Copy)]
pub struct QueueView {
    shuffle: bool,
    repeat: RepeatMode,
    len: usize,
    /// Номер текущего элемента в порядке воспроизведения (с нуля).
    index: usize,
}

impl QueueView {
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }
    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn index(&self) -> usize {
        self.index
    }
}

/// Состояние экрана SoundCloud: что ищем, что нашли, идёт ли поиск.
#[derive(Debug, Default)]
pub struct SoundCloudState {
    /// Запрос, по которому получены (или ищутся) результаты.
    pub query: String,
    pub results: Vec<Track>,
    /// Поиск идёт прямо сейчас (первый поиск может занять до ~30 с: добывается client_id).
    pub searching: bool,
    /// Ошибка последнего поиска (текст уже для пользователя).
    pub error: Option<String>,
    /// Был ли уже хотя бы один поиск (чтобы отличить «ничего не искали» от «ничего не нашли»).
    pub searched: bool,
}

/// Ответ фоновой задачи поиска: номер запроса (чтобы выбросить устаревшие), запрос и результат.
type SearchReply = (u64, String, Result<Vec<Track>, String>);

pub struct Controller {
    // Многопоточная среда tokio, живёт всё время работы приложения. ВАЖНО (см. «Принятые решения»
    // в CLAUDE.md): HTTP-клиент SoundCloud держит соединения в среде, где впервые ходил в сеть. Если
    // она `current_thread` и поток занят, запрос из движка зависает до таймаута. Поэтому поиск
    // ведём здесь, на многопоточной. Option нужен только ради `Drop` (см. ниже).
    rt: Option<tokio::runtime::Runtime>,
    /// Свои файлы. Arc (общий владелец): им пользуются и Controller (добавление, поиск), и движок.
    local: Arc<LocalProvider>,
    /// SoundCloud; None, если не удалось создать HTTP-клиент (причина в `sc.error`).
    soundcloud: Option<Arc<SoundCloudProvider>>,
    engine: Option<PlaybackEngine>,
    pub audio_error: Option<String>,

    pub queue: QueueView,
    pub library: Vec<Track>,
    /// Текущий трек (в том числе пока он грузится): копия `state().current` движка.
    pub now: Option<Track>,
    pub volume: f32,
    pub status: Option<String>,
    /// Последняя ошибка воспроизведения (трек не запустился, оборвалась загрузка).
    /// Стирается, когда пошёл следующий трек.
    pub play_error: Option<String>,
    /// История прослушивания за сеанс: новые сверху, без повторов (трек, сыгранный снова, поднимается наверх).
    pub history: Vec<Track>,
    pub sc: SoundCloudState,

    // --- копия снимка движка (обновляется в `tick`) ---
    playing: bool,
    loading: bool,
    position: Duration,
    download_progress: Option<f32>,
    /// Последний атомарный снимок амплитуды из mcrate-audio.
    visualizer: VisualizerFrame,
    /// Пауза, нажатая, пока онлайн-трек ещё грузится («стартовать на паузе»).
    pause_intent: bool,
    seek_hold: Option<(Duration, Instant)>,

    // --- поиск SoundCloud в фоне ---
    sc_tx: mpsc::Sender<SearchReply>,
    sc_rx: mpsc::Receiver<SearchReply>,
    sc_request: u64,
}

impl Controller {
    pub fn new() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("не удалось создать tokio runtime");

        // Библиотека сохраняется между запусками. Если папку приложения определить не удалось,
        // работаем только в памяти (как раньше).
        let local = Arc::new(match library_file() {
            Some(path) => LocalProvider::with_storage(path),
            None => LocalProvider::new(),
        });
        // load() заново читает сохранённые пути. Возвращает пути, которые не загрузились.
        let (unavailable, load_error) = match local.load() {
            Ok(list) => (list.len(), None),
            Err(e) => (0, Some(e.to_string())),
        };
        // Пустой запрос = все треки (так устроен LocalProvider::search).
        let library = rt.block_on(local.search("")).unwrap_or_default();
        let status = match (load_error, unavailable) {
            (Some(e), _) => Some(format!("Не удалось загрузить библиотеку: {e}")),
            (None, 0) => None,
            (None, n) => Some(format!("Не найдено файлов: {n} (удалены или перемещены)")),
        };

        // SoundCloud. `enter()` = «считать, что мы внутри среды tokio»: HTTP-клиенту это может понадобиться при создании.
        let mut sc_state = SoundCloudState::default();
        let soundcloud = {
            let _guard = rt.enter();
            match SoundCloudProvider::new() {
                Ok(p) => {
                    let p = match app_dir() {
                        // client_id публичный; без файла он добывался бы с сайта при каждом запуске (~30 с)
                        Some(dir) => p.with_client_id_file(dir.join("soundcloud_client_id.txt")),
                        None => p,
                    };
                    Some(Arc::new(p))
                }
                Err(e) => {
                    sc_state.error = Some(format!("SoundCloud недоступен: {e}"));
                    None
                }
            }
        };

        // Движок получает всех провайдеров и сам просит у нужного ссылку на звук (по `Track.provider`).
        // Нет звукового устройства: UI всё равно запускаем, ошибку покажем пользователю.
        let mut providers: Vec<Arc<dyn MusicProvider>> = vec![local.clone()];
        if let Some(sc) = &soundcloud {
            providers.push(sc.clone());
        }
        let (engine, audio_error) = match PlaybackEngine::new(providers) {
            Ok(e) => (Some(e), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let volume = 0.8;
        if let Some(engine) = &engine {
            engine.set_volume(volume);
        }

        let (sc_tx, sc_rx) = mpsc::channel();
        Self {
            rt: Some(rt),
            local,
            soundcloud,
            engine,
            audio_error,
            queue: QueueView { shuffle: false, repeat: RepeatMode::Off, len: 0, index: 0 },
            library,
            now: None,
            volume,
            status,
            play_error: None,
            history: Vec::new(),
            sc: sc_state,
            playing: false,
            loading: false,
            position: Duration::ZERO,
            download_progress: None,
            visualizer: VisualizerFrame::default(),
            pause_intent: false,
            seek_hold: None,
            sc_tx,
            sc_rx,
            sc_request: 0,
        }
    }

    fn rt(&self) -> &tokio::runtime::Runtime {
        self.rt.as_ref().expect("среда tokio живёт до Drop")
    }

    // ---------- библиотека ----------

    /// Добавить файлы и папки (из диалога или drag&drop). Папки читаются без вложенных.
    pub fn add_paths(&mut self, paths: &[PathBuf]) {
        let (mut added, mut failed) = (0usize, 0usize);
        for path in paths {
            if path.is_dir() {
                match self.local.add_folder(path) {
                    Ok(n) => added += n,
                    Err(_) => failed += 1,
                }
            } else if self.local.add_file(path).is_ok() {
                added += 1;
            } else {
                failed += 1;
            }
        }
        // Пустой запрос = все треки (так устроен LocalProvider::search).
        self.library = self.rt().block_on(self.local.search("")).unwrap_or_default();
        self.status = Some(match (added, failed) {
            (0, 0) => "Ничего не добавлено".to_string(),
            (a, 0) => format!("Добавлено треков: {a}"),
            (a, f) => format!("Добавлено: {a}, не удалось прочитать: {f}"),
        });
    }

    /// Поиск по своим файлам (локальный, мгновенный). Поиск по SoundCloud: `sc_search`.
    pub fn search(&self, query: &str) -> Vec<Track> {
        self.rt().block_on(self.local.search(query)).unwrap_or_default()
    }

    // ---------- SoundCloud ----------

    /// Запустить поиск по SoundCloud в фоне. UI не ждёт: результат подхватит `tick`.
    /// Новый поиск делает ответ прошлого устаревшим (его выбросим).
    pub fn sc_search(&mut self, query: &str) {
        let query = query.trim().to_string();
        if query.is_empty() {
            return;
        }
        let Some(provider) = self.soundcloud.clone() else {
            return; // причина уже лежит в `sc.error`
        };
        self.sc_request += 1;
        let request = self.sc_request;
        self.sc.query = query.clone();
        self.sc.searching = true;
        self.sc.error = None;

        let tx = self.sc_tx.clone();
        self.rt().spawn(async move {
            let result = provider.search(&query).await.map_err(|e| e.to_string());
            let _ = tx.send((request, query, result)); // UI закрылся: отправлять некому, это нормально
        });
    }

    fn drain_search(&mut self) {
        while let Ok((request, query, result)) = self.sc_rx.try_recv() {
            if request != self.sc_request {
                continue; // устаревший ответ: уже запущен новый поиск
            }
            self.sc.searching = false;
            self.sc.searched = true;
            self.sc.query = query;
            match result {
                Ok(tracks) => {
                    self.sc.results = tracks;
                    self.sc.error = None;
                }
                Err(e) => {
                    // Прошлые результаты оставляем: они ещё играбельны, а ошибку покажем рядом.
                    self.sc.error = Some(format!("Поиск не удался: {e}"));
                }
            }
        }
    }

    // ---------- воспроизведение ----------

    /// Заменить очередь списком `items` и начать с элемента `start`.
    pub fn play_list(&mut self, items: Vec<Track>, start: usize) {
        if items.is_empty() {
            return;
        }
        let Some(engine) = &self.engine else {
            self.status = Some(format!(
                "Нет звукового устройства: {}",
                self.audio_error.clone().unwrap_or_default()
            ));
            return;
        };
        engine.play_queue(items, start);
        self.status = None;
        self.play_error = None;
    }

    pub fn toggle_pause(&mut self) {
        if self.now.is_none() {
            return;
        }
        if let Some(engine) = &self.engine {
            engine.toggle_pause();
        }
    }

    pub fn next(&mut self) {
        if let Some(engine) = &self.engine {
            engine.next();
        }
    }

    pub fn prev(&mut self) {
        // Порог 3 секунд («назад» перезапускает трек) считает сам движок.
        if let Some(engine) = &self.engine {
            engine.prev();
        }
    }

    /// Вызывать каждый кадр: забирает события и снимок состояния движка и результаты поиска.
    pub fn tick(&mut self) {
        self.drain_search();
        let Some(engine) = &self.engine else { return };
        let events = engine.poll_events();
        let state = engine.state();

        for event in events {
            match event {
                PlaybackEvent::Loading(_) => self.pause_intent = false,
                PlaybackEvent::PauseChanged(paused) => self.pause_intent = paused,
                PlaybackEvent::TrackChanged(track) => {
                    self.pause_intent = false;
                    if let Some(track) = track {
                        self.status = None;
                        self.play_error = None;
                        // Пишем в поле напрямую, а не через `self.remember(..)`: `engine` выше
                        // занимает `self.engine` только для чтения, и метод на всём `&mut self`
                        // Rust не пропускает (borrow checker, проверка заимствований).
                        // Обращение к отдельному полю `self.history` ему не мешает.
                        push_history(&mut self.history, track);
                    }
                }
                PlaybackEvent::ModeChanged { shuffle, repeat } => {
                    self.queue.shuffle = shuffle;
                    self.queue.repeat = repeat;
                }
                PlaybackEvent::TrackFailed { track, reason } => {
                    let msg = format!("Не удалось сыграть «{}»: {reason}", track.title);
                    self.status = Some(msg.clone());
                    self.play_error = Some(msg);
                }
                PlaybackEvent::Error(reason) => {
                    self.status = Some(reason.clone());
                    self.play_error = Some(reason);
                }
                PlaybackEvent::QueueChanged { .. } | PlaybackEvent::QueueEnded => {}
            }
        }

        self.now = state.current;
        self.playing = state.is_playing;
        self.loading = state.is_loading;
        self.position = state.position;
        self.download_progress = state.download_progress;
        self.visualizer = engine.visualizer();
        // Громкость не копируем: её хозяин Controller (ползунок двигается плавно, а снимок отстаёт на кадр).
        self.queue = QueueView {
            shuffle: state.shuffle,
            repeat: state.repeat,
            len: state.queue_len,
            index: state.queue_index,
        };
    }

    /// Играет ли сейчас музыка. Пока онлайн-трек грузится, считаем, что «играет»
    /// (на кнопке пауза, нажатие отменяет автозапуск), если только пользователь не нажал паузу.
    pub fn is_playing(&self) -> bool {
        self.playing || (self.loading && !self.pause_intent)
    }

    /// Онлайн-трек выбран и скачивается, звука ещё нет.
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Сколько процентов трека скачано (только пока он докачивается во время игры).
    pub fn download_percent(&self) -> Option<u32> {
        self.download_progress.map(|f| (f.clamp(0.0, 1.0) * 100.0).round() as u32)
    }

    /// Нужна ли перерисовка без участия мыши: играет, грузится или идёт поиск.
    pub fn needs_repaint(&self) -> bool {
        self.is_playing() || self.loading || self.sc.searching
    }

    /// Неблокирующий снимок последних уровней сигнала для фонового визуализатора.
    pub fn visualizer(&self) -> &VisualizerFrame {
        &self.visualizer
    }

    pub fn position(&self) -> Duration {
        match self.seek_hold {
            Some((target, at)) if at.elapsed() < SEEK_HOLD => target,
            _ => self.position,
        }
    }

    pub fn seek_to(&mut self, secs: f32) {
        if let Some(engine) = &self.engine {
            let target = Duration::from_secs_f32(secs.max(0.0));
            engine.seek(target);
            self.seek_hold = Some((target, Instant::now()));
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(engine) = &self.engine {
            engine.set_volume(self.volume);
        }
    }

    /// Передать движку настройки воспроизведения (нормализация громкости). Можно звать на ходу.
    pub fn set_playback_settings(&self, settings: PlaybackSettings) {
        if let Some(engine) = &self.engine {
            engine.set_settings(settings);
        }
    }

    pub fn toggle_shuffle(&mut self) {
        let on = !self.queue.shuffle;
        if let Some(engine) = &self.engine {
            engine.set_shuffle(on);
            self.queue.shuffle = on; // сразу показываем новое значение, движок подтвердит событием
        }
    }

    pub fn cycle_repeat(&mut self) {
        if let Some(engine) = &self.engine {
            engine.cycle_repeat();
            self.queue.repeat = self.queue.repeat.cycle();
        }
    }

    pub fn repeat_mode(&self) -> RepeatMode {
        self.queue.repeat()
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        // Сначала движок (его поток может ещё ходить к провайдерам), потом среда tokio.
        self.engine = None;
        // Не ждём фоновые задачи (например, зависший DNS-запрос поиска): иначе окно закрывалось бы долго.
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, provider: ProviderKind) -> Track {
        Track { id: id.to_string(), provider, title: id.to_string(), artists: vec![], duration_secs: 1 }
    }

    #[test]
    fn history_puts_latest_first_without_duplicates() {
        let mut h = Vec::new();
        push_history(&mut h, track("a", ProviderKind::Local));
        push_history(&mut h, track("b", ProviderKind::Local));
        push_history(&mut h, track("a", ProviderKind::Local)); // сыгран снова: поднимается наверх
        let ids: Vec<_> = h.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn same_id_from_other_service_is_a_different_track() {
        let mut h = Vec::new();
        push_history(&mut h, track("1", ProviderKind::Local));
        push_history(&mut h, track("1", ProviderKind::SoundCloud));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn history_is_capped() {
        let mut h = Vec::new();
        for i in 0..HISTORY_MAX + 20 {
            push_history(&mut h, track(&i.to_string(), ProviderKind::Local));
        }
        assert_eq!(h.len(), HISTORY_MAX);
        assert_eq!(h[0].id, (HISTORY_MAX + 19).to_string());
    }
}

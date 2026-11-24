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

use mcrate_audio::{CacheStats, PlaybackEngine, PlaybackEvent, PlaybackSettings, RepeatMode, VisualizerFrame};
use mcrate_core::{History, MusicProvider, ProviderKind, Track, paths};
use mcrate_providers::{LocalProvider, SoundCloudProvider};

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

/// Сведения о кэше онлайн-треков, как их отдаёт движок.
#[derive(Debug, Clone)]
pub struct CacheInfo {
    /// Папка кэша.
    pub dir: PathBuf,
    /// Сколько треков и байт в кэше сейчас.
    pub stats: CacheStats,
    /// Сколько треков кэш хранит, старые сверх этого удаляются сами.
    pub limit: usize,
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
    /// Поиск идёт прямо сейчас.
    pub searching: bool,
    /// Ошибка последнего поиска (текст уже для пользователя).
    pub error: Option<String>,
    /// Был ли уже хотя бы один поиск (чтобы отличить «ничего не искали» от «ничего не нашли»).
    pub searched: bool,
}

impl SoundCloudState {
    /// Начало нового поиска: прошлые результаты убираются сразу. Иначе на время поиска они остались бы на экране
    /// под заголовком уже нового запроса, а индикатор загрузки встал бы поверх них.
    fn begin_search(&mut self, query: String) {
        self.query = query;
        self.results.clear();
        self.searching = true;
        self.error = None;
    }

    /// Поиск закончился (ответ не устаревший): результаты или ошибка. Прошлых результатов уже нет (`begin_search`),
    /// поэтому при ошибке на экране остаётся только она.
    fn finish_search(&mut self, query: String, result: Result<Vec<Track>, String>) {
        self.searching = false;
        self.searched = true;
        self.query = query;
        match result {
            Ok(tracks) => {
                self.results = tracks;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Поиск не удался: {e}")),
        }
    }
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
    /// История прослушивания (хранится на диске, см. `mcrate_core::History`).
    pub history: History,
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
    /// Собирает контроллер: библиотека на диске, провайдеры, движок воспроизведения и каналы фоновых задач.
    ///
    /// Длиннее 60 строк намеренно: это линейная сборка полей по порядку, в котором они зависят друг от друга
    /// (например, HTTP-клиент SoundCloud создаётся внутри среды tokio, см. комментарии ниже); разрез только размазал бы эту цепочку.
    pub fn new() -> Self {
        let rt =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("не удалось создать tokio runtime");

        // Библиотека сохраняется между запусками. Если папку приложения определить не удалось,
        // работаем только в памяти (как раньше).
        let local = Arc::new(match paths::library_file() {
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
                    let p = match paths::soundcloud_client_id_file() {
                        // client_id публичный; без файла он добывался бы с сайта при каждом запуске (~30 с)
                        Some(file) => p.with_client_id_file(file),
                        None => p,
                    };
                    let p = Arc::new(p);
                    // client_id добываем в фоне при запуске, чтобы первый поиск не ждал (при первом запуске
                    // это до ~30 с, дальше он берётся из файла). Ошибку не показываем: поиск попробует сам.
                    let warm = Arc::clone(&p);
                    rt.spawn(async move {
                        let _ = warm.prepare().await;
                    });
                    Some(p)
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
            history: History::open(paths::history_file()),
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
        self.sc.begin_search(query.clone());

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
            self.sc.finish_search(query, result);
        }
    }

    // ---------- воспроизведение ----------

    /// Заменить очередь списком `items` и начать с элемента `start`.
    pub fn play_list(&mut self, items: Vec<Track>, start: usize) {
        if items.is_empty() {
            return;
        }
        let Some(engine) = &self.engine else {
            self.status = Some(format!("Нет звукового устройства: {}", self.audio_error.clone().unwrap_or_default()));
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
                        self.history.push(track);
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
        self.queue = QueueView { shuffle: state.shuffle, repeat: state.repeat, len: state.queue_len, index: state.queue_index };
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

    /// Что сейчас в кэше онлайн-треков (для экрана «Хранилище и кеш»). `None`, если звукового движка нет.
    pub fn cache_info(&self) -> Option<CacheInfo> {
        let engine = self.engine.as_ref()?;
        Some(CacheInfo { dir: engine.cache_dir().to_path_buf(), stats: engine.cache_stats(), limit: engine.cache_limit() })
    }

    /// Очистить кэш онлайн-треков. Играющий и готовящийся следующий трек остаются; движок делает это
    /// в своём потоке, поэтому новый размер видно через `cache_info` со следующего кадра.
    pub fn cache_clear(&self) {
        if let Some(engine) = &self.engine {
            engine.cache_clear();
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

    /// Следующий режим повтора выбирает движок; новое значение приходит событием `ModeChanged` в `tick`.
    pub fn cycle_repeat(&self) {
        if let Some(engine) = &self.engine {
            engine.cycle_repeat();
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

    fn state_with_old_results() -> SoundCloudState {
        let mut sc = SoundCloudState::default();
        sc.begin_search("старый".into());
        sc.finish_search("старый".into(), Ok(vec![track("1", ProviderKind::SoundCloud), track("2", ProviderKind::SoundCloud)]));
        sc
    }

    #[test]
    fn new_search_drops_old_results_at_once() {
        let mut sc = state_with_old_results();
        assert_eq!(sc.results.len(), 2);
        sc.begin_search("новый".into());
        assert!(sc.results.is_empty(), "старые результаты не должны жить под индикатором нового поиска");
        assert!(sc.searching);
        assert_eq!(sc.query, "новый");
        assert!(sc.error.is_none());
        assert!(sc.searched, "«искали» остаётся: пустое состояние «ничего не искали» не должно вернуться");
    }

    #[test]
    fn failed_search_leaves_only_the_error() {
        let mut sc = state_with_old_results();
        sc.begin_search("новый".into());
        sc.finish_search("новый".into(), Err("нет сети".into()));
        assert!(!sc.searching);
        assert!(sc.results.is_empty());
        assert_eq!(sc.error.as_deref(), Some("Поиск не удался: нет сети"));
    }

    #[test]
    fn finished_search_shows_new_results() {
        let mut sc = state_with_old_results();
        sc.begin_search("новый".into());
        sc.finish_search("новый".into(), Ok(vec![track("3", ProviderKind::SoundCloud)]));
        assert!(!sc.searching);
        assert_eq!(sc.results.len(), 1);
        assert_eq!(sc.results[0].id, "3");
        assert!(sc.error.is_none());
    }
}

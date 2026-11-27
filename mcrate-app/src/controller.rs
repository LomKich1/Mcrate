//! Controller = мост между UI и ядром (mcrate-core / mcrate-audio / mcrate-providers).
//! UI не трогает провайдеры и плеер напрямую: все действия идут через этот тип.
//! Ядро (его делает LomKich) мы здесь НЕ меняем, только используем.
//!
//! Шаг 5c плана: Controller стал тонкой обёрткой над `PlaybackEngine`. Звук, очередь, смена трека,
//! «назад с порогом 3 с», перемотка и загрузка онлайн-треков живут в движке; здесь только:
//!   * команды (`play_list`, `next`, `seek_to`...) пересылаются движку;
//!   * каждый кадр (`tick`) читаем события и снимок состояния движка и копируем в поля для UI;
//!   * библиотека своих файлов (`LocalProvider`) и поиск по SoundCloud (в фоне, UI не ждёт).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use mcrate_audio::{CacheStats, PlaybackEngine, PlaybackEvent, PlaybackSettings, RepeatMode, VisualizerFrame, Volume};
use mcrate_core::{History, Likes, MusicProvider, Playlists, ProviderKind, Track, paths};
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
/// Длина падения стрелки и время исчезновения значка после завершения скачивания.
pub(crate) const DOWNLOAD_ICON_DROP: Duration = Duration::from_millis(620);
pub(crate) const DOWNLOAD_ICON_FADE: Duration = Duration::from_millis(220);

/// Значок должен завершить падение даже при очень быстром скачивании, затем плавно исчезнуть.
fn download_visual_expired(visual: DownloadVisual, now: Instant) -> bool {
    let Some(finished_at) = visual.finished_at else { return false };
    let fade_start = (visual.started_at + DOWNLOAD_ICON_DROP).max(finished_at);
    now.saturating_duration_since(fade_start) >= DOWNLOAD_ICON_FADE
}

/// Состояние анимации скачивания для одной строки списка.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DownloadVisual {
    /// Момент начала скачивания: от него считается падение стрелки.
    pub(crate) started_at: Instant,
    /// Момент завершения: после анимации значок плавно исчезает.
    pub(crate) finished_at: Option<Instant>,
}

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
    /// Онлайн-треки, у которых есть постоянный аудиофайл в папке загрузок.
    pub downloaded: Vec<Track>,
    /// Текущий трек (в том числе пока он грузится): копия `state().current` движка.
    pub now: Option<Track>,
    pub volume: Volume,
    pub status: Option<String>,
    /// Ошибка пользовательской загрузки; успешное сохранение не вызывает уведомление.
    pub download_status: Option<String>,
    /// Ключи треков с активной анимацией скачивания на обложке строки.
    download_visuals: Arc<HashMap<LikeKey, DownloadVisual>>,
    /// Последняя ошибка воспроизведения (трек не запустился, оборвалась загрузка).
    /// Стирается, когда пошёл следующий трек.
    pub play_error: Option<String>,
    /// История прослушивания за сеанс: новые сверху, без повторов (трек, сыгранный снова, поднимается наверх).
    /// История прослушивания (хранится на диске, см. `mcrate_core::History`).
    pub history: History,
    /// Понравившиеся треки (хранятся на диске, см. `mcrate_core::Likes`).
    pub likes: Likes,
    /// Те же лайки как множество ключей `(сервис, id)`: строки списков проверяют лайк по нему за O(1).
    /// Пересобирается при каждом изменении лайков.
    liked: Arc<HashSet<LikeKey>>,
    /// Плейлисты владельца (хранятся на диске, см. `mcrate_core::Playlists`).
    pub playlists: Playlists,
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
        let volume = Volume::default(); // сохранённую вернёт `restore_volume`
        if let Some(engine) = &engine {
            engine.set_volume(volume.level());
        }

        let (sc_tx, sc_rx) = mpsc::channel();
        let history = History::open(paths::history_file());
        let likes = Likes::open(paths::likes_file());
        let playlists = Playlists::open(paths::playlists_file());
        let mut downloaded = PlaybackEngine::list_downloaded_tracks();
        restore_download_metadata(&mut downloaded, &history, &likes, &playlists);
        let now = restore_last_track(engine.as_ref(), &history);
        Self {
            rt: Some(rt),
            local,
            soundcloud,
            engine,
            audio_error,
            queue: QueueView { shuffle: false, repeat: RepeatMode::Off, len: 0, index: 0 },
            library,
            downloaded,
            now,
            volume,
            status,
            download_status: None,
            download_visuals: Arc::new(HashMap::new()),
            play_error: None,
            history,
            liked: Arc::new(liked_keys(likes.items())),
            likes,
            playlists,
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
        let mut downloads_changed = false;

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
                PlaybackEvent::DownloadStarted(track) => {
                    let key = (track.provider, track.id.clone());
                    Arc::make_mut(&mut self.download_visuals).insert(
                        key,
                        DownloadVisual { started_at: Instant::now(), finished_at: None },
                    );
                    self.download_status = None;
                }
                PlaybackEvent::DownloadFinished { track, result } => {
                    let key = (track.provider, track.id.clone());
                    if let Some(visual) = Arc::make_mut(&mut self.download_visuals).get_mut(&key) {
                        visual.finished_at = Some(Instant::now());
                    }
                    downloads_changed |= result.is_ok();
                    self.download_status = result.err().map(|reason| format!("Не удалось скачать «{}»: {reason}", track.title));
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

        if downloads_changed {
            self.downloaded = engine.downloaded_tracks();
            restore_download_metadata(&mut self.downloaded, &self.history, &self.likes, &self.playlists);
        }
        self.now = state.current;
        self.playing = state.is_playing;
        self.loading = state.is_loading;
        self.position = state.position;
        self.download_progress = state.download_progress;
        self.visualizer = engine.visualizer();
        // Громкость не копируем: её хозяин Controller (ползунок двигается плавно, а снимок отстаёт на кадр).
        self.queue = QueueView { shuffle: state.shuffle, repeat: state.repeat, len: state.queue_len, index: state.queue_index };
        let now = Instant::now();
        if self.download_visuals.values().any(|visual| download_visual_expired(*visual, now)) {
            Arc::make_mut(&mut self.download_visuals).retain(|_, visual| !download_visual_expired(*visual, now));
        }
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

    pub fn set_volume(&mut self, level: f32) {
        self.volume.set(level);
        self.send_volume();
    }

    /// Кнопка динамика: выключить звук или вернуть прежнюю громкость.
    pub fn toggle_mute(&mut self) {
        self.volume.toggle_mute();
        self.send_volume();
    }

    /// Вернуть громкость с прошлого запуска.
    pub fn restore_volume(&mut self, volume: Volume) {
        self.volume = volume;
        self.send_volume();
    }

    fn send_volume(&self) {
        if let Some(engine) = &self.engine {
            engine.set_volume(self.volume.level());
        }
    }

    /// Что сейчас в кэше онлайн-треков (для экрана «Хранилище и кеш»). `None`, если звукового движка нет.
    pub fn cache_info(&self) -> Option<CacheInfo> {
        let engine = self.engine.as_ref()?;
        Some(CacheInfo { dir: engine.cache_dir().to_path_buf(), stats: engine.cache_stats(), limit: engine.cache_limit() })
    }

    /// Добавить трек в конец очереди (если ничего не играло и очередь пуста, движок сам начнёт его играть).
    pub fn add_to_queue(&mut self, track: Track) {
        let Some(engine) = &self.engine else {
            self.status = Some(format!("Нет звукового устройства: {}", self.audio_error.clone().unwrap_or_default()));
            return;
        };
        engine.add_to_queue(track);
    }

    /// Стоит ли лайк на треке.
    pub fn is_liked(&self, track: &Track) -> bool {
        self.liked.contains(&(track.provider, track.id.clone()))
    }

    /// Множество лайкнутых треков (дёшево копируется: это `Arc`), его отдают строкам списков.
    pub fn liked_keys(&self) -> Arc<HashSet<LikeKey>> {
        Arc::clone(&self.liked)
    }

    /// Состояния анимации скачивания, которые строки списков используют для обложек.
    pub(crate) fn download_visuals(&self) -> Arc<HashMap<LikeKey, DownloadVisual>> {
        Arc::clone(&self.download_visuals)
    }

    /// Поставить или снять лайк; возвращает, стоит ли он теперь. Всё делает ядро (`Likes::toggle`).
    pub fn toggle_like(&mut self, track: &Track) -> bool {
        let liked = self.likes.toggle(track);
        self.liked = Arc::new(liked_keys(self.likes.items()));
        liked
    }

    /// Убирает трек из понравившихся и обновляет быстрый набор для сердечек в строках.
    pub fn remove_like(&mut self, track: &Track) -> bool {
        let removed = self.likes.remove(track.provider, &track.id);
        if removed {
            self.liked = Arc::new(liked_keys(self.likes.items()));
        }
        removed
    }

    /// Удаляет скачанный файл через движок и сразу убирает трек из списка библиотеки.
    pub fn remove_downloaded_track(&mut self, track: &Track) -> Result<(), String> {
        if let Some(engine) = &self.engine {
            engine.remove_downloaded_track(track)?;
        } else {
            PlaybackEngine::remove_downloaded_track_from_disk(track)?;
        }
        self.downloaded.retain(|item| !(item.provider == track.provider && item.id == track.id));
        Ok(())
    }

    /// Перечитать список постоянных загрузок (например, при повторном открытии экрана).
    pub fn refresh_downloads(&mut self) {
        self.downloaded = PlaybackEngine::list_downloaded_tracks();
        restore_download_metadata(&mut self.downloaded, &self.history, &self.likes, &self.playlists);
    }

    /// Скачать онлайн-трек, не меняя очередь и текущее воспроизведение.
    pub fn download_track(&mut self, track: Track) {
        let Some(engine) = &self.engine else {
            self.download_status = Some(format!("Нет звукового движка: {}", self.audio_error.clone().unwrap_or_default()));
            return;
        };
        engine.download_track(track);
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

/// Ключ лайкнутого трека: сервис и id (у разных сервисов id могут совпасть).
pub type LikeKey = (ProviderKind, String);

/// Множество ключей по списку лайкнутых треков.
fn liked_keys(items: &[Track]) -> HashSet<LikeKey> {
    items.iter().map(|t| (t.provider, t.id.clone())).collect()
}


/// Последний игравший трек (первый в истории) возвращается в плеер без запуска. `None`: истории или звука нет.
fn restore_last_track(engine: Option<&PlaybackEngine>, history: &History) -> Option<Track> {
    let (engine, last) = (engine?, history.items().first()?);
    engine.restore_track(last.clone());
    Some(last.clone())
}

/// Старые файлы появились до индекса `downloads.txt`: дополняем их метаданные данными,
/// которые уже сохранены в истории, лайках или плейлистах. Идентификатор и сервис всегда должны совпасть.
fn restore_download_metadata(downloaded: &mut [Track], history: &History, likes: &Likes, playlists: &Playlists) {
    let mut known = history.items().to_vec();
    known.extend(likes.items().iter().cloned());
    for playlist in playlists.list() {
        known.extend(playlist.tracks.iter().cloned());
    }
    for track in downloaded.iter_mut().filter(|track| track.artists.is_empty() || track.duration_secs == 0) {
        if let Some(saved) = known.iter().find(|saved| saved.provider == track.provider && saved.id == track.id) {
            *track = saved.clone();
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
    fn short_download_keeps_the_arrow_until_it_lands_then_fades() {
        let started_at = Instant::now();
        let visual = DownloadVisual { started_at, finished_at: Some(started_at + Duration::from_millis(80)) };
        assert!(!download_visual_expired(visual, started_at + DOWNLOAD_ICON_DROP));
        assert!(download_visual_expired(visual, started_at + DOWNLOAD_ICON_DROP + DOWNLOAD_ICON_FADE));
    }

    #[test]
    fn active_download_visual_does_not_expire() {
        let now = Instant::now();
        let visual = DownloadVisual { started_at: now, finished_at: None };
        assert!(!download_visual_expired(visual, now + Duration::from_secs(60)));
    }

    #[test]
    fn liked_keys_tell_services_apart() {
        let keys = liked_keys(&[track("1", ProviderKind::Local), track("2", ProviderKind::SoundCloud)]);
        assert!(keys.contains(&(ProviderKind::Local, "1".to_string())));
        assert!(keys.contains(&(ProviderKind::SoundCloud, "2".to_string())));
        assert!(!keys.contains(&(ProviderKind::SoundCloud, "1".to_string())), "тот же id другого сервиса это другой трек");
        assert_eq!(keys.len(), 2);
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

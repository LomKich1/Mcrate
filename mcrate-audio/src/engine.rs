//! PlaybackEngine: единая точка входа в звук. Живёт в своём потоке.
//!
//! Онлайн-треки (SoundCloud и другие) грузятся в ФОНЕ: пока трек скачивается, движок
//! продолжает принимать команды (пауза, громкость, «следующий»). Готовый файл возвращается
//! в поток движка внутренней командой `LoadFinished` с «номером запроса»: если пользователь
//! успел переключить трек, результат устарел и выбрасывается.
//!
//! Играть начинаем, не дожидаясь конца загрузки: как только скачано `START_BYTES`, фоновая
//! задача шлёт `StreamReady`, и движок запускает докачиваемый файл (`AudioPlayer::play_stream`),
//! пока загрузка продолжается. Мелкие файлы и треки из кэша идут по-старому, через `LoadFinished`.
//!
//! Нормализация громкости (`PlaybackSettings`): пока трек играет, в фоне измеряется его громкость
//! (`loudness::measure_file`, по готовому файлу), и усиление плавно подтягивается к целевой громкости.
//! Замеры запоминаются на сессию, повторное проигрывание стартует уже с нужным усилением.
//!
//! Предзагрузка: когда текущий трек целиком на диске, движок готовит следующий (`queue.peek_next`)
//! во второй деке плеера и заранее мерит его громкость. Предзагрузка идёт в своём слоте
//! (свой номер запроса `preload_request`) и не мешает загрузке текущего трека. Когда трек доиграл
//! сам, движок переключается на готовую деку: без «загрузки» и сразу с нужным усилением.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex}; // mpsc = канал сообщений между потоками
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mcrate_core::{MusicProvider, ProviderKind, StreamSource, Track};
use mcrate_net::{DownloadProgress, StreamDownloader};

use crate::loudness::{self, Loudness, Measured};
use crate::normalize::{Normalizer, PlaybackSettings, TrackKey};
use crate::stream_cache::{part_path, StreamCache};
use crate::visualizer::{VisualizerFeed, VisualizerFrame};
use crate::{AudioError, AudioPlayer, PlayQueue, RepeatMode};

/// Сколько последних скачанных онлайн-треков хранить в кэше (остальные удаляются).
const MAX_CACHED_TRACKS: usize = 20;

/// Сколько байт должно быть скачано, чтобы начать играть, не дожидаясь конца: 128 КБ это
/// около 6 с звука AAC 160 кбит/с (на интернете от ~25 КБ/с докачка успевает за воспроизведением).
const START_BYTES: u64 = 128 * 1024;

/// Как часто фоновая задача проверяет, набралось ли `START_BYTES`.
const READY_POLL: Duration = Duration::from_millis(50);

/// Перематывать дальше скачанного нельзя (вызов повис бы до прихода данных), поэтому
/// цель перемотки зажимается в скачанную часть, минус этот запас.
const SEEK_MARGIN: Duration = Duration::from_secs(3);

/// Команды от UI к движку (внутренние, UI вызывает методы ниже).
enum Command {
    PlayQueue { items: Vec<Track>, start: usize },
    AddToQueue(Track),
    TogglePause,
    Next,
    Prev,
    Seek(Duration),
    SetVolume(f32),
    SetSettings(PlaybackSettings),
    SetShuffle(bool),
    CycleRepeat,
    Stop,
    Shutdown,
    /// Внутренняя: фоновая загрузка закончилась. `request` = номер запроса, на который это ответ.
    /// Эту команду шлёт сам движок (из фоновой задачи), UI её не видит.
    LoadFinished { request: u64, result: Result<PathBuf, String> },
    /// Внутренняя: файл ещё докачивается, но уже скачан запас: можно начинать играть.
    /// `part` = имя растущего файла, `dest` = итоговое имя (после докачки `part` переименуют в `dest`).
    StreamReady { request: u64, part: PathBuf, dest: PathBuf, progress: DownloadProgress },
    /// Внутренняя: трек, игравший во время докачки, докачан и лежит целиком по пути `path`
    /// (теперь его можно измерить).
    StreamComplete { request: u64, path: PathBuf },
    /// Внутренняя: фоновый замер громкости закончился. `None` = измерить нечего (тишина).
    /// Привязан к треку, а не к запросу: замер целого файла верен, даже если трек уже сменили.
    LoudnessReady { key: TrackKey, loudness: Option<Loudness> },
    /// Внутренняя: фоновая предзагрузка следующего трека закончилась (файл целиком готов).
    /// `request` = номер запроса предзагрузки (свой счётчик, не путать с `request` текущего трека).
    PreloadFinished { request: u64, result: Result<PathBuf, String> },
}

/// События от движка к UI: только дискретные изменения.
#[derive(Debug, Clone)]
pub enum PlaybackEvent {
    /// Трек выбран, но звука ещё нет: он скачивается. Дальше придёт `TrackChanged(Some)`
    /// (когда пошёл звук) или `TrackFailed` (если загрузка не удалась).
    Loading(Track),
    TrackChanged(Option<Track>), // None = ничего не играет
    PauseChanged(bool),          // true = на паузе
    ModeChanged { shuffle: bool, repeat: RepeatMode },
    QueueChanged { len: usize },
    TrackFailed { track: Track, reason: String }, // трек пропущен
    QueueEnded,
    Error(String), // нефатальная ошибка (например, не удалась перемотка)
}

/// Снимок состояния для отрисовки. Обновляется примерно раз в 100 мс.
#[derive(Debug, Clone)]
pub struct PlaybackState {
    /// Текущий трек. Пока трек грузится, это он же (с `is_loading == true`).
    pub current: Option<Track>,
    pub position: Duration,
    /// Длина трека; пока трек грузится, берётся из метаданных трека.
    pub duration: Option<Duration>,
    pub is_playing: bool,
    /// true, пока онлайн-трек скачивается и звука ещё нет (можно рисовать «крутилку»).
    pub is_loading: bool,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub queue_index: usize,
    pub queue_len: usize,
    /// Доля скачанного 0.0..=1.0, пока трек докачивается во время игры (для полосы «буфер»
    /// на прогресс-баре). None = докачивать нечего (трек скачан целиком, локальный или ничего не играет)
    /// либо долю не удалось посчитать.
    pub download_progress: Option<f32>,
    /// Во сколько раз нормализация сейчас меняет громкость трека (1.0 = не меняет).
    /// Меняется плавно, пока замер подтягивает громкость к целевой.
    pub normalization_gain: f32,
}

impl PlaybackState {
    fn initial() -> Self {
        Self {
            current: None,
            position: Duration::ZERO,
            duration: None,
            is_playing: false,
            is_loading: false,
            volume: 1.0,
            shuffle: false,
            repeat: RepeatMode::Off,
            queue_index: 0,
            queue_len: 0,
            download_progress: None,
            normalization_gain: 1.0,
        }
    }
}

/// Сторона UI: отправляет команды и читает события и снимок.
pub struct PlaybackEngine {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<PlaybackEvent>,
    state: Arc<Mutex<PlaybackState>>, // общий снимок: движок пишет, UI читает
    /// Атомарный снимок аудиоогибающей; UI читает его независимо от цикла состояния (100 мс).
    visualizer: VisualizerFeed,
    thread: Option<JoinHandle<()>>,
}

impl PlaybackEngine {
    /// Запускает поток движка. `providers`: сервисы, у которых он будет просить звук.
    /// Скачанные онлайн-треки кэшируются в `%TEMP%\mcrate\stream\` (не больше 20 штук).
    pub fn new(providers: Vec<Arc<dyn MusicProvider>>) -> Result<Self, AudioError> {
        Self::with_stream_cache(providers, StreamCache::default_dir())
    }

    /// Папка, куда движок складывает скачанные онлайн-треки (`%TEMP%\mcrate\stream`).
    pub fn stream_cache_dir() -> PathBuf {
        StreamCache::default_dir()
    }

    /// То же, что `new`, но папку кэша скачанных треков задаёшь сам (нужно тестам и примерам).
    pub fn with_stream_cache(
        providers: Vec<Arc<dyn MusicProvider>>,
        cache_dir: PathBuf,
    ) -> Result<Self, AudioError> {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (evt_tx, evt_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::channel::<Result<(), String>>();
        let state = Arc::new(Mutex::new(PlaybackState::initial()));
        let thread_state = Arc::clone(&state);
        let visualizer = VisualizerFeed::new();
        let thread_visualizer = visualizer.clone();
        // Копия отправителя команд для самого движка: через неё фоновые задачи
        // возвращают результат загрузки в поток движка
        let worker_tx = cmd_tx.clone();

        // `move` отдаёт замыканию владение переменными: оно работает в другом потоке
        let handle = thread::spawn(move || {
            // Устройство вывода и среду для async открываем ВНУТРИ потока.
            // new_multi_thread = у среды свои рабочие потоки, поэтому фоновые задачи
            // (скачивание) идут сами, пока поток движка занят командами. Двух потоков хватит.
            let init = AudioPlayer::with_visualizer(thread_visualizer).map_err(|e| e.to_string()).and_then(|player| {
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .map(|runtime| (player, runtime))
                    .map_err(|e| e.to_string())
            });
            let (player, runtime) = match init {
                Ok(pair) => {
                    let _ = init_tx.send(Ok(()));
                    pair
                }
                Err(msg) => {
                    let _ = init_tx.send(Err(msg));
                    return;
                }
            };

            let providers = providers.into_iter().map(|p| (p.kind(), p)).collect();
            Worker {
                player,
                runtime,
                providers,
                // Если сетевой клиент не создался, играть можно только локальные файлы;
                // ошибка всплывёт при первой попытке сыграть онлайн-трек
                downloader: StreamDownloader::new().map(Arc::new).map_err(|e| e.to_string()),
                cache: StreamCache::new(cache_dir, MAX_CACHED_TRACKS),
                commands_tx: worker_tx,
                queue: PlayQueue::new(),
                state: thread_state,
                events: evt_tx,
                volume: 1.0,
                loaded: false,
                duration: None,
                loading: false,
                loading_track: None,
                load_task: None,
                request: 0,
                start_paused: false,
                fail_streak: 0,
                stream_progress: None,
                stream_error_reported: false,
                normalizer: Normalizer::new(PlaybackSettings::default()),
                analysis: None,
                playing_file: None,
                last_gain_tick: Instant::now(),
                preload: None,
                preload_request: 0,
                preload_task: None,
                preload_analysis: None,
            }
            .run(cmd_rx);
        });

        // Ждём, пока поток сообщит, удалось ли открыть звук
        match init_rx.recv() {
            Ok(Ok(())) => Ok(Self { commands: cmd_tx, events: evt_rx, state, visualizer, thread: Some(handle) }),
            Ok(Err(msg)) => Err(AudioError::Engine(msg)),
            Err(_) => Err(AudioError::Engine("поток движка завершился при запуске".to_string())),
        }
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command); // `let _ =` = игнорируем ошибку (движок уже остановлен)
    }

    pub fn play_queue(&self, items: Vec<Track>, start: usize) { self.send(Command::PlayQueue { items, start }); }
    pub fn add_to_queue(&self, track: Track) { self.send(Command::AddToQueue(track)); }
    pub fn toggle_pause(&self) { self.send(Command::TogglePause); }
    pub fn next(&self) { self.send(Command::Next); }
    pub fn prev(&self) { self.send(Command::Prev); }
    pub fn seek(&self, pos: Duration) { self.send(Command::Seek(pos)); }
    pub fn set_volume(&self, volume: f32) { self.send(Command::SetVolume(volume)); }
    /// Настройки воспроизведения (сейчас: нормализация громкости). Можно менять на ходу.
    pub fn set_settings(&self, settings: PlaybackSettings) { self.send(Command::SetSettings(settings)); }
    pub fn set_shuffle(&self, on: bool) { self.send(Command::SetShuffle(on)); }
    pub fn cycle_repeat(&self) { self.send(Command::CycleRepeat); }
    pub fn stop(&self) { self.send(Command::Stop); }

    /// Забрать все накопившиеся события (не ждёт, можно звать каждый кадр).
    pub fn poll_events(&self) -> Vec<PlaybackEvent> {
        self.events.try_iter().collect()
    }

    /// Копия текущего снимка состояния.
    pub fn state(&self) -> PlaybackState {
        self.state.lock().unwrap().clone()
    }

    /// Последняя аудиоогибающая, обновляемая самим потоком вывода примерно 120 раз/с.
    /// Чтение использует атомарные значения и никогда не блокирует звуковой поток.
    pub fn visualizer(&self) -> VisualizerFrame {
        self.visualizer.snapshot()
    }
}

impl Drop for PlaybackEngine {
    // Drop вызывается, когда движок уничтожается: просим поток завершиться и ждём его
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Что получилось при попытке запустить трек.
enum Begin {
    /// Звук пошёл сразу (локальный файл). Внутри: длина, если декодер её знает.
    Playing(Option<Duration>),
    /// Запущена фоновая загрузка, результат придёт командой `LoadFinished`.
    Loading,
    /// Трек не удалось запустить, внутри причина.
    Failed(String),
}

/// «Сторож» фоновой загрузки. Гарантирует, что движок получит ответ на свой запрос,
/// даже если задача упала с паникой: при уничтожении сторож шлёт ошибку «загрузка прервана».
/// Если задачу отменили (`abort`) специально, этот ответ приходит с устаревшим номером
/// запроса и просто игнорируется.
struct LoadGuard {
    tx: mpsc::Sender<Command>,
    request: u64,
    sent: bool,
    /// true = это загрузка для предзагрузки: играть её по мере скачивания не нужно, ответ один
    /// (`PreloadFinished`, когда файл целиком готов).
    preload: bool,
}

impl LoadGuard {
    /// Итоговый ответ задачи: для текущего трека `LoadFinished`, для предзагрузки `PreloadFinished`.
    fn result_command(&self, result: Result<PathBuf, String>) -> Command {
        if self.preload {
            Command::PreloadFinished { request: self.request, result }
        } else {
            Command::LoadFinished { request: self.request, result }
        }
    }

    /// Отправить настоящий результат (сторож после этого молчит).
    fn finish(mut self, result: Result<PathBuf, String>) {
        self.sent = true;
        let _ = self.tx.send(self.result_command(result));
    }

    /// Сообщить, что можно играть докачиваемый файл. Сторож остаётся «на посту»: если
    /// задача потом упадёт, он пошлёт ошибку (движок её проигнорирует как устаревшую).
    /// Для предзагрузки ничего не шлём: она ждёт файл целиком.
    fn stream_ready(&self, part: PathBuf, dest: PathBuf, progress: DownloadProgress) {
        if !self.preload {
            let _ = self.tx.send(Command::StreamReady { request: self.request, part, dest, progress });
        }
    }

    /// Трек, игравший во время докачки, докачан и переименован в `path`. Это последнее
    /// сообщение задачи: сторож после него молчит. Для предзагрузки это обычный итог.
    fn stream_complete(mut self, path: PathBuf) {
        self.sent = true;
        let command = if self.preload {
            Command::PreloadFinished { request: self.request, result: Ok(path) }
        } else {
            Command::StreamComplete { request: self.request, path }
        };
        let _ = self.tx.send(command);
    }
}

impl Drop for LoadGuard {
    fn drop(&mut self) {
        if !self.sent {
            let _ = self.tx.send(self.result_command(Err("загрузка прервана".to_string())));
        }
    }
}

/// Ключ трека для замеров громкости и сравнения «тот же трек».
fn key_of(track: &Track) -> TrackKey {
    (track.provider, track.id.clone())
}

/// Состояние предзагрузки следующего трека.
enum PreloadState {
    /// Файл ещё скачивается в фоне.
    Loading,
    /// Файл стоит во второй деке плеера (на паузе). Внутри: путь к файлу и длина, если декодер её знает.
    Ready { path: PathBuf, duration: Option<Duration> },
    /// Не получилось (нет файла, битый файл, нет сети). Повторно не пробуем, пока следующий трек тот же.
    Failed,
}

/// Следующий трек, который движок готовит заранее.
struct Preload {
    key: TrackKey,
    state: PreloadState,
}

/// Сторона потока движка: здесь живёт вся логика.
struct Worker {
    player: AudioPlayer,
    runtime: tokio::runtime::Runtime,
    providers: HashMap<ProviderKind, Arc<dyn MusicProvider>>,
    downloader: Result<Arc<StreamDownloader>, String>,
    cache: StreamCache,
    commands_tx: mpsc::Sender<Command>, // для ответов фоновых задач самому себе
    queue: PlayQueue<Track>,
    state: Arc<Mutex<PlaybackState>>,
    events: mpsc::Sender<PlaybackEvent>,
    volume: f32,
    loaded: bool,               // true, пока какой-то трек загружен в плеер (играет или на паузе)
    duration: Option<Duration>, // длина загруженного (или грузящегося) трека
    loading: bool,              // true, пока идёт фоновая загрузка
    loading_track: Option<Track>, // какой именно трек сейчас грузится
    load_task: Option<tokio::task::JoinHandle<()>>, // ручка фоновой задачи, чтобы её отменить
    request: u64,               // номер текущего запроса; растёт при каждом запуске и отмене
    start_paused: bool,         // пользователь нажал паузу во время загрузки: стартуем на паузе
    fail_streak: usize,         // сколько треков подряд не запустилось (защита от бесконечного цикла)
    stream_progress: Option<DownloadProgress>, // Some = играет трек, который ещё мог докачиваться
    stream_error_reported: bool, // про обрыв докачки уже сообщили UI (чтобы не повторять каждые 100 мс)
    normalizer: Normalizer,      // замеры громкости и текущее усиление
    analysis: Option<Arc<AtomicBool>>, // Some = идёт замер текущего трека; внутри флаг отмены
    playing_file: Option<PathBuf>, // целый файл текущего трека, когда он известен (иначе замерить нельзя)
    last_gain_tick: Instant,     // когда в последний раз двигали усиление
    preload: Option<Preload>,    // Some = следующий трек запрошен (грузится, готов или не удался)
    preload_request: u64,        // номер запроса предзагрузки; растёт при каждом запуске и сбросе
    preload_task: Option<tokio::task::JoinHandle<()>>, // ручка фоновой загрузки следующего трека
    preload_analysis: Option<Arc<AtomicBool>>, // Some = идёт замер следующего трека; внутри флаг отмены
}

impl Worker {
    fn run(mut self, commands: mpsc::Receiver<Command>) {
        loop {
            // Ждём команду не дольше 100 мс; за это время успеваем проверить, не доиграл ли трек
            match commands.recv_timeout(Duration::from_millis(100)) {
                Ok(Command::Shutdown) => break,
                Ok(cmd) => self.handle(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break, // UI закрылся
            }

            if self.loaded && self.player.is_finished() {
                self.on_track_finished();
            }
            self.sync_preload();
            self.update_gain();
            self.report_stream_error();
            self.publish_state();
        }
        self.cancel_loading();
        self.player.stop();
        // Не ждём фоновые задачи (например, зависший DNS-запрос): просто бросаем среду
        self.runtime.shutdown_background();
    }

    fn emit(&self, event: PlaybackEvent) {
        let _ = self.events.send(event);
    }

    fn emit_modes(&self) {
        self.emit(PlaybackEvent::ModeChanged {
            shuffle: self.queue.shuffle(),
            repeat: self.queue.repeat(),
        });
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::PlayQueue { items, start } => {
                self.queue.set(items, start);
                self.emit(PlaybackEvent::QueueChanged { len: self.queue.len() });
                self.play_current();
            }
            Command::AddToQueue(track) => {
                self.queue.push(track);
                self.emit(PlaybackEvent::QueueChanged { len: self.queue.len() });
                // Если ничего не играло и очередь была пуста, сразу запускаем
                if !self.loaded && !self.loading && self.queue.len() == 1 {
                    self.play_current();
                }
            }
            Command::TogglePause => {
                if self.loaded {
                    if self.player.is_paused() {
                        self.player.resume();
                    } else {
                        self.player.pause();
                    }
                    self.emit(PlaybackEvent::PauseChanged(self.player.is_paused()));
                } else if self.loading {
                    // Звука ещё нет: запоминаем, что трек надо будет запустить на паузе
                    self.start_paused = !self.start_paused;
                    self.emit(PlaybackEvent::PauseChanged(self.start_paused));
                }
            }
            Command::Next => {
                if self.queue.next().is_some() {
                    self.play_current();
                }
            }
            Command::Prev => {
                // Как в большинстве плееров: дольше 3 секунд играет, значит перезапуск того же трека
                if self.loaded && self.player.position() > Duration::from_secs(3) {
                    self.play_current();
                } else {
                    self.queue.prev();
                    self.play_current();
                }
            }
            Command::Seek(pos) => {
                // Пока трек грузится, перематывать нечего: команду пропускаем
                if self.loaded {
                    let pos = self.clamp_seek(pos);
                    if let Err(e) = self.player.seek(pos) {
                        self.emit(PlaybackEvent::Error(format!("перемотка не удалась: {e}")));
                    }
                }
            }
            Command::SetVolume(v) => {
                self.volume = v.clamp(0.0, 1.0); // clamp = «зажать в границы»
                self.apply_volume(); // громкость запомнится и для грузящегося трека
            }
            Command::SetSettings(settings) => {
                self.normalizer.set_settings(settings);
                if self.normalizer.settings().normalize_volume {
                    self.begin_analysis(); // включили на ходу: если замера ещё нет, начинаем
                } else {
                    self.cancel_analysis(); // выключили: считать больше незачем
                }
            }
            Command::SetShuffle(on) => {
                self.queue.set_shuffle(on);
                self.emit_modes();
            }
            Command::CycleRepeat => {
                let next_mode = self.queue.repeat().cycle();
                self.queue.set_repeat(next_mode);
                self.emit_modes();
            }
            Command::Stop => self.set_idle(),
            Command::LoadFinished { request, result } => self.on_load_finished(request, result),
            Command::StreamReady { request, part, dest, progress } => {
                self.on_stream_ready(request, part, dest, progress)
            }
            Command::StreamComplete { request, path } => self.on_stream_complete(request, path),
            Command::LoudnessReady { key, loudness } => self.on_loudness_ready(key, loudness),
            Command::PreloadFinished { request, result } => self.on_preload_finished(request, result),
            Command::Shutdown => {} // обрабатывается в run()
        }
    }

    /// Запуск трека по команде пользователя (или когда прошлый доиграл): счётчик неудач с нуля.
    fn play_current(&mut self) {
        self.fail_streak = 0;
        self.start_current();
    }

    /// Запустить трек, выбранный в очереди. Если он не играет, пропустить и взять следующий.
    /// Онлайн-трек только ставится на загрузку, функция при этом сразу возвращается.
    fn start_current(&mut self) {
        loop {
            self.cancel_loading(); // отменяем прошлую загрузку, если она ещё шла
            if self.start_paused {
                // Пауза, нажатая во время загрузки, относилась к прошлому треку: новый стартует
                // не на паузе (как и раньше). UI узнаёт об этом событием.
                self.start_paused = false;
                self.emit(PlaybackEvent::PauseChanged(false));
            }
            let Some(track) = self.queue.current().cloned() else { break };
            match self.begin_track(&track) {
                Begin::Playing(duration) => {
                    self.finish_start(track, duration);
                    return;
                }
                Begin::Loading => return, // дальше всё решит on_load_finished
                Begin::Failed(reason) => {
                    self.emit(PlaybackEvent::TrackFailed { track, reason });
                    if !self.advance_after_failure() {
                        break;
                    }
                }
            }
        }
        self.set_idle();
        self.emit(PlaybackEvent::QueueEnded);
    }

    /// После неудачи: можно ли переходить к следующему треку? Если подряд не запустилось
    /// столько треков, сколько в очереди (или очередь кончилась), останавливаемся:
    /// иначе при повторе очереди и пропавшем интернете цикл крутился бы бесконечно.
    fn advance_after_failure(&mut self) -> bool {
        self.fail_streak += 1;
        self.fail_streak < self.queue.len() && self.queue.next().is_some()
    }

    /// Начать запуск трека: локальный играет сразу, онлайн уходит в фоновую загрузку.
    fn begin_track(&mut self, track: &Track) -> Begin {
        let Some(provider) = self.providers.get(&track.provider).cloned() else {
            return Begin::Failed(format!("провайдер {:?} не подключён", track.provider));
        };

        // Локальный файл готов мгновенно: ждём прямо здесь, как раньше (без «загрузки» и крутилки).
        // block_on = «дождаться завершения асинхронной функции, заблокировав этот поток»
        if track.provider == ProviderKind::Local {
            let source = match self.runtime.block_on(provider.get_stream(track)) {
                Ok(source) => source,
                Err(e) => return Begin::Failed(e.to_string()),
            };
            return match source {
                StreamSource::LocalFile(path) => {
                    // Громкость нового трека ставим ДО старта, чтобы первые такты уже звучали как надо
                    self.prepare_gain(track);
                    match self.player.play_file(&path) {
                        Ok(duration) => {
                            self.apply_volume();
                            self.playing_file = Some(path);
                            Begin::Playing(duration)
                        }
                        Err(e) => Begin::Failed(e.to_string()),
                    }
                }
                other => Begin::Failed(format!("источник {other:?} пока не поддерживается")),
            };
        }

        // Онлайн-трек: качаем в фоне. Сначала проверяем, что есть чем качать.
        let downloader = match &self.downloader {
            Ok(d) => Arc::clone(d),
            Err(msg) => return Begin::Failed(format!("сетевой клиент не создан: {msg}")),
        };

        // Прошлый трек больше не играет: пользователь выбрал новый
        self.player.stop();
        self.loaded = false;
        self.prepare_gain(track); // если трек уже мерили, он зазвучит сразу с нужным усилением

        self.loading = true;
        self.loading_track = Some(track.clone());
        self.duration = Some(Duration::from_secs(u64::from(track.duration_secs)));
        let request = self.request; // номер уже увеличен в cancel_loading, он уникален

        // Сторож и всё нужное задаче переезжают в неё (`async move`)
        let guard = LoadGuard { tx: self.commands_tx.clone(), request, sent: false, preload: false };
        let cache = self.cache.clone();
        let task_track = track.clone();
        let handle = self.runtime.spawn(run_load(provider, downloader, cache, task_track, guard));
        self.load_task = Some(handle);

        self.emit(PlaybackEvent::Loading(track.clone()));
        Begin::Loading
    }

    /// Звук пошёл: фиксируем состояние и сообщаем UI.
    fn finish_start(&mut self, track: Track, duration: Option<Duration>) {
        self.loaded = true;
        self.fail_streak = 0;
        // Если декодер не знает длину, берём из метаданных трека
        self.duration = duration.or_else(|| Some(Duration::from_secs(u64::from(track.duration_secs))));
        self.emit(PlaybackEvent::TrackChanged(Some(track)));
        self.emit(PlaybackEvent::PauseChanged(self.player.is_paused()));
        self.begin_analysis(); // замер идёт в фоне, пока трек играет
    }

    /// Фоновая загрузка закончилась (успешно или с ошибкой).
    fn on_load_finished(&mut self, request: u64, result: Result<PathBuf, String>) {
        // Устаревший ответ: пока трек грузился, пользователь выбрал другой или нажал стоп
        if request != self.request || !self.loading {
            return;
        }
        self.loading = false;
        self.load_task = None;
        let Some(track) = self.loading_track.take() else {
            self.set_idle();
            return;
        };

        // Файл есть: пробуем включить. Ошибка декодера тоже считается неудачей трека.
        let outcome = match result {
            Ok(path) => match self.player.play_file(&path) {
                Ok(duration) => {
                    self.playing_file = Some(path.clone());
                    Ok(duration)
                }
                Err(e) => {
                    // Файл из нашего кэша мог скачаться битым: удаляем, чтобы не играть его снова
                    if self.cache.contains(&path) {
                        let _ = fs::remove_file(&path);
                    }
                    Err(e.to_string())
                }
            },
            Err(reason) => Err(reason),
        };
        self.finish_loaded(track, outcome);
    }

    /// Файл ещё докачивается, но запас скачан: начинаем играть.
    fn on_stream_ready(&mut self, request: u64, part: PathBuf, dest: PathBuf, progress: DownloadProgress) {
        // Устаревший ответ (трек уже сменили): фоновую задачу при смене отменили, ничего не делаем
        if request != self.request || !self.loading {
            return;
        }
        self.loading = false;
        // Ручку фоновой задачи НЕ сбрасываем: она ещё докачивает файл, и при смене трека её надо отменить
        let Some(track) = self.loading_track.take() else {
            self.set_idle();
            return;
        };
        let outcome = self.player.play_stream(&part, &dest, &progress).map_err(|e| e.to_string());
        if outcome.is_ok() {
            self.stream_progress = Some(progress);
            self.stream_error_reported = false;
        }
        self.finish_loaded(track, outcome);
    }

    /// Общий конец запуска онлайн-трека: звук пошёл или трек не удался (тогда пропускаем его).
    fn finish_loaded(&mut self, track: Track, outcome: Result<Option<Duration>, String>) {
        match outcome {
            Ok(duration) => {
                self.apply_volume();
                if self.start_paused {
                    self.player.pause(); // пользователь нажал паузу, пока трек грузился
                }
                self.start_paused = false;
                self.finish_start(track, duration);
            }
            Err(reason) => {
                self.emit(PlaybackEvent::TrackFailed { track, reason });
                if self.advance_after_failure() {
                    self.start_current();
                } else {
                    self.set_idle();
                    self.emit(PlaybackEvent::QueueEnded);
                }
            }
        }
    }

    /// Не даёт перемотать дальше скачанного: иначе вызов ждал бы данные и подвесил движок.
    fn clamp_seek(&self, pos: Duration) -> Duration {
        match &self.stream_progress {
            Some(progress) if !progress.is_finished() => {
                let total = self.duration.unwrap_or(Duration::ZERO);
                let downloaded = total.mul_f32(progress.fraction().unwrap_or(0.0));
                pos.min(downloaded.saturating_sub(SEEK_MARGIN))
            }
            _ => pos,
        }
    }

    /// Если докачка оборвалась во время игры, один раз сообщаем UI (трек доиграет скачанное и закончится).
    fn report_stream_error(&mut self) {
        if self.stream_error_reported {
            return;
        }
        if let Some(error) = self.stream_progress.as_ref().and_then(|p| p.error()) {
            self.stream_error_reported = true;
            self.emit(PlaybackEvent::Error(format!("загрузка трека оборвалась: {error}")));
        }
    }

    /// Отменить текущую загрузку (если она идёт) и объявить все её будущие ответы устаревшими.
    fn cancel_loading(&mut self) {
        if let Some(task) = self.load_task.take() {
            task.abort(); // прервать скачивание: на медленном интернете это экономит трафик
        }
        self.request += 1;
        self.loading = false;
        self.loading_track = None;
        self.stream_progress = None;
        self.playing_file = None;
        self.cancel_analysis(); // замер относился к прошлому треку
        self.drop_preload(); // следующий трек теперь может быть другим; sync_preload подготовит заново
    }

    /// Выставить громкость: ползунок пользователя на плеере и усиление нормализации в источнике
    /// (итоговая громкость = ползунок × усиление). Заодно то же для готового следующего трека.
    fn apply_volume(&self) {
        // Ползунок на плеере, усиление нормализации в источнике (см. `gain.rs`): итог тот же,
        // но у каждого трека своё усиление
        self.player.set_volume(self.volume);
        self.player.set_gain(self.normalizer.gain());
        // Следующий трек (если он готов во второй деке) звучит со своим усилением и тем же ползунком
        if let Some(Preload { key, state: PreloadState::Ready { .. } }) = &self.preload {
            self.player.set_preload_volume(self.volume);
            self.player.set_preload_gain(self.normalizer.gain_for_key(key));
        }
    }

    /// Новый трек вот-вот начнётся: узнать для него усиление (из кэша замеров) и поставить громкость.
    fn prepare_gain(&mut self, track: &Track) {
        self.normalizer.begin_track((track.provider, track.id.clone()));
        self.apply_volume();
    }

    /// Раз в цикл (≈100 мс): плавно подвинуть усиление к цели.
    fn update_gain(&mut self) {
        let now = Instant::now();
        // Потолок на случай, если поток надолго «заснул»: усиление не должно прыгнуть разом
        let dt = now.duration_since(self.last_gain_tick).min(Duration::from_millis(500));
        self.last_gain_tick = now;
        if self.normalizer.tick(dt).is_some() {
            self.apply_volume();
        }
    }

    /// Запустить фоновый замер громкости текущего трека, если он нужен и возможен: нормализация
    /// включена, замера ещё нет, целый файл известен и замер не идёт.
    fn begin_analysis(&mut self) {
        if self.analysis.is_some() || !self.normalizer.needs_measure() {
            return;
        }
        let (Some(path), Some(key)) = (self.playing_file.clone(), self.normalizer.current().cloned()) else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.analysis = Some(Arc::clone(&cancel));
        let tx = self.commands_tx.clone();
        // spawn_blocking: замер декодирует весь трек, это работа для отдельного потока, а не для async-задачи
        self.runtime.spawn_blocking(move || {
            let loudness = match loudness::measure_file(&path, &cancel) {
                Ok(Measured::Done(loudness)) => Some(loudness),
                Ok(Measured::Unmeasurable) => None,
                // Отменили или файл не прочитался: ничего не запоминаем, трек играет без изменений
                Ok(Measured::Cancelled) | Err(_) => return,
            };
            let _ = tx.send(Command::LoudnessReady { key, loudness });
        });
    }

    /// Остановить идущий замер (он сам заметит флаг и прекратится).
    fn cancel_analysis(&mut self) {
        if let Some(cancel) = self.analysis.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Трек, игравший во время докачки, докачан: теперь есть целый файл для замера.
    fn on_stream_complete(&mut self, request: u64, path: PathBuf) {
        if request != self.request {
            return; // трек уже сменили
        }
        self.playing_file = Some(path);
        self.begin_analysis();
    }

    /// Пришёл замер. Запоминаем всегда: он верен для своего трека, даже если сейчас играет другой.
    fn on_loudness_ready(&mut self, key: TrackKey, loudness: Option<Loudness>) {
        if self.normalizer.current() == Some(&key) {
            self.analysis = None;
        }
        if self.preload.as_ref().is_some_and(|p| p.key == key) {
            self.preload_analysis = None;
        }
        self.normalizer.record(key, loudness);
        self.apply_volume(); // если замерили следующий трек, его громкость во второй деке обновится
    }

    fn on_track_finished(&mut self) {
        self.loaded = false;
        if self.queue.track_finished().is_some() {
            // Следующий уже готов во второй деке? Тогда без загрузки, иначе как обычно
            if !self.start_from_preload() {
                self.play_current();
            }
        } else {
            self.set_idle();
            self.emit(PlaybackEvent::QueueEnded);
        }
    }

    /// Предзагрузка: следить, чтобы во второй деке был именно тот трек, что сыграет следующим.
    /// Зовётся каждый цикл (≈100 мс). Готовим только когда текущий трек целиком на диске: пока он
    /// докачивается, сеть не делим.
    fn sync_preload(&mut self) {
        let wanted = if self.loaded && !self.loading && self.playing_file.is_some() {
            self.queue.peek_next().cloned()
        } else {
            None
        };
        let wanted_key = wanted.as_ref().map(key_of);
        if self.preload.as_ref().map(|p| &p.key) == wanted_key.as_ref() {
            return; // уже готовим (или уже готов, или не удался) именно этот трек
        }
        self.drop_preload();
        if let Some(track) = wanted {
            self.start_preload(track);
        }
    }

    /// Выбросить предзагрузку: отменить скачивание и замер, освободить вторую деку.
    fn drop_preload(&mut self) {
        if let Some(task) = self.preload_task.take() {
            task.abort(); // DownloadGuard внутри задачи прервёт скачивание и уберёт `.part`
        }
        if let Some(cancel) = self.preload_analysis.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.preload_request += 1; // поздние ответы этой предзагрузки станут устаревшими
        if let Some(Preload { state: PreloadState::Ready { .. }, .. }) = self.preload.take() {
            self.player.discard_preloaded();
        }
    }

    /// Начать подготовку следующего трека. Локальный ставится во вторую деку сразу,
    /// онлайн скачивается в фоне целиком (ответ придёт командой `PreloadFinished`).
    fn start_preload(&mut self, track: Track) {
        let key = key_of(&track);
        let failed = Preload { key: key.clone(), state: PreloadState::Failed };
        let Some(provider) = self.providers.get(&track.provider).cloned() else {
            self.preload = Some(failed);
            return;
        };

        if track.provider == ProviderKind::Local {
            let state = match self.runtime.block_on(provider.get_stream(&track)) {
                Ok(StreamSource::LocalFile(path)) => self.install_preload(&path),
                _ => PreloadState::Failed,
            };
            self.preload = Some(Preload { key, state });
            self.after_preload_ready();
            return;
        }

        let downloader = match &self.downloader {
            Ok(d) => Arc::clone(d),
            Err(_) => {
                self.preload = Some(failed);
                return;
            }
        };
        let guard = LoadGuard { tx: self.commands_tx.clone(), request: self.preload_request, sent: false, preload: true };
        let handle = self.runtime.spawn(run_load(provider, downloader, self.cache.clone(), track, guard));
        self.preload_task = Some(handle);
        self.preload = Some(Preload { key, state: PreloadState::Loading });
    }

    /// Поставить готовый файл во вторую деку. Битый файл из нашего кэша удаляем (как для текущего трека).
    fn install_preload(&mut self, path: &std::path::Path) -> PreloadState {
        match self.player.preload_file(path) {
            Ok(duration) => PreloadState::Ready { path: path.to_path_buf(), duration },
            Err(_) => {
                if self.cache.contains(path) {
                    let _ = fs::remove_file(path);
                }
                PreloadState::Failed
            }
        }
    }

    /// Фоновая предзагрузка закончилась.
    fn on_preload_finished(&mut self, request: u64, result: Result<PathBuf, String>) {
        if request != self.preload_request {
            return; // следующий трек уже другой
        }
        self.preload_task = None;
        if !matches!(self.preload, Some(Preload { state: PreloadState::Loading, .. })) {
            return;
        }
        let state = match result {
            Ok(path) => self.install_preload(&path),
            Err(_) => PreloadState::Failed,
        };
        if let Some(preload) = self.preload.as_mut() {
            preload.state = state;
        }
        self.after_preload_ready();
    }

    /// Следующий трек встал во вторую деку: выставить его громкость и при нужде измерить его заранее.
    fn after_preload_ready(&mut self) {
        self.apply_volume();
        let Some(Preload { key, state: PreloadState::Ready { path, .. } }) = &self.preload else { return };
        // Если это тот же трек, что играет (повтор одного), его уже меряет обычный замер
        if self.preload_analysis.is_some()
            || !self.normalizer.wants_measure(key)
            || self.normalizer.current() == Some(key)
        {
            return;
        }
        let (path, key) = (path.clone(), key.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        self.preload_analysis = Some(Arc::clone(&cancel));
        let tx = self.commands_tx.clone();
        self.runtime.spawn_blocking(move || {
            let loudness = match loudness::measure_file(&path, &cancel) {
                Ok(Measured::Done(loudness)) => Some(loudness),
                Ok(Measured::Unmeasurable) => None,
                Ok(Measured::Cancelled) | Err(_) => return,
            };
            let _ = tx.send(Command::LoudnessReady { key, loudness });
        });
    }

    /// Трек доиграл сам и следующий (`queue.current()`) готов во второй деке: переключаемся на него.
    /// Возвращает false, если готового нет (тогда движок запускает трек как обычно).
    fn start_from_preload(&mut self) -> bool {
        let Some(track) = self.queue.current().cloned() else { return false };
        let key = key_of(&track);
        let ready = matches!(&self.preload, Some(p) if p.key == key && matches!(p.state, PreloadState::Ready { .. }));
        if !ready {
            return false;
        }
        let Some(Preload { state: PreloadState::Ready { path, duration }, .. }) = self.preload.take() else {
            return false;
        };
        // Замер следующего трека, если он ещё идёт, становится замером текущего
        let measuring = self.preload_analysis.take();
        self.preload_request += 1;
        self.fail_streak = 0;
        self.cancel_loading(); // номер запроса вперёд, сброс прошлого трека (предзагрузки уже нет)
        // Усиление ставим ДО переключения, чтобы первые такты звучали как надо
        self.normalizer.begin_track(key);
        self.player.set_preload_volume(self.volume);
        self.player.set_preload_gain(self.normalizer.gain());
        if !self.player.switch_to_preloaded() {
            return false;
        }
        self.playing_file = Some(path);
        self.analysis = measuring;
        self.finish_start(track, duration);
        true
    }

    fn set_idle(&mut self) {
        self.cancel_loading();
        self.start_paused = false;
        self.player.stop();
        self.loaded = false;
        self.duration = None;
        self.normalizer.end_track();
        self.apply_volume();
        self.emit(PlaybackEvent::TrackChanged(None));
    }

    fn publish_state(&self) {
        let snapshot = PlaybackState {
            // Пока грузится, показываем грузящийся трек, чтобы UI мог нарисовать его название
            current: if self.loaded {
                self.queue.current().cloned()
            } else if self.loading {
                self.loading_track.clone()
            } else {
                None
            },
            position: if self.loaded { self.player.position() } else { Duration::ZERO },
            duration: self.duration,
            is_playing: self.loaded && !self.player.is_paused(),
            is_loading: self.loading,
            volume: self.volume,
            shuffle: self.queue.shuffle(),
            repeat: self.queue.repeat(),
            queue_index: self.queue.index(),
            queue_len: self.queue.len(),
            download_progress: self
                .stream_progress
                .as_ref()
                .filter(|p| !p.is_finished())
                .and_then(|p| p.fraction()),
            normalization_gain: self.player.gain(), // то, что реально применено к звуку
        };
        *self.state.lock().unwrap() = snapshot; // lock() = взять замок, чтобы записать безопасно
    }
}

/// Чем кончилась фоновая задача загрузки.
enum Loaded {
    /// Файл целиком готов (из кэша, локальный или быстро скачался): отвечаем `LoadFinished`.
    File(PathBuf),
    /// Скачали, пока играл трек: ответ `StreamReady` ушёл раньше. Внутри: итоговый файл.
    Streamed(PathBuf),
}

/// Отменяет загрузку, если задачу прервали (`abort`): иначе вложенная задача скачивания
/// продолжила бы качать трек, который уже никому не нужен. После успешного конца «снимаем с поста»
/// (`disarm`), иначе отмена обрезала бы звук у играющего готового файла.
struct DownloadGuard {
    progress: DownloadProgress,
    task: tokio::task::JoinHandle<Result<(), mcrate_core::ProviderError>>,
    part: PathBuf, // недокачанный файл: при отмене его удаляем, чтобы не копить мусор в кэше
    armed: bool,
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        if self.armed {
            self.progress.cancel(); // читатель файла проснётся и закончит трек
            self.task.abort();
            // Ошибку игнорируем: файл мог быть уже удалён или (на других ОС) занят плеером
            let _ = fs::remove_file(&self.part);
        }
    }
}

/// Задача фоновой загрузки: запускает `load_online` и отвечает движку (трек или предзагрузка).
async fn run_load(
    provider: Arc<dyn MusicProvider>,
    downloader: Arc<StreamDownloader>,
    cache: StreamCache,
    track: Track,
    guard: LoadGuard,
) {
    match load_online(provider, downloader, cache, track, &guard).await {
        Ok(Loaded::File(path)) => guard.finish(Ok(path)),
        Ok(Loaded::Streamed(path)) => guard.stream_complete(path), // StreamReady уже ушёл; теперь файл целый
        Err(reason) => guard.finish(Err(reason)),
    }
}

/// Фоновая загрузка онлайн-трека. Идёт на рабочем потоке среды tokio, поток движка не блокирует.
/// Порядок: кэш -> ссылка от провайдера -> скачивание -> файл в кэше.
///
/// Скачивание идёт в отдельной вложенной задаче. Пока набирается `START_BYTES`, мы ждём;
/// набралось и загрузка ещё идёт: шлём движку `StreamReady` (пусть начинает играть), и дальше
/// ждём конца загрузки, чтобы переименовать файл и прибрать кэш.
async fn load_online(
    provider: Arc<dyn MusicProvider>,
    downloader: Arc<StreamDownloader>,
    cache: StreamCache,
    track: Track,
    guard: &LoadGuard,
) -> Result<Loaded, String> {
    // 1. Уже скачан раньше? Тогда без сети и без запроса ссылки (ссылки сервисов всё равно протухают)
    let stem = StreamCache::stem(&track);
    if let Some(hit) = cache.find(&stem) {
        return Ok(Loaded::File(hit));
    }

    // 2. Просим у провайдера источник звука
    let source = provider.get_stream(&track).await.map_err(|e| e.to_string())?;
    if let StreamSource::LocalFile(path) = source {
        return Ok(Loaded::File(path)); // провайдер уже держит файл на диске: качать нечего
    }

    // 3. Качаем во временный файл `.part` и переименовываем: недокачанный файл не примут за готовый
    let dest = cache.target(&stem, &source);
    let part = part_path(&dest);
    fs::create_dir_all(cache.dir()).map_err(|e| format!("папка кэша {}: {e}", cache.dir().display()))?;

    let progress = DownloadProgress::new();
    let task = {
        let (downloader, progress, part, source) = (Arc::clone(&downloader), progress.clone(), part.clone(), source);
        tokio::spawn(async move { downloader.download_progressive(&source, &part, &progress).await })
    };
    let mut download = DownloadGuard { progress: progress.clone(), task, part: part.clone(), armed: true };

    // 4. Ждём, пока скачается запас, загрузка не закончится или не оборвётся
    while progress.written() < START_BYTES && !progress.is_finished() && progress.error().is_none() {
        tokio::time::sleep(READY_POLL).await;
    }
    if let Some(error) = progress.error() {
        let _ = fs::remove_file(&part); // убрать недокачанное
        return Err(error);
    }

    // 5. Если загрузка ещё идёт, сообщаем движку: можно играть
    let streaming = !progress.is_finished();
    if streaming {
        guard.stream_ready(part.clone(), dest.clone(), progress.clone());
    }

    // 6. Дожидаемся конца загрузки (для быстрых и мелких файлов это сразу)
    let outcome = (&mut download.task).await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = fs::remove_file(&part);
            return Err(e.to_string());
        }
        Err(e) => {
            let _ = fs::remove_file(&part);
            return Err(format!("загрузка прервана: {e}"));
        }
    }
    download.armed = false; // всё скачано, отмена больше не нужна

    // 7. Переименовываем. На Windows открытый плеером файл переименовать можно (Rust открывает
    //    файлы с общим доступом), и плеер продолжит читать тот же файл.
    fs::rename(&part, &dest).map_err(|e| format!("не удалось сохранить {}: {e}", dest.display()))?;

    // 8. Подчищаем старые файлы кэша (только что скачанный не трогаем)
    cache.prune(&dest);
    Ok(if streaming { Loaded::Streamed(dest) } else { Loaded::File(dest) })
}

#[cfg(test)]
mod tests {
    //! Тесты движка на поддельном провайдере: он отдаёт готовые WAV-файлы с задержкой,
    //! как будто они скачиваются. Сеть не нужна. Файлы беззвучные (нули), так что на ПК
    //! ничего не прозвучит. Если звукового устройства нет, тесты молча пропускаются.

    use super::*;
    use async_trait::async_trait;
    use mcrate_core::{ProviderError, ProviderResult};
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    /// Тесты по очереди, а не параллельно: у каждого свой движок и своё звуковое устройство.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct FakeProvider {
        /// id трека -> (файл, задержка «загрузки» в миллисекундах). Нет id в списке = ошибка.
        files: HashMap<String, (PathBuf, u64)>,
        calls: AtomicUsize, // сколько раз у провайдера просили ссылку
    }

    #[async_trait]
    impl MusicProvider for FakeProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::SoundCloud // не Local, чтобы трек шёл через фоновую загрузку
        }
        async fn search(&self, _query: &str) -> ProviderResult<Vec<Track>> {
            Ok(vec![])
        }
        async fn get_stream(&self, track: &Track) -> ProviderResult<StreamSource> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.files.get(&track.id) {
                Some((path, delay_ms)) => {
                    tokio::time::sleep(Duration::from_millis(*delay_ms)).await; // «скачивание»
                    Ok(StreamSource::LocalFile(path.clone()))
                }
                None => Err(ProviderError::NotAuthorized),
            }
        }
        async fn set_like(&self, _track: &Track, _liked: bool) -> ProviderResult<()> {
            Ok(())
        }
    }

    fn track(id: &str) -> Track {
        Track { id: id.to_string(), provider: ProviderKind::SoundCloud, title: id.to_string(), artists: vec![], duration_secs: 60 }
    }

    /// Пишет беззвучный WAV (8 кГц, моно, 16 бит): 44 байта заголовка и нули.
    fn write_silent_wav(path: &Path, seconds: u32) {
        let data_len = 8000 * 2 * seconds; // частота * байт на отсчёт * секунды
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes()); // размер блока fmt
        bytes.extend_from_slice(&1u16.to_le_bytes()); // формат 1 = обычный PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // каналов: 1
        bytes.extend_from_slice(&8000u32.to_le_bytes()); // частота
        bytes.extend_from_slice(&16000u32.to_le_bytes()); // байт в секунду
        bytes.extend_from_slice(&2u16.to_le_bytes()); // байт на отсчёт
        bytes.extend_from_slice(&16u16.to_le_bytes()); // бит на отсчёт
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.resize(bytes.len() + data_len as usize, 0);
        fs::write(path, bytes).unwrap();
    }

    /// Пишет WAV с синусом 440 Гц (8 кГц, моно, 16 бит), чтобы у трека была измеримая громкость.
    fn write_tone_wav(path: &Path, seconds: u32, amplitude: f32) {
        let frames = 8000 * seconds;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + frames * 2).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // моно
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(frames * 2).to_le_bytes());
        for n in 0..frames {
            let v = amplitude * (std::f32::consts::TAU * 440.0 * n as f32 / 8000.0).sin();
            bytes.extend_from_slice(&((v * 32767.0) as i16).to_le_bytes());
        }
        fs::write(path, bytes).unwrap();
    }

    /// Ждёт, пока усиление нормализации не удовлетворит условию (не дольше `timeout_ms`).
    fn wait_gain(fx: &mut Fixture, timeout_ms: u64, pred: impl Fn(f32) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            if pred(fx.engine.state().normalization_gain) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            fx.idle(50);
        }
    }

    /// Готовая «обстановка» теста: поддельный провайдер, движок и временная папка.
    struct Fixture {
        engine: PlaybackEngine,
        fake: Arc<FakeProvider>,
        dir: PathBuf,
        log: Vec<PlaybackEvent>, // все события, которые мы уже забрали у движка
    }

    impl Fixture {
        /// `tracks`: (id, задержка в мс). Ещё один id без файла можно использовать как «битый».
        /// Возвращает None, если нет звукового устройства (тест пропускаем).
        fn new(name: &str, tracks: &[(&str, u64)]) -> Option<Self> {
            Self::build(name, tracks, |wav| write_silent_wav(wav, 30))
        }

        /// То же, но все треки это громкий синус (амплитуда `amplitude`, 20 с): есть что измерять.
        fn with_tone(name: &str, tracks: &[(&str, u64)], amplitude: f32) -> Option<Self> {
            Self::build(name, tracks, |wav| write_tone_wav(wav, 20, amplitude))
        }

        /// Движок с чужим провайдером (например, раздающим файл по HTTP с задержкой).
        fn with_provider(name: &str, provider: Arc<dyn MusicProvider>) -> Option<Self> {
            let dir = std::env::temp_dir().join(format!("mcrate-engine-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let fake = Arc::new(FakeProvider { files: HashMap::new(), calls: AtomicUsize::new(0) });
            match PlaybackEngine::with_stream_cache(vec![provider], dir.join("cache")) {
                Ok(engine) => Some(Self { engine, fake, dir, log: Vec::new() }),
                Err(e) => {
                    eprintln!("тест пропущен: нет звукового устройства ({e})");
                    let _ = fs::remove_dir_all(&dir);
                    None
                }
            }
        }

        fn build(name: &str, tracks: &[(&str, u64)], write_wav: impl Fn(&Path)) -> Option<Self> {
            let dir = std::env::temp_dir().join(format!("mcrate-engine-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let wav = dir.join("silence.wav");
            write_wav(&wav);
            let files = tracks.iter().map(|(id, delay)| (id.to_string(), (wav.clone(), *delay))).collect();
            let fake = Arc::new(FakeProvider { files, calls: AtomicUsize::new(0) });
            let provider: Arc<dyn MusicProvider> = fake.clone();
            match PlaybackEngine::with_stream_cache(vec![provider], dir.join("cache")) {
                Ok(engine) => Some(Self { engine, fake, dir, log: Vec::new() }),
                Err(e) => {
                    eprintln!("тест пропущен: нет звукового устройства ({e})");
                    let _ = fs::remove_dir_all(&dir);
                    None
                }
            }
        }

        /// Ждёт событие, подходящее под условие (не дольше `timeout_ms`). Все события пишет в `log`.
        fn wait_for(&mut self, timeout_ms: u64, pred: impl Fn(&PlaybackEvent) -> bool) -> bool {
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            loop {
                let mut found = false;
                for event in self.engine.poll_events() {
                    found |= pred(&event);
                    self.log.push(event);
                }
                if found {
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                thread::sleep(Duration::from_millis(5));
            }
        }

        /// Просто ждём и копим события.
        fn idle(&mut self, ms: u64) {
            self.wait_for(ms, |_| false);
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn is_loading(id: &'static str) -> impl Fn(&PlaybackEvent) -> bool {
        move |e| matches!(e, PlaybackEvent::Loading(t) if t.id == id)
    }
    fn is_started(id: &'static str) -> impl Fn(&PlaybackEvent) -> bool {
        move |e| matches!(e, PlaybackEvent::TrackChanged(Some(t)) if t.id == id)
    }

    #[test]
    fn commands_work_while_track_is_loading() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::new("loading", &[("slow", 700)]) else { return };

        fx.engine.play_queue(vec![track("slow")], 0);
        assert!(fx.wait_for(1000, is_loading("slow")), "нет события Loading");
        fx.idle(150); // пусть снимок состояния обновится (он обновляется раз в 100 мс)
        let st = fx.engine.state();
        assert!(st.is_loading && !st.is_playing);
        assert_eq!(st.current.as_ref().map(|t| t.id.as_str()), Some("slow"));

        // Загрузка идёт ещё ~500 мс. Пауза и громкость должны отозваться раньше, чем она кончится
        fx.engine.set_volume(0.25);
        fx.engine.toggle_pause();
        assert!(fx.wait_for(300, |e| matches!(e, PlaybackEvent::PauseChanged(true))), "пауза не отозвалась во время загрузки");
        fx.idle(150);
        let st = fx.engine.state();
        assert!(st.is_loading, "загрузка должна ещё идти");
        assert!((st.volume - 0.25).abs() < 1e-6);

        // Трек догрузился и стартовал на паузе, как просил пользователь
        assert!(fx.wait_for(3000, is_started("slow")), "трек так и не стартовал");
        fx.idle(250);
        let st = fx.engine.state();
        assert!(!st.is_loading && !st.is_playing);
        assert_eq!(st.current.as_ref().map(|t| t.id.as_str()), Some("slow"));
    }

    #[test]
    fn stale_load_result_is_discarded() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::new("stale", &[("a", 600), ("b", 0)]) else { return };

        fx.engine.play_queue(vec![track("a"), track("b")], 0);
        assert!(fx.wait_for(1000, is_loading("a")));
        fx.engine.next(); // не дожидаясь «a», просим следующий трек
        assert!(fx.wait_for(3000, is_started("b")), "второй трек не стартовал");
        fx.idle(1000); // даём «a» время (если бы он не был отменён) вылезти с запозданием

        let a_started = fx.log.iter().any(|e| is_started("a")(e));
        let failed = fx.log.iter().any(|e| matches!(e, PlaybackEvent::TrackFailed { .. }));
        assert!(!a_started, "устаревший трек «a» не должен был начать играть");
        assert!(!failed);
        assert_eq!(fx.fake.calls.load(Ordering::SeqCst), 2); // ссылку просили для обоих
    }

    #[test]
    fn failed_online_track_is_skipped() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::new("skip", &[("good", 0)]) else { return };

        // «missing» в списке файлов нет, провайдер вернёт ошибку
        fx.engine.play_queue(vec![track("missing"), track("good")], 0);
        assert!(fx.wait_for(3000, is_started("good")), "после ошибки должен играть следующий");
        let failed_at = fx.log.iter().position(|e| matches!(e, PlaybackEvent::TrackFailed { track, .. } if track.id == "missing"));
        let started_at = fx.log.iter().position(|e| is_started("good")(e));
        assert!(failed_at.is_some() && failed_at < started_at, "TrackFailed должен прийти раньше старта следующего");
    }

    #[test]
    fn all_failing_queue_stops_instead_of_looping() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::new("loop", &[]) else { return };

        fx.engine.cycle_repeat(); // Off -> All: очередь по кругу, самый опасный случай
        fx.engine.play_queue(vec![track("x"), track("y")], 0);
        assert!(fx.wait_for(3000, |e| matches!(e, PlaybackEvent::QueueEnded)), "очередь должна остановиться");
        fx.idle(500); // если бы цикл не остановился, ссылок бы запросили намного больше
        let failures = fx.log.iter().filter(|e| matches!(e, PlaybackEvent::TrackFailed { .. })).count();
        assert_eq!(failures, 2);
        assert_eq!(fx.fake.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn pause_during_loading_does_not_leak_into_next_track() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::new("leak", &[("a", 600), ("b", 0)]) else { return };

        fx.engine.play_queue(vec![track("a"), track("b")], 0);
        assert!(fx.wait_for(1000, is_loading("a")));
        fx.engine.toggle_pause();
        assert!(fx.wait_for(500, |e| matches!(e, PlaybackEvent::PauseChanged(true))));
        fx.engine.next(); // переключаемся, не дождавшись «a»
        assert!(fx.wait_for(3000, is_started("b")));
        fx.idle(100);
        // После паузы (true) до старта «b» обязано прийти снятие паузы (false)
        let pause_true = fx.log.iter().position(|e| matches!(e, PlaybackEvent::PauseChanged(true))).unwrap();
        let pause_false = fx.log.iter().position(|e| matches!(e, PlaybackEvent::PauseChanged(false)));
        let started_b = fx.log.iter().position(|e| is_started("b")(e)).unwrap();
        assert!(pause_false.is_some_and(|i| i > pause_true && i <= started_b + 1), "«b» должен стартовать не на паузе");
    }

    #[test]
    fn stop_cancels_loading() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::new("stop", &[("slow", 500)]) else { return };

        fx.engine.play_queue(vec![track("slow")], 0);
        assert!(fx.wait_for(1000, is_loading("slow")));
        fx.engine.stop();
        assert!(fx.wait_for(1000, |e| matches!(e, PlaybackEvent::TrackChanged(None))));
        fx.idle(900); // загрузка «закончилась бы» в это время
        assert!(!fx.log.iter().any(|e| matches!(e, PlaybackEvent::TrackChanged(Some(_)))), "после стопа звук включаться не должен");
        let st = fx.engine.state();
        assert!(!st.is_loading && !st.is_playing && st.current.is_none());
    }

    /// Провайдер, отдающий ссылку на локальный HTTP-сервер: трек идёт по пути «играем во время докачки».
    struct HttpProvider {
        url: String,
    }

    #[async_trait]
    impl MusicProvider for HttpProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::SoundCloud
        }
        async fn search(&self, _query: &str) -> ProviderResult<Vec<Track>> {
            Ok(vec![])
        }
        async fn get_stream(&self, _track: &Track) -> ProviderResult<StreamSource> {
            Ok(StreamSource::DirectUrl(self.url.clone()))
        }
        async fn set_like(&self, _track: &Track, _liked: bool) -> ProviderResult<()> {
            Ok(())
        }
    }

    /// Поднимает на localhost сервер, который отдаёт `body` в два приёма: сразу первые `first` байт,
    /// остальное через `pause`. Так загрузка трека заведомо ещё идёт, когда он начал играть.
    /// Возвращает ссылку. Поток сервера живёт до конца тестового процесса.
    fn serve_slowly(body: Vec<u8>, first: usize, pause: Duration) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let body = body.clone();
                thread::spawn(move || {
                    let mut request = [0u8; 2048];
                    let _ = stream.read(&mut request);
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    // Ошибки записи игнорируем: клиент мог отменить загрузку
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&body[..first]);
                    let _ = stream.flush();
                    thread::sleep(pause);
                    let _ = stream.write_all(&body[first..]);
                });
            }
        });
        format!("http://127.0.0.1:{port}/track.wav")
    }

    /// Громкий синус 0,8 (моно) даёт примерно -2,5 LUFS; до цели -14 надо ослабить до ≈0,27.
    fn is_attenuated(gain: f32) -> bool {
        (0.2..0.35).contains(&gain)
    }

    /// Запускает трек сразу на паузе (пауза во время загрузки). Нормализации воспроизведение
    /// не нужно: замер читает файл сам, а усиление меняется по часам. Зато трек не «доиграется»
    /// раньше проверки (на виртуальном звуковом устройстве звук уходит быстрее реального времени).
    fn play_paused(fx: &mut Fixture, id: &'static str) {
        fx.engine.play_queue(vec![track(id)], 0);
        assert!(fx.wait_for(1000, is_loading(id)), "нет события Loading");
        fx.engine.toggle_pause();
        assert!(fx.wait_for(3000, is_started(id)), "трек не стартовал");
    }

    #[test]
    fn normalization_attenuates_loud_track_and_remembers_it() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::with_tone("norm-on", &[("loud", 300)], 0.8) else { return };

        fx.engine.set_settings(PlaybackSettings { normalize_volume: true, ..PlaybackSettings::default() });
        play_paused(&mut fx, "loud");
        // Сначала усиление 1.0 (замера ещё нет), потом замер заканчивается и громкость плавно падает
        assert!(
            wait_gain(&mut fx, 6000, is_attenuated),
            "усиление не пришло к ослаблению: {} (события: {:?})",
            fx.engine.state().normalization_gain,
            fx.log
        );

        // Тот же трек ещё раз: замер уже есть, стартуем сразу с нужным усилением, без «подъезда» от 1.0
        play_paused(&mut fx, "loud");
        fx.idle(150); // снимок состояния обновляется раз в 100 мс
        assert!(is_attenuated(fx.engine.state().normalization_gain), "повторный запуск стартовал без запомненного усиления");
    }

    #[test]
    fn normalization_disabled_leaves_gain_alone() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::with_tone("norm-off", &[("loud", 300)], 0.8) else { return };

        play_paused(&mut fx, "loud"); // настройки по умолчанию: нормализация выключена
        fx.idle(2500); // с запасом больше, чем нужно замеру и плавному изменению
        assert_eq!(fx.engine.state().normalization_gain, 1.0);
    }

    #[test]
    fn turning_normalization_off_restores_gain() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::with_tone("norm-toggle", &[("loud", 300)], 0.8) else { return };

        fx.engine.set_settings(PlaybackSettings { normalize_volume: true, ..PlaybackSettings::default() });
        play_paused(&mut fx, "loud");
        assert!(wait_gain(&mut fx, 6000, is_attenuated));

        fx.engine.set_settings(PlaybackSettings::default()); // выключили на ходу
        assert!(
            wait_gain(&mut fx, 6000, |g| g > 0.999),
            "усиление не вернулось к 1.0: {}",
            fx.engine.state().normalization_gain
        );
    }

    #[test]
    fn enabling_normalization_mid_track_measures_the_playing_track() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::with_tone("norm-mid", &[("loud", 300)], 0.8) else { return };

        play_paused(&mut fx, "loud"); // нормализация выключена
        fx.idle(300);
        assert_eq!(fx.engine.state().normalization_gain, 1.0);

        fx.engine.set_settings(PlaybackSettings { normalize_volume: true, ..PlaybackSettings::default() });
        assert!(wait_gain(&mut fx, 6000, is_attenuated), "включение на ходу не запустило замер: {}", fx.engine.state().normalization_gain);
    }

    #[test]
    fn normalization_waits_for_streamed_track_to_finish_downloading() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let src = std::env::temp_dir().join(format!("mcrate-engine-test-{}-stream-src.wav", std::process::id()));
        write_tone_wav(&src, 20, 0.8); // ≈320 КБ
        let body = fs::read(&src).unwrap();
        let _ = fs::remove_file(&src);
        // Первые 160 КБ (больше START_BYTES) сразу, остальное через 2,5 с
        let url = serve_slowly(body, 160 * 1024, Duration::from_millis(2500));
        let Some(mut fx) = Fixture::with_provider("norm-stream", Arc::new(HttpProvider { url })) else { return };

        fx.engine.set_settings(PlaybackSettings { normalize_volume: true, ..PlaybackSettings::default() });
        play_paused(&mut fx, "s");

        // Трек играет, а докачка ещё идёт: целого файла нет, мерить рано, усиление не трогаем
        fx.idle(800);
        let st = fx.engine.state();
        assert!(st.download_progress.is_some(), "трек должен ещё докачиваться (иначе тест проверяет не тот путь)");
        assert_eq!(st.normalization_gain, 1.0, "замер не должен начинаться до конца докачки");

        // Докачалось: замер целого файла и плавное ослабление
        assert!(
            wait_gain(&mut fx, 10_000, is_attenuated),
            "после докачки замер не сработал: {} (события: {:?})",
            fx.engine.state().normalization_gain,
            fx.log
        );
    }

    // ----- предзагрузка следующего трека -----

    /// Очередь из нескольких треков, первый стартует сразу на паузе (см. `play_paused`).
    fn play_queue_paused(fx: &mut Fixture, ids: &[&'static str]) {
        let first = ids[0];
        fx.engine.play_queue(ids.iter().map(|id| track(id)).collect(), 0);
        assert!(fx.wait_for(1000, is_loading(first)), "нет события Loading");
        fx.engine.toggle_pause();
        assert!(fx.wait_for(3000, is_started(first)), "трек не стартовал");
    }

    /// Ждёт, пока у провайдера не попросят ссылку `n` раз (первая просьба за текущий трек,
    /// вторая за предзагрузку следующего).
    fn wait_calls(fx: &mut Fixture, calls: &AtomicUsize, n: usize, timeout_ms: u64) -> bool {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while calls.load(Ordering::SeqCst) < n {
            if Instant::now() >= deadline {
                return false;
            }
            fx.idle(20);
        }
        true
    }

    #[test]
    fn next_track_is_preloaded_while_current_plays() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::new("pre-basic", &[("a", 300), ("b", 300)]) else { return };
        let fake = Arc::clone(&fx.fake);

        play_queue_paused(&mut fx, &["a", "b"]);
        assert!(wait_calls(&mut fx, &fake.calls, 2, 3000), "следующий трек не запрошен заранее");
        // Один раз, а не каждый цикл движка
        fx.idle(500);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2, "предзагрузка повторяется");
    }

    #[test]
    fn nothing_is_preloaded_at_end_of_queue_until_a_track_is_added() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::new("pre-end", &[("a", 300), ("b", 300)]) else { return };
        let fake = Arc::clone(&fx.fake);

        play_queue_paused(&mut fx, &["a"]);
        fx.idle(600);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1, "после последнего трека готовить нечего");

        fx.engine.add_to_queue(track("b")); // теперь у «a» есть следующий
        assert!(wait_calls(&mut fx, &fake.calls, 2, 3000), "добавленный в конец трек не предзагрузился");
    }

    #[test]
    fn preload_follows_the_queue_mode() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::new("pre-mode", &[("a", 300), ("b", 300)]) else { return };
        let fake = Arc::clone(&fx.fake);

        play_queue_paused(&mut fx, &["a", "b"]);
        assert!(wait_calls(&mut fx, &fake.calls, 2, 3000));

        // Повтор один трек (Off -> All -> One): после «a» снова «a», предзагруженный «b» не подходит
        fx.engine.cycle_repeat();
        fx.engine.cycle_repeat();
        assert!(wait_calls(&mut fx, &fake.calls, 3, 3000), "режим сменился, а предзагрузка осталась старой");
    }

    #[test]
    fn failed_preload_is_not_retried_every_tick() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // У «bad» нет файла: провайдер отвечает ошибкой
        let Some(mut fx) = Fixture::new("pre-fail", &[("a", 300)]) else { return };
        let fake = Arc::clone(&fx.fake);

        play_queue_paused(&mut fx, &["a", "bad"]);
        assert!(wait_calls(&mut fx, &fake.calls, 2, 3000));
        fx.idle(800);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2, "неудачную предзагрузку нельзя повторять каждый цикл");
    }

    #[test]
    fn natural_end_switches_to_preloaded_track_without_loading() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut fx) = Fixture::new("pre-switch", &[("a", 300), ("b", 300)]) else { return };
        let fake = Arc::clone(&fx.fake);

        play_queue_paused(&mut fx, &["a", "b"]);
        assert!(wait_calls(&mut fx, &fake.calls, 2, 3000));
        fx.idle(800); // «b» «скачивается» 300 мс и встаёт во вторую деку

        fx.engine.toggle_pause(); // «a» играет и скоро доиграет сам
        assert!(fx.wait_for(5000, |e| matches!(e, PlaybackEvent::TrackChanged(Some(t)) if t.id == "b")), "«b» не заиграл (события: {:?})", fx.log);
        assert!(
            !fx.log.iter().any(|e| matches!(e, PlaybackEvent::Loading(t) if t.id == "b")),
            "предзагруженный трек прошёл через загрузку (события: {:?})",
            fx.log
        );
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2, "ссылку на «b» запросили второй раз");
    }

    #[test]
    fn next_track_loudness_is_measured_before_it_starts() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // «a» громкий, «b» тихий: усиление у них очень разное, значит стартовое значение «b» не спутать с хвостом «a»
        let pid = std::process::id();
        let loud = std::env::temp_dir().join(format!("mcrate-engine-test-{pid}-ahead-a.wav"));
        let quiet = std::env::temp_dir().join(format!("mcrate-engine-test-{pid}-ahead-b.wav"));
        write_tone_wav(&loud, 20, 0.8);
        write_tone_wav(&quiet, 20, 0.05);
        let files = HashMap::from([("a".to_string(), (loud.clone(), 300u64)), ("b".to_string(), (quiet.clone(), 300u64))]);
        let fake = Arc::new(FakeProvider { files, calls: AtomicUsize::new(0) });
        let fx = Fixture::with_provider("norm-ahead", fake.clone());
        let Some(mut fx) = fx else {
            let _ = (fs::remove_file(&loud), fs::remove_file(&quiet));
            return;
        };

        fx.engine.set_settings(PlaybackSettings { normalize_volume: true, ..PlaybackSettings::default() });
        play_queue_paused(&mut fx, &["a", "b"]);
        assert!(wait_gain(&mut fx, 6000, is_attenuated), "«a» не измерился");
        assert!(wait_calls(&mut fx, &fake.calls, 2, 3000));
        fx.idle(2500); // «b» «скачивается», ставится во вторую деку и меряется, пока «a» на паузе

        fx.engine.toggle_pause(); // «a» доигрывает, начинается «b»
        assert!(fx.wait_for(5000, |e| matches!(e, PlaybackEvent::TrackChanged(Some(t)) if t.id == "b")));
        // Без заблаговременного замера усиление стартовало бы с 1.0 и ползло бы к цели секунды (6 дБ/с)
        assert!(
            wait_gain(&mut fx, 400, |g| g > 3.0),
            "«b» стартовал без своего усиления: {}",
            fx.engine.state().normalization_gain
        );
        let _ = (fs::remove_file(&loud), fs::remove_file(&quiet));
    }
}

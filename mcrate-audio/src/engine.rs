//! PlaybackEngine: единая точка входа в звук. Живёт в своём потоке.
//!
//! Онлайн-треки (SoundCloud и другие) грузятся в ФОНЕ: пока трек скачивается, движок
//! продолжает принимать команды (пауза, громкость, «следующий»). Готовый файл возвращается
//! в поток движка внутренней командой `LoadFinished` с «номером запроса»: если пользователь
//! успел переключить трек, результат устарел и выбрасывается.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex}; // mpsc = канал сообщений между потоками
use std::thread::{self, JoinHandle};
use std::time::Duration;

use mcrate_core::{MusicProvider, ProviderKind, StreamSource, Track};
use mcrate_net::StreamDownloader;

use crate::stream_cache::{part_path, StreamCache};
use crate::{AudioError, AudioPlayer, PlayQueue, RepeatMode};

/// Сколько последних скачанных онлайн-треков хранить в кэше (остальные удаляются).
const MAX_CACHED_TRACKS: usize = 20;

/// Команды от UI к движку (внутренние, UI вызывает методы ниже).
enum Command {
    PlayQueue { items: Vec<Track>, start: usize },
    AddToQueue(Track),
    TogglePause,
    Next,
    Prev,
    Seek(Duration),
    SetVolume(f32),
    SetShuffle(bool),
    CycleRepeat,
    Stop,
    Shutdown,
    /// Внутренняя: фоновая загрузка закончилась. `request` = номер запроса, на который это ответ.
    /// Эту команду шлёт сам движок (из фоновой задачи), UI её не видит.
    LoadFinished { request: u64, result: Result<PathBuf, String> },
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
        }
    }
}

/// Сторона UI: отправляет команды и читает события и снимок.
pub struct PlaybackEngine {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<PlaybackEvent>,
    state: Arc<Mutex<PlaybackState>>, // общий снимок: движок пишет, UI читает
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
        // Копия отправителя команд для самого движка: через неё фоновые задачи
        // возвращают результат загрузки в поток движка
        let worker_tx = cmd_tx.clone();

        // `move` отдаёт замыканию владение переменными: оно работает в другом потоке
        let handle = thread::spawn(move || {
            // Устройство вывода и среду для async открываем ВНУТРИ потока.
            // new_multi_thread = у среды свои рабочие потоки, поэтому фоновые задачи
            // (скачивание) идут сами, пока поток движка занят командами. Двух потоков хватит.
            let init = AudioPlayer::new().map_err(|e| e.to_string()).and_then(|player| {
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
            }
            .run(cmd_rx);
        });

        // Ждём, пока поток сообщит, удалось ли открыть звук
        match init_rx.recv() {
            Ok(Ok(())) => Ok(Self { commands: cmd_tx, events: evt_rx, state, thread: Some(handle) }),
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
}

impl LoadGuard {
    /// Отправить настоящий результат (сторож после этого молчит).
    fn finish(mut self, result: Result<PathBuf, String>) {
        self.sent = true;
        let _ = self.tx.send(Command::LoadFinished { request: self.request, result });
    }
}

impl Drop for LoadGuard {
    fn drop(&mut self) {
        if !self.sent {
            let _ = self.tx.send(Command::LoadFinished {
                request: self.request,
                result: Err("загрузка прервана".to_string()),
            });
        }
    }
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
                    if let Err(e) = self.player.seek(pos) {
                        self.emit(PlaybackEvent::Error(format!("перемотка не удалась: {e}")));
                    }
                }
            }
            Command::SetVolume(v) => {
                self.volume = v.clamp(0.0, 1.0); // clamp = «зажать в границы»
                self.player.set_volume(self.volume); // громкость запомнится и для грузящегося трека
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
                StreamSource::LocalFile(path) => match self.player.play_file(&path) {
                    Ok(duration) => {
                        self.player.set_volume(self.volume);
                        Begin::Playing(duration)
                    }
                    Err(e) => Begin::Failed(e.to_string()),
                },
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

        self.loading = true;
        self.loading_track = Some(track.clone());
        self.duration = Some(Duration::from_secs(u64::from(track.duration_secs)));
        let request = self.request; // номер уже увеличен в cancel_loading, он уникален

        // Сторож и всё нужное задаче переезжают в неё (`async move`)
        let guard = LoadGuard { tx: self.commands_tx.clone(), request, sent: false };
        let cache = self.cache.clone();
        let task_track = track.clone();
        let handle = self.runtime.spawn(async move {
            let result = load_online(provider, downloader, cache, task_track).await;
            guard.finish(result);
        });
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
                Ok(duration) => Ok(duration),
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

        match outcome {
            Ok(duration) => {
                self.player.set_volume(self.volume);
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

    /// Отменить текущую загрузку (если она идёт) и объявить все её будущие ответы устаревшими.
    fn cancel_loading(&mut self) {
        if let Some(task) = self.load_task.take() {
            task.abort(); // прервать скачивание: на медленном интернете это экономит трафик
        }
        self.request += 1;
        self.loading = false;
        self.loading_track = None;
    }

    fn on_track_finished(&mut self) {
        self.loaded = false;
        if self.queue.track_finished().is_some() {
            self.play_current();
        } else {
            self.set_idle();
            self.emit(PlaybackEvent::QueueEnded);
        }
    }

    fn set_idle(&mut self) {
        self.cancel_loading();
        self.start_paused = false;
        self.player.stop();
        self.loaded = false;
        self.duration = None;
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
        };
        *self.state.lock().unwrap() = snapshot; // lock() = взять замок, чтобы записать безопасно
    }
}

/// Фоновая загрузка онлайн-трека. Идёт на рабочем потоке среды tokio, поток движка не блокирует.
/// Порядок: кэш -> ссылка от провайдера -> скачивание -> файл в кэше. Возвращает путь к файлу.
async fn load_online(
    provider: Arc<dyn MusicProvider>,
    downloader: Arc<StreamDownloader>,
    cache: StreamCache,
    track: Track,
) -> Result<PathBuf, String> {
    // 1. Уже скачан раньше? Тогда без сети и без запроса ссылки (ссылки сервисов всё равно протухают)
    let stem = StreamCache::stem(&track);
    if let Some(hit) = cache.find(&stem) {
        return Ok(hit);
    }

    // 2. Просим у провайдера источник звука
    let source = provider.get_stream(&track).await.map_err(|e| e.to_string())?;
    if let StreamSource::LocalFile(path) = source {
        return Ok(path); // провайдер уже держит файл на диске: качать нечего
    }

    // 3. Качаем во временный файл `.part` и переименовываем: недокачанный файл не примут за готовый
    let dest = cache.target(&stem, &source);
    let part = part_path(&dest);
    fs::create_dir_all(cache.dir()).map_err(|e| format!("папка кэша {}: {e}", cache.dir().display()))?;
    if let Err(e) = downloader.download_to_file(&source, &part).await {
        let _ = fs::remove_file(&part); // убрать недокачанное
        return Err(e.to_string());
    }
    fs::rename(&part, &dest).map_err(|e| format!("не удалось сохранить {}: {e}", dest.display()))?;

    // 4. Подчищаем старые файлы кэша (только что скачанный не трогаем)
    cache.prune(&dest);
    Ok(dest)
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
            let dir = std::env::temp_dir().join(format!("mcrate-engine-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let wav = dir.join("silence.wav");
            write_silent_wav(&wav, 30);
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
}

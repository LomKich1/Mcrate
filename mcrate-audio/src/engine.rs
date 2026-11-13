//! PlaybackEngine: единая точка входа в звук. Живёт в своём потоке.

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex}; // mpsc = канал сообщений между потоками
use std::thread::{self, JoinHandle};
use std::time::Duration;

use mcrate_core::{MusicProvider, ProviderKind, StreamSource, Track};

use crate::{AudioError, AudioPlayer, PlayQueue, RepeatMode};

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
}

/// События от движка к UI: только дискретные изменения.
#[derive(Debug, Clone)]
pub enum PlaybackEvent {
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
    pub current: Option<Track>,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub is_playing: bool,
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
    pub fn new(providers: Vec<Arc<dyn MusicProvider>>) -> Result<Self, AudioError> {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (evt_tx, evt_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::channel::<Result<(), String>>();
        let state = Arc::new(Mutex::new(PlaybackState::initial()));
        let thread_state = Arc::clone(&state);

        // `move` отдаёт замыканию владение переменными: оно работает в другом потоке
        let handle = thread::spawn(move || {
            // Устройство вывода и среду для async открываем ВНУТРИ потока
            let init = AudioPlayer::new().map_err(|e| e.to_string()).and_then(|player| {
                tokio::runtime::Builder::new_current_thread()
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
                queue: PlayQueue::new(),
                state: thread_state,
                events: evt_tx,
                volume: 1.0,
                loaded: false,
                duration: None,
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

/// Сторона потока движка: здесь живёт вся логика.
struct Worker {
    player: AudioPlayer,
    runtime: tokio::runtime::Runtime,
    providers: HashMap<ProviderKind, Arc<dyn MusicProvider>>,
    queue: PlayQueue<Track>,
    state: Arc<Mutex<PlaybackState>>,
    events: mpsc::Sender<PlaybackEvent>,
    volume: f32,
    loaded: bool,               // true, пока какой-то трек загружен (играет или на паузе)
    duration: Option<Duration>, // длина загруженного трека
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
        self.player.stop();
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
                if !self.loaded && self.queue.len() == 1 {
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
                if self.loaded {
                    if let Err(e) = self.player.seek(pos) {
                        self.emit(PlaybackEvent::Error(format!("перемотка не удалась: {e}")));
                    }
                }
            }
            Command::SetVolume(v) => {
                self.volume = v.clamp(0.0, 1.0); // clamp = «зажать в границы»
                self.player.set_volume(self.volume);
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
            Command::Shutdown => {} // обрабатывается в run()
        }
    }

    /// Запустить трек, выбранный в очереди. Если он не играет, пропустить и взять следующий.
    fn play_current(&mut self) {
        // Не больше len попыток, чтобы не зациклиться, если не играет ни один трек
        for _ in 0..self.queue.len() {
            let Some(track) = self.queue.current().cloned() else { break };
            match self.start_track(&track) {
                Ok(duration) => {
                    self.loaded = true;
                    // Если декодер не знает длину, берём из метаданных трека
                    self.duration = duration
                        .or_else(|| Some(Duration::from_secs(u64::from(track.duration_secs))));
                    self.emit(PlaybackEvent::TrackChanged(Some(track)));
                    self.emit(PlaybackEvent::PauseChanged(false));
                    return;
                }
                Err(reason) => {
                    self.emit(PlaybackEvent::TrackFailed { track, reason });
                    if self.queue.next().is_none() {
                        break;
                    }
                }
            }
        }
        self.set_idle();
        self.emit(PlaybackEvent::QueueEnded);
    }

    /// Получить у провайдера источник звука и включить его.
    fn start_track(&self, track: &Track) -> Result<Option<Duration>, String> {
        let provider = self
            .providers
            .get(&track.provider)
            .ok_or_else(|| format!("провайдер {:?} не подключён", track.provider))?;

        // block_on = «дождаться завершения асинхронной функции, заблокировав этот поток»
        let source = self
            .runtime
            .block_on(provider.get_stream(track))
            .map_err(|e| e.to_string())?;

        match source {
            StreamSource::LocalFile(path) => {
                let duration = self.player.play_file(&path).map_err(|e| e.to_string())?;
                self.player.set_volume(self.volume);
                Ok(duration)
            }
            other => Err(format!("источник {other:?} пока не поддерживается")),
        }
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
        self.player.stop();
        self.loaded = false;
        self.duration = None;
        self.emit(PlaybackEvent::TrackChanged(None));
    }

    fn publish_state(&self) {
        let snapshot = PlaybackState {
            current: if self.loaded { self.queue.current().cloned() } else { None },
            position: if self.loaded { self.player.position() } else { Duration::ZERO },
            duration: self.duration,
            is_playing: self.loaded && !self.player.is_paused(),
            volume: self.volume,
            shuffle: self.queue.shuffle(),
            repeat: self.queue.repeat(),
            queue_index: self.queue.index(),
            queue_len: self.queue.len(),
        };
        *self.state.lock().unwrap() = snapshot; // lock() = взять замок, чтобы записать безопасно
    }
}
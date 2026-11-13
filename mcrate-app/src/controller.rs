//! Controller = мост между UI и ядром (mcrate-core / mcrate-audio / mcrate-providers).
//! UI не трогает провайдеры и плеер напрямую: все действия идут через этот тип.
//! Ядро (его делает LomKich) мы здесь НЕ меняем, только используем.

use std::path::PathBuf;
use std::time::Duration;

use mcrate_audio::{AudioPlayer, PlayQueue, RepeatMode};
use mcrate_core::{MusicProvider, StreamSource, Track};
use mcrate_providers::LocalProvider;

/// Где лежит файл со списком сохранённых треков: %APPDATA%\Mcrate\library.txt
/// (APPDATA = переменная окружения Windows, обычно C:\Users\<имя>\AppData\Roaming).
/// Option, потому что переменной теоретически может не быть.
fn library_file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("Mcrate").join("library.txt"))
}

pub struct Controller {
    // Runtime = «движок» для async-кода. Методы MusicProvider асинхронные, а UI синхронный,
    // поэтому ждём результат через block_on (для локальных файлов это мгновенно).
    // Для сетевых провайдеров вызовы надо будет переделать на spawn + канал, чтобы не вешать UI.
    rt: tokio::runtime::Runtime,
    provider: LocalProvider,
    audio: Option<AudioPlayer>,
    pub audio_error: Option<String>,
    pub queue: PlayQueue<Track>,
    pub library: Vec<Track>,
    pub now: Option<Track>,
    pub volume: f32,
    pub status: Option<String>,
    ended: bool, // очередь доиграла до конца
}

impl Controller {
    pub fn new() -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("не удалось создать tokio runtime");
        // Нет звукового устройства: UI всё равно запускаем, ошибку покажем пользователю.
        let (audio, audio_error) = match AudioPlayer::new() {
            Ok(a) => (Some(a), None),
            Err(e) => (None, Some(e.to_string())),
        };
        // Библиотека сохраняется между запусками. Если папку приложения определить не удалось,
        // работаем только в памяти (как раньше).
        let provider = match library_file() {
            Some(path) => LocalProvider::with_storage(path),
            None => LocalProvider::new(),
        };
        // load() заново читает сохранённые пути. Возвращает пути, которые не загрузились.
        let (unavailable, load_error) = match provider.load() {
            Ok(list) => (list.len(), None),
            Err(e) => (0, Some(e.to_string())),
        };
        // Пустой запрос = все треки (так устроен LocalProvider::search).
        let library = rt.block_on(provider.search("")).unwrap_or_default();
        let status = match (load_error, unavailable) {
            (Some(e), _) => Some(format!("Не удалось загрузить библиотеку: {e}")),
            (None, 0) => None,
            (None, n) => Some(format!("Не найдено файлов: {n} (удалены или перемещены)")),
        };
        Self {
            rt,
            provider,
            audio,
            audio_error,
            queue: PlayQueue::new(),
            library,
            now: None,
            volume: 0.8,
            status,
            ended: false,
        }
    }

    // ---------- библиотека ----------

    /// Добавить файлы и папки (из диалога или drag&drop). Папки читаются без вложенных.
    pub fn add_paths(&mut self, paths: &[PathBuf]) {
        let (mut added, mut failed) = (0usize, 0usize);
        for path in paths {
            if path.is_dir() {
                match self.provider.add_folder(path) {
                    Ok(n) => added += n,
                    Err(_) => failed += 1,
                }
            } else if self.provider.add_file(path).is_ok() {
                added += 1;
            } else {
                failed += 1;
            }
        }
        // Пустой запрос = все треки (так устроен LocalProvider::search).
        self.library = self.rt.block_on(self.provider.search("")).unwrap_or_default();
        self.status = Some(match (added, failed) {
            (0, 0) => "Ничего не добавлено".to_string(),
            (a, 0) => format!("Добавлено треков: {a}"),
            (a, f) => format!("Добавлено: {a}, не удалось прочитать: {f}"),
        });
    }

    pub fn search(&self, query: &str) -> Vec<Track> {
        self.rt.block_on(self.provider.search(query)).unwrap_or_default()
    }

    // ---------- воспроизведение ----------

    /// Заменить очередь списком `items` и начать с элемента `start`.
    pub fn play_list(&mut self, items: Vec<Track>, start: usize) {
        self.queue.set(items, start);
        self.play_current();
    }

    fn play_current(&mut self) {
        let Some(track) = self.queue.current().cloned() else { return };
        let Some(audio) = &self.audio else {
            self.status = Some(format!(
                "Нет звукового устройства: {}",
                self.audio_error.clone().unwrap_or_default()
            ));
            return;
        };
        match self.rt.block_on(self.provider.get_stream(&track)) {
            Ok(StreamSource::LocalFile(path)) => match audio.play_file(&path) {
                Ok(_) => {
                    audio.set_volume(self.volume);
                    self.now = Some(track);
                    self.ended = false;
                    self.status = None;
                }
                Err(e) => self.status = Some(e.to_string()),
            },
            Ok(other) => self.status = Some(format!("Этот источник пока не поддерживается: {other:?}")),
            Err(e) => self.status = Some(e.to_string()),
        }
    }

    pub fn toggle_pause(&mut self) {
        if self.now.is_none() {
            return;
        }
        if self.ended {
            // Очередь закончилась: «играть» запускает текущий трек заново.
            self.play_current();
        } else if let Some(a) = &self.audio {
            if a.is_paused() { a.resume() } else { a.pause() }
        }
    }

    pub fn next(&mut self) {
        if self.queue.next().is_some() {
            self.play_current();
        }
    }

    pub fn prev(&mut self) {
        // Как в большинстве плееров: после 3 секунд «назад» перезапускает трек.
        if self.position() > Duration::from_secs(3) {
            self.play_current();
        } else if self.queue.prev().is_some() {
            self.play_current();
        }
    }

    /// Вызывать каждый кадр: переключает трек, когда предыдущий доиграл.
    pub fn tick(&mut self) {
        if self.now.is_none() || self.ended {
            return;
        }
        let finished = self.audio.as_ref().is_some_and(|a| a.is_finished());
        if finished {
            if self.queue.track_finished().is_some() {
                self.play_current();
            } else {
                self.ended = true;
            }
        }
    }

    pub fn is_playing(&self) -> bool {
        !self.ended
            && self.now.is_some()
            && self.audio.as_ref().is_some_and(|a| !a.is_paused() && !a.is_finished())
    }

    pub fn position(&self) -> Duration {
        self.audio.as_ref().map(|a| a.position()).unwrap_or_default()
    }

    pub fn seek_to(&mut self, secs: f32) {
        if let Some(a) = &self.audio {
            if let Err(e) = a.seek(Duration::from_secs_f32(secs.max(0.0))) {
                self.status = Some(e.to_string());
            }
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(a) = &self.audio {
            a.set_volume(self.volume);
        }
    }

    pub fn toggle_shuffle(&mut self) {
        let on = !self.queue.shuffle();
        self.queue.set_shuffle(on);
    }

    pub fn cycle_repeat(&mut self) {
        let next = self.queue.repeat().cycle();
        self.queue.set_repeat(next);
    }

    pub fn repeat_mode(&self) -> RepeatMode {
        self.queue.repeat()
    }
}

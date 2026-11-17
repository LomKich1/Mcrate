//! mcrate-audio: воспроизведение звука. Обёртка над rodio,
//! чтобы остальной проект не зависел от деталей библиотеки.

pub mod queue;
pub use queue::{PlayQueue, RepeatMode}; // чтобы писать mcrate_audio::PlayQueue
pub mod engine;
pub use engine::{PlaybackEngine, PlaybackEvent, PlaybackState};
mod growing_file; // чтение файла, который ещё докачивается (внутренний)
mod loudness; // измерение громкости трека по EBU R128 (внутренний)
mod normalize; // нормализация громкости: настройки, усиление, плавная смена
pub use normalize::{PlaybackSettings, DEFAULT_TARGET_LUFS};
mod stream_cache; // кэш скачанных онлайн-треков (внутренний, наружу не торчит)
mod visualizer; // неблокирующий анализатор аудио для визуализатора
pub use visualizer::{VisualizerFrame, VISUALIZER_POINTS};

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

// Decoder  = превращает файл (mp3/flac/...) в поток звуковых данных
// Player   = очередь воспроизведения: пауза, громкость, перемотка
// MixerDeviceSink = «выход на звуковую карту»; пока он жив, звук играет
// Source   = трейт, который даёт методы вроде total_duration() (длина трека)
use mcrate_net::{Available, DownloadProgress};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use thiserror::Error;

use crate::growing_file::{GrowingFile, HEADER_LEN};
use crate::visualizer::{AnalyzedSource, VisualizerFeed};

#[derive(Debug, Error)]
pub enum AudioError {
    // #[from] = автоматическое превращение чужой ошибки в нашу при использовании `?`
    #[error("не удалось открыть файл: {0}")]
    Io(#[from] std::io::Error),
    #[error("не удалось открыть звуковое устройство: {0}")]
    Device(#[from] rodio::DeviceSinkError),
    #[error("не удалось декодировать файл (формат не поддерживается или файл повреждён): {0}")]
    Decode(#[from] rodio::decoder::DecoderError),
    #[error("перемотка не удалась: {0}")]
    Seek(#[from] rodio::source::SeekError),
    #[error("ошибка движка воспроизведения: {0}")]
    Engine(String),
}

/// Одна «дека» (отсек): свой `Player` на общем выходе и свой докачиваемый файл.
/// Деки две: пока одна играет, во вторую можно заранее подготовить следующий трек
/// (предзагрузка) и потом переключиться на него без паузы или наложить кроссфейдом.
struct Deck {
    player: Player,
    /// Some(...) = в деке играет файл, который ещё докачивается (см. `play_stream`).
    stream: Mutex<Option<StreamCtx>>,
}

impl Deck {
    fn new(sink: &MixerDeviceSink) -> Self {
        Self { player: Player::connect_new(sink.mixer()), stream: Mutex::new(None) }
    }

    /// Доступ к данным докачиваемого файла. Если другой поток упал с паникой, держа замок,
    /// данные всё равно годятся.
    fn ctx(&self) -> MutexGuard<'_, Option<StreamCtx>> {
        self.stream.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Убрать из деки всё, что в ней есть (заодно ставит её на паузу).
    fn clear(&self) {
        *self.ctx() = None;
        self.player.clear();
    }
}

pub struct AudioPlayer {
    // Порядок полей важен: Rust удаляет поля сверху вниз.
    // Сначала удаляем деки (плееры), потом устройство вывода.
    decks: [Deck; 2],
    /// Номер активной деки (0 или 1): та, что сейчас «играет». Вторая (простаивающая)
    /// держит предзагруженный следующий трек на паузе или пуста.
    active: AtomicUsize,
    visualizer: VisualizerFeed,
    _sink: MixerDeviceSink, // `_` в начале = «поле нужно, чтобы жить, но мы его не читаем»
}

/// Всё, что нужно, чтобы заново открыть докачиваемый файл (для перемотки).
struct StreamCtx {
    part: PathBuf,
    dest: PathBuf,
    progress: DownloadProgress,
    /// Подсказка формата для декодера (расширение итогового файла).
    hint: Option<String>,
    /// Файл в формате MP4: назад по нему перематывать нельзя, поэтому каждая перемотка
    /// строит НОВЫЙ декодер с начала файла и перематывает вперёд (см. `seek`).
    rebuild_on_seek: bool,
    /// С какого места трека начался текущий декодер: `Player` считает позицию от начала
    /// своего источника, а после пересборки источник начинается не с нуля.
    offset: Duration,
}

impl AudioPlayer {
    /// Открывает звуковое устройство по умолчанию (то, что выбрано в Windows).
    pub fn new() -> Result<Self, AudioError> {
        Self::with_visualizer(VisualizerFeed::new())
    }

    /// Внутренний конструктор: движок передаёт общий атомарный буфер UI и аудиопотоку.
    pub(crate) fn with_visualizer(visualizer: VisualizerFeed) -> Result<Self, AudioError> {
        let mut sink = DeviceSinkBuilder::open_default_sink()?;
        sink.log_on_drop(false); // не печатать предупреждение при закрытии
        let decks = [Deck::new(&sink), Deck::new(&sink)]; // обе деки подключены к «выходу»
        Ok(Self { decks, active: AtomicUsize::new(0), visualizer, _sink: sink })
    }

    fn active_index(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    /// Дека, которая играет сейчас: все обычные методы (пауза, позиция, перемотка...) работают с ней.
    fn active_deck(&self) -> &Deck {
        &self.decks[self.active_index()]
    }

    /// Вторая дека: сюда готовится следующий трек.
    fn idle_deck(&self) -> &Deck {
        &self.decks[1 - self.active_index()]
    }

    /// Открывает файл и ставит его в `deck` (старое содержимое деки убирается, дека на паузе).
    /// Если файл не открылся, дека остаётся как была.
    fn install_file(&self, deck: &Deck, path: &Path) -> Result<Option<Duration>, AudioError> {
        let file = File::open(path)?;
        let decoder = Decoder::try_from(file)?; // сам определяет формат по содержимому
        let total = decoder.total_duration();

        deck.clear(); // убрать предыдущий трек; это обычный файл, а не докачиваемый
        deck.player.append(AnalyzedSource::new(decoder, self.visualizer.clone())); // анализировать исходный звук
        Ok(total)
    }

    /// То же для файла, который ещё докачивается (подробности и про MP4 см. `play_stream`).
    fn install_stream(
        &self,
        deck: &Deck,
        part: &Path,
        dest: &Path,
        progress: &DownloadProgress,
    ) -> Result<Option<Duration>, AudioError> {
        // Подождать первые байты: по ним определяется формат
        match progress.wait_for_bytes(HEADER_LEN as u64, STREAM_START_TIMEOUT) {
            Available::Data(_) => {}
            Available::Eof => return Err(AudioError::Engine("загруженный файл пуст".to_string())),
            Available::Failed(message) => return Err(AudioError::Engine(format!("загрузка не удалась: {message}"))),
            Available::Cancelled => return Err(AudioError::Engine("загрузка отменена".to_string())),
            Available::Timeout => return Err(AudioError::Engine("данные не приходят".to_string())),
        }
        let hint = dest.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
        let reader = GrowingFile::open(part, dest, progress.clone())?;
        let mp4 = reader.looks_like_mp4();
        let decoder = build_stream_decoder(reader, mp4, hint.as_deref())?;
        let total = decoder.total_duration();

        deck.clear();
        *deck.ctx() = Some(StreamCtx {
            part: part.to_path_buf(),
            dest: dest.to_path_buf(),
            progress: progress.clone(),
            hint,
            rebuild_on_seek: mp4,
            offset: Duration::ZERO,
        });
        deck.player.append(AnalyzedSource::new(decoder, self.visualizer.clone()));
        Ok(total)
    }

    /// Запускает файл. Возвращает длину трека (Option: у некоторых форматов она неизвестна).
    /// Это «играть ЭТО прямо сейчас»: предзагруженный следующий трек выбрасывается
    /// (он готовился для другого порядка событий).
    pub fn play_file(&self, path: &Path) -> Result<Option<Duration>, AudioError> {
        let deck = self.active_deck();
        let total = self.install_file(deck, path)?;
        self.discard_preloaded();
        self.visualizer.clear();
        deck.player.play(); // снять паузу
        Ok(total)
    }

    /// Запускает файл, который ещё докачивается (играть, пока качается).
    ///
    /// `part` = имя, под которым файл растёт; `dest` = итоговое имя (после докачки `part`
    /// переименовывают в `dest`, поэтому открываем что найдётся). `progress` сообщает, сколько
    /// байт уже записано. Вызывать, когда уже скачан запас (несколько сотен КБ): пока декодер
    /// определяет формат, он читает начало файла и будет ждать недостающие байты.
    ///
    /// Если загрузка уже закончилась, это то же, что `play_file(dest)`.
    ///
    /// Про формат: у MP4 (в нём SoundCloud) декодеру нельзя включать режим «seekable»: он тогда
    /// требует знать длину файла, читает его до самого конца и (в нашей версии rodio) паникует,
    /// если длины нет. Поэтому MP4 открываем без «seekable», а перемотку делаем пересборкой
    /// декодера (`seek`). Для остальных форматов (MP3, AAC-ADTS) «seekable» безопасен.
    pub fn play_stream(
        &self,
        part: &Path,
        dest: &Path,
        progress: &DownloadProgress,
    ) -> Result<Option<Duration>, AudioError> {
        if progress.is_finished() && dest.exists() {
            return self.play_file(dest);
        }
        let deck = self.active_deck();
        let total = self.install_stream(deck, part, dest, progress)?;
        self.discard_preloaded();
        self.visualizer.clear();
        deck.player.play();
        Ok(total)
    }

    // ----- предзагрузка следующего трека во вторую деку -----

    /// Готовит файл во второй деке: он стоит на паузе, не звучит и не двигается
    /// (на паузе источник не читается), а играющий трек не затрагивается.
    /// Старая предзагрузка заменяется. Громкость второй деки сначала равна громкости
    /// активной; `set_preload_volume` меняет её до переключения.
    pub fn preload_file(&self, path: &Path) -> Result<Option<Duration>, AudioError> {
        let idle = self.idle_deck();
        let total = self.install_file(idle, path)?;
        idle.player.set_volume(self.active_deck().player.volume());
        Ok(total)
    }

    /// То же для докачиваемого файла (см. `play_stream`).
    pub fn preload_stream(
        &self,
        part: &Path,
        dest: &Path,
        progress: &DownloadProgress,
    ) -> Result<Option<Duration>, AudioError> {
        if progress.is_finished() && dest.exists() {
            return self.preload_file(dest);
        }
        let idle = self.idle_deck();
        let total = self.install_stream(idle, part, dest, progress)?;
        idle.player.set_volume(self.active_deck().player.volume());
        Ok(total)
    }

    /// Есть ли подготовленный трек во второй деке?
    pub fn has_preloaded(&self) -> bool {
        !self.idle_deck().player.empty()
    }

    /// Громкость предзагруженного трека (например, с его усилением нормализации).
    pub fn set_preload_volume(&self, volume: f32) {
        self.idle_deck().player.set_volume(volume);
    }

    /// Выбросить предзагруженный трек.
    pub fn discard_preloaded(&self) {
        let idle = self.idle_deck();
        if !idle.player.empty() || idle.ctx().is_some() {
            idle.clear();
        }
    }

    /// Переключиться на предзагруженный трек: текущий убирается, подготовленный становится
    /// активным. Пауза сохраняется (стояли на паузе, останемся на паузе, играли, играем).
    /// Без предзагрузки ничего не делает и возвращает `false`.
    pub fn switch_to_preloaded(&self) -> bool {
        if !self.has_preloaded() {
            return false;
        }
        let old = self.active_index();
        let was_paused = self.decks[old].player.is_paused();
        self.decks[old].clear(); // старый трек замолкает
        self.active.store(1 - old, Ordering::Release);
        self.visualizer.clear();
        if !was_paused {
            self.decks[1 - old].player.play();
        }
        true
    }

    pub fn pause(&self) { self.active_deck().player.pause(); }
    pub fn resume(&self) { self.active_deck().player.play(); }
    pub fn stop(&self) {
        // Стоп убирает всё: и играющий трек, и предзагруженный
        for deck in &self.decks {
            deck.clear();
        }
        self.visualizer.clear();
    }
    pub fn is_paused(&self) -> bool { self.active_deck().player.is_paused() }

    /// Трек доиграл (или ничего не загружено)?
    pub fn is_finished(&self) -> bool { self.active_deck().player.empty() }

    /// Громкость: 1.0 = как в файле, 0.5 = вдвое тише.
    pub fn set_volume(&self, volume: f32) {
        // Визуализатор от громкости не зависит: он анализирует звук ДО ползунка громкости.
        self.active_deck().player.set_volume(volume);
    }

    /// Текущая позиция воспроизведения.
    pub fn position(&self) -> Duration {
        let deck = self.active_deck();
        let offset = deck.ctx().as_ref().map_or(Duration::ZERO, |c| c.offset);
        deck.player.get_pos() + offset
    }

    /// Перемотка на указанную позицию.
    ///
    /// Для докачиваемого MP4 это пересборка: новый декодер поверх того же файла с начала,
    /// перемотка вперёд до нужного места (по уже скачанным данным это быстро), и он заменяет
    /// старый. Прямая перемотка назад там ненадёжна: декодер помнит лишь последние ~64 КБ,
    /// а на более далёкую перемотку отвечает «успехом», после которого звук просто кончается.
    /// Перематывать дальше скачанного нельзя: вызов будет ждать данные (см. `play_stream`).
    pub fn seek(&self, pos: Duration) -> Result<(), AudioError> {
        let deck = self.active_deck();
        let rebuild = deck.ctx().as_ref().and_then(|c| {
            c.rebuild_on_seek.then(|| (c.part.clone(), c.dest.clone(), c.progress.clone(), c.hint.clone()))
        });
        let Some((part, dest, progress, hint)) = rebuild else {
            deck.player.try_seek(pos)?;
            return Ok(());
        };

        let reader = GrowingFile::open(&part, &dest, progress)?;
        let mut decoder = build_stream_decoder(reader, true, hint.as_deref())?;
        decoder.try_seek(pos)?; // читает данные вперёд до нужного места
        let was_paused = deck.player.is_paused();
        deck.player.clear(); // заодно ставит на паузу
        self.visualizer.clear();
        deck.player.append(AnalyzedSource::new(decoder, self.visualizer.clone()));
        if !was_paused {
            deck.player.play();
        }
        if let Some(ctx) = deck.ctx().as_mut() {
            ctx.offset = pos;
        }
        Ok(())
    }
}

/// Сколько ждём первые байты докачиваемого файла.
const STREAM_START_TIMEOUT: Duration = Duration::from_secs(15);

/// Строит декодер поверх докачиваемого файла. `mp4 = true` отключает режим «seekable» (почему,
/// см. `AudioPlayer::play_stream`).
fn build_stream_decoder(
    reader: GrowingFile,
    mp4: bool,
    hint: Option<&str>,
) -> Result<Decoder<GrowingFile>, AudioError> {
    let mut builder = Decoder::builder().with_data(reader).with_seekable(!mp4);
    if let Some(hint) = hint {
        builder = builder.with_hint(hint); // подсказка формата; на неверную декодер не обидится
    }
    Ok(builder.build()?)
}

#[cfg(test)]
mod stream_tests {
    //! Тесты игры «по мере скачивания». Докачку имитирует поток, который дописывает файл кусками.
    //! Первые три теста работают без звукового устройства, последний при его отсутствии пропускается.

    use super::*;
    use std::fs;
    use std::io::Write;
    use std::thread;

    /// Маленький фрагментированный MP4 (4 с синуса, 8 кГц, AAC): как кусочек SoundCloud.
    const FMP4: &[u8] = include_bytes!("../tests/data/fmp4_4s.m4a");

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("mcrate-stream-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Дописывает `data` в файл кусками по `chunk` байт с паузами, как загрузчик.
    fn spawn_writer(path: PathBuf, data: Vec<u8>, progress: DownloadProgress, chunk: usize, pause_ms: u64) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut file = File::create(&path).unwrap();
            for part in data.chunks(chunk) {
                file.write_all(part).unwrap();
                progress.add_bytes(part.len() as u64);
                thread::sleep(Duration::from_millis(pause_ms));
            }
            progress.finish();
        })
    }

    /// WAV: 8 кГц, моно, 16 бит, синус.
    fn sine_wav(seconds: u32) -> Vec<u8> {
        let samples = 8000 * seconds;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(samples * 2).to_le_bytes());
        for i in 0..samples {
            let v = ((i as f32 * 440.0 * std::f32::consts::TAU / 8000.0).sin() * 8000.0) as i16;
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes
    }

    /// Открывает полностью готовый файл (загрузка «уже закончилась»).
    fn finished_reader(dir: &TempDir, name: &str, data: &[u8]) -> GrowingFile {
        let path = dir.0.join(name);
        fs::write(&path, data).unwrap();
        let progress = DownloadProgress::new();
        progress.add_bytes(data.len() as u64);
        progress.finish();
        GrowingFile::open(&path, &path, progress).unwrap()
    }

    #[test]
    fn wav_decodes_completely_while_growing() {
        let dir = TempDir::new("wav");
        let part = dir.0.join("a.wav.part");
        let data = sine_wav(3);
        fs::write(&part, &data[..2000]).unwrap(); // первый кусок уже есть
        let progress = DownloadProgress::new();
        progress.add_bytes(2000);
        let reader = GrowingFile::open(&part, &part, progress.clone()).unwrap();
        // остальное «докачивается» уже после того, как декодер начал читать
        let rest = data[2000..].to_vec();
        let (p2, part2) = (progress.clone(), part.clone());
        let writer = thread::spawn(move || {
            let mut f = fs::OpenOptions::new().append(true).open(&part2).unwrap();
            for chunk in rest.chunks(3000) {
                thread::sleep(Duration::from_millis(3));
                f.write_all(chunk).unwrap();
                p2.add_bytes(chunk.len() as u64);
            }
            p2.finish();
        });
        let decoder = build_stream_decoder(reader, false, Some("wav")).unwrap();
        assert_eq!(decoder.count(), 3 * 8000); // все сэмплы на месте, ни одного не потеряно
        writer.join().unwrap();
    }

    #[test]
    fn fmp4_plays_while_growing_like_after_full_download() {
        let dir = TempDir::new("fmp4");
        // эталон: сколько сэмплов выходит из полностью готового файла
        let reference = {
            let reader = finished_reader(&dir, "ref.m4a", FMP4);
            assert!(reader.looks_like_mp4());
            build_stream_decoder(reader, true, Some("m4a")).unwrap().count()
        };
        assert!(reference >= 4 * 8000, "эталон слишком короткий: {reference}");

        // тот же файл, но он растёт маленькими кусками уже во время декодирования
        let part = dir.0.join("g.m4a.part");
        File::create(&part).unwrap();
        let progress = DownloadProgress::new();
        let writer = spawn_writer(part.clone(), FMP4.to_vec(), progress.clone(), 700, 2);
        // как в движке: ждём запас, и только потом открываем
        assert!(matches!(progress.wait_for_bytes(HEADER_LEN as u64, Duration::from_secs(5)), Available::Data(_)));
        let reader = GrowingFile::open(&part, &part, progress).unwrap();
        assert!(reader.looks_like_mp4());
        let decoder = build_stream_decoder(reader, true, Some("m4a")).unwrap();
        assert_eq!(decoder.count(), reference);
        writer.join().unwrap();
    }

    #[test]
    fn fmp4_rebuild_seek_works_in_any_direction() {
        let dir = TempDir::new("rebuild");
        let path = dir.0.join("s.m4a");
        fs::write(&path, FMP4).unwrap();
        let total = {
            let reader = finished_reader(&dir, "t.m4a", FMP4);
            build_stream_decoder(reader, true, Some("m4a")).unwrap().count() as f64
        };
        // как делает `AudioPlayer::seek`: каждый раз НОВЫЙ декодер, перемотка вперёд от начала
        for secs in [2.0, 0.5, 3.0, 1.0, 0.0] {
            let reader = finished_reader(&dir, "r.m4a", FMP4);
            let mut decoder = build_stream_decoder(reader, true, Some("m4a")).unwrap();
            decoder.try_seek(Duration::from_secs_f64(secs)).unwrap();
            let left = decoder.count() as f64;
            let expected = total - secs * 8000.0;
            // допуск: кадры AAC по 1024 сэмпла, перемотка попадает на границу кадра
            assert!((left - expected).abs() <= 2100.0, "после перемотки на {secs} с осталось {left}, ждали около {expected}");
        }
    }

    // ----- две деки: предзагрузка и переключение -----

    fn device() -> Option<AudioPlayer> {
        match AudioPlayer::new() {
            Ok(player) => Some(player),
            Err(_) => {
                eprintln!("тест пропущен: нет звукового устройства");
                None
            }
        }
    }

    fn wav_file(dir: &TempDir, name: &str, seconds: u32) -> PathBuf {
        let path = dir.0.join(name);
        fs::write(&path, sine_wav(seconds)).unwrap();
        path
    }

    #[test]
    fn preload_leaves_active_track_alone_and_does_not_advance() {
        let Some(player) = device() else { return };
        let dir = TempDir::new("preload");
        let (a, b) = (wav_file(&dir, "a.wav", 30), wav_file(&dir, "b.wav", 5));
        player.play_file(&a).unwrap();
        player.pause();
        assert!(!player.has_preloaded());

        let total = player.preload_file(&b).unwrap().expect("длина WAV известна");
        assert!(total >= Duration::from_millis(4900) && total <= Duration::from_millis(5100), "{total:?}");
        assert!(player.has_preloaded());
        assert!(player.is_paused(), "активный трек должен остаться на паузе");
        assert!(!player.is_finished(), "активный трек не тронут");

        // Предзагруженная дека на паузе не читает источник: за это время она не уходит вперёд.
        thread::sleep(Duration::from_millis(300));
        assert!(player.idle_deck().player.is_paused());
        assert_eq!(player.idle_deck().player.get_pos(), Duration::ZERO);
        assert!(!player.idle_deck().player.empty());
    }

    #[test]
    fn switch_makes_preloaded_active_and_keeps_pause() {
        let Some(player) = device() else { return };
        let dir = TempDir::new("switch");
        let (a, b) = (wav_file(&dir, "a.wav", 30), wav_file(&dir, "b.wav", 5));
        player.play_file(&a).unwrap();
        player.pause();
        player.preload_file(&b).unwrap();
        let old = player.active_index();

        assert!(player.switch_to_preloaded());
        assert_eq!(player.active_index(), 1 - old);
        assert!(!player.has_preloaded(), "после переключения предзагрузки нет");
        assert!(player.decks[old].player.empty(), "старый трек убран");
        assert!(player.is_paused(), "пауза сохраняется");
        assert!(!player.is_finished(), "новый трек загружен в активную деку");
        assert_eq!(player.position(), Duration::ZERO);
    }

    #[test]
    fn switch_keeps_playing_state() {
        let Some(player) = device() else { return };
        let dir = TempDir::new("switch-play");
        let (a, b) = (wav_file(&dir, "a.wav", 30), wav_file(&dir, "b.wav", 30));
        player.play_file(&a).unwrap();
        player.preload_file(&b).unwrap();
        assert!(player.switch_to_preloaded());
        assert!(!player.is_paused(), "играли, значит играем");
    }

    #[test]
    fn switch_without_preload_does_nothing() {
        let Some(player) = device() else { return };
        let dir = TempDir::new("switch-none");
        player.play_file(&wav_file(&dir, "a.wav", 30)).unwrap();
        player.pause();
        let old = player.active_index();
        assert!(!player.switch_to_preloaded());
        assert_eq!(player.active_index(), old);
        assert!(!player.is_finished());
    }

    #[test]
    fn play_file_and_stop_discard_preloaded() {
        let Some(player) = device() else { return };
        let dir = TempDir::new("discard");
        let (a, b, c) = (wav_file(&dir, "a.wav", 30), wav_file(&dir, "b.wav", 30), wav_file(&dir, "c.wav", 30));
        player.play_file(&a).unwrap();
        player.preload_file(&b).unwrap();
        player.play_file(&c).unwrap(); // явный запуск другого трека: предзагрузка устарела
        assert!(!player.has_preloaded());

        player.preload_file(&b).unwrap();
        player.stop();
        assert!(!player.has_preloaded());
        assert!(player.is_finished());
    }

    #[test]
    fn failed_preload_changes_nothing() {
        let Some(player) = device() else { return };
        let dir = TempDir::new("bad");
        let (a, b) = (wav_file(&dir, "a.wav", 30), wav_file(&dir, "b.wav", 30));
        player.play_file(&a).unwrap();
        player.pause();
        player.preload_file(&b).unwrap();
        assert!(player.preload_file(&dir.0.join("нет-такого.wav")).is_err());
        assert!(player.has_preloaded(), "прежняя предзагрузка осталась");
        assert!(!player.is_finished(), "активный трек не тронут");
    }

    #[test]
    fn preloaded_stream_keeps_seeking_after_switch() {
        let Some(player) = device() else { return };
        let dir = TempDir::new("preload-stream");
        let a = wav_file(&dir, "a.wav", 30);
        let part = dir.0.join("p.m4a.part");
        let dest = dir.0.join("p.m4a");
        File::create(&part).unwrap();
        let progress = DownloadProgress::new();
        let writer = spawn_writer(part.clone(), FMP4.to_vec(), progress.clone(), 2000, 1);
        assert!(matches!(progress.wait_for_bytes(4000, Duration::from_secs(5)), Available::Data(_)));

        player.play_file(&a).unwrap();
        player.pause();
        player.preload_stream(&part, &dest, &progress).unwrap();
        assert!(player.has_preloaded());
        assert!(player.idle_deck().ctx().is_some(), "данные докачки лежат в предзагруженной деке");
        assert!(player.switch_to_preloaded());
        assert!(player.active_deck().ctx().is_some(), "и переехали вместе с ней");

        // перемотка работает через пересборку по данным уже новой активной деки
        writer.join().unwrap();
        player.seek(Duration::from_secs(2)).unwrap();
        let pos = player.position();
        assert!(pos >= Duration::from_secs(2) && pos < Duration::from_secs(3), "{pos:?}");
        assert!(player.is_paused());
        player.stop();
    }

    #[test]
    fn audio_player_streams_and_seeks_fmp4() {
        let Ok(player) = AudioPlayer::new() else {
            eprintln!("тест пропущен: нет звукового устройства");
            return;
        };
        let dir = TempDir::new("player");
        let part = dir.0.join("p.m4a.part");
        let dest = dir.0.join("p.m4a");
        File::create(&part).unwrap();
        let progress = DownloadProgress::new();
        let writer = spawn_writer(part.clone(), FMP4.to_vec(), progress.clone(), 2000, 1);
        // дождёмся запаса, как движок
        assert!(matches!(progress.wait_for_bytes(4000, Duration::from_secs(5)), Available::Data(_)));
        player.play_stream(&part, &dest, &progress).unwrap();
        assert!(!player.is_paused());

        // перемотка вперёд и назад пересборкой; позиция считается с учётом смещения
        player.seek(Duration::from_secs(2)).unwrap();
        let pos = player.position();
        assert!(pos >= Duration::from_secs(2) && pos < Duration::from_secs(3), "позиция после перемотки: {pos:?}");
        player.seek(Duration::from_secs(1)).unwrap();
        let pos = player.position();
        assert!(pos >= Duration::from_secs(1) && pos < Duration::from_secs(2), "позиция после перемотки назад: {pos:?}");

        // пауза сохраняется при перемотке
        player.pause();
        player.seek(Duration::from_secs(2)).unwrap();
        assert!(player.is_paused());
        writer.join().unwrap();
        player.stop();
    }
}

//! mcrate-audio: воспроизведение звука. Обёртка над rodio,
//! чтобы остальной проект не зависел от деталей библиотеки.

pub mod queue;
pub use queue::{PlayQueue, RepeatMode}; // чтобы писать mcrate_audio::PlayQueue

use std::fs::File;
use std::path::Path;
use std::time::Duration;

// Decoder  = превращает файл (mp3/flac/...) в поток звуковых данных
// Player   = очередь воспроизведения: пауза, громкость, перемотка
// MixerDeviceSink = «выход на звуковую карту»; пока он жив, звук играет
// Source   = трейт, который даёт методы вроде total_duration() (длина трека)
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use thiserror::Error;

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
}

pub struct AudioPlayer {
    // Порядок полей важен: Rust удаляет поля сверху вниз.
    // Сначала удаляем player, потом устройство вывода.
    player: Player,
    _sink: MixerDeviceSink, // `_` в начале = «поле нужно, чтобы жить, но мы его не читаем»
}

impl AudioPlayer {
    /// Открывает звуковое устройство по умолчанию (то, что выбрано в Windows).
    pub fn new() -> Result<Self, AudioError> {
        let mut sink = DeviceSinkBuilder::open_default_sink()?;
        sink.log_on_drop(false); // не печатать предупреждение при закрытии
        let player = Player::connect_new(sink.mixer()); // подключаем очередь к «выходу»
        Ok(Self { player, _sink: sink })
    }

    /// Запускает файл. Возвращает длину трека (Option: у некоторых форматов она неизвестна).
    pub fn play_file(&self, path: &Path) -> Result<Option<Duration>, AudioError> {
        let file = File::open(path)?;
        let decoder = Decoder::try_from(file)?; // сам определяет формат по содержимому
        let total = decoder.total_duration();

        self.player.clear();        // убрать предыдущий трек (заодно ставит на паузу)
        self.player.append(decoder); // добавить новый в очередь
        self.player.play();         // снять паузу
        Ok(total)
    }

    pub fn pause(&self) { self.player.pause(); }
    pub fn resume(&self) { self.player.play(); }
    pub fn stop(&self) { self.player.clear(); }
    pub fn is_paused(&self) -> bool { self.player.is_paused() }

    /// Трек доиграл (или ничего не загружено)?
    pub fn is_finished(&self) -> bool { self.player.empty() }

    /// Громкость: 1.0 = как в файле, 0.5 = вдвое тише.
    pub fn set_volume(&self, volume: f32) { self.player.set_volume(volume); }

    /// Текущая позиция воспроизведения.
    pub fn position(&self) -> Duration { self.player.get_pos() }

    /// Перемотка на указанную позицию.
    pub fn seek(&self, pos: Duration) -> Result<(), AudioError> {
        self.player.try_seek(pos)?;
        Ok(())
    }
}
//! Усиление нормализации громкости на уровне источника.
//!
//! Раньше усиление умножалось на ползунок и ставилось громкостью `Player`. Но громкость у `Player`
//! одна на всю очередь: при бесшовном стыке двух треков (второй дописан в очередь первого) у них
//! не получилось бы разное усиление (см. «Принятые решения», бесшовность). Поэтому усиление теперь
//! живёт в самом источнике: у каждого трека свой `TrackGain`, а громкость `Player` остаётся
//! ползунком пользователя.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use rodio::Source;

/// Общий множитель громкости одного трека (1.0 = без изменений). Клонирование даёт ещё одну
/// ручку на то же число: движок меняет его из своего потока, а звуковой поток читает при выдаче
/// каждого сэмпла. Число хранится как биты `f32` в атомарной переменной (читать и писать можно из
/// разных потоков без замка, звуковой поток не блокируется).
#[derive(Clone, Debug)]
pub(crate) struct TrackGain(Arc<AtomicU32>);

impl TrackGain {
    pub(crate) fn new(value: f32) -> Self {
        let gain = Self(Arc::new(AtomicU32::new(1.0f32.to_bits())));
        gain.set(value);
        gain
    }

    pub(crate) fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    /// Не число или отрицательное значение не принимаем как есть: звук не должен пропасть или
    /// «взорваться» из-за ошибки в расчёте (отрицательное становится нулём, не число единицей).
    pub(crate) fn set(&self, value: f32) {
        let value = if value.is_finite() { value.max(0.0) } else { 1.0 };
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }
}

/// Источник, умножающий каждый сэмпл на `TrackGain`. Множитель читается на каждом сэмпле, поэтому
/// изменение действует сразу (плавность задаёт тот, кто меняет число).
pub(crate) struct GainSource<S> {
    inner: S,
    gain: TrackGain,
}

impl<S> GainSource<S>
where
    S: Source<Item = f32>,
{
    pub(crate) fn new(inner: S, gain: TrackGain) -> Self {
        Self { inner, gain }
    }
}

impl<S> Iterator for GainSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        self.inner.next().map(|sample| sample * self.gain.get())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S> Source for GainSource<S>
where
    S: Source<Item = f32>,
{
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::Player;
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    const RATE: u32 = 8000;

    fn constant(value: f32, frames: usize) -> SamplesBuffer {
        SamplesBuffer::new(NonZero::new(2u16).unwrap(), NonZero::new(RATE).unwrap(), vec![value; frames * 2])
    }

    #[test]
    fn gain_is_sanitized() {
        let gain = TrackGain::new(0.5);
        assert_eq!(gain.get(), 0.5);
        gain.set(f32::NAN);
        assert_eq!(gain.get(), 1.0);
        gain.set(f32::INFINITY);
        assert_eq!(gain.get(), 1.0);
        gain.set(-3.0);
        assert_eq!(gain.get(), 0.0);
        let clone = gain.clone();
        clone.set(2.0);
        assert_eq!(gain.get(), 2.0, "клон должен быть ручкой на то же число");
    }

    #[test]
    fn source_scales_samples_and_follows_the_handle_immediately() {
        let gain = TrackGain::new(0.5);
        let mut source = GainSource::new(constant(1.0, 100), gain.clone());
        assert_eq!(source.by_ref().take(4).collect::<Vec<_>>(), vec![0.5; 4]);
        gain.set(0.25);
        assert_eq!(source.next(), Some(0.25), "новое значение действует со следующего сэмпла");
    }

    #[test]
    fn source_passes_metadata_and_seek_through() {
        let mut source = GainSource::new(constant(1.0, 800), TrackGain::new(1.0));
        assert_eq!(source.channels().get(), 2);
        assert_eq!(source.sample_rate().get(), RATE);
        assert_eq!(source.total_duration(), Some(Duration::from_millis(100)));
        source.try_seek(Duration::from_millis(50)).unwrap();
        assert_eq!(source.count(), 800, "после перемотки на середину осталась половина: 400 кадров по 2 сэмпла");
    }

    #[test]
    fn player_volume_and_source_gain_multiply_like_the_old_single_volume() {
        // Было: Player::set_volume(ползунок * усиление). Стало: ползунок на Player, усиление в источнике.
        let (player, mut out) = Player::new();
        player.set_volume(0.5);
        player.append(GainSource::new(constant(1.0, 400), TrackGain::new(0.4)));
        let samples: Vec<f32> = out.by_ref().take(800).collect();
        assert!(samples.iter().all(|s| (s - 0.2).abs() < 1e-6), "ожидали 0.5 * 0.4 = 0.2, получили {:?}", &samples[..4]);
    }

    #[test]
    fn tracks_in_one_queue_keep_their_own_gain_exactly_at_the_boundary() {
        // Это то, ради чего усиление вынесено в источник (шаг 4b): с громкостью Player новый трек
        // звучал бы с усилением прошлого, пока движок не заметит стык (опрос раз в 100 мс).
        let (player, mut out) = Player::new();
        player.append(GainSource::new(constant(1.0, 400), TrackGain::new(0.8)));
        player.append(GainSource::new(constant(1.0, 400), TrackGain::new(0.2)));
        let first: Vec<f32> = out.by_ref().take(800).collect();
        let second: Vec<f32> = out.by_ref().take(800).collect();
        assert!(first.iter().all(|s| (s - 0.8).abs() < 1e-6));
        assert!(second.iter().all(|s| (s - 0.2).abs() < 1e-6), "первый сэмпл второго трека: {}", second[0]);
    }
}

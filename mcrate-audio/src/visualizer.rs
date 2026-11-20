//! Неблокирующая аудиовизуализация поверх декодированного сигнала rodio.
//!
//! Анализатор не меняет звук: он раскладывает короткое окно сэмплов на частоты (FFT, быстрое
//! преобразование Фурье) и публикует 128 уровней частотных полос в атомарный массив.
//! Полосы идут от низких частот к высоким и стоят на месте: форма пульсирует, а не ползёт
//! вбок, как лента громкости по времени. UI читает только копию последнего кадра.

use std::f32::consts::PI;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use rodio::Source;

/// Количество полос спектра, доступных визуализатору (слева низкие частоты, справа высокие).
pub const VISUALIZER_POINTS: usize = 128;

/// Размер окна FFT (степень двойки). 2048 сэмплов = ~43 мс при 48 кГц: шаг по частоте ~23 Гц.
const FFT_SIZE: usize = 2048;
/// Диапазон полос (Гц). Верхняя граница урезается до 45% частоты дискретизации.
const MIN_HZ: f32 = 45.0;
const MAX_HZ: f32 = 16_000.0;
/// Громкость полосы в децибелах (дБ) относительно полной шкалы: ниже пола уровень 0, выше потолка 1.
const DB_FLOOR: f32 = -72.0;
const DB_CEIL: f32 = -14.0;
/// В музыке высокие частоты тише низких; без поправки правая половина почти не двигалась бы.
/// Добавка растёт от 0 дБ у низких частот до этого значения у высоких.
const TILT_DB: f32 = 16.0;

/// Публичный снимок последних уровней спектра.
#[derive(Clone, Debug)]
pub struct VisualizerFrame {
    /// Нормализованные уровни 0.0..=1.0: индекс 0 это низкие частоты, последний это высокие.
    pub samples: [f32; VISUALIZER_POINTS],
    /// Увеличивается при каждой публикации нового кадра или очистке.
    pub sequence: u64,
}

impl Default for VisualizerFrame {
    fn default() -> Self {
        Self { samples: [0.0; VISUALIZER_POINTS], sequence: 0 }
    }
}

#[derive(Clone)]
pub(crate) struct VisualizerFeed {
    shared: Arc<VisualizerShared>,
}

struct VisualizerShared {
    levels: [AtomicU32; VISUALIZER_POINTS],
    sequence: AtomicU64,
}

impl VisualizerFeed {
    pub(crate) fn new() -> Self {
        Self {
            shared: Arc::new(VisualizerShared {
                levels: std::array::from_fn(|_| AtomicU32::new(0.0_f32.to_bits())),
                sequence: AtomicU64::new(0),
            }),
        }
    }

    /// Скопировать уровни без mutex и без ожидания аудиопотока.
    pub(crate) fn snapshot(&self) -> VisualizerFrame {
        let samples = std::array::from_fn(|i| f32::from_bits(self.shared.levels[i].load(Ordering::Relaxed)));
        VisualizerFrame { samples, sequence: self.shared.sequence.load(Ordering::Acquire) }
    }

    pub(crate) fn clear(&self) {
        for level in &self.shared.levels {
            level.store(0.0_f32.to_bits(), Ordering::Relaxed);
        }
        self.shared.sequence.fetch_add(1, Ordering::Release);
    }

    /// Опубликовать новый кадр спектра (только атомарные записи, без блокировок).
    fn publish(&self, levels: &[f32; VISUALIZER_POINTS]) {
        for (slot, level) in self.shared.levels.iter().zip(levels) {
            slot.store(level.to_bits().min(1.0_f32.to_bits()), Ordering::Relaxed);
        }
        self.shared.sequence.fetch_add(1, Ordering::Release);
    }
}

/// Быстрое преобразование Фурье «по основанию 2» (radix-2: размер обязан быть степенью двойки).
/// Таблицы считаются один раз при создании; сам расчёт не выделяет память.
struct Fft {
    bit_reverse: Vec<usize>,
    cos: Vec<f32>,
    sin: Vec<f32>,
}

impl Fft {
    fn new(n: usize) -> Self {
        debug_assert!(n.is_power_of_two());
        let bits = n.trailing_zeros();
        let bit_reverse = (0..n).map(|i| i.reverse_bits() >> (usize::BITS - bits)).collect();
        let cos = (0..n / 2).map(|t| (2.0 * PI * t as f32 / n as f32).cos()).collect();
        let sin = (0..n / 2).map(|t| (2.0 * PI * t as f32 / n as f32).sin()).collect();
        Self { bit_reverse, cos, sin }
    }

    /// Считает FFT на месте: `re` и `im` это действительная и мнимая части сигнала.
    fn run(&self, re: &mut [f32], im: &mut [f32]) {
        let n = re.len();
        // Шаг 1: переставляем элементы в «битово-обратном» порядке (так работает схема бабочек).
        for (i, &j) in self.bit_reverse.iter().enumerate().take(n) {
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        // Шаг 2: «бабочки» (butterfly) склеивают маленькие спектры в большие: длины 2, 4, 8 ... n.
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let (c, s) = (self.cos[k * step], self.sin[k * step]);
                    let (a, b) = (start + k, start + k + half);
                    // Умножение на поворот exp(-i·2πk/len).
                    let tr = re[b] * c + im[b] * s;
                    let ti = im[b] * c - re[b] * s;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
    }
}

/// Какие бины (ячейки) FFT попадают в одну полосу и какая поправка ей положена.
#[derive(Clone, Copy)]
struct Band {
    /// Границы полосы в единицах бинов (дробные).
    lo: f32,
    hi: f32,
    tilt_db: f32,
}

/// Весь расчёт спектра. Все буферы созданы заранее, в аудиопотоке память не выделяется.
struct Spectrum {
    fft: Fft,
    window: Vec<f32>,
    ring: Vec<f32>,
    ring_pos: usize,
    re: Vec<f32>,
    im: Vec<f32>,
    bands: [Band; VISUALIZER_POINTS],
}

/// Верхняя граница полос для данной частоты дискретизации.
fn top_hz(sample_rate: f32) -> f32 {
    MAX_HZ.min(sample_rate * 0.45)
}

/// Граница полосы `i` (0..=VISUALIZER_POINTS) в герцах: частоты растут по экспоненте, как слышит ухо.
fn edge_hz(i: usize, sample_rate: f32) -> f32 {
    let top = top_hz(sample_rate).max(MIN_HZ * 2.0);
    MIN_HZ * (top / MIN_HZ).powf(i as f32 / VISUALIZER_POINTS as f32)
}

impl Spectrum {
    fn new(sample_rate: f32) -> Self {
        let bin_hz = sample_rate / FFT_SIZE as f32;
        let max_bin = (FFT_SIZE / 2 - 1) as f32;
        let bands = std::array::from_fn(|i| Band {
            lo: (edge_hz(i, sample_rate) / bin_hz).clamp(1.0, max_bin),
            hi: (edge_hz(i + 1, sample_rate) / bin_hz).clamp(1.0, max_bin),
            tilt_db: TILT_DB * i as f32 / (VISUALIZER_POINTS - 1) as f32,
        });
        // Окно Хэнна: плавно гасит края куска сигнала, иначе спектр «размазывается».
        let window = (0..FFT_SIZE).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / FFT_SIZE as f32).cos()).collect();
        Self {
            fft: Fft::new(FFT_SIZE),
            window,
            ring: vec![0.0; FFT_SIZE],
            ring_pos: 0,
            re: vec![0.0; FFT_SIZE],
            im: vec![0.0; FFT_SIZE],
            bands,
        }
    }

    fn push_mono(&mut self, sample: f32) {
        self.ring[self.ring_pos] = sample;
        self.ring_pos = (self.ring_pos + 1) % FFT_SIZE;
    }

    fn reset(&mut self) {
        self.ring.fill(0.0);
        self.ring_pos = 0;
    }

    /// Считает уровни всех полос по последним `FFT_SIZE` сэмплам.
    fn analyze(&mut self, out: &mut [f32; VISUALIZER_POINTS]) {
        for i in 0..FFT_SIZE {
            // Старейший сэмпл лежит в `ring_pos`: разворачиваем кольцо по порядку времени.
            self.re[i] = self.ring[(self.ring_pos + i) % FFT_SIZE] * self.window[i];
            self.im[i] = 0.0;
        }
        self.fft.run(&mut self.re, &mut self.im);

        // У окна Хэнна пик синуса амплитуды A равен A·N/4, отсюда множитель 4/N.
        let scale = 4.0 / FFT_SIZE as f32;
        let magnitude = |bin: usize| (self.re[bin] * self.re[bin] + self.im[bin] * self.im[bin]).sqrt();
        for (level, band) in out.iter_mut().zip(self.bands.iter()) {
            let mag = if band.hi - band.lo < 1.0 {
                // Полоса уже одного бина (низкие частоты): плавно смешиваем два соседних бина.
                let pos = (band.lo + band.hi) * 0.5;
                let base = pos.floor() as usize;
                let frac = pos - base as f32;
                magnitude(base) * (1.0 - frac) + magnitude((base + 1).min(FFT_SIZE / 2 - 1)) * frac
            } else {
                // Широкая полоса: берём самый громкий бин внутри.
                (band.lo.floor() as usize..=band.hi.ceil() as usize).map(|bin| magnitude(bin.min(FFT_SIZE / 2 - 1))).fold(0.0_f32, f32::max)
            };
            let db = 20.0 * (mag * scale + 1e-9).log10() + band.tilt_db;
            *level = ((db - DB_FLOOR) / (DB_CEIL - DB_FLOOR)).clamp(0.0, 1.0);
        }
    }
}

/// Прозрачная обёртка над декодером. Передаёт каждый исходный сэмпл дальше без изменений,
/// а раз в ~8,3 мс публикует новый кадр спектра. От ползунка громкости результат не зависит:
/// анализатор стоит до него и видит исходный сигнал трека.
pub(crate) struct AnalyzedSource<S> {
    inner: S,
    feed: VisualizerFeed,
    spectrum: Spectrum,
    levels: [f32; VISUALIZER_POINTS],
    channels: usize,
    /// Сколько сэмплов (по всем каналам) проходит между кадрами спектра.
    window_samples: usize,
    accumulated_samples: usize,
    /// Сумма каналов текущего кадра аудио: из неё получается один моно-сэмпл.
    frame_sum: f32,
    frame_fill: usize,
}

impl<S> AnalyzedSource<S>
where
    S: Source<Item = f32>,
{
    pub(crate) fn new(inner: S, feed: VisualizerFeed) -> Self {
        // В rodio 0.23 каналы и частота это NonZero (число, которое не бывает нулём),
        // поэтому `.get()` достаёт обычное число.
        let channels = usize::from(inner.channels().get());
        let sample_rate = inner.sample_rate().get();
        // 120 кадров спектра в секунду: хватает для высокочастотных дисплеев.
        let window_samples = ((sample_rate as usize * channels) / 120).max(channels);
        Self {
            inner,
            feed,
            spectrum: Spectrum::new(sample_rate as f32),
            levels: [0.0; VISUALIZER_POINTS],
            channels,
            window_samples,
            accumulated_samples: 0,
            frame_sum: 0.0,
            frame_fill: 0,
        }
    }

    fn reset_window(&mut self) {
        self.accumulated_samples = 0;
        self.frame_sum = 0.0;
        self.frame_fill = 0;
        self.spectrum.reset();
        self.feed.clear();
    }
}

impl<S> Iterator for AnalyzedSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;
        if sample.is_finite() {
            self.frame_sum += sample;
        }
        self.frame_fill += 1;
        if self.frame_fill >= self.channels {
            // Все каналы кадра собраны: среднее и есть моно-сэмпл для анализа.
            self.spectrum.push_mono(self.frame_sum / self.channels as f32);
            self.frame_sum = 0.0;
            self.frame_fill = 0;
        }

        self.accumulated_samples += 1;
        if self.accumulated_samples >= self.window_samples {
            self.spectrum.analyze(&mut self.levels);
            self.feed.publish(&self.levels);
            self.accumulated_samples = 0;
        }

        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S> Source for AnalyzedSource<S>
where
    S: Source<Item = f32>,
{
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> NonZero<u16> {
        self.inner.channels()
    }

    fn sample_rate(&self) -> NonZero<u32> {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(position)?;
        self.reset_window();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strongest_band(frame: &VisualizerFrame) -> usize {
        frame.samples.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, _)| i).unwrap()
    }

    #[test]
    fn fft_finds_the_frequency_of_a_sine() {
        // Синус ровно на бине 64: весь пик должен лежать в бине 64.
        let n = 256;
        let fft = Fft::new(n);
        let mut re: Vec<f32> = (0..n).map(|i| (2.0 * PI * 64.0 * i as f32 / n as f32).sin()).collect();
        let mut im = vec![0.0; n];
        fft.run(&mut re, &mut im);
        let mags: Vec<f32> = (0..n / 2).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt()).collect();
        let peak = mags.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(peak, 64);
        assert!((mags[64] - n as f32 / 2.0).abs() < 0.01);
    }

    #[test]
    fn spectrum_peak_sits_at_the_tone_frequency_and_stays_put() {
        let feed = VisualizerFeed::new();
        let source = rodio::source::SineWave::new(440.0).take_duration(Duration::from_millis(250));
        let mut analyzed = AnalyzedSource::new(source, feed.clone());
        let rate = analyzed.sample_rate().get() as f32;
        let mut peaks = Vec::new();
        let mut count = 0;
        while analyzed.next().is_some() {
            count += 1;
            // Берём кадры уже после того, как окно FFT заполнилось звуком.
            if count > FFT_SIZE * 2 && count % 400 == 0 {
                peaks.push(strongest_band(&feed.snapshot()));
            }
        }
        assert!(peaks.len() > 3);
        // Пик не ползёт: одна и та же полоса во всех кадрах.
        assert!(peaks.iter().all(|p| *p == peaks[0]));
        // И эта полоса лежит на 440 Гц.
        let (lo, hi) = (edge_hz(peaks[0], rate), edge_hz(peaks[0] + 1, rate));
        assert!(lo * 0.93 <= 440.0 && 440.0 <= hi * 1.07, "полоса {lo}..{hi} Гц");
    }

    #[test]
    fn silence_gives_zero_levels() {
        let feed = VisualizerFeed::new();
        let source =
            rodio::source::Zero::new(NonZero::new(1).unwrap(), NonZero::new(48_000).unwrap()).take_duration(Duration::from_millis(100));
        let mut analyzed = AnalyzedSource::new(source, feed.clone());
        while analyzed.next().is_some() {}
        assert!(feed.snapshot().samples.iter().all(|level| *level == 0.0));
    }

    #[test]
    fn clear_resets_the_snapshot() {
        let feed = VisualizerFeed::new();
        feed.publish(&[0.4; VISUALIZER_POINTS]);
        feed.clear();
        let frame = feed.snapshot();
        assert!(frame.samples.iter().all(|sample| *sample == 0.0));
        assert_eq!(frame.sequence, 2);
    }
}

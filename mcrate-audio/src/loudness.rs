//! Измерение громкости трека по стандарту ITU-R BS.1770 / EBU R128 (единица LUFS).
//!
//! Как считается: звук пропускается через «K-фильтр» (он имитирует чувствительность уха:
//! чуть усиливает верха и режет самые низы), затем нарезается на блоки по 400 мс с шагом
//! 100 мс, у каждого блока считается средняя энергия. Тихие блоки (паузы, затишья)
//! отбрасываются двойным «гейтом»: абсолютным (-70 LUFS) и относительным (на 10 LU ниже
//! средней громкости). Среднее по оставшимся блокам и есть громкость трека.
//!
//! Измеритель потоковый: ему можно скармливать отсчёты по мере декодирования, весь трек в
//! памяти не хранится (на каждые 100 мс звука остаётся одно число).

use std::collections::VecDeque;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use rodio::{Decoder, Source};

use crate::AudioError;

/// Результат замера трека.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// Интегральная громкость в LUFS (чем меньше, тем тише; типичный поп-трек: -14…-8).
    pub lufs: f32,
    /// Самый большой по модулю отсчёт (линейно, 1.0 = 0 дБFS). Нужен, чтобы не усиливать
    /// тихий трек до искажений.
    pub peak: f32,
}

/// Чем кончился замер файла.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Measured {
    Done(Loudness),
    /// Измерить нечего: тишина (всё ниже -70 LUFS), трек короче 400 мс или в файле
    /// посреди трека поменялся формат звука.
    Unmeasurable,
    /// Замер прервали флагом отмены.
    Cancelled,
}

/// Длина блока в шагах: 4 шага по 100 мс = блок 400 мс.
const BLOCK_STEPS: usize = 4;
/// Абсолютный порог гейта, LUFS.
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
/// Относительный порог гейта: на столько LU ниже средней громкости.
const RELATIVE_GATE_LU: f64 = 10.0;
/// Поправка из стандарта: K-фильтр на 997 Гц усиливает на +0,691 дБ, поправка это компенсирует.
/// В итоге синус 997 Гц на полной шкале в одном канале = -3,01 LUFS, в обоих каналах = 0 LUFS.
const LUFS_OFFSET: f64 = -0.691;

/// Громкость (LUFS) из средней энергии.
fn lufs_of(energy: f64) -> f64 {
    LUFS_OFFSET + 10.0 * energy.log10()
}

/// Обратное преобразование: энергия, соответствующая громкости.
fn energy_of(lufs: f64) -> f64 {
    10f64.powf((lufs - LUFS_OFFSET) / 10.0)
}

/// Один фильтр второго порядка (biquad), форма «transposed direct form II».
#[derive(Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// K-фильтр: ступень 1 (полка +4 дБ на верхах) и ступень 2 (срез ниже ~38 Гц).
/// Коэффициенты в стандарте даны для 48 кГц, здесь пересчитываются под любую частоту
/// (формулы те же, что в эталонных реализациях libebur128 и pyloudnorm).
#[derive(Clone, Copy)]
struct KWeighting {
    shelf: Biquad,
    high_pass: Biquad,
}

impl KWeighting {
    fn new(sample_rate: f64) -> Self {
        // Ступень 1: высокочастотная полка
        let gain_db = 3.999_843_853_973_347;
        let q = 0.707_175_236_955_419_6;
        let fc = 1_681.974_450_955_532;
        let k = (std::f64::consts::PI * fc / sample_rate).tan();
        let vh = 10f64.powf(gain_db / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad {
            b0: (vh + vb * k / q + k * k) / a0,
            b1: 2.0 * (k * k - vh) / a0,
            b2: (vh - vb * k / q + k * k) / a0,
            a1: 2.0 * (k * k - 1.0) / a0,
            a2: (1.0 - k / q + k * k) / a0,
            z1: 0.0,
            z2: 0.0,
        };

        // Ступень 2: фильтр высоких частот (RLB)
        let q = 0.500_327_037_325_395_3;
        let fc = 38.135_470_876_139_82;
        let k = (std::f64::consts::PI * fc / sample_rate).tan();
        let a0 = 1.0 + k / q + k * k;
        let high_pass = Biquad {
            b0: 1.0,
            b1: -2.0,
            b2: 1.0,
            a1: 2.0 * (k * k - 1.0) / a0,
            a2: (1.0 - k / q + k * k) / a0,
            z1: 0.0,
            z2: 0.0,
        };
        Self { shelf, high_pass }
    }

    fn process(&mut self, x: f64) -> f64 {
        self.high_pass.process(self.shelf.process(x))
    }
}

/// Потоковый измеритель интегральной громкости.
pub struct LoudnessMeter {
    channels: usize,
    filters: Vec<KWeighting>,
    /// Вес канала в сумме энергий. Моно считаем как два одинаковых канала: rodio выводит
    /// моно сразу в оба динамика, и на слух это на 3 LU громче одиночного канала.
    channel_weight: f64,
    /// Сколько кадров (по одному отсчёту на канал) в шаге 100 мс.
    step_frames: usize,
    frames_in_step: usize,
    channel_in_frame: usize,
    step_energy: f64,
    /// Энергии последних шагов (для скользящего блока в 4 шага).
    recent_steps: VecDeque<f64>,
    /// Средняя энергия каждого завершённого блока 400 мс.
    blocks: Vec<f64>,
    peak: f32,
}

impl LoudnessMeter {
    /// `channels` и `sample_rate` берутся у декодера. Нулевые значения (так не бывает) превращаются в 1.
    pub fn new(channels: u16, sample_rate: u32) -> Self {
        let channels = usize::from(channels.max(1));
        let sample_rate = sample_rate.max(1);
        Self {
            channels,
            filters: vec![KWeighting::new(f64::from(sample_rate)); channels],
            channel_weight: if channels == 1 { 2.0 } else { 1.0 },
            step_frames: ((f64::from(sample_rate) / 10.0).round() as usize).max(1),
            frames_in_step: 0,
            channel_in_frame: 0,
            step_energy: 0.0,
            recent_steps: VecDeque::with_capacity(BLOCK_STEPS),
            blocks: Vec::new(),
            peak: 0.0,
        }
    }

    /// Скормить измерителю отсчёты: чередуются по каналам (Л, П, Л, П, …), как отдаёт декодер.
    pub fn push(&mut self, samples: &[f32]) {
        for &sample in samples {
            self.push_one(sample);
        }
    }

    fn push_one(&mut self, sample: f32) {
        self.peak = self.peak.max(sample.abs());
        let filtered = self.filters[self.channel_in_frame].process(f64::from(sample));
        self.step_energy += self.channel_weight * filtered * filtered;

        self.channel_in_frame += 1;
        if self.channel_in_frame == self.channels {
            self.channel_in_frame = 0;
            self.frames_in_step += 1;
            if self.frames_in_step == self.step_frames {
                self.finish_step();
            }
        }
    }

    fn finish_step(&mut self) {
        if self.recent_steps.len() == BLOCK_STEPS {
            self.recent_steps.pop_front();
        }
        self.recent_steps.push_back(self.step_energy);
        self.step_energy = 0.0;
        self.frames_in_step = 0;
        if self.recent_steps.len() == BLOCK_STEPS {
            let total: f64 = self.recent_steps.iter().sum();
            self.blocks.push(total / (BLOCK_STEPS * self.step_frames) as f64);
        }
    }

    /// Итог по всему, что скормили. None = измерить нечего (тишина или меньше 400 мс звука).
    pub fn finish(&self) -> Option<Loudness> {
        let absolute = energy_of(ABSOLUTE_GATE_LUFS);
        let above_absolute: Vec<f64> = self.blocks.iter().copied().filter(|&e| e > absolute).collect();
        if above_absolute.is_empty() {
            return None;
        }
        let mean_all = above_absolute.iter().sum::<f64>() / above_absolute.len() as f64;
        let relative = energy_of(lufs_of(mean_all) - RELATIVE_GATE_LU);

        let (sum, count) = above_absolute
            .iter()
            .filter(|&&e| e > relative)
            .fold((0.0, 0usize), |(s, n), &e| (s + e, n + 1));
        // Блоки, прошедшие относительный гейт, есть всегда (самый громкий выше среднего), но на всякий случай
        if count == 0 {
            return None;
        }
        Some(Loudness { lufs: lufs_of(sum / count as f64) as f32, peak: self.peak })
    }
}

/// Как часто (в отсчётах) замер файла смотрит на флаг отмены и на смену формата звука.
const CHECK_EVERY: usize = 1 << 16;

/// Декодирует файл целиком и измеряет громкость. Работает долго (весь трек), поэтому зовите
/// из фонового потока; флаг `cancel` позволяет прервать замер.
pub fn measure_file(path: &Path, cancel: &AtomicBool) -> Result<Measured, AudioError> {
    let file = File::open(path)?;
    let mut decoder = Decoder::try_from(file)?;
    let channels = decoder.channels().get();
    let sample_rate = decoder.sample_rate().get();
    let mut meter = LoudnessMeter::new(channels, sample_rate);

    let mut buffer = Vec::with_capacity(CHECK_EVERY);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Measured::Cancelled);
        }
        if decoder.channels().get() != channels || decoder.sample_rate().get() != sample_rate {
            return Ok(Measured::Unmeasurable); // формат поменялся на ходу: честно считать нечем
        }
        buffer.clear();
        buffer.extend(decoder.by_ref().take(CHECK_EVERY));
        if buffer.is_empty() {
            break; // файл кончился
        }
        meter.push(&buffer);
    }
    Ok(match meter.finish() {
        Some(loudness) => Measured::Done(loudness),
        None => Measured::Unmeasurable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// Синус `freq` Гц с амплитудой `amp`, `channels` одинаковых каналов, `seconds` секунд.
    fn sine(freq: f32, amp: f32, rate: u32, channels: u16, seconds: f32) -> Vec<f32> {
        let frames = (rate as f32 * seconds) as usize;
        let mut out = Vec::with_capacity(frames * usize::from(channels));
        for n in 0..frames {
            let v = amp * (TAU * freq * n as f32 / rate as f32).sin();
            for _ in 0..channels {
                out.push(v);
            }
        }
        out
    }

    fn measure(samples: &[f32], channels: u16, rate: u32) -> Option<Loudness> {
        let mut meter = LoudnessMeter::new(channels, rate);
        meter.push(samples);
        meter.finish()
    }

    fn assert_close(actual: f32, expected: f32, tolerance: f32) {
        assert!((actual - expected).abs() <= tolerance, "ожидали {expected} ± {tolerance}, получили {actual}");
    }

    #[test]
    fn matches_reference_signals_from_the_standard() {
        // EBU Tech 3341: синус 997 Гц, -23 dBFS, в обоих каналах = ровно -23,0 LUFS
        let amp = 10f32.powf(-23.0 / 20.0);
        let l = measure(&sine(997.0, amp, 48_000, 2, 5.0), 2, 48_000).unwrap();
        assert_close(l.lufs, -23.0, 0.05);
        assert_close(l.peak, amp, 0.001);

        // Полная шкала в обоих каналах = 0 LUFS
        let l = measure(&sine(997.0, 1.0, 48_000, 2, 5.0), 2, 48_000).unwrap();
        assert_close(l.lufs, 0.0, 0.05);
        assert_close(l.peak, 1.0, 0.001);

        // Полная шкала только в левом канале (правый молчит) = -3,01 LUFS
        let both = sine(997.0, 1.0, 48_000, 2, 5.0);
        let left_only: Vec<f32> = both.chunks(2).flat_map(|frame| [frame[0], 0.0]).collect();
        let l = measure(&left_only, 2, 48_000).unwrap();
        assert_close(l.lufs, -3.01, 0.05);
    }

    #[test]
    fn halving_amplitude_lowers_loudness_by_6_db() {
        let loud = measure(&sine(997.0, 0.5, 48_000, 2, 5.0), 2, 48_000).unwrap();
        let quiet = measure(&sine(997.0, 0.25, 48_000, 2, 5.0), 2, 48_000).unwrap();
        assert_close(loud.lufs - quiet.lufs, 6.02, 0.05);
    }

    #[test]
    fn result_does_not_depend_on_sample_rate() {
        let a = measure(&sine(997.0, 0.5, 44_100, 2, 5.0), 2, 44_100).unwrap();
        let b = measure(&sine(997.0, 0.5, 48_000, 2, 5.0), 2, 48_000).unwrap();
        let c = measure(&sine(997.0, 0.5, 22_050, 2, 5.0), 2, 22_050).unwrap();
        assert_close(a.lufs, b.lufs, 0.1);
        assert_close(c.lufs, b.lufs, 0.2);
    }

    #[test]
    fn mono_counts_as_the_same_signal_in_both_speakers() {
        let mono = measure(&sine(997.0, 0.5, 48_000, 1, 5.0), 1, 48_000).unwrap();
        let stereo = measure(&sine(997.0, 0.5, 48_000, 2, 5.0), 2, 48_000).unwrap();
        assert_close(mono.lufs, stereo.lufs, 0.05);
    }

    #[test]
    fn low_frequencies_count_less_than_mid_frequencies() {
        // K-фильтр режет самые низы: синус 40 Гц при той же амплитуде заметно «тише» синуса 1 кГц
        let bass = measure(&sine(40.0, 0.5, 48_000, 2, 5.0), 2, 48_000).unwrap();
        let mid = measure(&sine(997.0, 0.5, 48_000, 2, 5.0), 2, 48_000).unwrap();
        assert!(bass.lufs < mid.lufs - 2.0, "бас {} против середины {}", bass.lufs, mid.lufs);
    }

    #[test]
    fn silence_is_unmeasurable() {
        assert_eq!(measure(&vec![0.0; 48_000 * 2 * 3], 2, 48_000), None);
        // Очень тихий шум ниже -70 LUFS тоже отсекается абсолютным гейтом
        assert_eq!(measure(&sine(997.0, 0.00001, 48_000, 2, 3.0), 2, 48_000), None);
    }

    #[test]
    fn shorter_than_one_block_is_unmeasurable() {
        assert_eq!(measure(&sine(997.0, 0.5, 48_000, 2, 0.3), 2, 48_000), None);
    }

    #[test]
    fn relative_gate_ignores_quiet_passages() {
        // 4 с громко (0,5) и 4 с очень тихо (0,01, на 34 дБ ниже): тихая часть не должна утянуть
        // громкость вниз, ведь она ниже относительного порога
        let mut mixed = sine(997.0, 0.5, 48_000, 2, 4.0);
        mixed.extend(sine(997.0, 0.01, 48_000, 2, 4.0));
        let gated = measure(&mixed, 2, 48_000).unwrap();
        let loud_only = measure(&sine(997.0, 0.5, 48_000, 2, 4.0), 2, 48_000).unwrap();
        assert_close(gated.lufs, loud_only.lufs, 0.2);
    }

    #[test]
    fn peak_is_the_largest_sample() {
        let mut samples = sine(997.0, 0.3, 48_000, 2, 2.0);
        samples[1000] = -0.9;
        let l = measure(&samples, 2, 48_000).unwrap();
        assert_close(l.peak, 0.9, 0.0001);
    }

    #[test]
    fn feeding_in_chunks_gives_the_same_result() {
        let samples = sine(997.0, 0.5, 48_000, 2, 3.0);
        let whole = measure(&samples, 2, 48_000).unwrap();
        let mut meter = LoudnessMeter::new(2, 48_000);
        for chunk in samples.chunks(1001) {
            meter.push(chunk); // нечётный размер: куски режут кадры пополам, измеритель это переживает
        }
        assert_eq!(meter.finish().unwrap(), whole);
    }

    /// Пишет 16-битный WAV с синусом: `channels` каналов, `rate` Гц.
    fn write_wav(path: &Path, samples_f32: &[f32], channels: u16, rate: u32) {
        let data: Vec<u8> = samples_f32.iter().flat_map(|s| ((s * 32767.0) as i16).to_le_bytes()).collect();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        wav.extend_from_slice(&(channels * 2).to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::write(path, wav).unwrap();
    }

    fn temp_wav(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mcrate-loudness-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn measures_a_real_wav_file() {
        let path = temp_wav("tone.wav");
        write_wav(&path, &sine(997.0, 0.5, 44_100, 2, 3.0), 2, 44_100);
        let cancel = AtomicBool::new(false);
        let Measured::Done(l) = measure_file(&path, &cancel).unwrap() else { panic!("ждали Done") };
        // Амплитуда 0,5 в двух каналах: 0 - 6,02 = -6,02 LUFS (допуск на 16-битное округление)
        assert_close(l.lufs, -6.02, 0.1);
        assert_close(l.peak, 0.5, 0.01);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn silent_file_is_unmeasurable_and_cancel_is_respected() {
        let path = temp_wav("silent.wav");
        write_wav(&path, &vec![0.0; 44_100 * 2 * 2], 2, 44_100);
        assert_eq!(measure_file(&path, &AtomicBool::new(false)).unwrap(), Measured::Unmeasurable);
        assert_eq!(measure_file(&path, &AtomicBool::new(true)).unwrap(), Measured::Cancelled);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_an_error() {
        let result = measure_file(Path::new("/нет/такого/файла.wav"), &AtomicBool::new(false));
        assert!(result.is_err());
    }
}

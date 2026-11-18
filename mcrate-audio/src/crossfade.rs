//! Кроссфейд: чистая логика (без звука и потоков), поэтому тестируется без устройства.
//!
//! Два трека играют одновременно: уходящий затихает, приходящий нарастает. Кривая РАВНОЙ МОЩНОСТИ
//! (equal power): `out = cos(t·π/2)`, `in = sin(t·π/2)`. Сумма квадратов всегда 1, поэтому
//! суммарная громкость на середине фейда не проваливается (у линейной кривой там «яма» ≈ -3 дБ).
//! Решение записано в `CLAUDE.md`, «Принятые решения».

use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

/// Минимальная длина фейда: короче неразличимо на слух и дало бы щелчок.
pub(crate) const MIN_FADE: Duration = Duration::from_millis(50);

/// Множители громкости (уходящий, приходящий) в точке `t` ∈ [0, 1] (вне границ обрезается).
pub(crate) fn equal_power(t: f32) -> (f32, f32) {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
    ((t * FRAC_PI_2).cos(), (t * FRAC_PI_2).sin())
}

/// Ход идущего фейда. Время копится по шагам `advance(dt)`, а не по часам: пауза во время фейда
/// (движок перестаёт звать `advance`) замораживает и сам фейд.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fade {
    total: Duration,
    elapsed: Duration,
}

impl Fade {
    /// Фейд длиной `total` (меньше `MIN_FADE` поднимается до `MIN_FADE`).
    pub(crate) fn new(total: Duration) -> Self {
        Self { total: total.max(MIN_FADE), elapsed: Duration::ZERO }
    }

    /// Сдвинуть фейд вперёд.
    pub(crate) fn advance(&mut self, dt: Duration) {
        self.elapsed = (self.elapsed + dt).min(self.total);
    }

    /// Доля пройденного, 0.0 … 1.0.
    pub(crate) fn progress(&self) -> f32 {
        self.elapsed.as_secs_f32() / self.total.as_secs_f32()
    }

    pub(crate) fn is_done(&self) -> bool {
        self.elapsed >= self.total
    }

    /// Множители (уходящий, приходящий) сейчас.
    pub(crate) fn gains(&self) -> (f32, f32) {
        equal_power(self.progress())
    }
}

/// Длина фейда для трека: заказанная, но не длиннее оставшегося до конца трека (трек короче
/// фейда укорачивает фейд, а не ломается) и не короче `MIN_FADE`. `None`, если фейду нет места
/// (до конца трека меньше `MIN_FADE`).
#[allow(dead_code)] // движок начнёт звать в шаге 5б
pub(crate) fn fit(requested: Duration, remaining: Duration) -> Option<Duration> {
    let len = requested.min(remaining);
    (len >= MIN_FADE).then_some(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn curve_has_exact_ends() {
        let (out0, in0) = equal_power(0.0);
        let (out1, in1) = equal_power(1.0);
        assert!(close(out0, 1.0) && close(in0, 0.0));
        assert!(close(out1, 0.0) && close(in1, 1.0));
    }

    #[test]
    fn power_stays_constant_all_along() {
        // Сумма квадратов = 1 в любой точке (линейная кривая дала бы 0,5 на середине)
        for i in 0..=100 {
            let (out, inc) = equal_power(i as f32 / 100.0);
            assert!(close(out * out + inc * inc, 1.0), "t = {i}/100");
        }
    }

    #[test]
    fn curve_is_monotonic_and_clamped() {
        let mut last = equal_power(0.0);
        for i in 1..=100 {
            let now = equal_power(i as f32 / 100.0);
            assert!(now.0 <= last.0 && now.1 >= last.1);
            last = now;
        }
        assert_eq!(equal_power(-3.0), equal_power(0.0));
        assert_eq!(equal_power(7.0), equal_power(1.0));
        assert_eq!(equal_power(f32::NAN), equal_power(1.0));
    }

    #[test]
    fn fade_advances_by_steps_and_stops_at_end() {
        let mut fade = Fade::new(Duration::from_secs(4));
        assert!(!fade.is_done() && close(fade.progress(), 0.0));
        fade.advance(Duration::from_secs(1));
        assert!(close(fade.progress(), 0.25));
        fade.advance(Duration::from_secs(10)); // с запасом: дальше конца не уходит
        assert!(fade.is_done() && close(fade.progress(), 1.0));
        let (out, inc) = fade.gains();
        assert!(close(out, 0.0) && close(inc, 1.0));
    }

    #[test]
    fn zero_length_fade_is_lifted_to_minimum() {
        let mut fade = Fade::new(Duration::ZERO);
        assert!(!fade.is_done()); // не делится на ноль и не «готов» до первого шага
        fade.advance(MIN_FADE);
        assert!(fade.is_done());
    }

    #[test]
    fn short_track_shortens_the_fade() {
        let secs = Duration::from_secs;
        assert_eq!(fit(secs(5), secs(30)), Some(secs(5)));
        assert_eq!(fit(secs(5), secs(2)), Some(secs(2))); // трек короче фейда: фейд короче
        assert_eq!(fit(secs(5), Duration::from_millis(10)), None); // места нет
        assert_eq!(fit(secs(5), Duration::ZERO), None);
    }
}

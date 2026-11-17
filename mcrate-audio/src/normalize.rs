//! Нормализация громкости: настройки воспроизведения, расчёт усиления по замеру трека
//! и плавное применение (без щелчков). Здесь нет работы со звуком и потоками, поэтому
//! всё проверяется обычными тестами, звуковое устройство не нужно.

use std::collections::HashMap;
use std::time::Duration;

use mcrate_core::ProviderKind;

use crate::loudness::Loudness;

/// Целевая громкость по умолчанию: как у большинства стриминговых сервисов.
pub const DEFAULT_TARGET_LUFS: f32 = -14.0;
/// Допустимые границы целевой громкости, LUFS.
const TARGET_RANGE: (f32, f32) = (-30.0, -6.0);
/// Усиливать можно не больше чем в 4 раза (+12 дБ).
const MAX_GAIN: f32 = 4.0;
/// Ослаблять можно не больше чем в 20 раз (-26 дБ): защита от странных замеров.
const MIN_GAIN: f32 = 0.05;
/// Как быстро усиление меняется на ходу, дБ в секунду. 6 дБ/с = плавный «подъезд», а не скачок.
const RAMP_DB_PER_SEC: f32 = 6.0;
/// Сколько треков помним в кэше замеров. При переполнении кэш просто очищается.
const MAX_CACHED: usize = 4096;

/// Настройки воспроизведения. Позже сюда добавятся кроссфейд и бесшовность.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackSettings {
    /// Выравнивать громкость треков (чтобы не приходилось крутить ползунок между треками).
    pub normalize_volume: bool,
    /// До какой громкости выравнивать, LUFS (-14 = как у стримингов, -23 = стандарт вещания).
    pub target_lufs: f32,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self { normalize_volume: false, target_lufs: DEFAULT_TARGET_LUFS }
    }
}

impl PlaybackSettings {
    /// Приводит значения к допустимым: целевая громкость зажимается в границы, а
    /// нечисло (NaN) заменяется значением по умолчанию.
    pub fn sanitized(mut self) -> Self {
        self.target_lufs = if self.target_lufs.is_finite() {
            self.target_lufs.clamp(TARGET_RANGE.0, TARGET_RANGE.1)
        } else {
            DEFAULT_TARGET_LUFS
        };
        self
    }
}

/// Во сколько раз надо изменить громкость трека, чтобы он зазвучал на `target_lufs`.
///
/// Ослабляем всегда, когда трек громче цели. Усиливаем, только пока хватает запаса до
/// 0 дБFS по пику: иначе тихий трек с резкими пиками начал бы хрипеть. Громкость трека,
/// у которой нет конечного значения, не меняем.
pub fn gain_for(loudness: Loudness, target_lufs: f32) -> f32 {
    if !loudness.lufs.is_finite() {
        return 1.0;
    }
    let wanted = 10f32.powf((target_lufs - loudness.lufs) / 20.0);
    let gain = if wanted > 1.0 {
        // Запас по пику; если пик выше единицы (декодер выдал «перегруз»), усиливать нельзя совсем
        let headroom = if loudness.peak > 0.0 { (1.0 / loudness.peak).max(1.0) } else { MAX_GAIN };
        wanted.min(headroom).min(MAX_GAIN)
    } else {
        wanted
    };
    gain.max(MIN_GAIN)
}

/// Какой трек: сервис и его id (так же трек опознаёт остальной проект).
pub type TrackKey = (ProviderKind, String);

/// Помнит замеры, решает, какое усиление нужно сейчас, и плавно к нему подходит.
pub struct Normalizer {
    settings: PlaybackSettings,
    /// Замеры треков за эту сессию. `None` = мерили, но измерить нечего (тишина).
    cache: HashMap<TrackKey, Option<Loudness>>,
    current: Option<TrackKey>,
    /// Усиление, к которому идём.
    target: f32,
    /// Усиление, которое применено сейчас.
    gain: f32,
}

impl Normalizer {
    pub fn new(settings: PlaybackSettings) -> Self {
        Self { settings: settings.sanitized(), cache: HashMap::new(), current: None, target: 1.0, gain: 1.0 }
    }

    pub fn settings(&self) -> PlaybackSettings {
        self.settings
    }

    /// Усиление, которое сейчас надо применить (1.0 = без изменений).
    pub fn gain(&self) -> f32 {
        self.gain
    }

    /// Ключ текущего трека.
    pub fn current(&self) -> Option<&TrackKey> {
        self.current.as_ref()
    }

    /// Новые настройки. Усиление поедет к новому значению плавно; усиление сразу не прыгает.
    pub fn set_settings(&mut self, settings: PlaybackSettings) {
        self.settings = settings.sanitized();
        self.target = self.target_for_current();
    }

    /// Начался новый трек. Новый трек стартует сразу с нужным усилением (границу между
    /// треками плавить не нужно). Если замера нет, начинаем без изменений.
    pub fn begin_track(&mut self, key: TrackKey) {
        self.current = Some(key);
        self.target = self.target_for_current();
        self.gain = self.target;
    }

    /// Трек закончился или остановлен.
    pub fn end_track(&mut self) {
        self.current = None;
        self.target = 1.0;
        self.gain = 1.0;
    }

    /// Нужно ли мерить текущий трек: нормализация включена, трек есть, а замера нет.
    pub fn needs_measure(&self) -> bool {
        self.settings.normalize_volume && self.current.as_ref().is_some_and(|key| !self.cache.contains_key(key))
    }

    /// Записать результат замера. Если это текущий трек, усиление поедет к нужному значению плавно.
    pub fn record(&mut self, key: TrackKey, loudness: Option<Loudness>) {
        if self.cache.len() >= MAX_CACHED {
            self.cache.clear();
        }
        let is_current = self.current.as_ref() == Some(&key);
        self.cache.insert(key, loudness);
        if is_current {
            self.target = self.target_for_current();
        }
    }

    /// Прошло `dt` времени: подвинуть усиление к цели. Возвращает новое значение, если оно изменилось.
    pub fn tick(&mut self, dt: Duration) -> Option<f32> {
        if self.gain == self.target {
            return None;
        }
        let current_db = 20.0 * self.gain.log10();
        let target_db = 20.0 * self.target.log10();
        let max_step = RAMP_DB_PER_SEC * dt.as_secs_f32();
        let diff = target_db - current_db;
        self.gain = if diff.abs() <= max_step.max(0.001) {
            self.target
        } else {
            10f32.powf((current_db + diff.signum() * max_step) / 20.0)
        };
        Some(self.gain)
    }

    fn target_for_current(&self) -> f32 {
        if !self.settings.normalize_volume {
            return 1.0;
        }
        match self.current.as_ref().and_then(|key| self.cache.get(key)) {
            Some(Some(loudness)) => gain_for(*loudness, self.settings.target_lufs),
            _ => 1.0, // замера нет или трек не измерить: не трогаем
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str) -> TrackKey {
        (ProviderKind::SoundCloud, id.to_string())
    }

    fn loud(lufs: f32, peak: f32) -> Loudness {
        Loudness { lufs, peak }
    }

    fn on() -> PlaybackSettings {
        PlaybackSettings { normalize_volume: true, ..PlaybackSettings::default() }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 0.01, "ожидали {expected}, получили {actual}");
    }

    #[test]
    fn loud_track_is_attenuated_to_target() {
        // -8 LUFS при цели -14: надо тише на 6 дБ, то есть примерно вдвое
        assert_close(gain_for(loud(-8.0, 1.0), -14.0), 0.501);
    }

    #[test]
    fn quiet_track_is_boosted_only_within_peak_headroom() {
        // -20 LUFS при цели -14: хотелось бы +6 дБ (x2). Пик 0,4 даёт запас x2,5: усиливаем полностью
        assert_close(gain_for(loud(-20.0, 0.4), -14.0), 1.995);
        // Пик 0,8 даёт запас только x1,25: выше нельзя, иначе искажения
        assert_close(gain_for(loud(-20.0, 0.8), -14.0), 1.25);
        // Пик 1,0: усиливать нельзя совсем
        assert_close(gain_for(loud(-20.0, 1.0), -14.0), 1.0);
    }

    #[test]
    fn overshooting_peak_never_attenuates_a_track_that_is_already_on_target() {
        assert_close(gain_for(loud(-14.0, 1.3), -14.0), 1.0);
    }

    #[test]
    fn gain_is_limited_both_ways() {
        assert_close(gain_for(loud(-60.0, 0.01), -14.0), MAX_GAIN); // хотелось бы x200
        assert_close(gain_for(loud(30.0, 1.0), -14.0), MIN_GAIN); // абсурдно громкий замер
        assert_close(gain_for(loud(f32::NEG_INFINITY, 0.5), -14.0), 1.0);
        assert_close(gain_for(loud(f32::NAN, 0.5), -14.0), 1.0);
    }

    #[test]
    fn settings_are_sanitized() {
        let s = PlaybackSettings { normalize_volume: true, target_lufs: -100.0 }.sanitized();
        assert_eq!(s.target_lufs, -30.0);
        let s = PlaybackSettings { normalize_volume: true, target_lufs: 5.0 }.sanitized();
        assert_eq!(s.target_lufs, -6.0);
        let s = PlaybackSettings { normalize_volume: true, target_lufs: f32::NAN }.sanitized();
        assert_eq!(s.target_lufs, DEFAULT_TARGET_LUFS);
        assert!(!PlaybackSettings::default().normalize_volume);
    }

    #[test]
    fn disabled_normalizer_never_changes_gain_or_asks_for_measurement() {
        let mut n = Normalizer::new(PlaybackSettings::default());
        n.begin_track(key("a"));
        assert!(!n.needs_measure());
        n.record(key("a"), Some(loud(-6.0, 1.0)));
        assert_eq!(n.gain(), 1.0);
        assert_eq!(n.tick(Duration::from_secs(1)), None);
    }

    #[test]
    fn unmeasured_track_starts_at_unity_then_ramps_to_target() {
        let mut n = Normalizer::new(on());
        n.begin_track(key("a"));
        assert!(n.needs_measure());
        assert_eq!(n.gain(), 1.0);

        n.record(key("a"), Some(loud(-8.0, 1.0))); // цель: x0,501 (-6 дБ)
        assert!(!n.needs_measure());
        assert_eq!(n.gain(), 1.0); // применяется не скачком...

        let first = n.tick(Duration::from_millis(500)).unwrap(); // 6 дБ/с * 0,5 с = -3 дБ
        assert_close(first, 0.708);
        let second = n.tick(Duration::from_millis(500)).unwrap(); // ещё -3 дБ: цель достигнута
        assert_close(second, 0.501);
        assert_eq!(n.tick(Duration::from_millis(500)), None); // дальше менять нечего
    }

    #[test]
    fn known_track_starts_immediately_at_the_right_gain() {
        let mut n = Normalizer::new(on());
        n.begin_track(key("a"));
        n.record(key("a"), Some(loud(-8.0, 1.0)));
        // Следующее проигрывание того же трека: замер уже есть, плавать не нужно
        n.begin_track(key("b"));
        n.begin_track(key("a"));
        assert_close(n.gain(), 0.501);
        assert!(!n.needs_measure());
    }

    #[test]
    fn late_result_for_another_track_does_not_touch_current_gain() {
        let mut n = Normalizer::new(on());
        n.begin_track(key("a"));
        n.record(key("b"), Some(loud(-8.0, 1.0))); // замер b пришёл, когда уже играет a
        assert_eq!(n.tick(Duration::from_secs(1)), None);
        assert_eq!(n.gain(), 1.0);
        n.begin_track(key("b")); // зато при запуске b он уже учтён
        assert_close(n.gain(), 0.501);
    }

    #[test]
    fn unmeasurable_track_keeps_unity_and_is_not_measured_again() {
        let mut n = Normalizer::new(on());
        n.begin_track(key("silent"));
        n.record(key("silent"), None);
        assert_eq!(n.gain(), 1.0);
        assert_eq!(n.tick(Duration::from_secs(1)), None);
        assert!(!n.needs_measure());
    }

    #[test]
    fn toggling_on_and_off_mid_track_ramps_smoothly() {
        let mut n = Normalizer::new(PlaybackSettings::default());
        n.begin_track(key("a"));
        n.record(key("a"), Some(loud(-8.0, 1.0)));
        assert_eq!(n.gain(), 1.0);

        n.set_settings(on()); // замер уже есть: цель сразу известна, но едем плавно
        assert_eq!(n.gain(), 1.0);
        assert!(n.tick(Duration::from_millis(100)).unwrap() < 1.0);

        n.set_settings(PlaybackSettings::default()); // выключили: плавно назад к 1.0
        let mut g = n.gain();
        for _ in 0..100 {
            if let Some(next) = n.tick(Duration::from_millis(100)) {
                assert!(next >= g, "громкость должна расти монотонно");
                g = next;
            }
        }
        assert_eq!(n.gain(), 1.0);
    }

    #[test]
    fn changing_target_recomputes_gain_for_current_track() {
        let mut n = Normalizer::new(on());
        n.begin_track(key("a"));
        n.record(key("a"), Some(loud(-8.0, 1.0)));
        n.set_settings(PlaybackSettings { normalize_volume: true, target_lufs: -20.0 });
        n.tick(Duration::from_secs(10));
        assert_close(n.gain(), 0.251); // -12 дБ
    }

    #[test]
    fn end_track_resets_gain() {
        let mut n = Normalizer::new(on());
        n.begin_track(key("a"));
        n.record(key("a"), Some(loud(-8.0, 1.0)));
        n.begin_track(key("a"));
        assert!(n.gain() < 1.0);
        n.end_track();
        assert_eq!(n.gain(), 1.0);
        assert!(n.current().is_none());
        assert!(!n.needs_measure());
    }

    #[test]
    fn cache_is_bounded() {
        let mut n = Normalizer::new(on());
        for i in 0..(MAX_CACHED + 10) {
            n.record(key(&i.to_string()), None);
        }
        assert!(n.cache.len() <= MAX_CACHED);
    }
}

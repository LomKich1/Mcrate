//! Положение ползунка громкости и выключение звука (mute) кнопкой.
//!
//! Правило: повторное нажатие возвращает ровно ту громкость, что была до выключения, даже самую маленькую.

/// Громкость при первом запуске и когда возвращать нечего (ползунок увели в ноль руками).
pub const DEFAULT_VOLUME: f32 = 0.8;

/// Ползунок и громкость, которую вернёт повторное нажатие кнопки. Обе от 0.0 до 1.0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume {
    level: f32,
    before_mute: f32,
}

impl Default for Volume {
    fn default() -> Self {
        Self { level: DEFAULT_VOLUME, before_mute: DEFAULT_VOLUME }
    }
}

impl Volume {
    /// Восстановить сохранённые значения. Испорченное число (NaN) заменяется на `DEFAULT_VOLUME`.
    pub fn restore(level: f32, before_mute: f32) -> Self {
        Self { level: sanitize(level), before_mute: sanitize(before_mute) }
    }

    pub fn level(self) -> f32 {
        self.level
    }

    pub fn before_mute(self) -> f32 {
        self.before_mute
    }

    /// Выключен ли звук. Любая громкость больше нуля, даже крошечная, это ещё не «выключено».
    pub fn is_muted(self) -> bool {
        self.level <= 0.0
    }

    pub fn set(&mut self, level: f32) {
        self.level = sanitize(level);
    }

    /// Кнопка динамика: выключить звук или вернуть прежний.
    pub fn toggle_mute(&mut self) {
        if !self.is_muted() {
            self.before_mute = self.level;
            self.level = 0.0;
        } else if self.before_mute > 0.0 {
            self.level = self.before_mute;
        } else {
            self.level = DEFAULT_VOLUME;
        }
    }
}

fn sanitize(level: f32) -> f32 {
    if level.is_nan() { DEFAULT_VOLUME } else { level.clamp(0.0, 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmute_returns_even_the_smallest_volume() {
        for level in [0.0005, 0.01, 0.19, 1.0] {
            let mut volume = Volume::restore(level, DEFAULT_VOLUME);
            volume.toggle_mute();
            assert!(volume.is_muted());
            volume.toggle_mute();
            assert_eq!(volume.level(), level);
        }
    }

    #[test]
    fn muted_state_survives_restore() {
        let mut volume = Volume::restore(0.3, 0.3);
        volume.toggle_mute();
        let mut restored = Volume::restore(volume.level(), volume.before_mute());
        restored.toggle_mute();
        assert_eq!(restored.level(), 0.3);
    }

    #[test]
    fn nothing_to_return_gives_default() {
        let mut volume = Volume::restore(0.0, 0.0);
        volume.toggle_mute();
        assert_eq!(volume.level(), DEFAULT_VOLUME);
    }

    #[test]
    fn broken_values_are_fixed() {
        let volume = Volume::restore(f32::NAN, 7.0);
        assert_eq!((volume.level(), volume.before_mute()), (DEFAULT_VOLUME, 1.0));
    }
}

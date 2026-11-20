//! Настройки оформления. Не секретные, поэтому можно хранить в обычном файле
//! (eframe сам кладёт их в папку данных приложения). Токены сюда НЕ попадают.

use serde::{Deserialize, Serialize}; // макросы: превращают структуру в текст и обратно

/// Какой из двух дизайнов включён.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Design {
    Material3,
    LiquidGlass,
}

/// Готовые цветовые темы. Список и цвета перенесены из проекта LomKich1/sheduleappreload
/// (ui/theme/AppTheme.kt), чтобы у двух приложений был единый вид.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemePreset {
    Dark,
    Light,
    Amoled, // «Монохром»: чисто чёрный фон, серые тона
    Embermire,
    VelvetRose,
    MistyWave,
    Glacier,
    GreenDusk,
    UrbanGray,
    SilentDawn,
    Sky,
    /// Свой цвет: палитра строится из `GraphicsSettings::accent`.
    Custom,
}

impl ThemePreset {
    pub const ALL: [ThemePreset; 12] = [
        Self::Dark,
        Self::Light,
        Self::Amoled,
        Self::Embermire,
        Self::VelvetRose,
        Self::MistyWave,
        Self::Glacier,
        Self::GreenDusk,
        Self::UrbanGray,
        Self::SilentDawn,
        Self::Sky,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Тёмная",
            Self::Light => "Светлая",
            Self::Amoled => "Монохром",
            Self::Embermire => "Эмбермайр",
            Self::VelvetRose => "Бархатная роза",
            Self::MistyWave => "Туманная волна",
            Self::Glacier => "Ледник",
            Self::GreenDusk => "Зелёные сумерки",
            Self::UrbanGray => "Городской серый",
            Self::SilentDawn => "Безмолвный рассвет",
            Self::Sky => "Голубая",
            Self::Custom => "Свой цвет",
        }
    }
}

/// Системный материал под окном (только Windows 11, сборка 22621+).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowBackdrop {
    Off,
    Mica,    // спокойный материал, подтягивает цвет обоев рабочего стола
    Acrylic, // полупрозрачный блюр того, что находится за окном
}

/// Качество «живого» фона стекла: во сколько раз уменьшается закадровая картинка, которую
/// преломляет стекло. Меньше = быстрее на слабых видеокартах, но стекло чуть «мылит».
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlassQuality {
    Full,        // родное разрешение
    Balanced,    // 0.75 по ширине и высоте
    Performance, // 0.5 по ширине и высоте
}

impl GlassQuality {
    pub const ALL: [GlassQuality; 3] = [Self::Full, Self::Balanced, Self::Performance];

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Полное",
            Self::Balanced => "Баланс",
            Self::Performance => "Быстро",
        }
    }
}

/// Всё, что настраивается на экране «Графика».
/// `#[serde(default)]`: если в старом файле настроек нет нового поля, берём значение по умолчанию.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct GraphicsSettings {
    pub design: Design,
    /// Выбранная цветовая тема.
    pub theme: ThemePreset,
    /// Только для темы «Свой цвет»: тёмная (true) или светлая (false) схема.
    pub dark: bool,
    /// Только для темы «Свой цвет»: исходный цвет, из него строится палитра.
    pub accent: [u8; 3],
    /// Радиус блюра стекла, в логических пикселях.
    pub blur_radius: f32,
    /// Сила преломления на краях стекла.
    pub refraction: f32,
    /// Непрозрачность «плёнки» стекла: 0 = чистый блюр, 1 = плотная заливка.
    pub glass_opacity: f32,
    /// Качество живого фона стекла (см. `GlassQuality`).
    pub glass_quality: GlassQuality,
    /// Отключает непрерывное движение декоративной волны и визуализатора.
    pub reduce_motion: bool,
    pub window_backdrop: WindowBackdrop,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            design: Design::LiquidGlass,
            theme: ThemePreset::Dark,
            dark: true,
            accent: [0x4F, 0x7C, 0xFF],
            blur_radius: 22.0,
            refraction: 5.0,
            glass_opacity: 0.28,
            glass_quality: GlassQuality::Balanced,
            reduce_motion: false,
            window_backdrop: WindowBackdrop::Off,
        }
    }
}

impl GraphicsSettings {
    /// Тёмная ли итоговая схема: у готовых тем это зашито в тему, у «Свой цвет» решает `dark`.
    pub fn is_dark(&self) -> bool {
        match self.theme {
            ThemePreset::Light => false,
            ThemePreset::Custom => self.dark,
            _ => true,
        }
    }

    /// Всё, от чего зависит палитра. Если ключ не изменился, палитру и обои пересчитывать не надо.
    pub fn palette_key(&self) -> (ThemePreset, [u8; 3], bool) {
        (self.theme, self.accent, self.dark)
    }
}

/// Ключ, под которым настройки лежат в хранилище eframe.
pub const STORAGE_KEY: &str = "mcrate.graphics.v1";

/// Язык интерфейса. Пока это только настройка: переводов строк в приложении нет,
/// поэтому выбор влияет на подпись в настройках, а не на тексты экранов.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    Russian,
    English,
}

impl Language {
    /// Полное название (для строки настроек).
    pub fn label(self) -> &'static str {
        match self {
            Self::Russian => "Русский",
            Self::English => "English",
        }
    }

    /// Двухбуквенный код (крупная надпись на плитке «Язык»).
    pub fn code(self) -> &'static str {
        match self {
            Self::Russian => "RU",
            Self::English => "EN",
        }
    }
}

/// Качество потока при воспроизведении. Пока только настройка: движок (`PlaybackEngine`)
/// выбор не читает, а UI по правилам дорожки B в `mcrate-audio` ничего не меняет.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamQuality {
    Low,
    Medium,
    High,
    Lossless,
}

impl StreamQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Низкое",
            Self::Medium => "Среднее",
            Self::High => "Высокое",
            Self::Lossless => "Lossless",
        }
    }
}

/// Размер текста: на самом деле масштаб всего интерфейса (размеры шрифтов зашиты в десятках
/// мест, поэтому меняем `zoom_factor` окна, а не шрифты по отдельности).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextSize {
    Small,
    Medium,
    Large,
}

impl TextSize {
    pub const ALL: [TextSize; 3] = [Self::Small, Self::Medium, Self::Large];

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Маленький",
            Self::Medium => "Средний",
            Self::Large => "Крупный",
        }
    }

    /// Множитель масштаба интерфейса.
    pub fn zoom(self) -> f32 {
        match self {
            Self::Small => 0.9,
            Self::Medium => 1.0,
            Self::Large => 1.1,
        }
    }
}

/// Качество рисования волны. Пока только настройка: волна (`ui/wave.rs`) её не читает.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaveQuality {
    Low,
    Medium,
    High,
}

impl WaveQuality {
    pub const ALL: [WaveQuality; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Низкое",
            Self::Medium => "Среднее",
            Self::High => "Высокое",
        }
    }
}

/// Варианты кроссфейда в секундах (0 = выключен). Порядок совпадает с чипами на экране.
pub const CROSSFADE_OPTIONS: [u8; 5] = [0, 1, 3, 5, 10];

/// Общие пользовательские настройки (не оформление). Хранятся отдельно от `GraphicsSettings`:
/// их изменение не должно пересобирать тему и палитру каждый раз.
/// `#[serde(default)]`: поле, которого нет в старом файле, получает значение по умолчанию.
/// Приватность пока только сохраняется. Из настроек воспроизведения движок читает нормализацию
/// громкости, кроссфейд и бесшовность (см. `playback_settings`); остальное пока только хранится.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Preferences {
    pub language: Language,
    /// Показывать уведомления (пока только переключатель, системных уведомлений нет).
    pub notifications: bool,
    /// Скачивать обновления автоматически (пока только переключатель: механизма обновлений нет).
    pub auto_update: bool,
    pub stream_quality: StreamQuality,
    // ---- воспроизведение ----
    /// Длина кроссфейда в секундах (одно из `CROSSFADE_OPTIONS`).
    pub crossfade_secs: u8,
    pub gapless: bool,
    pub normalize_volume: bool,
    pub autoplay: bool,
    pub background_play: bool,
    // ---- приватность ----
    pub public_profile: bool,
    pub show_listening: bool,
    pub share_stats: bool,
    pub personal_recs: bool,
    pub disable_analytics: bool,
    // ---- персонализация ----
    pub text_size: TextSize,
    pub cover_morph: bool,
    pub wave_quality: WaveQuality,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: Language::Russian,
            notifications: true,
            auto_update: true,
            stream_quality: StreamQuality::High,
            crossfade_secs: 3,
            gapless: true,
            normalize_volume: true,
            autoplay: false,
            background_play: true,
            public_profile: true,
            show_listening: false,
            share_stats: true,
            personal_recs: true,
            disable_analytics: false,
            text_size: TextSize::Medium,
            cover_morph: true,
            wave_quality: WaveQuality::High,
        }
    }
}

impl Preferences {
    /// Что из настроек знает движок воспроизведения: нормализация громкости, длина кроссфейда и
    /// бесшовность. Целевые LUFS в интерфейсе не настраиваются и остаются по умолчанию.
    pub fn playback_settings(&self) -> mcrate_audio::PlaybackSettings {
        mcrate_audio::PlaybackSettings {
            normalize_volume: self.normalize_volume,
            crossfade_secs: self.crossfade_secs,
            gapless: self.gapless,
            ..Default::default()
        }
    }
}

/// Ключ, под которым `Preferences` лежат в хранилище eframe.
pub const PREFS_KEY: &str = "mcrate.prefs.v1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_settings_follow_normalize_switch() {
        let mut prefs = Preferences::default();
        assert!(prefs.normalize_volume, "по умолчанию нормализация включена");
        assert!(prefs.playback_settings().normalize_volume);

        prefs.normalize_volume = false;
        assert!(!prefs.playback_settings().normalize_volume);
    }

    #[test]
    fn playback_settings_follow_crossfade_and_gapless() {
        let mut prefs = Preferences::default();
        assert_eq!(prefs.playback_settings().crossfade_secs, prefs.crossfade_secs);
        assert!(prefs.playback_settings().gapless, "по умолчанию бесшовность включена");

        for secs in CROSSFADE_OPTIONS {
            prefs.crossfade_secs = secs;
            assert_eq!(prefs.playback_settings().crossfade_secs, secs);
        }
        prefs.gapless = false;
        assert!(!prefs.playback_settings().gapless);
    }

    #[test]
    fn unrelated_prefs_do_not_change_playback_settings() {
        // Иначе приложение слало бы движку настройки при каждом клике по несвязанным переключателям
        let mut prefs = Preferences::default();
        let before = prefs.playback_settings();
        prefs.notifications = !prefs.notifications;
        prefs.autoplay = !prefs.autoplay;
        prefs.background_play = !prefs.background_play;
        prefs.cover_morph = !prefs.cover_morph;
        assert_eq!(prefs.playback_settings(), before);
    }
}

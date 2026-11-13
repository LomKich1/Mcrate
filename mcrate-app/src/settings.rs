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
        Self::Dark, Self::Light, Self::Amoled, Self::Embermire, Self::VelvetRose, Self::MistyWave,
        Self::Glacier, Self::GreenDusk, Self::UrbanGray, Self::SilentDawn, Self::Sky, Self::Custom,
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
    /// Главный выключатель блюра внутри приложения. false = все поверхности непрозрачные.
    pub blur_enabled: bool,
    /// Радиус блюра стекла, в логических пикселях.
    pub blur_radius: f32,
    /// Сила преломления на краях стекла.
    pub refraction: f32,
    /// Непрозрачность «плёнки» стекла: 0 = чистый блюр, 1 = плотная заливка.
    pub glass_opacity: f32,
    pub window_backdrop: WindowBackdrop,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            design: Design::LiquidGlass,
            theme: ThemePreset::Dark,
            dark: true,
            accent: [0x4F, 0x7C, 0xFF],
            blur_enabled: true,
            blur_radius: 22.0,
            refraction: 5.0,
            glass_opacity: 0.45,
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


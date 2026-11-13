//! Настройки оформления. Не секретные, поэтому можно хранить в обычном файле
//! (eframe сам кладёт их в папку данных приложения). Токены сюда НЕ попадают.

use serde::{Deserialize, Serialize}; // макросы: превращают структуру в текст и обратно

/// Какой из двух дизайнов включён.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Design {
    Material3,
    LiquidGlass,
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
    pub dark: bool,
    /// Исходный цвет (seed), из него Material 3 строит всю палитру.
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

/// Ключ, под которым настройки лежат в хранилище eframe.
pub const STORAGE_KEY: &str = "mcrate.graphics.v1";

/// Готовые акцентные цвета для быстрого выбора.
pub const ACCENT_PRESETS: [[u8; 3]; 8] = [
    [0x4F, 0x7C, 0xFF], // синий
    [0x7B, 0x5C, 0xFF], // фиолетовый
    [0xE0, 0x4F, 0xA5], // розовый
    [0xE5, 0x48, 0x4D], // красный
    [0xF2, 0x8C, 0x28], // оранжевый
    [0x2F, 0xB5, 0x6B], // зелёный
    [0x1F, 0xB5, 0xB0], // бирюзовый
    [0x8A, 0x94, 0xA6], // серо-стальной
];

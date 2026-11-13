//! Цветовые роли Material 3 в цветах egui.
//! Обе темы оформления (Material 3 и Liquid Glass) берут цвета отсюда, поэтому выбранная
//! цветовая тема работает одинаково в обоих дизайнах.
//!
//! Источник цветов: проект LomKich1/sheduleappreload, файл ui/theme/AppTheme.kt.
//! Там три готовые схемы (Тёмная, Светлая, Монохром) и восемь цветных тёмных, которые
//! строятся из трёх опорных цветов (акцент, фон, карточки) функцией `darkSchemeFrom`.
//! Ниже тот же алгоритм на Rust, поэтому цвета совпадают с приложением расписания.

use egui::Color32;
use material_colors::color::Rgb;
use material_colors::dynamic_color::{SpecVersion, Variant};
use material_colors::theme::ThemeBuilder;

use crate::settings::{GraphicsSettings, ThemePreset};

/// Набор ролей. Названия совпадают со спецификацией M3 (primary, surface_container и т.д.).
#[derive(Clone, Debug)]
pub struct Palette {
    pub dark: bool,
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub secondary_container: Color32,
    pub on_secondary_container: Color32,
    pub tertiary_container: Color32,
    pub surface: Color32,
    pub surface_container_lowest: Color32,
    pub surface_container_low: Color32,
    pub surface_container: Color32,
    pub surface_container_high: Color32,
    pub surface_container_highest: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub outline_variant: Color32,
    pub error: Color32,
}

// ---------------------------------------------------------------------------------------------
// Вспомогательные функции цвета
// ---------------------------------------------------------------------------------------------

/// 0xRRGGBB -> Color32.
const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Тот же цвет с заданной прозрачностью (0..=255). Для оверлеев «состояний» кнопок.
pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// HSL (оттенок 0..360, насыщенность 0..1, светлота 0..1) -> Color32.
/// HSL это способ задать цвет через «какой цвет, насколько яркий, насколько светлый».
fn hsl(h: f32, s: f32, l: f32) -> Color32 {
    let h = h.rem_euclid(360.0) / 360.0;
    let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if s == 0.0 {
        return Color32::from_rgb(to_u8(l), to_u8(l), to_u8(l));
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let channel = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    Color32::from_rgb(to_u8(channel(h + 1.0 / 3.0)), to_u8(channel(h)), to_u8(channel(h - 1.0 / 3.0)))
}

/// Color32 -> (оттенок 0..360, насыщенность 0..1, светлота 0..1).
fn to_hsl(c: Color32) -> (f32, f32, f32) {
    let (r, g, b) = (c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0 * 360.0, s, l)
}

// Oklab: цветовое пространство, в котором смешивание цветов выглядит естественно
// (серединка между двумя цветами не получается грязной). Compose `lerp` смешивает именно в нём,
// поэтому для точного совпадения цветов с приложением расписания делаем так же.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}
fn to_oklab(c: Color32) -> [f32; 3] {
    let r = srgb_to_linear(c.r() as f32 / 255.0);
    let g = srgb_to_linear(c.g() as f32 / 255.0);
    let b = srgb_to_linear(c.b() as f32 / 255.0);
    let l = (0.412_221_47 * r + 0.536_332_54 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_84 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}
fn from_oklab(lab: [f32; 3]) -> Color32 {
    let l = (lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2]).powi(3);
    let m = (lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2]).powi(3);
    let s = (lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2]).powi(3);
    let conv = |v: f32| (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8;
    Color32::from_rgb(
        conv(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
        conv(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s),
        conv(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
    )
}

/// Смешать `a` и `b` в Oklab: t = 0 даёт `a`, t = 1 даёт `b`.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let (x, y) = (to_oklab(a), to_oklab(b));
    from_oklab([x[0] + (y[0] - x[0]) * t, x[1] + (y[1] - x[1]) * t, x[2] + (y[2] - x[2]) * t])
}

/// Смешать `base` с `overlay` в пропорции `amount` (0..1). Нужно для «слоёв состояния» M3.
pub fn with_alpha_over(base: Color32, overlay: Color32, amount: f32) -> Color32 {
    mix(base, overlay, amount)
}

// ---------------------------------------------------------------------------------------------
// Схемы
// ---------------------------------------------------------------------------------------------

/// Цветная тёмная схема из трёх опорных цветов (порт `darkSchemeFrom`).
/// `primary` акцент, `background` фон окна, `surface` цвет карточек.
fn dark_scheme_from(primary: Color32, background: Color32, surface: Color32) -> Palette {
    let (h, s, _) = to_hsl(primary);
    let (bh, _, _) = to_hsl(background);

    let on_surface = hsl(bh, 0.10, 0.90);
    Palette {
        dark: true,
        primary,
        on_primary: hsl(h, s * 0.90, 0.16),
        primary_container: hsl(h, s * 0.55, 0.30),
        on_primary_container: hsl(h, s, 0.90),
        secondary_container: hsl(h, s * 0.22, 0.27),
        on_secondary_container: hsl(h, s * 0.40, 0.90),
        tertiary_container: hsl(h + 55.0, s * 0.30, 0.28),
        surface: background,
        surface_container_lowest: mix(background, Color32::BLACK, 0.35),
        surface_container_low: mix(background, surface, 0.50),
        surface_container: surface,
        surface_container_high: mix(surface, on_surface, 0.07),
        surface_container_highest: mix(surface, on_surface, 0.14),
        on_surface,
        on_surface_variant: hsl(bh, 0.08, 0.76),
        outline_variant: mix(surface, on_surface, 0.20),
        error: hex(0xFFB4AB),
    }
}

fn dark_baseline() -> Palette {
    Palette {
        dark: true,
        primary: hex(0xAAC7FF),
        on_primary: hex(0x0A305F),
        primary_container: hex(0x284777),
        on_primary_container: hex(0xD6E3FF),
        secondary_container: hex(0x3E4759),
        on_secondary_container: hex(0xDAE2F9),
        tertiary_container: hex(0x583E5B),
        surface: hex(0x111318),
        surface_container_lowest: hex(0x0C0E13),
        surface_container_low: hex(0x191C20),
        surface_container: hex(0x1D2024),
        surface_container_high: hex(0x282A2F),
        surface_container_highest: hex(0x33353A),
        on_surface: hex(0xE2E2E9),
        on_surface_variant: hex(0xC4C6D0),
        outline_variant: hex(0x44474E),
        error: hex(0xFFB4AB),
    }
}

fn light_baseline() -> Palette {
    Palette {
        dark: false,
        primary: hex(0x415F91),
        on_primary: hex(0xFFFFFF),
        primary_container: hex(0xD6E3FF),
        on_primary_container: hex(0x001B3E),
        secondary_container: hex(0xDAE2F9),
        on_secondary_container: hex(0x131C2B),
        tertiary_container: hex(0xFAD8FD),
        surface: hex(0xF9F9FF),
        surface_container_lowest: hex(0xFFFFFF),
        surface_container_low: hex(0xF3F3FA),
        surface_container: hex(0xEDEDF4),
        surface_container_high: hex(0xE7E8EE),
        surface_container_highest: hex(0xE2E2E9),
        on_surface: hex(0x1A1B20),
        on_surface_variant: hex(0x44474E),
        outline_variant: hex(0xC4C6D0),
        error: hex(0xBA1A1A),
    }
}

/// «Монохром» (AMOLED): чёрный фон, серые тона, ноль цвета.
fn amoled() -> Palette {
    Palette {
        dark: true,
        primary: hex(0xE2E2E2),
        on_primary: hex(0x1A1A1A),
        primary_container: hex(0x303030),
        on_primary_container: hex(0xE6E6E6),
        secondary_container: hex(0x2E2E2E),
        on_secondary_container: hex(0xE2E2E2),
        tertiary_container: hex(0x3A3A3A),
        surface: hex(0x000000),
        surface_container_lowest: hex(0x000000),
        surface_container_low: hex(0x0D0D0D),
        surface_container: hex(0x141414),
        surface_container_high: hex(0x1C1C1C),
        surface_container_highest: hex(0x262626),
        on_surface: hex(0xE6E6E6),
        on_surface_variant: hex(0xB0B0B0),
        outline_variant: hex(0x2E2E2E),
        error: hex(0xFFB4AB),
    }
}

/// «Свой цвет», тёмный вариант: из одного цвета делаем те же три опорных цвета,
/// что у готовых тем (акцент светлый и яркий, фон и карточки в том же оттенке, но почти серые).
fn custom_dark(seed: [u8; 3]) -> Palette {
    let (h, s, _) = to_hsl(Color32::from_rgb(seed[0], seed[1], seed[2]));
    dark_scheme_from(hsl(h, s.clamp(0.35, 0.90), 0.78), hsl(h, 0.14, 0.12), hsl(h, 0.12, 0.16))
}

/// «Свой цвет», светлый вариант: у готовых тем светлой цветной нет, берём схему Material 3
/// из крейта material-colors (спецификация 2025).
fn custom_light(seed: [u8; 3]) -> Palette {
    let c = |rgb: Rgb| Color32::from_rgb(rgb.red, rgb.green, rgb.blue);
    let theme = ThemeBuilder::with_source(Rgb::new(seed[0], seed[1], seed[2]))
        .spec_version(SpecVersion::Spec2025)
        .variant(Variant::TonalSpot)
        .build();
    let s = theme.schemes.light;
    Palette {
        dark: false,
        primary: c(s.primary),
        on_primary: c(s.on_primary),
        primary_container: c(s.primary_container),
        on_primary_container: c(s.on_primary_container),
        secondary_container: c(s.secondary_container),
        on_secondary_container: c(s.on_secondary_container),
        tertiary_container: c(s.tertiary_container),
        surface: c(s.surface),
        surface_container_lowest: c(s.surface_container_lowest),
        surface_container_low: c(s.surface_container_low),
        surface_container: c(s.surface_container),
        surface_container_high: c(s.surface_container_high),
        surface_container_highest: c(s.surface_container_highest),
        on_surface: c(s.on_surface),
        on_surface_variant: c(s.on_surface_variant),
        outline_variant: c(s.outline_variant),
        error: c(s.error),
    }
}

impl Palette {
    /// Палитра для выбранной темы. Для готовых тем `seed` и `custom_dark` не используются.
    pub fn for_preset(preset: ThemePreset, seed: [u8; 3], custom_is_dark: bool) -> Self {
        let colored = |p: u32, bg: u32, surf: u32| dark_scheme_from(hex(p), hex(bg), hex(surf));
        match preset {
            ThemePreset::Dark => dark_baseline(),
            ThemePreset::Light => light_baseline(),
            ThemePreset::Amoled => amoled(),
            ThemePreset::Embermire => colored(0xF5B09C, 0x231B1A, 0x2E2524),
            ThemePreset::VelvetRose => colored(0xEEB0CB, 0x231A20, 0x2F2329),
            ThemePreset::MistyWave => colored(0x96E5E3, 0x1A1F22, 0x232A2E),
            ThemePreset::Glacier => colored(0x94D3EE, 0x1B1F24, 0x242930),
            ThemePreset::GreenDusk => colored(0xE2E27C, 0x211F17, 0x2B2920),
            ThemePreset::UrbanGray => colored(0xC9C6C9, 0x1B1B1D, 0x262628),
            ThemePreset::SilentDawn => colored(0x88D89C, 0x18201B, 0x222B25),
            ThemePreset::Sky => colored(0x58C4DC, 0x1A1C20, 0x24262B),
            ThemePreset::Custom if custom_is_dark => custom_dark(seed),
            ThemePreset::Custom => custom_light(seed),
        }
    }

    pub fn from_settings(s: &GraphicsSettings) -> Self {
        Self::for_preset(s.theme, s.accent, s.dark)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklab_roundtrip_is_stable() {
        for c in [hex(0x231B1A), hex(0xF5B09C), hex(0x000000), hex(0xFFFFFF), hex(0x58C4DC)] {
            assert_eq!(from_oklab(to_oklab(c)), c, "{c:?}");
        }
    }

    #[test]
    fn mix_endpoints() {
        let (a, b) = (hex(0x231B1A), hex(0xE6E1E0));
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
    }

    #[test]
    fn hsl_roundtrip_close() {
        let c = hex(0xF5B09C);
        let (h, s, l) = to_hsl(c);
        let back = hsl(h, s, l);
        for (x, y) in [(c.r(), back.r()), (c.g(), back.g()), (c.b(), back.b())] {
            assert!((x as i32 - y as i32).abs() <= 1);
        }
    }

    #[test]
    fn every_preset_builds_and_has_readable_text() {
        for preset in ThemePreset::ALL {
            for dark in [true, false] {
                let p = Palette::for_preset(preset, [0x4F, 0x7C, 0xFF], dark);
                // Текст должен заметно отличаться по яркости от фона.
                let lum = |c: Color32| to_oklab(c)[0];
                assert!((lum(p.on_surface) - lum(p.surface)).abs() > 0.5, "{preset:?}");
            }
        }
    }
}

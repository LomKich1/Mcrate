//! Полная цветовая схема Material 3 в терминах egui.
//!
//! Material 3 использует семантические роли, а не произвольные пары цветов.
//! M3 получает схемы Light/Dark из одного seed через Material Color Utilities.
//! Liquid Glass намеренно продолжает использовать прежние значения уже существовавших
//! ролей: его визуальные параметры не меняются этим этапом.

use egui::Color32;
use material_colors::color::Rgb;
use material_colors::dynamic_color::{SpecVersion, Variant};
use material_colors::scheme::Scheme;
use material_colors::theme::ThemeBuilder;

use crate::settings::{Design, GraphicsSettings, ThemePreset};

/// Семантические роли Material 3 (включая fixed и surface roles спецификации 2025).
#[derive(Clone, Debug)]
pub struct Palette {
    pub dark: bool,

    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub inverse_primary: Color32,
    pub primary_fixed: Color32,
    pub primary_fixed_dim: Color32,
    pub on_primary_fixed: Color32,
    pub on_primary_fixed_variant: Color32,

    pub secondary: Color32,
    pub on_secondary: Color32,
    pub secondary_container: Color32,
    pub on_secondary_container: Color32,
    pub secondary_fixed: Color32,
    pub secondary_fixed_dim: Color32,
    pub on_secondary_fixed: Color32,
    pub on_secondary_fixed_variant: Color32,

    pub tertiary: Color32,
    pub on_tertiary: Color32,
    pub tertiary_container: Color32,
    pub on_tertiary_container: Color32,
    pub tertiary_fixed: Color32,
    pub tertiary_fixed_dim: Color32,
    pub on_tertiary_fixed: Color32,
    pub on_tertiary_fixed_variant: Color32,

    pub error: Color32,
    pub on_error: Color32,
    pub error_container: Color32,
    pub on_error_container: Color32,

    pub surface_dim: Color32,
    pub surface: Color32,
    pub surface_tint: Color32,
    pub surface_bright: Color32,
    pub surface_container_lowest: Color32,
    pub surface_container_low: Color32,
    pub surface_container: Color32,
    pub surface_container_high: Color32,
    pub surface_container_highest: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub outline: Color32,
    pub outline_variant: Color32,
    pub inverse_surface: Color32,
    pub inverse_on_surface: Color32,
    pub surface_variant: Color32,
    pub background: Color32,
    pub on_background: Color32,
    pub shadow: Color32,
    pub scrim: Color32,
}

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
// Генерация и схемы
// ---------------------------------------------------------------------------------------------

fn from_rgb(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.red, rgb.green, rgb.blue)
}

/// Преобразовать готовую динамическую схему MCU в набор ролей приложения.
fn from_scheme(s: &Scheme, dark: bool) -> Palette {
    Palette {
        dark,
        primary: from_rgb(s.primary),
        on_primary: from_rgb(s.on_primary),
        primary_container: from_rgb(s.primary_container),
        on_primary_container: from_rgb(s.on_primary_container),
        inverse_primary: from_rgb(s.inverse_primary),
        primary_fixed: from_rgb(s.primary_fixed),
        primary_fixed_dim: from_rgb(s.primary_fixed_dim),
        on_primary_fixed: from_rgb(s.on_primary_fixed),
        on_primary_fixed_variant: from_rgb(s.on_primary_fixed_variant),
        secondary: from_rgb(s.secondary),
        on_secondary: from_rgb(s.on_secondary),
        secondary_container: from_rgb(s.secondary_container),
        on_secondary_container: from_rgb(s.on_secondary_container),
        secondary_fixed: from_rgb(s.secondary_fixed),
        secondary_fixed_dim: from_rgb(s.secondary_fixed_dim),
        on_secondary_fixed: from_rgb(s.on_secondary_fixed),
        on_secondary_fixed_variant: from_rgb(s.on_secondary_fixed_variant),
        tertiary: from_rgb(s.tertiary),
        on_tertiary: from_rgb(s.on_tertiary),
        tertiary_container: from_rgb(s.tertiary_container),
        on_tertiary_container: from_rgb(s.on_tertiary_container),
        tertiary_fixed: from_rgb(s.tertiary_fixed),
        tertiary_fixed_dim: from_rgb(s.tertiary_fixed_dim),
        on_tertiary_fixed: from_rgb(s.on_tertiary_fixed),
        on_tertiary_fixed_variant: from_rgb(s.on_tertiary_fixed_variant),
        error: from_rgb(s.error),
        on_error: from_rgb(s.on_error),
        error_container: from_rgb(s.error_container),
        on_error_container: from_rgb(s.on_error_container),
        surface_dim: from_rgb(s.surface_dim),
        surface: from_rgb(s.surface),
        surface_tint: from_rgb(s.surface_tint),
        surface_bright: from_rgb(s.surface_bright),
        surface_container_lowest: from_rgb(s.surface_container_lowest),
        surface_container_low: from_rgb(s.surface_container_low),
        surface_container: from_rgb(s.surface_container),
        surface_container_high: from_rgb(s.surface_container_high),
        surface_container_highest: from_rgb(s.surface_container_highest),
        on_surface: from_rgb(s.on_surface),
        on_surface_variant: from_rgb(s.on_surface_variant),
        outline: from_rgb(s.outline),
        outline_variant: from_rgb(s.outline_variant),
        inverse_surface: from_rgb(s.inverse_surface),
        inverse_on_surface: from_rgb(s.inverse_on_surface),
        surface_variant: from_rgb(s.surface_variant),
        background: from_rgb(s.background),
        on_background: from_rgb(s.on_background),
        shadow: from_rgb(s.shadow),
        scrim: from_rgb(s.scrim),
    }
}

/// Единый генератор обеих цветовых схем. Цветовые роли Light/Dark строятся
/// одним алгоритмом, одной версией спецификации и одним вариантом Tonal Spot.
fn from_seed(seed: [u8; 3], dark: bool) -> Palette {
    let theme = ThemeBuilder::with_source(Rgb::new(seed[0], seed[1], seed[2]))
        .spec_version(SpecVersion::Spec2025)
        .variant(Variant::TonalSpot)
        .build();
    let scheme = if dark { &theme.schemes.dark } else { &theme.schemes.light };
    from_scheme(scheme, dark)
}

/// AMOLED-режим сохраняет чёрные поверхности, используя семантическую шкалу M3
/// для остальных ролей. Все текстовые пары на поверхностях остаются контрастными.
fn material3_amoled() -> Palette {
    let mut p = from_seed([0xE2, 0xE2, 0xE2], true);

    p.primary = hex(0xE2E2E2);
    p.on_primary = hex(0x1A1A1A);
    p.primary_container = hex(0x303030);
    p.on_primary_container = hex(0xE6E6E6);
    p.primary_fixed = hex(0xE2E2E2);
    p.primary_fixed_dim = hex(0xC6C6C6);
    p.on_primary_fixed = hex(0x1A1A1A);
    p.on_primary_fixed_variant = hex(0x303030);

    p.secondary = hex(0xC6C6C6);
    p.on_secondary = hex(0x202020);
    p.secondary_container = hex(0x2E2E2E);
    p.on_secondary_container = hex(0xE2E2E2);
    p.secondary_fixed = hex(0xC6C6C6);
    p.secondary_fixed_dim = hex(0xB0B0B0);
    p.on_secondary_fixed = hex(0x1A1A1A);
    p.on_secondary_fixed_variant = hex(0x303030);

    p.tertiary = hex(0xD0D0D0);
    p.on_tertiary = hex(0x1A1A1A);
    p.tertiary_container = hex(0x3A3A3A);
    p.on_tertiary_container = hex(0xE6E6E6);
    p.tertiary_fixed = hex(0xD0D0D0);
    p.tertiary_fixed_dim = hex(0xBDBDBD);
    p.on_tertiary_fixed = hex(0x1A1A1A);
    p.on_tertiary_fixed_variant = hex(0x303030);

    p.surface_dim = hex(0x000000);
    p.surface = hex(0x000000);
    p.surface_tint = p.primary;
    p.surface_bright = hex(0x262626);
    p.surface_container_lowest = hex(0x000000);
    p.surface_container_low = hex(0x0D0D0D);
    p.surface_container = hex(0x141414);
    p.surface_container_high = hex(0x1C1C1C);
    p.surface_container_highest = hex(0x262626);
    p.on_surface = hex(0xE6E6E6);
    p.on_surface_variant = hex(0xB0B0B0);
    p.outline = hex(0x8A8A8A);
    p.outline_variant = hex(0x4A4A4A);
    p.inverse_surface = hex(0xE6E6E6);
    p.inverse_on_surface = hex(0x1A1A1A);
    p.surface_variant = hex(0x2E2E2E);
    p.background = hex(0x000000);
    p.on_background = hex(0xE6E6E6);
    p.shadow = Color32::BLACK;
    p.scrim = Color32::BLACK;
    p
}

// ---------------------------------------------------------------------------------------------
// Совместимость с Liquid Glass
// ---------------------------------------------------------------------------------------------

/// Красно-чёрная палитра, перенесённая из RedDark.attheme.
/// Общая для Material 3 и Liquid Glass: акценты и поверхности не должны
/// отличаться при переключении дизайна интерфейса.
fn red_dark_palette() -> Palette {
    let mut p = from_seed([0xE9, 0x31, 0x2F], true);

    // Акценты: основной красный, более светлый красный для ссылок и мягкий оттенок для текста.
    p.primary = hex(0xE9312F);
    p.on_primary = hex(0x110A0B);
    p.primary_container = hex(0x7D2023);
    p.on_primary_container = hex(0xEEEEEE);
    p.inverse_primary = hex(0xFF8A87);
    p.primary_fixed = hex(0xFF8A87);
    p.primary_fixed_dim = hex(0xE9312F);
    p.on_primary_fixed = hex(0x110A0B);
    p.on_primary_fixed_variant = hex(0x110A0B);

    p.secondary = hex(0xFF5C59);
    p.on_secondary = hex(0x110A0B);
    // Выбранный элемент — глубокий красный, а не серо-коричневый.
    p.secondary_container = hex(0x651A1D);
    p.on_secondary_container = hex(0xE6B9B8);
    p.secondary_fixed = hex(0xFF5C59);
    p.secondary_fixed_dim = hex(0xE9312F);
    p.on_secondary_fixed = hex(0x110A0B);
    p.on_secondary_fixed_variant = hex(0x110A0B);

    p.tertiary = hex(0xFF8A87);
    p.on_tertiary = hex(0x110A0B);
    p.tertiary_container = hex(0x383132);
    p.on_tertiary_container = hex(0xEEEEEE);
    p.tertiary_fixed = hex(0xFF8A87);
    p.tertiary_fixed_dim = hex(0xE9312F);
    p.on_tertiary_fixed = hex(0x110A0B);
    p.on_tertiary_fixed_variant = hex(0x110A0B);

    p.error = hex(0xF26366);
    p.on_error = hex(0x110A0B);
    p.error_container = hex(0x5C1D20);
    p.on_error_container = hex(0xEEEEEE);

    // Поверхности взяты из фонов и панелей исходной темы, от почти чёрного до приподнятого.
    p.surface_dim = hex(0x110A0B);
    p.surface = hex(0x160F10);
    p.surface_tint = p.primary;
    p.surface_bright = hex(0x494243);
    p.surface_container_lowest = hex(0x110A0B);
    p.surface_container_low = hex(0x1E1718);
    p.surface_container = hex(0x272021);
    p.surface_container_high = hex(0x2E2728);
    p.surface_container_highest = hex(0x383132);

    // Большинство вторичных подписей в исходной теме нейтрально-серые.
    // Розовый #E6B9B8 оставляем только для выбранного красного контейнера.
    p.on_surface = hex(0xEEEEEE);
    p.on_surface_variant = hex(0x999999);
    p.outline = hex(0x777777);
    p.outline_variant = hex(0x272021);
    p.inverse_surface = hex(0xEEEEEE);
    p.inverse_on_surface = hex(0x160F10);
    p.surface_variant = hex(0x494243);
    p.background = hex(0x160F10);
    p.on_background = hex(0xEEEEEE);
    p.shadow = Color32::BLACK;
    p.scrim = Color32::BLACK;
    p
}

/// Тёмная авторская палитра старого оформления. Для Liquid Glass сохраняем
/// исходные значения существовавших ролей; недостающие роли заполняются MCU.
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
        ..from_seed([primary.r(), primary.g(), primary.b()], true)
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
        ..from_seed([0xAA, 0xC7, 0xFF], true)
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
        ..from_seed([0x41, 0x5F, 0x91], false)
    }
}

/// Прежняя монохромная палитра Liquid Glass; значения поверхностей сохранены.
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
        ..from_seed([0xE2, 0xE2, 0xE2], true)
    }
}

fn custom_dark(seed: [u8; 3]) -> Palette {
    let (h, s, _) = to_hsl(Color32::from_rgb(seed[0], seed[1], seed[2]));
    dark_scheme_from(hsl(h, s.clamp(0.35, 0.90), 0.78), hsl(h, 0.14, 0.12), hsl(h, 0.12, 0.16))
}

/// Совместимый светлый custom для Liquid Glass. И раньше он уже использовал MCU Spec2025.
fn custom_light(seed: [u8; 3]) -> Palette {
    from_seed(seed, false)
}

impl Palette {
    /// Палитра старого дизайна Liquid Glass. Существовавшие цвета намеренно сохранены.
    fn for_glass_preset(preset: ThemePreset, seed: [u8; 3], custom_is_dark: bool) -> Self {
        let colored = |p: u32, bg: u32, surf: u32| dark_scheme_from(hex(p), hex(bg), hex(surf));
        match preset {
            ThemePreset::Dark => dark_baseline(),
            ThemePreset::Light => light_baseline(),
            ThemePreset::Amoled => amoled(),
            ThemePreset::Embermire => red_dark_palette(),
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

    /// Полная динамическая палитра Material 3 Spec2025 для каждой предустановленной темы.
    fn for_material3_preset(preset: ThemePreset, seed: [u8; 3], custom_is_dark: bool) -> Self {
        let (source, dark) = match preset {
            ThemePreset::Dark => ([0xAA, 0xC7, 0xFF], true),
            ThemePreset::Light => ([0x41, 0x5F, 0x91], false),
            ThemePreset::Amoled => return material3_amoled(),
            ThemePreset::Embermire => return red_dark_palette(),
            ThemePreset::VelvetRose => ([0xEE, 0xB0, 0xCB], true),
            ThemePreset::MistyWave => ([0x96, 0xE5, 0xE3], true),
            ThemePreset::Glacier => ([0x94, 0xD3, 0xEE], true),
            ThemePreset::GreenDusk => ([0xE2, 0xE2, 0x7C], true),
            // Для «Городского серого» сохраняем исходную палитру проекта:
            // акцент #C9C6C9, фон #1B1B1D и базовую поверхность #262628.
            // M3-дополнительные роли берутся из той же серой основы.
            ThemePreset::UrbanGray => return Self::material3_urban_gray(),
            ThemePreset::SilentDawn => ([0x88, 0xD8, 0x9C], true),
            ThemePreset::Sky => ([0x58, 0xC4, 0xDC], true),
            ThemePreset::Custom => (seed, custom_is_dark),
        };
        from_seed(source, dark)
    }

    /// Материализованная M3-схема с прежними цветами авторской темы Urban Gray.
    /// Семантические роли, которых не было в старой палитре, дополняются из MCU,
    /// но уже существовавшие цвета акцента и поверхностей остаются неизменными.
    fn material3_urban_gray() -> Self {
        dark_scheme_from(hex(0xC9C6C9), hex(0x1B1B1D), hex(0x262628))
    }

    /// Выбрать палитру в зависимости от дизайна. Это изолирует Material 3 от Liquid Glass:
    /// переход на новую генерацию не меняет существующие цвета стеклянного оформления.
    pub fn from_settings(s: &GraphicsSettings, design: Design) -> Self {
        match design {
            Design::Material3 => Self::for_material3_preset(s.theme, s.accent, s.dark),
            Design::LiquidGlass => Self::for_glass_preset(s.theme, s.accent, s.dark),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relative_luminance(c: Color32) -> f32 {
        let channel = |v: u8| {
            let s = v as f32 / 255.0;
            if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    fn contrast_ratio(a: Color32, b: Color32) -> f32 {
        let (la, lb) = (relative_luminance(a), relative_luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

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
    fn urban_gray_keeps_legacy_palette_roles_in_material3() {
        let seed = [0x4F, 0x7C, 0xFF];
        let p = Palette::for_material3_preset(ThemePreset::UrbanGray, seed, true);
        let legacy = Palette::for_glass_preset(ThemePreset::UrbanGray, seed, true);

        // Все цветовые роли, существовавшие до M3, совпадают с исходной
        // «Городской серой»; недостающие M3-роли остаются дополнением схемы.
        assert_eq!(p.primary, hex(0xC9C6C9));
        assert_eq!(p.surface, hex(0x1B1B1D));
        assert_eq!(p.surface_container, hex(0x262628));
        assert_eq!(p.dark, legacy.dark);
        assert_eq!(p.primary, legacy.primary);
        assert_eq!(p.on_primary, legacy.on_primary);
        assert_eq!(p.primary_container, legacy.primary_container);
        assert_eq!(p.on_primary_container, legacy.on_primary_container);
        assert_eq!(p.secondary_container, legacy.secondary_container);
        assert_eq!(p.on_secondary_container, legacy.on_secondary_container);
        assert_eq!(p.tertiary_container, legacy.tertiary_container);
        assert_eq!(p.surface, legacy.surface);
        assert_eq!(p.surface_container_lowest, legacy.surface_container_lowest);
        assert_eq!(p.surface_container_low, legacy.surface_container_low);
        assert_eq!(p.surface_container, legacy.surface_container);
        assert_eq!(p.surface_container_high, legacy.surface_container_high);
        assert_eq!(p.surface_container_highest, legacy.surface_container_highest);
        assert_eq!(p.on_surface, legacy.on_surface);
        assert_eq!(p.on_surface_variant, legacy.on_surface_variant);
        assert_eq!(p.outline_variant, legacy.outline_variant);
        assert_eq!(p.error, legacy.error);
    }

    #[test]
    fn material3_presets_have_complete_readable_schemes() {
        for preset in ThemePreset::ALL {
            for custom_dark in [true, false] {
                let p = Palette::for_material3_preset(preset, [0x4F, 0x7C, 0xFF], custom_dark);
                for (background, foreground, role) in [
                    (p.primary, p.on_primary, "on_primary"),
                    (p.primary_container, p.on_primary_container, "on_primary_container"),
                    (p.secondary, p.on_secondary, "on_secondary"),
                    (p.secondary_container, p.on_secondary_container, "on_secondary_container"),
                    (p.tertiary, p.on_tertiary, "on_tertiary"),
                    (p.tertiary_container, p.on_tertiary_container, "on_tertiary_container"),
                    (p.error, p.on_error, "on_error"),
                    (p.error_container, p.on_error_container, "on_error_container"),
                    (p.surface, p.on_surface, "on_surface"),
                    (p.surface_dim, p.on_surface, "on_surface / surface_dim"),
                    (p.surface_bright, p.on_surface, "on_surface / surface_bright"),
                    (p.surface_container_lowest, p.on_surface, "on_surface / surface_container_lowest"),
                    (p.surface_container_low, p.on_surface, "on_surface / surface_container_low"),
                    (p.surface_container, p.on_surface, "on_surface / surface_container"),
                    (p.surface_container_high, p.on_surface, "on_surface / surface_container_high"),
                    (p.surface_container_highest, p.on_surface, "on_surface / surface_container_highest"),
                    (p.surface_variant, p.on_surface_variant, "on_surface_variant"),
                    (p.inverse_surface, p.inverse_on_surface, "inverse_on_surface"),
                    (p.background, p.on_background, "on_background"),
                    (p.primary_fixed, p.on_primary_fixed, "on_primary_fixed"),
                    (p.primary_fixed, p.on_primary_fixed_variant, "on_primary_fixed_variant"),
                    (p.secondary_fixed, p.on_secondary_fixed, "on_secondary_fixed"),
                    (p.secondary_fixed, p.on_secondary_fixed_variant, "on_secondary_fixed_variant"),
                    (p.tertiary_fixed, p.on_tertiary_fixed, "on_tertiary_fixed"),
                    (p.tertiary_fixed, p.on_tertiary_fixed_variant, "on_tertiary_fixed_variant"),
                ] {
                    assert!(
                        contrast_ratio(background, foreground) >= 4.5,
                        "{preset:?} {role}: {}:1",
                        contrast_ratio(background, foreground)
                    );
                }
            }
        }
    }

    #[test]
    fn liquid_glass_keeps_legacy_palette_values() {
        let dark = Palette::for_glass_preset(ThemePreset::Dark, [0x4F, 0x7C, 0xFF], true);
        assert_eq!(dark.primary, hex(0xAAC7FF));
        assert_eq!(dark.surface, hex(0x111318));
        let ember = Palette::for_glass_preset(ThemePreset::Embermire, [0x4F, 0x7C, 0xFF], true);
        assert_eq!(ember.primary, hex(0xE9312F));
        assert_eq!(ember.surface, hex(0x160F10));
        assert_eq!(ember.surface_container_low, hex(0x1E1718));
        assert_eq!(ember.surface_container, hex(0x272021));
        assert_eq!(ember.on_surface_variant, hex(0x999999));
        assert_eq!(ember.secondary_container, hex(0x651A1D));
    }

    #[test]
    fn embermire_uses_red_dark_palette_in_both_designs() {
        let settings = crate::settings::GraphicsSettings {
            theme: ThemePreset::Embermire,
            ..Default::default()
        };
        let material = Palette::from_settings(&settings, Design::Material3);
        let glass = Palette::from_settings(&settings, Design::LiquidGlass);

        for palette in [&material, &glass] {
            assert_eq!(palette.primary, hex(0xE9312F));
            assert_eq!(palette.secondary, hex(0xFF5C59));
            assert_eq!(palette.primary_container, hex(0x7D2023));
            assert_eq!(palette.surface, hex(0x160F10));
            assert_eq!(palette.surface_container_low, hex(0x1E1718));
            assert_eq!(palette.surface_container, hex(0x272021));
            assert_eq!(palette.surface_container_high, hex(0x2E2728));
            assert_eq!(palette.surface_container_highest, hex(0x383132));
            assert_eq!(palette.on_surface_variant, hex(0x999999));
            assert_eq!(palette.secondary_container, hex(0x651A1D));
            assert!(contrast_ratio(palette.primary, palette.on_primary) >= 4.5);
            assert!(contrast_ratio(palette.secondary_container, palette.on_secondary_container) >= 4.5);
            assert!(contrast_ratio(palette.surface, palette.on_surface) >= 4.5);
        }
        assert_eq!(material.primary, glass.primary);
        assert_eq!(material.surface_container, glass.surface_container);
    }
}

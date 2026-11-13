//! Цветовые роли Material 3, переведённые в цвета egui.
//! Обе темы (Material 3 и Liquid Glass) берут цвета отсюда, поэтому акцент
//! и светлая/тёмная схема работают одинаково в обоих дизайнах.

use egui::Color32;
use material_colors::color::Rgb;
use material_colors::dynamic_color::{SpecVersion, Variant};
use material_colors::theme::ThemeBuilder;

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

/// Rgb из material-colors -> Color32 из egui.
fn c(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.red, rgb.green, rgb.blue)
}

impl Palette {
    /// Строит палитру из одного цвета. Используем спецификацию 2025 (Material 3 Expressive)
    /// и вариант TonalSpot (классическая «спокойная» схема).
    pub fn from_seed(seed: [u8; 3], dark: bool) -> Self {
        let theme = ThemeBuilder::with_source(Rgb::new(seed[0], seed[1], seed[2]))
            .spec_version(SpecVersion::Spec2025)
            .variant(Variant::TonalSpot)
            .build();
        let s = if dark { theme.schemes.dark } else { theme.schemes.light };
        Self {
            dark,
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
}

/// Тот же цвет с заданной прозрачностью (0..=255). Для оверлеев «состояний» кнопок.
pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

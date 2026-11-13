//! Дизайн Liquid Glass. Стекло рисует крейт egui_glass (шейдер на wgpu: блюр + преломление
//! краёв + блик). Ограничение библиотеки: стекло «видит» только заранее загруженные обои
//! (static backdrop), а не соседние виджеты. Поэтому обои генерируем из цветов палитры.

use egui::{ColorImage, Margin, Ui};
use egui_glass::{Glass, GlassStyle};

use super::{Palette, Surface, Theme, material3, with_alpha};

const W: usize = 512;
const H: usize = 320;

/// Процедурные обои: несколько мягких цветных пятен на тёмной/светлой основе.
/// Чем контрастнее картинка, тем лучше видно преломление.
pub fn wallpaper(p: &Palette) -> ColorImage {
    let base = p.surface_container_lowest;
    // (x, y, радиус, цвет, сила), координаты в долях от размера картинки
    let blobs = [
        (0.18_f32, 0.28_f32, 0.55_f32, p.primary, 0.85_f32),
        (0.82, 0.22, 0.50, p.tertiary_container, 0.95),
        (0.55, 0.88, 0.60, p.secondary_container, 0.90),
        (0.92, 0.80, 0.35, p.primary_container, 0.80),
    ];
    let mut img = ColorImage::filled([W, H], base);
    for y in 0..H {
        for x in 0..W {
            let (fx, fy) = (x as f32 / W as f32, y as f32 / H as f32);
            let mut col = base;
            for (bx, by, r, color, strength) in blobs {
                // Расстояние учитывает соотношение сторон, чтобы пятна были круглыми.
                let d = (((fx - bx) * 1.6).powi(2) + (fy - by).powi(2)).sqrt() / r;
                if d < 1.0 {
                    let t = 1.0 - d;
                    let falloff = t * t * (3.0 - 2.0 * t); // smoothstep: плавный спад к краю
                    col = col.lerp_to_gamma(color, falloff * strength);
                }
            }
            img[(x, y)] = col;
        }
    }
    img
}

pub fn paint_background(theme: &Theme, ui: &mut Ui, rect: egui::Rect) {
    if theme.glass_active() {
        egui_glass::show_backdrop(ui, rect); // рисует загруженные обои на весь rect
    } else {
        material3::paint_background(theme, ui, rect);
    }
}

pub fn surface<R>(theme: &Theme, ui: &mut Ui, role: Surface, add: impl FnOnce(&mut Ui) -> R) -> R {
    // Блюр выключен -> непрозрачные поверхности (то, что README egui_glass называет
    // «reduced transparency»: это обязанность приложения, и мы её выполняем).
    if !theme.glass_active() {
        return material3::surface(theme, ui, role, add);
    }

    let s = &theme.settings;
    let p = &theme.palette;

    let base = match (role, p.dark) {
        (Surface::PlayerBar, true) => GlassStyle::dark(),
        (Surface::PlayerBar, false) => GlassStyle::regular(),
        (_, true) => GlassStyle::panel_dark(),
        (_, false) => GlassStyle::panel(),
    };
    let tint_color = match role {
        Surface::PlayerBar | Surface::Dialog => p.surface_container_high,
        _ => p.surface_container,
    };
    let style = GlassStyle {
        blur: s.blur_radius,
        refraction: s.refraction,
        corner_radius: theme.pane_radius() as f32,
        tint: with_alpha(tint_color, (s.glass_opacity.clamp(0.0, 1.0) * 255.0) as u8),
        ..base
    };

    let margin = match role {
        Surface::Rail => Margin::symmetric(12, 14),
        Surface::Content => Margin::same(20),
        Surface::PlayerBar => Margin::symmetric(24, 12),
        Surface::Dialog => Margin::same(24),
    };

    // Glass::show выставляет «плоские» стили дочерним виджетам; возвращаем наш стиль,
    // чтобы текст и кнопки выглядели одинаково в обоих дизайнах.
    let app_style = ui.style().clone();
    Glass::new(style)
        .inner_margin(margin)
        .show(ui, |ui| {
            ui.set_style(app_style);
            match role {
                Surface::Rail | Surface::Content => ui.set_min_size(ui.available_size()),
                Surface::PlayerBar => ui.set_min_width(ui.available_width()),
                Surface::Dialog => {}
            }
            add(ui)
        })
        .inner
}

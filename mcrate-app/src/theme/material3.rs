//! Дизайн Material 3 (Expressive): плотные тональные поверхности, крупные скругления,
//! роли цвета surface_container_* вместо теней-«высот».

use egui::{Color32, CornerRadius, Frame, Margin, Stroke, Ui};

use super::{Surface, Theme};

pub fn paint_background(theme: &Theme, ui: &mut Ui, rect: egui::Rect) {
    // Если включён Mica/Acrylic, фон не закрашиваем: системный материал виден сквозь окно.
    if theme.window_backdrop_active() {
        return;
    }
    ui.painter().rect_filled(rect, 0.0, theme.palette.surface);
}

/// Контейнер в стиле M3. Используется и как запасной вариант для Liquid Glass,
/// когда блюр выключен («уменьшить прозрачность»).
pub fn surface<R>(theme: &Theme, ui: &mut Ui, role: Surface, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = &theme.palette;
    let radius = CornerRadius::same(theme.pane_radius());

    let (fill, margin, stroke) = match role {
        // Навигация сливается с фоном (как в проводнике Windows 11).
        Surface::Rail => (Color32::TRANSPARENT, Margin::symmetric(8, 8), Stroke::NONE),
        Surface::Content => (p.surface_container_low, Margin::same(20), Stroke::NONE),
        Surface::PlayerBar => (p.surface_container_high, Margin::symmetric(22, 12), Stroke::NONE),
        Surface::Dialog => (p.surface_container_high, Margin::same(24), Stroke::new(1.0, p.outline_variant)),
    };

    let mut frame = Frame::new().fill(fill).corner_radius(radius).inner_margin(margin).stroke(stroke);
    // Плавающие элементы получают мягкую тень; тёмной теме нужна чуть плотнее.
    if matches!(role, Surface::PlayerBar | Surface::Dialog) {
        frame = frame.shadow(egui::Shadow {
            offset: [0, 4],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 110 } else { 45 }),
        });
    }

    frame
        .show(ui, |ui| {
            match role {
                // Рейка и основная область занимают всё выделенное место.
                Surface::Rail | Surface::Content => ui.set_min_size(ui.available_size()),
                Surface::PlayerBar => ui.set_min_width(ui.available_width()),
                Surface::Dialog => {}
            }
            add(ui)
        })
        .inner
}

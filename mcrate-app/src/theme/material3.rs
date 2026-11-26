//! Дизайн Material 3 (Expressive): плотные тональные поверхности, крупные скругления,
//! роли цвета surface_container_* вместо теней-«высот».

use egui::{Color32, CornerRadius, Frame, Margin, Stroke, Ui};

use super::{Surface, Theme, with_alpha};

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
        Surface::Rail => (Color32::TRANSPARENT, Margin::symmetric(12, 14), Stroke::NONE),
        Surface::Content => (p.surface_container_low, Margin::same(20), Stroke::NONE),
        Surface::PlayerBar => (p.surface_container_high, Margin::symmetric(22, 12), Stroke::NONE),
        // На экране «Сейчас играет» панель остаётся тональной, но слегка просвечивает,
        // чтобы фоновая аудиоволна была видна и в Material 3.
        Surface::Dialog => (with_alpha(p.surface_container_high, 224), Margin::same(24), Stroke::new(1.0, p.outline_variant)),
    };

    let mut frame = Frame::new().fill(fill).corner_radius(radius).inner_margin(margin).stroke(stroke);
    // Мягкие тени получают диалоги и мини-плеер Material 3.
    let shadow = match role {
        Surface::Dialog => Some(egui::Shadow {
            offset: [0, 4],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 110 } else { 45 }),
        }),
        // Тень выходит за плеер примерно на 12 px в стороны и 14 px вниз. Это меньше отступов (16 px) до краёв
        // контентной панели, поэтому на край панели она не ложится.
        Surface::PlayerBar if !theme.glass_active() => Some(egui::Shadow {
            offset: [0, 2],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 90 } else { 36 }),
        }),
        _ => None,
    };
    if let Some(shadow) = shadow {
        frame = frame.shadow(shadow);
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

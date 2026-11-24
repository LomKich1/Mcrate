//! Пустое состояние экрана: кольцо с иконкой, заголовок и пояснение по центру.
//! Общее для библиотеки, SoundCloud и заглушек коллекции; ветка темы (стекло / Material 3) выбирается здесь.

use egui::{RichText, Sense, Stroke, Ui, Vec2};

use crate::icons::{self, Icon};
use crate::theme::{Theme, with_alpha};

/// Верхний отступ, при котором блок стоит чуть выше середины свободной области
/// (22 % высоты за вычетом того, что снизу закрыто плеером), но не меньше 16 px.
pub(in crate::ui) fn empty_state_top_space(available_height: f32, bottom_inset: f32) -> f32 {
    ((available_height - bottom_inset).max(1.0) * 0.22).max(16.0)
}

/// Пустое состояние без дополнительного содержимого.
pub(in crate::ui) fn empty_state(ui: &mut Ui, theme: &Theme, top_space: f32, icon: Icon, headline: &str, text: &str) {
    empty_state_with(ui, theme, top_space, EmptyText { icon, headline, text }, |_| {});
}

/// Что написано в пустом состоянии: иконка, заголовок и пояснение.
#[derive(Clone, Copy)]
pub(in crate::ui) struct EmptyText<'a> {
    pub icon: Icon,
    pub headline: &'a str,
    pub text: &'a str,
}

/// То же, но под текстом можно добавить свой блок (например, баннер ошибки): он тоже по центру.
pub(in crate::ui) fn empty_state_with(ui: &mut Ui, theme: &Theme, top_space: f32, content: EmptyText, below: impl FnOnce(&mut Ui)) {
    let EmptyText { icon, headline, text } = content;
    let p = &theme.palette;
    ui.vertical_centered(|ui| {
        ui.add_space(top_space);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(88.0), Sense::hover());
        if theme.glass_active() {
            ui.painter().circle_stroke(rect.center(), 42.0, Stroke::new(0.9, with_alpha(p.on_surface, 46)));
            icons::paint(theme, ui.painter(), icon, rect.center(), 40.0, p.on_surface);
        } else {
            ui.painter().circle_filled(rect.center(), 44.0, theme.tonal_fill());
            icons::paint(theme, ui.painter(), icon, rect.center(), 40.0, theme.on_tonal());
        }
        ui.add_space(14.0);
        ui.label(RichText::new(headline).size(20.0).strong());
        ui.label(RichText::new(text).size(14.0).color(p.on_surface_variant));
        below(ui);
    });
}

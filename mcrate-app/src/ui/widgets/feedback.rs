//! Обратная связь пользователю: снэкбар, баннер ошибки и индикатор загрузки.

use super::glide::squish;
use super::loading_indicator::M3ELoadingIndicator;
use crate::icons::{self, Icon};
use crate::theme::{ShapeRole, TextRole, Theme};
use crate::ui::ripple;
use egui::{Align, Align2, CornerRadius, CursorIcon, FontId, Frame, Layout, Margin, Response, Sense, Ui, vec2};

/// M3 snackbar: инверсная поверхность, контрастный текст и действие inverse-primary.
/// Возвращает true, если пользователь нажал на закрытие.
pub(crate) fn status_snackbar(ui: &mut Ui, theme: &Theme, message: &str) -> bool {
    let p = &theme.palette;
    Frame::new()
        .fill(p.inverse_surface)
        .corner_radius(CornerRadius::same(theme.shape_radius(ShapeRole::Medium)))
        .inner_margin(Margin::symmetric(16, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (base_rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                    let rect = squish(ui, response.id, &response, base_rect, 0.06, 0.06);
                    let tint = ripple::StateTint { color: p.inverse_on_surface, hover_alpha: 20, press_alpha: 31 };
                    ripple::state_layer_colored(ui, theme, ui.painter(), &response, ripple::LayerArea { rect, corner: 15.0 }, tint);
                    icons::paint(theme, ui.painter(), Icon::Close, rect.center(), 18.0, p.inverse_primary);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(message).size(theme.text_size(TextRole::BodyMedium)).color(p.inverse_on_surface),
                        )
                        .wrap(),
                    );
                    response.on_hover_text("Закрыть").on_hover_cursor(CursorIcon::PointingHand).clicked()
                })
                .inner
            })
            .inner
        })
        .inner
}

/// Ошибка в контейнере семантического цвета. В Liquid Glass остаётся прежний тихий текстовый стиль.
pub(crate) fn error_banner(ui: &mut Ui, theme: &Theme, message: &str) {
    let p = &theme.palette;
    if theme.glass_active() {
        ui.label(egui::RichText::new(message).size(13.0).color(p.error));
        return;
    }

    Frame::new()
        .fill(p.error_container)
        .corner_radius(CornerRadius::same(theme.shape_radius(ShapeRole::Medium)))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (badge, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                ui.painter().circle_filled(badge.center(), 9.0, p.error);
                ui.painter().text(badge.center(), Align2::CENTER_CENTER, "!", FontId::proportional(13.0), p.on_error);
                ui.add(
                    egui::Label::new(egui::RichText::new(message).size(theme.text_size(TextRole::BodySmall)).color(p.on_error_container))
                        .wrap(),
                );
            });
        });
}

/// Индикатор загрузки Material 3 Expressive (морфирующая последовательность из семи фигур), без подписей.
/// Тонкая обёртка над `M3ELoadingIndicator` (`widgets/loading_indicator/`): цвет `primary`, в обеих темах
/// одинаково; при «Уменьшить анимацию» фигура неподвижна. Сам просит перерисовку, пока виден.
pub(crate) fn loading_indicator(ui: &mut Ui, theme: &Theme, size: f32) -> Response {
    ui.add(M3ELoadingIndicator::new().size(size).color(theme.palette.primary).reduce_motion(theme.settings.reduce_motion))
}

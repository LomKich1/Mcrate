//! Обратная связь пользователю: снэкбар, баннер ошибки и индикатор загрузки.

use super::glide::squish;
use crate::icons::{self, Icon};
use crate::theme::{ShapeRole, TextRole, Theme};
use crate::ui::anim::{STANDARD, cubic_bezier};
use crate::ui::ripple;
use egui::{Align, Align2, CornerRadius, CursorIcon, FontId, Frame, Layout, Margin, Pos2, Response, Sense, Shape, Stroke, Ui, Vec2, vec2};

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
                    ripple::state_layer_colored(ui, theme, ui.painter(), &response, rect, 15.0, tint);
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

/// Индикатор загрузки Material 3 (indeterminate circular progress), без подписей.
/// Анимация повторяет спецификацию M3: дуга растёт и сжимается (голова и хвост бегут с задержкой),
/// пока вся фигура вращается; один цикл 1332 мс, пять циклов до полного повтора вращения.
/// Рисуется в обеих темах одинаково, цвет `primary`. Сам просит перерисовку, пока виден.
pub(crate) fn loading_indicator(ui: &mut Ui, theme: &Theme, size: f32) -> Response {
    const CYCLE: f64 = 1.332; // секунд на один рост и сжатие дуги
    const HEAD_TAIL: f32 = 0.666; // длительность бега головы и хвоста
    const JUMP: f32 = 290.0; // на сколько градусов вырастает дуга
    const ROTATIONS: f64 = 5.0; // циклов до повтора вращения
    const BASE_DEG: f32 = 286.0; // равномерное вращение за ROTATIONS циклов

    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    ui.ctx().request_repaint();

    let time = ui.input(|i| i.time);
    let in_cycle = (time % CYCLE) as f32;
    let cycle_index = (time / CYCLE).floor() as i64 % ROTATIONS as i64;
    let ease = |x: f32| cubic_bezier(STANDARD, x.clamp(0.0, 1.0));
    let end = JUMP * ease(in_cycle / HEAD_TAIL);
    let start = JUMP * ease((in_cycle - HEAD_TAIL) / HEAD_TAIL);
    let base = BASE_DEG * ((time % (CYCLE * ROTATIONS)) / (CYCLE * ROTATIONS)) as f32;
    let offset = -90.0 + (cycle_index as f32 * JUMP) % 360.0 + base;
    let sweep = (end - start).abs().max(0.5);

    let stroke_w = 4.0 * size / 48.0; // M3: дорожка 4 dp при размере 48 dp
    let radius = (size - stroke_w) * 0.5;
    let color = theme.palette.primary;
    let steps = ((sweep / 4.0).ceil() as usize).max(2);
    let point = |deg: f32| {
        let a = deg.to_radians();
        rect.center() + vec2(a.cos(), a.sin()) * radius
    };
    let points: Vec<Pos2> = (0..=steps).map(|i| point(start + offset + sweep * i as f32 / steps as f32)).collect();
    let painter = ui.painter();
    painter.add(Shape::line(points.clone(), Stroke::new(stroke_w, color)));
    // Круглые концы дуги (в egui у линии они плоские).
    for end_point in [points[0], points[steps]] {
        painter.circle_filled(end_point, stroke_w * 0.5, color);
    }
    resp
}

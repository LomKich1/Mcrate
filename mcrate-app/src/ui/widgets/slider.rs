//! Ползунок для перемотки и громкости.

use crate::settings::Design;
use crate::theme::{Theme, with_alpha};
use egui::{Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

/// Результат ползунка: `value` = Some(доля 0..1), пока пользователь тянет или кликает.
pub(crate) struct BarOutput {
    pub response: Response,
    pub value: Option<f32>,
}

/// Ползунок для перемотки и громкости. M3 Expressive: ручка-палочка и дорожка с зазором,
/// Liquid Glass сохраняет своё стеклянное оформление.
pub(crate) fn slider_bar(ui: &mut Ui, theme: &Theme, width: f32, frac: f32, enabled: bool, label: &str) -> BarOutput {
    let p = &theme.palette;
    let sense = if enabled { Sense::click_and_drag() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 24.0), sense);
    let pad = 10.0;
    let (left, right) = (rect.left() + pad, rect.right() - pad);
    let cy = rect.center().y;
    let active = resp.hovered() || resp.dragged();
    let frac = frac.clamp(0.0, 1.0);
    let x = left + (right - left) * frac;

    let track_bg = with_alpha(p.on_surface, if p.dark { 50 } else { 40 });
    let fg = if enabled { p.primary } else { with_alpha(p.on_surface_variant, 120) };

    let paint = SliderPaint { resp: &resp, rect, left, right, cy, x, active, enabled, track_bg, fg };
    match theme.design() {
        Design::Material3 => paint_m3_slider(ui, theme, paint),
        Design::LiquidGlass => paint_glass_slider(ui, theme, paint),
    }

    let pointer_value = if resp.dragged() || resp.clicked() || resp.drag_stopped() {
        resp.interact_pointer_pos().map(|pos| ((pos.x - left) / (right - left)).clamp(0.0, 1.0))
    } else {
        None
    };
    // Клавиатурное управление делает прогресс и громкость доступнее: стрелки меняют
    // значение на 1%, Shift + стрелка — на 10%, Home/End переходят к границам.
    let keyboard_value = if enabled && resp.has_focus() { keyboard_slider_value(ui, frac) } else { None };
    let value = pointer_value.or(keyboard_value);
    resp.widget_info(|| egui::WidgetInfo::slider(enabled, value.unwrap_or(frac) as f64, label));
    BarOutput { response: resp, value }
}

/// Геометрия и состояние ползунка, общие для обеих отрисовок.
#[derive(Clone, Copy)]
struct SliderPaint<'a> {
    resp: &'a Response,
    /// Весь прямоугольник ползунка.
    rect: Rect,
    /// Левый и правый края дорожки (с учётом отступов под ручку).
    left: f32,
    right: f32,
    /// Вертикальный центр дорожки.
    cy: f32,
    /// Положение ручки по x.
    x: f32,
    /// Курсор над ползунком или идёт перетаскивание.
    active: bool,
    enabled: bool,
    track_bg: Color32,
    fg: Color32,
}

/// Material 3 Expressive: рисует дорожку из двух частей, точку-стоп и ручку-палочку.
fn paint_m3_slider(ui: &Ui, theme: &Theme, s: SliderPaint<'_>) {
    let p = &theme.palette;
    let SliderPaint { resp, rect, left, right, cy, x, track_bg, fg, .. } = s;
    let painter = ui.painter();
    // M3 Expressive Slider (спецификация 2024): ручка — вертикальная палочка
    // (4dp, при перетаскивании сужается до 2dp), дорожка разрезана на две части
    // зазором 6dp вокруг ручки, у концов, обращённых к ручке, углы 2dp,
    // а на дальнем конце неактивной части стоит точка-стоп 4dp.
    const HANDLE_W: f32 = 4.0;
    const HANDLE_W_PRESSED: f32 = 2.0;
    const GAP: f32 = 6.0; // зазор между ручкой и дорожкой
    const TRACK_H: f32 = 6.0;
    const INNER_R: u8 = 2; // скругление «внутренних» углов дорожки
    const STOP_D: f32 = 4.0; // диаметр точки-стопа
    let outer_r = (TRACK_H / 2.0) as u8;

    // Ширина ручки меняется плавно (0.1 с), как в M3.
    let hw = ui.ctx().animate_value_with_time(resp.id.with("handle_w"), if resp.dragged() { HANDLE_W_PRESSED } else { HANDLE_W }, 0.1);
    let handle_h = (rect.height() - 4.0).max(HANDLE_W);
    let active_end = x - hw / 2.0 - GAP;
    let inactive_start = x + hw / 2.0 + GAP;

    // Активная часть (слева от ручки). Если ручка у левого края, её нет.
    if active_end > left {
        painter.rect_filled(
            Rect::from_min_max(pos2(left, cy - TRACK_H / 2.0), pos2(active_end, cy + TRACK_H / 2.0)),
            CornerRadius { nw: outer_r, sw: outer_r, ne: INNER_R, se: INNER_R },
            fg,
        );
    }
    // Неактивная часть (справа от ручки) и точка-стоп у её дальнего края.
    if inactive_start < right {
        painter.rect_filled(
            Rect::from_min_max(pos2(inactive_start, cy - TRACK_H / 2.0), pos2(right, cy + TRACK_H / 2.0)),
            CornerRadius { nw: INNER_R, sw: INNER_R, ne: outer_r, se: outer_r },
            track_bg,
        );
        let dot_x = right - STOP_D / 2.0 - 1.0;
        if dot_x - STOP_D / 2.0 > inactive_start + 2.0 {
            painter.circle_filled(pos2(dot_x, cy), STOP_D / 2.0, fg);
        }
    }
    // Ручка-палочка.
    let handle = Rect::from_center_size(pos2(x, cy), vec2(hw, handle_h));
    painter.rect_filled(handle, hw / 2.0, fg);
    if resp.has_focus() {
        painter.rect_stroke(handle.expand(3.0), 4.0, Stroke::new(2.0, p.primary), StrokeKind::Outside);
    }
}

/// Liquid Glass: тонкая дорожка и белая ручка, под курсором ручка превращается в стекло.
fn paint_glass_slider(ui: &Ui, theme: &Theme, s: SliderPaint<'_>) {
    let p = &theme.palette;
    let SliderPaint { resp, left, right, cy, x, active, enabled, track_bg, fg, .. } = s;
    let painter = ui.painter();
    let track_height = ui.ctx().animate_value_with_time(resp.id.with("glass_track_height"), if active { 7.0 } else { 5.0 }, 0.12);
    let track_rect = Rect::from_min_max(pos2(left, cy - track_height / 2.0), pos2(right, cy + track_height / 2.0));
    painter.rect_filled(track_rect, track_height / 2.0, track_bg);
    if x > left {
        painter.rect_filled(
            Rect::from_min_max(pos2(left, cy - track_height / 2.0), pos2(x, cy + track_height / 2.0)),
            track_height / 2.0,
            fg,
        );
    }
    let thumb_r = ui.ctx().animate_value_with_time(resp.id.with("glass_thumb_radius"), if active { 9.5 } else { 7.5 }, 0.12);
    let thumb_rect = Rect::from_center_size(pos2(x, cy), vec2(thumb_r * 2.0, thumb_r * 2.0));
    if active && enabled {
        // Как у Apple: стекло показывается ненадолго на активных ползунках и переключателях,
        // а не превращает в стекло всю поверхность контента.
        let style = crate::theme::glass::control_style(theme, false, thumb_r);
        egui_glass::paint_glass(ui, thumb_rect, &style);
        painter.circle_stroke(pos2(x, cy), thumb_r - 0.5, Stroke::new(0.9, with_alpha(p.on_surface, if p.dark { 120 } else { 92 })));
    } else {
        painter.circle_filled(pos2(x, cy + 1.2), thumb_r + 0.5, egui::Color32::from_black_alpha(54));
        painter.circle_filled(pos2(x, cy), thumb_r, egui::Color32::WHITE);
        painter.circle_stroke(pos2(x, cy), thumb_r - 0.6, Stroke::new(0.8, with_alpha(p.on_surface, 70)));
        painter.line_segment(
            [pos2(x - thumb_r * 0.38, cy - thumb_r * 0.48), pos2(x + thumb_r * 0.25, cy - thumb_r * 0.48)],
            Stroke::new(0.8, with_alpha(Color32::WHITE, 180)),
        );
    }
    if resp.has_focus() {
        painter.circle_stroke(pos2(x, cy), thumb_r + 4.0, Stroke::new(1.6, p.primary));
    }
}

/// Клавиатурное управление: стрелки меняют значение на 1%, Shift + стрелка на 10%, Home/End переходят к границам.
fn keyboard_slider_value(ui: &Ui, frac: f32) -> Option<f32> {
    ui.input(|input| {
        let step = if input.modifiers.shift { 0.10 } else { 0.01 };
        if input.key_pressed(egui::Key::ArrowLeft) || input.key_pressed(egui::Key::ArrowDown) {
            Some((frac - step).clamp(0.0, 1.0))
        } else if input.key_pressed(egui::Key::ArrowRight) || input.key_pressed(egui::Key::ArrowUp) {
            Some((frac + step).clamp(0.0, 1.0))
        } else if input.key_pressed(egui::Key::Home) {
            Some(0.0)
        } else if input.key_pressed(egui::Key::End) {
            Some(1.0)
        } else {
            None
        }
    })
}

//! Кнопки: иконочные и «пилюли» в Material 3 и Liquid Glass.

use super::glide::squish;
use crate::icons::{self, Icon};
use crate::theme::{ShapeRole, TextRole, Theme, with_alpha};
use crate::ui::ripple;
use egui::{Color32, CursorIcon, FontId, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonKind {
    Plain,    // только иконка без постоянной подложки
    Outlined, // контурная кнопка
    Tonal,    // приглушённая заливка
    Filled,   // главный акцент (кнопка «играть»)
}

/// Что нарисовать в круглой кнопке с иконкой (`icon_button`, `glass_icon_button`).
#[derive(Clone, Copy)]
pub(crate) struct IconButton<'a> {
    pub icon: Icon,
    /// Диаметр кнопки в px.
    pub diameter: f32,
    pub kind: ButtonKind,
    /// Включённое состояние (переключатель): у `Plain` даёт тонированный фон.
    pub active: bool,
    /// Подсказка при наведении.
    pub tip: &'a str,
}

impl<'a> IconButton<'a> {
    /// Простая (`Plain`) неактивная кнопка.
    pub(crate) fn new(icon: Icon, diameter: f32, tip: &'a str) -> Self {
        Self { icon, diameter, kind: ButtonKind::Plain, active: false, tip }
    }

    pub(crate) fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    pub(crate) fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

/// Отдельная круглая кнопка в контентном слое: в Liquid Glass используется реальный
/// GPU-материал GlassButton, а не имитация прозрачности. Не использовать внутри PlayerBar
/// или Rail — там уже есть общая стеклянная поверхность и нужны плоские дочерние контроли.
pub(crate) fn glass_icon_button(ui: &mut Ui, theme: &Theme, spec: IconButton) -> Response {
    if !theme.glass_active() {
        return icon_button(ui, theme, spec);
    }
    let IconButton { icon, diameter, kind, active, tip } = spec;

    let p = &theme.palette;
    let prominent = kind == ButtonKind::Filled;
    let response = egui_glass::GlassButton::new("")
        .style(crate::theme::glass::control_style(theme, prominent, diameter.max(44.0) * 0.5))
        .icon()
        .min_size(Vec2::splat(diameter.max(44.0)))
        .accessible_name(tip)
        .show(ui);
    let foreground = if prominent {
        p.on_primary
    } else if active {
        p.primary
    } else {
        p.on_surface
    };
    // GlassButton animates its own glass surface, but custom-painted glyphs are
    // outside that widget. Apply the same press transform to the glyph itself.
    let visual_rect = squish(ui, response.id, &response, response.rect, 0.055, 0.055);
    let glyph_scale = (visual_rect.width() / response.rect.width()).clamp(0.8, 1.0);
    icons::paint(theme, ui.painter(), icon, visual_rect.center(), diameter * 0.58 * glyph_scale, foreground);
    response.on_hover_text(tip).on_hover_cursor(CursorIcon::PointingHand)
}

/// Содержимое и размер pill-кнопки, уже посчитанные `pill_button`.
struct PillContent<'a> {
    label: &'a str,
    icon: Option<Icon>,
    kind: ButtonKind,
    /// Подпись, разложенная в строку (по ней считался `size`).
    galley: std::sync::Arc<egui::Galley>,
    size: Vec2,
}

/// Pill-кнопка с настоящим Liquid Glass backdrop callback. Иконка и подпись остаются
/// отдельными глифами, поэтому сохраняют разный размер/шрифт и корректную доступность.
fn glass_pill_button(ui: &mut Ui, theme: &Theme, pill: PillContent) -> Response {
    let PillContent { label, icon, kind, galley, size } = pill;
    let p = &theme.palette;
    let response = egui_glass::GlassButton::new("")
        .style(crate::theme::glass::control_style(theme, kind == ButtonKind::Filled, f32::INFINITY))
        .icon()
        .min_size(size.max(vec2(44.0, 44.0)))
        .accessible_name(label)
        .show(ui);
    let foreground = if kind == ButtonKind::Filled { p.on_primary } else { p.on_surface };
    let visual_rect = squish(ui, response.id, &response, response.rect, 0.035, 0.075);
    let glyph_scale = (visual_rect.width() / response.rect.width()).clamp(0.8, 1.0);
    let icon_width = if icon.is_some() { 28.0 } else { 0.0 };
    let content_width = galley.size().x + icon_width;
    let original_left = response.rect.center().x - content_width * 0.5;
    let mut x = visual_rect.center().x + (original_left - response.rect.center().x) * glyph_scale;
    if let Some(icon) = icon {
        let icon_center =
            visual_rect.center() + (pos2(original_left + 10.0, response.rect.center().y) - response.rect.center()) * glyph_scale;
        icons::paint(theme, ui.painter(), icon, icon_center, 18.0 * glyph_scale, foreground);
        x += icon_width * glyph_scale;
    }
    let text_galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme.text_size(TextRole::LabelLarge) * glyph_scale),
        foreground,
    );
    ui.painter().galley(pos2(x, visual_rect.center().y - text_galley.size().y * 0.5), text_galley, foreground);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// Круглая кнопка с иконкой. Внутри уже стеклянного PlayerBar используется плоский контрол,
/// чтобы не создавать второй оптический слой поверх стекла.
pub(crate) fn icon_button(ui: &mut Ui, theme: &Theme, spec: IconButton) -> Response {
    let IconButton { icon, diameter, kind, active, tip } = spec;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    let glass = theme.glass_active();
    let visual_rect = squish(ui, resp.id, &resp, rect, 0.055, 0.055);
    let fg = icon_button_foreground(theme, kind, active, glass);
    if glass {
        icon_button_glass(ui, theme, &resp, visual_rect, kind, active);
    } else {
        icon_button_m3(ui, theme, &resp, visual_rect, kind, active);
    }

    let glyph_scale = (visual_rect.width() / rect.width()).clamp(0.8, 1.0);
    let mut glyph_center = visual_rect.center();
    if icon == Icon::Play && theme.glass_active() {
        glyph_center.x += 1.2;
    }
    icons::paint(theme, ui.painter(), icon, glyph_center, diameter * 0.6 * glyph_scale, fg);
    let resp = resp.on_hover_text(tip).on_hover_cursor(CursorIcon::PointingHand);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, tip));
    resp
}

/// Цвет иконки круглой кнопки.
fn icon_button_foreground(theme: &Theme, kind: ButtonKind, active: bool, glass: bool) -> Color32 {
    let p = &theme.palette;
    match kind {
        ButtonKind::Filled => p.on_primary,
        ButtonKind::Tonal => {
            if glass {
                p.on_surface
            } else {
                theme.on_tonal()
            }
        }
        ButtonKind::Outlined => p.primary,
        ButtonKind::Plain => {
            if active {
                p.primary
            } else {
                p.on_surface
            }
        }
    }
}

/// Круглая кнопка в Liquid Glass: тихая оптическая поверхность, prominent tint только у главного действия.
fn icon_button_glass(ui: &Ui, theme: &Theme, resp: &Response, visual_rect: Rect, kind: ButtonKind, active: bool) {
    let p = &theme.palette;
    let painter = ui.painter();
    let r = visual_rect.width() / 2.0;
    // В Liquid Glass кнопки не используют плотную M3 tonal-заливку:
    // только важное действие получает prominent tint, остальные — тихую оптическую поверхность.
    let pressed = resp.is_pointer_button_down_on();
    let hovered = resp.hovered();
    let state_t = ui.ctx().animate_bool_with_time(resp.id.with("glass_feedback"), pressed || hovered || resp.has_focus() || active, 0.14);
    let (fill, stroke) = match kind {
        ButtonKind::Filled => (Some(with_alpha(p.primary, if p.dark { 224 } else { 218 })), with_alpha(p.on_primary, 92)),
        ButtonKind::Tonal => (
            Some(with_alpha(p.surface_container_high, (82.0 + state_t * 60.0) as u8)),
            with_alpha(p.on_surface, (42.0 + state_t * 38.0) as u8),
        ),
        ButtonKind::Outlined => (
            Some(with_alpha(p.surface_container_high, (36.0 + state_t * 80.0) as u8)),
            with_alpha(p.on_surface, (54.0 + state_t * 34.0) as u8),
        ),
        ButtonKind::Plain if active => (Some(with_alpha(p.primary, (54.0 + state_t * 30.0) as u8)), with_alpha(p.primary, 86)),
        ButtonKind::Plain if hovered || pressed || resp.has_focus() => {
            (Some(with_alpha(p.on_surface, (10.0 + state_t * 28.0) as u8)), with_alpha(p.on_surface, 48))
        }
        ButtonKind::Plain => (None, Color32::TRANSPARENT),
    };
    if kind == ButtonKind::Filled {
        painter.circle_filled(visual_rect.center() + vec2(0.0, 1.2), r, Color32::from_black_alpha(if p.dark { 46 } else { 26 }));
    }
    if let Some(fill) = fill {
        painter.circle_filled(visual_rect.center(), r, fill);
        if !matches!(kind, ButtonKind::Plain) || active {
            painter.circle_stroke(visual_rect.center(), r - 0.5, Stroke::new(0.9, stroke));
            // Короткий верхний блик помогает читать кнопку как прозрачный контрол,
            // но не превращает каждый элемент в яркую стеклянную капсулу.
            let c = visual_rect.center();
            painter.line_segment(
                [pos2(c.x - r * 0.34, c.y - r * 0.76), pos2(c.x + r * 0.34, c.y - r * 0.76)],
                Stroke::new(0.8, with_alpha(Color32::WHITE, if p.dark { 36 } else { 74 })),
            );
        }
    }
    if resp.has_focus() {
        painter.circle_stroke(visual_rect.center(), r + 2.0, Stroke::new(1.6, with_alpha(p.primary, 220)));
    }
}

/// Круглая кнопка в Material 3: заливка по виду, слой состояния и контур.
fn icon_button_m3(ui: &Ui, theme: &Theme, resp: &Response, visual_rect: Rect, kind: ButtonKind, active: bool) {
    let p = &theme.palette;
    let painter = ui.painter();
    let r = visual_rect.width() / 2.0;
    let (bg, _) = match kind {
        ButtonKind::Filled => (Some(p.primary), p.on_primary),
        ButtonKind::Tonal => (Some(theme.tonal_fill()), theme.on_tonal()),
        ButtonKind::Outlined => (None, p.primary),
        ButtonKind::Plain => (if active { Some(with_alpha(p.primary, if p.dark { 42 } else { 34 })) } else { None }, p.on_surface),
    };
    if let Some(bg) = bg {
        painter.circle_filled(visual_rect.center(), r, bg);
    }
    // Слой состояния: кружок от курсора (ripple.rs). Для круглой кнопки форма обрезки это сам круг.
    let circle = Rect::from_center_size(visual_rect.center(), Vec2::splat(r * 2.0));
    ripple::state_layer(ui, theme, painter, resp, circle, r);
    if kind == ButtonKind::Outlined {
        painter.circle_stroke(visual_rect.center(), r - 0.5, Stroke::new(1.0, p.outline));
    }
    if resp.has_focus() {
        painter.circle_stroke(visual_rect.center(), r + 1.5, Stroke::new(2.0, p.primary));
    }
}

/// Кнопка-«таблетка» с текстом и необязательной иконкой.
pub(crate) fn pill_button(ui: &mut Ui, theme: &Theme, label: &str, icon: Option<Icon>, kind: ButtonKind) -> Response {
    let p = &theme.palette;
    let glass = theme.glass_active();
    let fg = match kind {
        ButtonKind::Filled => p.on_primary,
        ButtonKind::Tonal if glass => p.on_surface,
        ButtonKind::Tonal => theme.on_tonal(),
        ButtonKind::Outlined => p.primary,
        ButtonKind::Plain => p.on_surface,
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(theme.text_size(TextRole::LabelLarge)), fg);
    let icon_w = if icon.is_some() { 20.0 + 8.0 } else { 0.0 };
    let size = vec2(galley.size().x + icon_w + 36.0, 40.0);
    if glass {
        return glass_pill_button(ui, theme, PillContent { label, icon, kind, galley, size });
    }
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let visual_rect = squish(ui, resp.id, &resp, rect, 0.035, 0.075);
    let pill_radius = (theme.shape_radius(ShapeRole::Full) as f32).min(visual_rect.height() * 0.5);
    let painter = ui.painter();

    let bg = match kind {
        ButtonKind::Filled => p.primary,
        ButtonKind::Tonal => theme.tonal_fill(),
        ButtonKind::Outlined | ButtonKind::Plain => Color32::TRANSPARENT,
    };
    painter.rect_filled(visual_rect, pill_radius, bg);
    ripple::state_layer(ui, theme, painter, &resp, visual_rect, pill_radius);
    if kind == ButtonKind::Outlined {
        painter.rect_stroke(visual_rect, pill_radius, Stroke::new(1.0, p.outline), StrokeKind::Inside);
    }
    if resp.has_focus() {
        painter.rect_stroke(visual_rect.expand(2.0), pill_radius + 2.0, Stroke::new(2.0, p.primary), StrokeKind::Outside);
    }

    let mut x = visual_rect.left() + 18.0;
    if let Some(icon) = icon {
        let glyph_scale = (visual_rect.width() / size.x).clamp(0.8, 1.0);
        icons::paint(theme, painter, icon, pos2(x + 10.0, visual_rect.center().y), 18.0 * glyph_scale, fg);
        x += icon_w;
    }
    let glyph_scale = (visual_rect.width() / size.x).clamp(0.8, 1.0);
    let text_galley =
        painter.layout_no_wrap(label.to_owned(), FontId::proportional(theme.text_size(TextRole::LabelLarge) * glyph_scale), fg);
    painter.galley(pos2(x, visual_rect.center().y - text_galley.size().y / 2.0), text_galley, fg);
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    resp
}

//! Поисковая строка, общая для всех экранов.

use super::glide::squish;
use crate::icons::{self, Icon};
use crate::theme::{ShapeRole, Theme, with_alpha};
use crate::ui::layout::Breakpoint;
use crate::ui::ripple;
use egui::{Align, Align2, Color32, Frame, Layout, Margin, Rect, Response, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

/// Material 3: на сколько px поднять поле ввода, чтобы середина строчных букв стояла на одной оси с лупой.
/// (Шрифт сажает буквы ниже центра строки; `margin` здесь не работает: при `Frame::NONE` он игнорируется.)
const M3_SEARCH_TEXT_LIFT: f32 = 5.0;

/// Максимальная ширина поиска Material 3 в зависимости от размера окна.
const M3_SEARCH_MAX_WIDTH: f32 = 760.0;

/// Общая поисковая строка для всех экранов. Геометрия одинакова,
/// а экран задаёт собственную подсказку и обработку Enter.
pub(crate) fn search_bar(ui: &mut Ui, theme: &Theme, value: &mut String, hint: &str) -> Response {
    let max_width = match Breakpoint::from_width(ui.ctx().viewport_rect().width()) {
        Breakpoint::Compact => 560.0,
        Breakpoint::Medium => M3_SEARCH_MAX_WIDTH,
        Breakpoint::Expanded => 1120.0,
    };
    search_bar_with_max_width(ui, theme, value, hint, max_width)
}

fn search_bar_with_max_width(ui: &mut Ui, theme: &Theme, value: &mut String, hint: &str, max_width: f32) -> Response {
    if theme.glass_active() { search_bar_glass(ui, theme, value, hint) } else { search_bar_m3(ui, theme, value, hint, max_width) }
}

/// Liquid Glass: inline-поле внутри контентного слоя. Используем спокойную стандартную
/// поверхность (не отдельную стеклянную капсулу) и точную общую ось выравнивания.
fn search_bar_glass(ui: &mut Ui, theme: &Theme, value: &mut String, hint: &str) -> Response {
    let p = &theme.palette;

    let width = ui.available_width();
    let (rect, outer) = ui.allocate_exact_size(vec2(width, 52.0), Sense::hover());
    let edit_id = ui.make_persistent_id(("mcrate_search_field", hint));
    let focused_before = ui.memory(|memory| memory.has_focus(edit_id));
    let hovered = outer.hovered();
    paint_glass_search_surface(ui, theme, rect, hovered);

    // Фиксированная геометрия: одинаковый вертикальный центр у лупы, текста и clear-кнопки.
    let response = ui
        .scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(16.0, 3.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let (icon_rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
            icons::paint(theme, ui.painter(), Icon::Search, icon_rect.center(), 20.0, p.on_surface_variant);
            ui.add_space(12.0);

            let show_clear = !value.is_empty();
            let clear_width = if show_clear { 28.0 } else { 0.0 };
            let field_width = (ui.available_width() - clear_width - if show_clear { 8.0 } else { 0.0 }).max(40.0);
            let edit = ui.add_sized(
                vec2(field_width, 36.0),
                egui::TextEdit::singleline(value)
                    .id(edit_id)
                    .frame(Frame::NONE)
                    .hint_text(hint)
                    .desired_width(field_width)
                    .margin(Margin::ZERO)
                    .align(Align2::LEFT_CENTER),
            );

            if show_clear {
                glass_clear_button(ui, theme, value);
            }
            edit
        })
        .inner;

    let focused = focused_before || response.has_focus();
    let border = if focused {
        with_alpha(p.primary, if p.dark { 150 } else { 135 })
    } else {
        with_alpha(p.on_surface, if hovered { 52 } else { 32 })
    };
    ui.painter().rect_stroke(rect, 17.0, Stroke::new(if focused { 1.4 } else { 0.8 }, border), StrokeKind::Inside);
    response.accessible_name(hint)
}

/// Фон поля поиска в Liquid Glass: заливка и блик по верхней кромке при наведении.
fn paint_glass_search_surface(ui: &Ui, theme: &Theme, rect: Rect, hovered: bool) {
    let p = &theme.palette;
    let bg_alpha = if p.dark {
        if hovered { 78 } else { 60 }
    } else if hovered {
        112
    } else {
        92
    };
    ui.painter().rect_filled(rect, 17.0, with_alpha(p.surface_container_high, bg_alpha));
    if hovered {
        ui.painter().line_segment(
            [pos2(rect.left() + 18.0, rect.top() + 1.0), pos2(rect.right() - 18.0, rect.top() + 1.0)],
            Stroke::new(0.8, with_alpha(Color32::WHITE, if p.dark { 32 } else { 70 })),
        );
    }
}

/// Кнопка очистки справа в поле поиска Liquid Glass (круглая подсветка при наведении и нажатии).
fn glass_clear_button(ui: &mut Ui, theme: &Theme, value: &mut String) {
    let p = &theme.palette;
    ui.add_space(8.0);
    let (clear_base, clear_resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let clear_rect = squish(ui, clear_resp.id, &clear_resp, clear_base, 0.06, 0.06);
    let color = if clear_resp.hovered() { p.on_surface } else { p.on_surface_variant };
    if clear_resp.hovered() || clear_resp.is_pointer_button_down_on() {
        ui.painter().circle_filled(
            clear_rect.center(),
            12.0,
            with_alpha(p.on_surface, if clear_resp.is_pointer_button_down_on() { 34 } else { 20 }),
        );
    }
    icons::paint(theme, ui.painter(), Icon::Close, clear_rect.center(), 16.0, color);
    if clear_resp.clicked() {
        value.clear();
    }
}

/// Material 3: SearchBar собственной геометрии (поле 56 px, общая ось у лупы и редактора).
fn search_bar_m3(ui: &mut Ui, theme: &Theme, value: &mut String, hint: &str, max_width: f32) -> Response {
    let p = &theme.palette;

    // Поле фиксированной высоты 56 px, общая ось у лупы и редактора, заметная обводка при фокусе
    // и кнопка очистки.
    let available_width = ui.available_width();
    let width = available_width.min(max_width);
    let (layout_rect, _) = ui.allocate_exact_size(vec2(available_width, 56.0), Sense::hover());
    let rect = Rect::from_center_size(layout_rect.center(), vec2(width, 56.0));
    let radius = theme.shape_radius(ShapeRole::Full);
    let edit_id = ui.make_persistent_id(("mcrate_search_field", hint));
    let focused_before = ui.memory(|memory| memory.has_focus(edit_id));
    let hovered = ui.interact(rect, ui.make_persistent_id(("mcrate_search_surface", hint)), Sense::hover()).hovered();
    ui.painter().rect_filled(rect, radius, p.surface_container_high);

    let icon_center = pos2(rect.left() + 32.0, rect.center().y);
    icons::paint(theme, ui.painter(), Icon::Search, icon_center, 24.0, p.on_surface_variant);

    let show_clear = !value.is_empty();
    let clear_space = if show_clear { 40.0 } else { 0.0 };
    let field_left = rect.left() + 56.0;
    let field_right = (rect.right() - 20.0 - clear_space).max(field_left + 32.0);
    // Редактор центрируем по геометрии поля (без скрытого смещения); оптическая ось подгоняется M3_SEARCH_TEXT_LIFT.
    let field_rect = Rect::from_min_size(
        pos2(field_left, rect.center().y - 18.0 - M3_SEARCH_TEXT_LIFT),
        vec2((field_right - field_left).max(32.0), 36.0),
    );
    let response = ui
        .scope_builder(UiBuilder::new().max_rect(field_rect).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.add_sized(
                field_rect.size(),
                egui::TextEdit::singleline(value)
                    .id(edit_id)
                    .frame(Frame::NONE)
                    .hint_text(hint)
                    .desired_width(field_rect.width())
                    .margin(Margin::ZERO)
                    .align(Align2::LEFT_CENTER),
            )
        })
        .inner
        .accessible_name(hint);

    if show_clear {
        m3_clear_button(ui, theme, rect, hint, value);
    }

    let focused = focused_before || response.has_focus();
    paint_m3_search_border(ui, theme, (rect, radius), (focused, hovered));
    response
}

/// Кнопка очистки (крестик) справа в поле поиска Material 3. `rect`: всё поле поиска.
fn m3_clear_button(ui: &mut Ui, theme: &Theme, rect: Rect, hint: &str, value: &mut String) {
    let p = &theme.palette;
    let clear_rect = Rect::from_center_size(pos2(rect.right() - 32.0, rect.center().y), vec2(28.0, 28.0));
    let clear_resp = ui.interact(clear_rect, ui.make_persistent_id(("mcrate_search_clear", hint)), Sense::click());
    let clear_rect = squish(ui, clear_resp.id, &clear_resp, clear_rect, 0.06, 0.06);
    let color = if clear_resp.hovered() { p.on_surface } else { p.on_surface_variant };
    ripple::state_layer(ui, theme, ui.painter(), &clear_resp, clear_rect, 14.0);
    let c = clear_rect.center();
    let d = 4.0;
    ui.painter().line_segment([pos2(c.x - d, c.y - d), pos2(c.x + d, c.y + d)], Stroke::new(1.6, color));
    ui.painter().line_segment([pos2(c.x + d, c.y - d), pos2(c.x - d, c.y + d)], Stroke::new(1.6, color));
    if clear_resp.clicked() {
        value.clear();
    }
    clear_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Очистить поиск"));
    clear_resp.on_hover_text("Очистить поиск");
}

/// Обводка поля поиска Material 3: толще и ярче при фокусе, тонкая при наведении, без обводки в покое.
fn paint_m3_search_border(ui: &Ui, theme: &Theme, (rect, radius): (Rect, u8), (focused, hovered): (bool, bool)) {
    let p = &theme.palette;
    let border = if focused {
        p.primary
    } else if hovered {
        p.outline
    } else {
        with_alpha(p.outline_variant, 0)
    };
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(
            if focused {
                2.0
            } else if hovered {
                1.0
            } else {
                0.0
            },
            border,
        ),
        StrokeKind::Inside,
    );
}

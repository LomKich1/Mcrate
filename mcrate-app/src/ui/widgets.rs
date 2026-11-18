//! Общие виджеты. Рисуются вручную (painter), цвета берут из темы,
//! поэтому выглядят правильно и в Material 3, и в Liquid Glass.

use egui::{Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Layout, Margin, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2, layers::ShapeIdx};
use mcrate_core::Track;

use crate::icons::{self, Icon};
use crate::settings::Design;
use super::layout::Breakpoint;
use crate::theme::{ShapeRole, TextRole, Theme, with_alpha};


/// Сдвиг текста в поле поиска по вертикали, px (вниз при положительном). Одна ручка для обеих тем:
/// текст и лупа должны стоять на одной оси. Отступ сверху сдвигает центр строки на половину своего значения.
const SEARCH_TEXT_SHIFT: i8 = 0;

/// Material 3: на сколько px поднять поле ввода, чтобы середина строчных букв стояла на одной оси с лупой.
/// (Шрифт сажает буквы ниже центра строки; `margin` здесь не работает: при `Frame::NONE` он игнорируется.)
const M3_SEARCH_TEXT_LIFT: f32 = 5.0;

/// Максимальная ширина поиска Material 3 в зависимости от размера окна.
const M3_SEARCH_MAX_WIDTH: f32 = 760.0;

fn search_text_margin() -> Margin {
    Margin { left: 0, right: 0, top: SEARCH_TEXT_SHIFT * 2, bottom: 0 }
}

/// Общая поисковая строка для всех экранов. Геометрия одинакова,
/// а экран задаёт собственную подсказку и обработку Enter.
pub fn search_bar(ui: &mut Ui, theme: &Theme, value: &mut String, hint: &str) -> Response {
    let max_width = match Breakpoint::from_width(ui.ctx().viewport_rect().width()) {
        Breakpoint::Compact => 560.0,
        Breakpoint::Medium => M3_SEARCH_MAX_WIDTH,
        Breakpoint::Expanded => 1120.0,
    };
    search_bar_with_max_width(ui, theme, value, hint, max_width)
}

fn search_bar_with_max_width(ui: &mut Ui, theme: &Theme, value: &mut String, hint: &str, max_width: f32) -> Response {
    let p = &theme.palette;

    // Material 3 сохраняет собственную геометрию SearchBar. В Liquid Glass это
    // inline-поле внутри контентного слоя, поэтому используем спокойную стандартную
    // поверхность (не отдельную стеклянную капсулу) и точную общую ось выравнивания.
    if theme.glass_active() {
        let width = ui.available_width();
        let (rect, outer) = ui.allocate_exact_size(vec2(width, 52.0), Sense::hover());
        let edit_id = ui.make_persistent_id(("mcrate_search_field", hint));
        let focused_before = ui.memory(|memory| memory.has_focus(edit_id));
        let hovered = outer.hovered();
        let bg_alpha = if p.dark { if hovered { 78 } else { 60 } } else if hovered { 112 } else { 92 };
        ui.painter().rect_filled(rect, 17.0, with_alpha(p.surface_container_high, bg_alpha));
        if hovered {
            ui.painter().line_segment(
                [pos2(rect.left() + 18.0, rect.top() + 1.0), pos2(rect.right() - 18.0, rect.top() + 1.0)],
                Stroke::new(0.8, with_alpha(Color32::WHITE, if p.dark { 32 } else { 70 })),
            );
        }

        // Фиксированная геометрия: одинаковый вертикальный центр у лупы, текста и clear-кнопки.
        let response = ui.scope_builder(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(16.0, 3.0)))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
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
                        .margin(search_text_margin())
                        .align(Align2::LEFT_CENTER),
                );

                if show_clear {
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
                edit
            },
        ).inner;

        let focused = focused_before || response.has_focus();
        let border = if focused {
            with_alpha(p.primary, if p.dark { 150 } else { 135 })
        } else {
            with_alpha(p.on_surface, if hovered { 52 } else { 32 })
        };
        ui.painter().rect_stroke(rect, 17.0, Stroke::new(if focused { 1.4 } else { 0.8 }, border), StrokeKind::Inside);
        return response.accessible_name(hint);
    }

    // Material 3 SearchBar: fixed 56 px field, a shared center axis for the
    // search icon and editor, visible focus treatment, and a clear affordance.
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
    // Редактор центрируем по геометрии поля (без скрытого смещения). Подгонка оптической
    // оси делается одной константой SEARCH_TEXT_SHIFT для обеих тем.
    let field_rect = Rect::from_min_size(
        pos2(field_left, rect.center().y - 18.0 - M3_SEARCH_TEXT_LIFT),
        vec2((field_right - field_left).max(32.0), 36.0),
    );
    let response = ui.scope_builder(
        UiBuilder::new()
            .max_rect(field_rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.add_sized(
                field_rect.size(),
                egui::TextEdit::singleline(value)
                    .id(edit_id)
                    .frame(Frame::NONE)
                    .hint_text(hint)
                    .desired_width(field_rect.width())
                    .margin(search_text_margin())
                    .align(Align2::LEFT_CENTER),
            )
        },
    ).inner.accessible_name(hint);

    if show_clear {
        let clear_rect = Rect::from_center_size(pos2(rect.right() - 32.0, rect.center().y), vec2(28.0, 28.0));
        let clear_resp = ui.interact(clear_rect, ui.make_persistent_id(("mcrate_search_clear", hint)), Sense::click());
        let clear_rect = squish(ui, clear_resp.id, &clear_resp, clear_rect, 0.06, 0.06);
        let color = if clear_resp.hovered() { p.on_surface } else { p.on_surface_variant };
        super::ripple::state_layer(ui, theme, ui.painter(), &clear_resp, clear_rect, 14.0, p.on_surface, 20, 31);
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

    let focused = focused_before || response.has_focus();
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
        Stroke::new(if focused { 2.0 } else if hovered { 1.0 } else { 0.0 }, border),
        StrokeKind::Inside,
    );
    response
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Plain,    // только иконка без постоянной подложки
    Outlined, // контурная кнопка
    Tonal,    // приглушённая заливка
    Filled,   // главный акцент (кнопка «играть»)
}

/// M3 snackbar: инверсная поверхность, контрастный текст и действие inverse-primary.
/// Возвращает true, если пользователь нажал на закрытие.
pub fn status_snackbar(ui: &mut Ui, theme: &Theme, message: &str) -> bool {
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
                    super::ripple::state_layer_colored(
                        ui, theme, ui.painter(), &response, rect, 15.0,
                        p.inverse_on_surface, 20, 31,
                    );
                    icons::paint(theme, ui.painter(), Icon::Close, rect.center(), 18.0, p.inverse_primary);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(message)
                                .size(theme.text_size(TextRole::BodyMedium))
                                .color(p.inverse_on_surface),
                        )
                        .wrap(),
                    );
                    response.on_hover_text("Закрыть").on_hover_cursor(CursorIcon::PointingHand).clicked()
                }).inner
            }).inner
        })
        .inner
}

/// Ошибка в контейнере семантического цвета. В Liquid Glass остаётся прежний тихий текстовый стиль.
pub fn error_banner(ui: &mut Ui, theme: &Theme, message: &str) {
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
                ui.painter().text(
                    badge.center(),
                    Align2::CENTER_CENTER,
                    "!",
                    FontId::proportional(13.0),
                    p.on_error,
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(message)
                            .size(theme.text_size(TextRole::BodySmall))
                            .color(p.on_error_container),
                    )
                    .wrap(),
                );
            });
        });
}


// ---------------------------------------------------------------------------------------------
// Анимации по образцу демо «Анимации плеера в стиле Riff»
// ---------------------------------------------------------------------------------------------

/// CSS `cubic-bezier(x1, y1, x2, y2)`: превращает время 0..1 в прогресс (может выходить за 1: «пружина»).
/// Решаем x(s) = t бисекцией (деление отрезка пополам), затем берём y(s).
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let at = |a: f32, b: f32, s: f32| 3.0 * (1.0 - s) * (1.0 - s) * s * a + 3.0 * (1.0 - s) * s * s * b + s * s * s;
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        if at(x1, x2, mid) < t { lo = mid } else { hi = mid }
    }
    at(y1, y2, (lo + hi) / 2.0)
}

/// Кривая «нажатия» из демо: `cubic-bezier(.3, 1.6, .5, 1)`, с небольшим перелётом.
fn press_curve(t: f32) -> f32 {
    cubic_bezier(0.3, 1.6, 0.5, 1.0, t)
}

/// Плавное «сжатие» при нажатии (в демо `scale(.88, .94)` у кнопки): прямоугольник уменьшается
/// вокруг центра на `sx`/`sy` (доли) с перелётом при нажатии и отпускании.
pub(super) fn squish(ui: &Ui, id: Id, resp: &Response, rect: Rect, sx: f32, sy: f32) -> Rect {
    let t = ui.ctx().animate_bool_with_time(id.with("press"), resp.is_pointer_button_down_on(), 0.18);
    let k = press_curve(t);
    Rect::from_center_size(rect.center(), vec2(rect.width() * (1.0 - sx * k), rect.height() * (1.0 - sy * k)))
}

/// Параметры физической пружины, как в HTML-демо:
/// F = -k * (x - target) - c * velocity.
const SPRING_STIFFNESS: f32 = 500.0;
const SPRING_DAMPING: f32 = 34.0;
const SPRING_EPSILON: f32 = 0.05;

/// Один шаг пружины. Скорость намеренно не обнуляется при смене цели:
/// быстрые перемещения разворачивают текущий импульс, а не начинают анимацию заново.
pub(super) fn spring_step(value: &mut f32, velocity: &mut f32, target: f32, dt: f32) -> bool {
    if dt > 0.0 {
        let force = -SPRING_STIFFNESS * (*value - target) - SPRING_DAMPING * *velocity;
        *velocity += force * dt;
        *value += *velocity * dt;
    }

    if (*value - target).abs() < SPRING_EPSILON && (*velocity).abs() < SPRING_EPSILON {
        *value = target;
        *velocity = 0.0;
        false
    } else {
        true
    }
}

/// Состояние скользящей подсветки (хранится в памяти egui).
/// Положение и скорость разделены для двух углов прямоугольника — так подсветка
/// корректно анимируется и при изменении размеров целевой строки.
#[derive(Clone, Copy)]
struct Glide {
    key: u64,
    target: Rect,
    min: Pos2,
    max: Pos2,
    velocity_min: Vec2,
    velocity_max: Vec2,
    alpha: f32,
    alive: bool,
}

impl Default for Glide {
    fn default() -> Self {
        Self {
            key: 0,
            target: Rect::ZERO,
            min: pos2(0.0, 0.0),
            max: pos2(0.0, 0.0),
            velocity_min: Vec2::ZERO,
            velocity_max: Vec2::ZERO,
            alpha: 0.0,
            alive: false,
        }
    }
}

/// Резервирует слой, на котором после отрисовки строк будет нарисована подсветка.
pub fn glide_begin(ui: &Ui) -> ShapeIdx {
    ui.painter().add(Shape::Noop)
}

/// Скользящая подсветка под курсором: одна на весь список. При переходе на другую
/// строку пружина получает новую цель, сохраняя текущую скорость — даже при быстрых
/// перемещениях курсора она не телепортируется и не начинает движение с нуля.
pub fn glide_end(ui: &Ui, theme: &Theme, slot: ShapeIdx, salt: Id, target: Option<(u64, Rect)>, radius: f32) {
    glide_end_impl(ui, theme, slot, salt, target, radius, None);
}

/// То же, что `glide_end`, но в Material 3 вместо тёмной подложки рисует плоский «слой состояния»
/// (цвет контента с заданной непрозрачностью 0..255), как у M3-карточек. Слот надо резервировать
/// ПОСЛЕ карточек (`glide_begin` после их добавления): слой ложится поверх них и плавно ездит между ними.
/// В Liquid Glass параметр `flat` игнорируется (там стеклянная подсветка).
pub fn glide_end_flat(ui: &Ui, theme: &Theme, slot: ShapeIdx, salt: Id, target: Option<(u64, Rect)>, radius: f32, flat: (Color32, u8)) {
    glide_end_impl(ui, theme, slot, salt, target, radius, Some(flat));
}

fn glide_end_impl(ui: &Ui, theme: &Theme, slot: ShapeIdx, salt: Id, target: Option<(u64, Rect)>, radius: f32, flat: Option<(Color32, u8)>) {
    if !theme.glass_active() {
        // Material 3: подсветка растёт кружком из точки, куда навели курсор, и едет за ним (см. ripple.rs).
        let (color, hover_a) = flat.unwrap_or((theme.hover_glide_color(), theme.hover_glide_alpha()));
        let press_a = if flat.is_some() { hover_a as u16 * 3 / 2 } else { hover_a as u16 * 5 / 4 }.min(255) as u8;
        super::ripple::glide_ripple(ui, theme, slot, salt, target, radius, color, hover_a, press_a);
        return;
    }
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.064);
    let mut g: Glide = ui.ctx().data_mut(|d| d.get_temp(salt)).unwrap_or_default();
    let mut moving = false;

    if let Some((key, rect)) = target {
        if !g.alive || g.alpha < 0.02 {
            // После полного исчезновения подсветка появляется сразу под новым пунктом.
            g.key = key;
            g.target = rect;
            g.min = rect.min;
            g.max = rect.max;
            g.velocity_min = Vec2::ZERO;
            g.velocity_max = Vec2::ZERO;
            g.alive = true;
        } else {
            // Меняем только цель. Текущее положение и импульс сохраняются.
            if g.key != key {
                g.key = key;
            }
            g.target = rect;
        }
    }

    if g.alive {
        moving |= spring_step(&mut g.min.x, &mut g.velocity_min.x, g.target.min.x, dt);
        moving |= spring_step(&mut g.min.y, &mut g.velocity_min.y, g.target.min.y, dt);
        moving |= spring_step(&mut g.max.x, &mut g.velocity_max.x, g.target.max.x, dt);
        moving |= spring_step(&mut g.max.y, &mut g.velocity_max.y, g.target.max.y, dt);
    }

    if target.is_some() {
        g.alpha += (1.0 - g.alpha) * (1.0 - (-dt * 20.0).exp());
        if g.alpha > 0.99 {
            g.alpha = 1.0;
        } else {
            moving = true;
        }
    } else {
        g.alpha -= g.alpha * (1.0 - (-dt * 14.0).exp());
        if g.alpha < 0.01 {
            g.alpha = 0.0;
            g.alive = false;
        } else {
            moving = true;
        }
    }

    if g.alive && g.alpha > 0.0 {
        let shown = Rect::from_min_max(g.min, g.max);
        if theme.glass_active() {
            // В Liquid Glass анимируется не цветная подсветка, а сама стеклянная
            // поверхность выбранной строки: материал остаётся настоящим GPU glass.
            // Выделение = только эффекты стекла (как у мини-плеера), без заливки. Кладём в слот,
            // который резервирует glide_begin: фигура оказывается ПОД текстом и обложками.
            let mut style = crate::theme::glass::selection_style(theme, radius);
            style.border *= g.alpha;
            style.shadow *= g.alpha;
            ui.painter().set(slot, egui_glass::glass_shape(ui, shown, &style));
        }
    }
    if moving {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(salt, g));
}

/// Отдельная круглая кнопка в контентном слое: в Liquid Glass используется реальный
/// GPU-материал GlassButton, а не имитация прозрачности. Не использовать внутри PlayerBar
/// или Rail — там уже есть общая стеклянная поверхность и нужны плоские дочерние контроли.
pub fn glass_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: Icon,
    diameter: f32,
    kind: ButtonKind,
    active: bool,
    tip: &str,
) -> Response {
    if !theme.glass_active() {
        return icon_button(ui, theme, icon, diameter, kind, active, tip);
    }

    let p = &theme.palette;
    let prominent = kind == ButtonKind::Filled;
    let response = egui_glass::GlassButton::new("")
        .style(crate::theme::glass::control_style(theme, prominent, diameter.max(44.0) * 0.5))
        .icon()
        .min_size(Vec2::splat(diameter.max(44.0)))
        .accessible_name(tip)
        .show(ui);
    let foreground = if prominent { p.on_primary } else if active { p.primary } else { p.on_surface };
    // GlassButton animates its own glass surface, but custom-painted glyphs are
    // outside that widget. Apply the same press transform to the glyph itself.
    let visual_rect = squish(ui, response.id, &response, response.rect, 0.055, 0.055);
    let glyph_scale = (visual_rect.width() / response.rect.width()).clamp(0.8, 1.0);
    icons::paint(theme, ui.painter(), icon, visual_rect.center(), diameter * 0.58 * glyph_scale, foreground);
    response.on_hover_text(tip).on_hover_cursor(CursorIcon::PointingHand)
}

/// Pill-кнопка с настоящим Liquid Glass backdrop callback. Иконка и подпись остаются
/// отдельными глифами, поэтому сохраняют разный размер/шрифт и корректную доступность.
fn glass_pill_button(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    icon: Option<Icon>,
    kind: ButtonKind,
    galley: std::sync::Arc<egui::Galley>,
    size: Vec2,
) -> Response {
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
        let icon_center = visual_rect.center() + (pos2(original_left + 10.0, response.rect.center().y) - response.rect.center()) * glyph_scale;
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
pub fn icon_button(ui: &mut Ui, theme: &Theme, icon: Icon, diameter: f32, kind: ButtonKind, active: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    let p = &theme.palette;
    let glass = theme.glass_active();
    let visual_rect = squish(ui, resp.id, &resp, rect, 0.055, 0.055);
    let fg = match kind {
        ButtonKind::Filled => p.on_primary,
        ButtonKind::Tonal => if glass { p.on_surface } else { theme.on_tonal() },
        ButtonKind::Outlined => p.primary,
        ButtonKind::Plain => if active { p.primary } else { p.on_surface },
    };
    let painter = ui.painter();
    let r = visual_rect.width() / 2.0;

    if glass {
        // В Liquid Glass кнопки не используют плотную M3 tonal-заливку:
        // только важное действие получает prominent tint, остальные — тихую оптическую поверхность.
        let pressed = resp.is_pointer_button_down_on();
        let hovered = resp.hovered();
        let state_t = ui.ctx().animate_bool_with_time(
            resp.id.with("glass_feedback"),
            pressed || hovered || resp.has_focus() || active,
            0.14,
        );
        let (fill, stroke) = match kind {
            ButtonKind::Filled => (
                Some(with_alpha(p.primary, if p.dark { 224 } else { 218 })),
                with_alpha(p.on_primary, 92),
            ),
            ButtonKind::Tonal => (
                Some(with_alpha(p.surface_container_high, (82.0 + state_t * 60.0) as u8)),
                with_alpha(p.on_surface, (42.0 + state_t * 38.0) as u8),
            ),
            ButtonKind::Outlined => (
                Some(with_alpha(p.surface_container_high, (36.0 + state_t * 80.0) as u8)),
                with_alpha(p.on_surface, (54.0 + state_t * 34.0) as u8),
            ),
            ButtonKind::Plain if active => (
                Some(with_alpha(p.primary, (54.0 + state_t * 30.0) as u8)),
                with_alpha(p.primary, 86),
            ),
            ButtonKind::Plain if hovered || pressed || resp.has_focus() => (
                Some(with_alpha(p.on_surface, (10.0 + state_t * 28.0) as u8)),
                with_alpha(p.on_surface, 48),
            ),
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
            painter.circle_stroke(
                visual_rect.center(),
                r + 2.0,
                Stroke::new(1.6, with_alpha(p.primary, 220)),
            );
        }
    } else {
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
        super::ripple::state_layer(ui, theme, painter, &resp, circle, r, fg, 20, 31);
        if kind == ButtonKind::Outlined {
            painter.circle_stroke(visual_rect.center(), r - 0.5, Stroke::new(1.0, p.outline));
        }
        if resp.has_focus() {
            painter.circle_stroke(visual_rect.center(), r + 1.5, Stroke::new(2.0, p.primary));
        }
    }

    let glyph_scale = (visual_rect.width() / rect.width()).clamp(0.8, 1.0);
    let mut glyph_center = visual_rect.center();
    if icon == Icon::Play && theme.glass_active() { glyph_center.x += 1.2; }
    icons::paint(theme, painter, icon, glyph_center, diameter * 0.6 * glyph_scale, fg);
    let resp = resp.on_hover_text(tip).on_hover_cursor(CursorIcon::PointingHand);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, tip));
    resp
}

/// Кнопка-«таблетка» с текстом и необязательной иконкой.
pub fn pill_button(ui: &mut Ui, theme: &Theme, label: &str, icon: Option<Icon>, kind: ButtonKind) -> Response {
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
        return glass_pill_button(ui, theme, label, icon, kind, galley, size);
    }
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let visual_rect = squish(ui, resp.id, &resp, rect, 0.035, 0.075);
    let pill_radius = if glass { 20.0 } else { (theme.shape_radius(ShapeRole::Full) as f32).min(visual_rect.height() * 0.5) };
    let painter = ui.painter();

    if glass {
        let state_t = ui.ctx().animate_bool_with_time(
            resp.id.with("glass_feedback"),
            resp.hovered() || resp.is_pointer_button_down_on() || resp.has_focus(),
            0.14,
        );
        let (fill, stroke) = match kind {
            ButtonKind::Filled => (with_alpha(p.primary, if p.dark { 224 } else { 218 }), with_alpha(p.on_primary, 90)),
            ButtonKind::Tonal => (
                with_alpha(p.surface_container_high, (82.0 + state_t * 64.0) as u8),
                with_alpha(p.on_surface, (42.0 + state_t * 38.0) as u8),
            ),
            ButtonKind::Outlined => (
                with_alpha(p.surface_container_high, (36.0 + state_t * 80.0) as u8),
                with_alpha(p.on_surface, (54.0 + state_t * 34.0) as u8),
            ),
            ButtonKind::Plain => (with_alpha(p.on_surface, (state_t * 28.0) as u8), with_alpha(p.on_surface, 30)),
        };
        if kind == ButtonKind::Filled {
            painter.rect_filled(visual_rect.translate(vec2(0.0, 1.0)), 20.0, Color32::from_black_alpha(if p.dark { 42 } else { 24 }));
        }
            painter.rect_filled(visual_rect, pill_radius, fill);
        if !matches!(kind, ButtonKind::Plain) || resp.hovered() || resp.has_focus() {
            painter.rect_stroke(visual_rect, pill_radius, Stroke::new(0.9, stroke), StrokeKind::Inside);
        }
        if resp.has_focus() {
            painter.rect_stroke(
                visual_rect.expand(2.0),
                pill_radius + 2.0,
                Stroke::new(1.6, with_alpha(p.primary, 220)),
                StrokeKind::Outside,
            );
        }
    } else {
        let bg = match kind {
            ButtonKind::Filled => p.primary,
            ButtonKind::Tonal => theme.tonal_fill(),
            ButtonKind::Outlined | ButtonKind::Plain => Color32::TRANSPARENT,
        };
        painter.rect_filled(visual_rect, pill_radius, bg);
        super::ripple::state_layer(ui, theme, painter, &resp, visual_rect, pill_radius, fg, 20, 31);
        if kind == ButtonKind::Outlined {
            painter.rect_stroke(visual_rect, pill_radius, Stroke::new(1.0, p.outline), StrokeKind::Inside);
        }
        if resp.has_focus() {
            painter.rect_stroke(visual_rect.expand(2.0), pill_radius + 2.0, Stroke::new(2.0, p.primary), StrokeKind::Outside);
        }
    }

    let mut x = visual_rect.left() + 18.0;
    if let Some(icon) = icon {
        let glyph_scale = (visual_rect.width() / size.x).clamp(0.8, 1.0);
        icons::paint(theme, painter, icon, pos2(x + 10.0, visual_rect.center().y), 18.0 * glyph_scale, fg);
        x += icon_w;
    }
    let glyph_scale = (visual_rect.width() / size.x).clamp(0.8, 1.0);
    let text_galley = painter.layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme.text_size(TextRole::LabelLarge) * glyph_scale),
        fg,
    );
    painter.galley(pos2(x, visual_rect.center().y - text_galley.size().y / 2.0), text_galley, fg);
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    resp
}

/// Интерполяция sRGBA для плавного изменения цвета текста навигации.
fn mix_color(from: Color32, to: Color32, t: f32) -> Color32 {
    let a = from.to_array();
    let b = to.to_array();
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
        (a[3] as f32 + (b[3] as f32 - a[3] as f32) * t).round() as u8,
    )
}

/// Доля вдавливания пункта навигации при нажатии. Общая для пункта и пилюли выбора (rail.rs), чтобы они сжимались синхронно.
pub(super) const NAV_PRESS: f32 = 0.06;

/// Пункт навигации (ширина = вся доступная, высота 48, форма «таблетка»).
/// `compact`: только иконка по центру (узкая панель), подпись показывается во всплывающей подсказке.
pub fn nav_item(ui: &mut Ui, theme: &Theme, icon: Icon, label: &str, selected: bool, enabled: bool, trailing: Option<&str>, compact: bool) -> Response {
    let p = &theme.palette;
    let size = vec2(ui.available_width(), 48.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let id = resp.id;
    // Вдавливание строго равномерное (одинаково по ширине и высоте): разные коэффициенты давали ощущение наклона.
    let rect = if enabled { squish(ui, id, &resp, rect, NAV_PRESS, NAV_PRESS) } else { rect };
    let painter = ui.painter();

    // Выбранную капсулу рисует rail.rs. Для остальных пунктов Liquid Glass добавляет
    // лишь лёгкий hover-glow, чтобы не наслаивать вторую стеклянную поверхность.
    if theme.glass_active() && enabled && !selected {
        let t = ui.ctx().animate_bool_with_time(id.with("glass_hover"), resp.hovered() || resp.is_pointer_button_down_on(), 0.16);
        if t > 0.01 {
            painter.rect_filled(rect.shrink(1.0), 22.0, with_alpha(p.on_surface, (t * if resp.is_pointer_button_down_on() { 32.0 } else { 20.0 }) as u8));
        }
    } else if enabled && !selected {
        // У выбранного пункта слой не рисуем: он уже подсвечен пилюлей (rail.rs).
        // M3: кружок от курсора при наведении, при нажатии разрастается до краёв пункта.
        super::ripple::state_layer(ui, theme, painter, &resp, rect, 24.0, p.on_surface, 20, 31);
    }
    if resp.has_focus() {
        painter.rect_stroke(
            rect.shrink(2.0),
            22.0,
            Stroke::new(if theme.glass_active() { 1.5 } else { 2.0 }, p.primary),
            StrokeKind::Inside,
        );
    }

    let fg = if !enabled {
        with_alpha(p.on_surface_variant, 120)
    } else {
        // Как в CSS-примере: активный и наведённый пункты плавно переходят
        // от приглушённого текста к основному цвету за 0.3 секунды.
        let selected_fg = match theme.design() {
            Design::Material3 => theme.on_tonal(),
            Design::LiquidGlass => p.on_surface,
        };
        let emphasized = selected || resp.hovered();
        let emphasized_fg = if selected { selected_fg } else { p.on_surface };
        let t = ui.ctx().animate_bool_with_time(id.with("emphasis"), emphasized, 0.3);
        mix_color(p.on_surface_variant, emphasized_fg, t)
    };

    // Фон выбранного пункта рисует rail.rs: пилюля, которая плавно скользит между пунктами.
    let glyph_scale = if enabled {
        (rect.width() / size.x).clamp(0.8, 1.0)
    } else { 1.0 };
    if compact {
        icons::paint(theme, painter, icon, rect.center(), 24.0 * glyph_scale, fg);
    } else {
        let icon_center = rect.center() + vec2((28.0 - size.x * 0.5) * glyph_scale, 0.0);
        icons::paint(theme, painter, icon, icon_center, 24.0 * glyph_scale, fg);
        let label_size = if theme.glass_active() { 15.0 } else { theme.text_size(TextRole::LabelLarge) };
        painter.text(pos2(rect.left() + 52.0 * glyph_scale, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(label_size * glyph_scale), fg);
        if let Some(t) = trailing {
            painter.text(pos2(rect.right() - 14.0 * glyph_scale, rect.center().y), Align2::RIGHT_CENTER, t, FontId::proportional(11.0 * glyph_scale), fg);
        }
    }
    let resp = if compact { resp.on_hover_text(label) } else { resp };
    let resp = if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp };
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, selected, label));
    resp
}

/// Результат ползунка: `value` = Some(доля 0..1), пока пользователь тянет или кликает.
pub struct BarOutput {
    pub response: Response,
    pub value: Option<f32>,
}

/// Ползунок для перемотки и громкости. M3 Expressive: ручка-палочка и дорожка с зазором,
/// Liquid Glass сохраняет своё стеклянное оформление.
pub fn slider_bar(ui: &mut Ui, theme: &Theme, width: f32, frac: f32, enabled: bool, label: &str) -> BarOutput {
    let p = &theme.palette;
    let sense = if enabled { Sense::click_and_drag() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 24.0), sense);
    let pad = 10.0;
    let (left, right) = (rect.left() + pad, rect.right() - pad);
    let cy = rect.center().y;
    let active = resp.hovered() || resp.dragged();
    let frac = frac.clamp(0.0, 1.0);
    let x = left + (right - left) * frac;

    let painter = ui.painter();
    let track_bg = with_alpha(p.on_surface, if p.dark { 50 } else { 40 });
    let fg = if enabled { p.primary } else { with_alpha(p.on_surface_variant, 120) };

    match theme.design() {
        Design::Material3 => {
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
            let hw = ui.ctx().animate_value_with_time(
                resp.id.with("handle_w"),
                if resp.dragged() { HANDLE_W_PRESSED } else { HANDLE_W },
                0.1,
            );
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
        Design::LiquidGlass => {
            let track_height = ui.ctx().animate_value_with_time(
                resp.id.with("glass_track_height"),
                if active { 7.0 } else { 5.0 },
                0.12,
            );
            let track_rect = Rect::from_min_max(
                pos2(left, cy - track_height / 2.0),
                pos2(right, cy + track_height / 2.0),
            );
            painter.rect_filled(track_rect, track_height / 2.0, track_bg);
            if x > left {
                painter.rect_filled(
                    Rect::from_min_max(pos2(left, cy - track_height / 2.0), pos2(x, cy + track_height / 2.0)),
                    track_height / 2.0,
                    fg,
                );
            }
            let thumb_r = ui.ctx().animate_value_with_time(
                resp.id.with("glass_thumb_radius"),
                if active { 9.5 } else { 7.5 },
                0.12,
            );
            let thumb_rect = Rect::from_center_size(pos2(x, cy), vec2(thumb_r * 2.0, thumb_r * 2.0));
            if active && enabled {
                // Apple shows the glass material transiently on active sliders/toggles,
                // rather than turning the whole content surface into glass.
                let style = crate::theme::glass::control_style(theme, false, thumb_r);
                egui_glass::paint_glass(ui, thumb_rect, &style);
                painter.circle_stroke(
                    pos2(x, cy),
                    thumb_r - 0.5,
                    Stroke::new(0.9, with_alpha(p.on_surface, if p.dark { 120 } else { 92 })),
                );
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
    }

    let pointer_value = if resp.dragged() || resp.clicked() || resp.drag_stopped() {
        resp.interact_pointer_pos().map(|pos| ((pos.x - left) / (right - left)).clamp(0.0, 1.0))
    } else {
        None
    };
    // Клавиатурное управление делает прогресс и громкость доступнее: стрелки меняют
    // значение на 1%, Shift + стрелка — на 10%, Home/End переходят к границам.
    let keyboard_value = if enabled && resp.has_focus() {
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
    } else {
        None
    };
    let value = pointer_value.or(keyboard_value);
    resp.widget_info(|| egui::WidgetInfo::slider(enabled, value.unwrap_or(frac) as f64, label));
    BarOutput { response: resp, value }
}

/// Цветная плитка-заглушка вместо обложки (обложек в ядре пока нет).
pub fn cover_tile(ui: &mut Ui, theme: &Theme, rect: Rect, seed: &str, radius: f32) {
    let p = &theme.palette;
    let h = seed.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let (mut bg, fg) = match h % 3 {
        0 => (p.primary_container, p.on_primary_container),
        1 => (p.secondary_container, p.on_secondary_container),
        _ => (
            p.tertiary_container,
            if theme.design() == Design::Material3 {
                p.on_tertiary_container
            } else {
                p.on_surface
            },
        ),
    };
    // Эти цветные плитки — временная замена обложек, а не M3-контейнеры.
    // В Liquid Glass слегка приглушаем тон, чтобы заглушки не спорили с альбомным рядом.
    if theme.glass_active() {
        bg = bg.lerp_to_gamma(p.surface_container_lowest, 0.26);
    }
    let painter = ui.painter();
    painter.rect_filled(rect, radius, bg);
    icons::paint(theme, painter, Icon::Note, rect.center(), rect.height() * 0.5, with_alpha(fg, 200));
}

/// Индикатор загрузки Material 3 (indeterminate circular progress), без подписей.
/// Анимация повторяет спецификацию M3: дуга растёт и сжимается (голова и хвост бегут с задержкой),
/// пока вся фигура вращается; один цикл 1332 мс, пять циклов до полного повтора вращения.
/// Рисуется в обеих темах одинаково, цвет `primary`. Сам просит перерисовку, пока виден.
pub fn loading_indicator(ui: &mut Ui, theme: &Theme, size: f32) -> Response {
    const CYCLE: f64 = 1.332; // секунд на один рост и сжатие дуги
    const HEAD_TAIL: f32 = 0.666; // длительность бега головы и хвоста
    const JUMP: f32 = 290.0; // на сколько градусов вырастает дуга
    const ROTATIONS: f64 = 5.0; // циклов до повтора вращения
    const BASE_DEG: f32 = 286.0; // равномерное вращение за ROTATIONS циклов
    const STANDARD: [f32; 4] = [0.4, 0.0, 0.2, 1.0];

    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    ui.ctx().request_repaint();

    let time = ui.input(|i| i.time);
    let in_cycle = (time % CYCLE) as f32;
    let cycle_index = (time / CYCLE).floor() as i64 % ROTATIONS as i64;
    let ease = |x: f32| super::transition::cubic_bezier(STANDARD, x.clamp(0.0, 1.0));
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

/// Контур загрузки: яркий отрезок с затухающим хвостом бежит по периметру обложки, пока трек грузится.
/// Чисел нет. Контур рисуется чуть внутри обложки (`rect`), чтобы его не обрезала область панели.
/// При включённом «уменьшении движения» вместо бега показывается неподвижная рамка.
pub fn loading_outline(ui: &Ui, theme: &Theme, cover: Rect, radius: f32, time: f64) {
    const STEPS: usize = 6; // сегментов на каждый скруглённый угол
    const TRAIL: f32 = 0.30; // длина бегущего отрезка, доля периметра
    const LAP_SECS: f64 = 1.4; // один круг по периметру
    const SEGMENTS: usize = 28; // на сколько кусочков режем отрезок для затухания хвоста

    let p = &theme.palette;
    let rect = cover.shrink(1.5);
    let r = (radius - 1.5).clamp(0.0, rect.width().min(rect.height()) * 0.5);

    // Замкнутая ломаная по скруглённому прямоугольнику, по часовой стрелке (ось y экрана смотрит вниз).
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0_f32),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    let mut pts: Vec<Pos2> = Vec::with_capacity(4 * (STEPS + 1) + 1);
    for (center, start) in corners {
        for i in 0..=STEPS {
            let a = (start + 90.0 * i as f32 / STEPS as f32).to_radians();
            pts.push(center + vec2(a.cos(), a.sin()) * r);
        }
    }
    let ring = pts.clone();
    pts.push(pts[0]); // замыкаем, чтобы последний отрезок периметра тоже считался

    // Накопленная длина вдоль периметра: по ней находим точку на расстоянии `s` от старта.
    let mut cum: Vec<f32> = Vec::with_capacity(pts.len());
    cum.push(0.0);
    for w in pts.windows(2) {
        let last = *cum.last().unwrap_or(&0.0);
        cum.push(last + (w[1] - w[0]).length());
    }
    let total = *cum.last().unwrap_or(&0.0);
    if total <= f32::EPSILON {
        return;
    }
    let point_at = |s: f32| -> Pos2 {
        let s = s.rem_euclid(total); // за пределами круга заворачиваем в начало
        let i = cum.partition_point(|&c| c <= s).saturating_sub(1).min(pts.len() - 2);
        let seg = cum[i + 1] - cum[i];
        if seg <= f32::EPSILON { pts[i] } else { pts[i].lerp(pts[i + 1], (s - cum[i]) / seg) }
    };

    let painter = ui.painter();
    if theme.settings.reduce_motion {
        let _ = painter.add(Shape::closed_line(ring, Stroke::new(2.0, with_alpha(p.primary, 170))));
        return;
    }
    // Тусклая дорожка по всему периметру, чтобы было видно, где бежит отрезок.
    let _ = painter.add(Shape::closed_line(ring, Stroke::new(1.5, with_alpha(p.primary, 60))));
    let head = ((time / LAP_SECS).fract() as f32) * total;
    let trail = total * TRAIL;
    for k in 0..SEGMENTS {
        let a = k as f32 / SEGMENTS as f32;
        let b = (k + 1) as f32 / SEGMENTS as f32;
        let alpha = (255.0 * b).round() as u8; // хвост тает, голова самая яркая
        painter.line_segment(
            [point_at(head - trail * (1.0 - a)), point_at(head - trail * (1.0 - b))],
            Stroke::new(2.5, with_alpha(p.primary, alpha)),
        );
    }
    ui.ctx().request_repaint(); // анимация идёт сама, без движения мыши
}

pub fn fmt_time(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub fn artists_text(track: &Track) -> String {
    if track.artists.is_empty() {
        "Неизвестный исполнитель".to_string()
    } else {
        track.artists.join(", ")
    }
}

/// Строка списка треков. В Material 3 двухстрочный элемент занимает 72 px;
/// Liquid Glass сохраняет исходные 56 px для совместимости с прежней геометрией.
/// Индекс выводится отдельной колонкой: он нужен для порядка в библиотеке и списка «Мои треки».
pub fn track_row(ui: &mut Ui, theme: &Theme, index: usize, track: &Track, is_current: bool, is_playing: bool, time: f64) -> Response {
    let p = &theme.palette;
    let row_height = theme.list_item_height(2);
    let row_width = ui.available_width();
    // На узкой панели убираем номер и уменьшаем обложку, чтобы название и длительность
    // не конкурировали за оставшееся место. На обычной ширине сохраняется исходная геометрия.
    let compact = !theme.glass_active() && row_width < 520.0;
    let show_index = theme.glass_active() || row_width >= 460.0;
    let cover_size = if theme.glass_active() || compact { 40.0 } else { 48.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(row_width, row_height), Sense::click());
    // Подсветку и pressed-state в Material 3 уже рисует общий glide/ripple списка.
    // Дополнительное сжатие здесь уменьшало только фон текущего трека, тогда как
    // общий слой оставался исходного размера — получались две вложенные карточки.
    // В Liquid Glass собственного M3 ripple нет, поэтому там сохраняем squish.
    if !ui.is_rect_visible(rect) {
        return resp; // не в кадре: ничего не считаем и не рисуем
    }
    let id = ui.scope_id().with(("track_row", &track.id));
    // Продавливание: содержимое строки чуть сжимается при нажатии. В Material 3 фон текущего трека
    // и слой подсветки остаются исходного размера (`row_rect`), сжимается только содержимое.
    let row_rect = rect;
    let rect = squish(ui, id, &resp, rect, 0.015, 0.07);
    let painter = ui.painter();

    if theme.glass_active() && resp.is_pointer_button_down_on() && !is_current {
        let radius = theme.shape_radius(ShapeRole::Large) as f32;
        painter.rect_filled(rect, radius, with_alpha(p.on_surface, 20));
    }
    if is_current {
        if theme.glass_active() {
            // Тот же цвет, что у выбранного пункта меню (`selected_fill`), плюс акцентная полоска слева.
            painter.rect_filled(rect, theme.shape_radius(ShapeRole::Large), theme.selected_fill());
            painter.rect_filled(
                Rect::from_min_size(pos2(rect.left() + 4.0, rect.top() + 12.0), vec2(2.5, 32.0)),
                1.25,
                with_alpha(p.primary, 210),
            );
        } else {
            painter.rect_filled(row_rect, theme.shape_radius(ShapeRole::Large), p.secondary_container);
        }
    }

    let cover_left = if show_index { 34.0 } else { 16.0 };
    if show_index {
        painter.text(
            pos2(rect.left() + 26.0, rect.center().y),
            Align2::RIGHT_CENTER,
            format!("{}", index + 1),
            FontId::proportional(12.5),
            p.on_surface_variant,
        );
    }
    let cover = Rect::from_min_size(pos2(rect.left() + cover_left, rect.center().y - cover_size * 0.5), Vec2::splat(cover_size));
    let cover_radius = if theme.glass_active() { 12.0 } else { theme.shape_radius(ShapeRole::Medium) as f32 };
    cover_tile(ui, theme, cover, &track.id, cover_radius);
    let painter = ui.painter();
    if is_current {
        painter.rect_filled(cover, 12.0, egui::Color32::from_black_alpha(110));
        icons::paint_equalizer(painter, cover.center(), 18.0, egui::Color32::WHITE, time, is_playing);
    }

    let text_left = cover.right() + if compact { 10.0 } else { 14.0 };
    let dur_x = rect.right() - if compact { 12.0 } else { 16.0 };
    let duration_gap = if compact { 44.0 } else { 56.0 };
    let clip = Rect::from_min_max(pos2(text_left, rect.top()), pos2((dur_x - duration_gap).max(text_left), rect.bottom()));
    let clipped = painter.with_clip_rect(clip);
    let title_color = if is_current {
        if theme.glass_active() { p.primary } else { p.on_secondary_container }
    } else {
        p.on_surface
    };
    let title_offset = if theme.glass_active() { 9.0 } else { 11.0 };
    let caption_offset = if theme.glass_active() { 10.0 } else { 12.0 };
    let supporting_role = if theme.glass_active() { TextRole::BodySmall } else { TextRole::BodyMedium };
    clipped.text(pos2(text_left, rect.center().y - title_offset), Align2::LEFT_CENTER, &track.title, FontId::proportional(theme.text_size(TextRole::TitleMedium)), title_color);
    clipped.text(pos2(text_left, rect.center().y + caption_offset), Align2::LEFT_CENTER, artists_text(track), FontId::proportional(theme.text_size(supporting_role)), p.on_surface_variant);
    let time_size = if theme.glass_active() { 12.5 } else { theme.text_size(TextRole::LabelSmall) };
    painter.text(pos2(dur_x, rect.center().y), Align2::RIGHT_CENTER, fmt_time(track.duration_secs as u64), FontId::proportional(time_size), p.on_surface_variant);

    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let accessible_label = format!("{} — {}", track.title, artists_text(track));
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, is_current, accessible_label.clone()));
    resp
}

/// Маленький заголовок группы (например «Сервисы»).
pub fn section_label(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(egui::RichText::new(text).size(12.5).color(theme.palette.on_surface_variant));
    });
}

#[cfg(test)]
mod tests {
    use super::spring_step;

    #[test]
    fn spring_overshoots_and_settles() {
        let (mut value, mut velocity, target) = (0.0_f32, 0.0_f32, 100.0_f32);
        let mut overshot = false;
        let mut settled = false;

        for _ in 0..600 {
            let moving = spring_step(&mut value, &mut velocity, target, 1.0 / 60.0);
            assert!(value.is_finite() && velocity.is_finite());
            overshot |= value > target;
            if !moving {
                settled = true;
                break;
            }
        }

        assert!(overshot, "пружина должна слегка перелететь за цель");
        assert!(settled, "пружина должна остановиться у цели");
        assert_eq!(value, target);
        assert_eq!(velocity, 0.0);
    }

    #[test]
    fn retargeting_keeps_existing_velocity() {
        let (mut value, mut velocity) = (0.0_f32, 0.0_f32);
        for _ in 0..5 {
            spring_step(&mut value, &mut velocity, 100.0, 1.0 / 60.0);
        }
        let velocity_before_retarget = velocity;
        assert!(velocity_before_retarget > 0.0);

        spring_step(&mut value, &mut velocity, 20.0, 1.0 / 60.0);
        assert!(velocity > 0.0, "новая цель не должна сбрасывать инерцию");
    }
}

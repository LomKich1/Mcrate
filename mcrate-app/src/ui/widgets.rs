//! Общие виджеты. Рисуются вручную (painter), цвета берут из темы,
//! поэтому выглядят правильно и в Material 3, и в Liquid Glass.

use egui::{Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Layout, Margin, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2, layers::ShapeIdx};
use mcrate_core::Track;

use crate::icons::{self, Icon};
use crate::settings::Design;
use crate::theme::{Theme, with_alpha};


/// Общая поисковая строка для главной и библиотеки. Геометрия одинакова,
/// а экран может задавать собственную подсказку и обработку Enter.
pub fn search_bar(ui: &mut Ui, theme: &Theme, value: &mut String, hint: &str) -> Response {
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

        // Фиксированная геометрия: одинаковый вертикальный центр у лупы, текста и clear-кнопки.
        let response = ui.scope_builder(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(16.0, 3.0)))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (icon_rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                icons::paint(ui.painter(), Icon::Search, icon_rect.center(), 20.0, p.on_surface_variant);
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
                        .margin(Margin::same(0))
                        .align(Align2::LEFT_CENTER),
                );

                if show_clear {
                    ui.add_space(8.0);
                    let (clear_rect, clear_resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                    let color = if clear_resp.hovered() { p.on_surface } else { p.on_surface_variant };
                    if clear_resp.hovered() {
                        ui.painter().circle_filled(clear_rect.center(), 12.0, with_alpha(p.on_surface, 20));
                    }
                    let c = clear_rect.center();
                    let d = 4.0;
                    ui.painter().line_segment([pos2(c.x - d, c.y - d), pos2(c.x + d, c.y + d)], Stroke::new(1.6, color));
                    ui.painter().line_segment([pos2(c.x + d, c.y - d), pos2(c.x - d, c.y + d)], Stroke::new(1.6, color));
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
        ui.painter().rect_stroke(rect, 17.0, Stroke::new(if focused { 1.25 } else { 0.8 }, border), StrokeKind::Inside);
        return response;
    }

    Frame::new()
        .fill(p.surface_container_high)
        .corner_radius(CornerRadius::same(28))
        .inner_margin(Margin::symmetric(20, 0))
        .show(ui, |ui| {
            ui.set_min_size(vec2(ui.available_width(), 56.0));
            let visuals = ui.visuals_mut();
            visuals.extreme_bg_color = Color32::TRANSPARENT;
            visuals.selection.stroke = Stroke::NONE;
            for widget in [
                &mut visuals.widgets.inactive,
                &mut visuals.widgets.hovered,
                &mut visuals.widgets.active,
                &mut visuals.widgets.open,
            ] {
                widget.bg_fill = Color32::TRANSPARENT;
                widget.weak_bg_fill = Color32::TRANSPARENT;
                widget.bg_stroke = Stroke::NONE;
            }
            ui.horizontal(|ui| {
                ui.set_min_height(56.0);
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                icons::paint(ui.painter(), Icon::Search, icon_rect.center(), 24.0, p.on_surface_variant);
                ui.add_space(8.0);
                ui.add(
                    egui::TextEdit::singleline(value)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY)
                        .margin(Margin::symmetric(0, 0)),
                )
            })
            .inner
        })
        .inner
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Plain,  // только иконка
    Tonal,  // приглушённая заливка
    Filled, // главный акцент (кнопка «играть»)
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
fn squish(ui: &Ui, id: Id, resp: &Response, rect: Rect, sx: f32, sy: f32) -> Rect {
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
        let alpha = (theme.hover_glide_alpha() as f32 * g.alpha).round() as u8;
        ui.painter().set(slot, Shape::rect_filled(shown, radius, with_alpha(theme.hover_glide_color(), alpha)));
    }
    if moving {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(salt, g));
}

/// «Слой состояния» из M3: при наведении поверх кладётся 8% цвета контента, при нажатии 12%.
fn state_alpha(r: &Response) -> u8 {
    if r.is_pointer_button_down_on() {
        30
    } else if r.hovered() {
        20
    } else {
        0
    }
}

/// Круглая кнопка с иконкой.
pub fn icon_button(ui: &mut Ui, theme: &Theme, icon: Icon, diameter: f32, kind: ButtonKind, active: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    let p = &theme.palette;
    let glass = theme.glass_active();
    let visual_rect = if glass { squish(ui, resp.id, &resp, rect, 0.055, 0.055) } else { rect };
    let fg = match kind {
        ButtonKind::Filled => p.on_primary,
        ButtonKind::Tonal => if glass { p.on_surface } else { theme.on_tonal() },
        ButtonKind::Plain => if active { p.primary } else { p.on_surface },
    };
    let painter = ui.painter();
    let r = visual_rect.width() / 2.0;

    if glass {
        // В Liquid Glass кнопки не используют плотную M3 tonal-заливку:
        // только важное действие получает prominent tint, остальные — тихую оптическую поверхность.
        let pressed = resp.is_pointer_button_down_on();
        let hovered = resp.hovered();
        let (fill, stroke) = match kind {
            ButtonKind::Filled => (
                Some(with_alpha(p.primary, if p.dark { 224 } else { 218 })),
                with_alpha(p.on_primary, 92),
            ),
            ButtonKind::Tonal => (
                Some(with_alpha(p.surface_container_high, if pressed { 142 } else if hovered { 112 } else { 82 })),
                with_alpha(p.on_surface, if hovered { 72 } else { 42 }),
            ),
            ButtonKind::Plain if active => (
                Some(with_alpha(p.primary, if pressed { 82 } else if hovered { 68 } else { 54 })),
                with_alpha(p.primary, 86),
            ),
            ButtonKind::Plain if hovered || pressed => (
                Some(with_alpha(p.on_surface, if pressed { 38 } else { 24 })),
                with_alpha(p.on_surface, 42),
            ),
            ButtonKind::Plain => (None, Color32::TRANSPARENT),
        };
        if kind == ButtonKind::Filled {
            painter.circle_filled(visual_rect.center() + vec2(0.0, 1.2), r, Color32::from_black_alpha(if p.dark { 46 } else { 26 }));
        }
        if let Some(fill) = fill {
            painter.circle_filled(visual_rect.center(), r, fill);
            if kind != ButtonKind::Plain || active {
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
    } else {
        let (bg, _) = match kind {
            ButtonKind::Filled => (Some(p.primary), p.on_primary),
            ButtonKind::Tonal => (Some(theme.tonal_fill()), theme.on_tonal()),
            ButtonKind::Plain => (if active { Some(with_alpha(p.primary, if p.dark { 42 } else { 34 })) } else { None }, p.on_surface),
        };
        if let Some(bg) = bg {
            painter.circle_filled(visual_rect.center(), r, bg);
        }
        let a = state_alpha(&resp);
        if a > 0 {
            painter.circle_filled(visual_rect.center(), r, with_alpha(fg, a));
        }
    }

    icons::paint(painter, icon, visual_rect.center(), diameter * 0.6, fg);
    resp.on_hover_text(tip).on_hover_cursor(CursorIcon::PointingHand)
}

/// Кнопка-«таблетка» с текстом и необязательной иконкой.
pub fn pill_button(ui: &mut Ui, theme: &Theme, label: &str, icon: Option<Icon>, kind: ButtonKind) -> Response {
    let p = &theme.palette;
    let glass = theme.glass_active();
    let fg = match kind {
        ButtonKind::Filled => p.on_primary,
        ButtonKind::Tonal if glass => p.on_surface,
        ButtonKind::Tonal => theme.on_tonal(),
        ButtonKind::Plain => p.on_surface,
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(14.0), fg);
    let icon_w = if icon.is_some() { 20.0 + 8.0 } else { 0.0 };
    let size = vec2(galley.size().x + icon_w + 36.0, 40.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let visual_rect = if glass { squish(ui, resp.id, &resp, rect, 0.035, 0.075) } else { rect };
    let painter = ui.painter();

    if glass {
        let (fill, stroke) = match kind {
            ButtonKind::Filled => (with_alpha(p.primary, if p.dark { 224 } else { 218 }), with_alpha(p.on_primary, 90)),
            ButtonKind::Tonal => (
                with_alpha(p.surface_container_high, if resp.is_pointer_button_down_on() { 146 } else if resp.hovered() { 112 } else { 82 }),
                with_alpha(p.on_surface, if resp.hovered() { 72 } else { 42 }),
            ),
            ButtonKind::Plain => (with_alpha(p.on_surface, if resp.hovered() { 28 } else { 0 }), with_alpha(p.on_surface, 30)),
        };
        if kind == ButtonKind::Filled {
            painter.rect_filled(visual_rect.translate(vec2(0.0, 1.0)), 20.0, Color32::from_black_alpha(if p.dark { 42 } else { 24 }));
        }
        painter.rect_filled(visual_rect, 20.0, fill);
        if kind != ButtonKind::Plain || resp.hovered() {
            painter.rect_stroke(visual_rect, 20.0, Stroke::new(0.9, stroke), StrokeKind::Inside);
        }
    } else {
        let bg = match kind {
            ButtonKind::Filled => p.primary,
            ButtonKind::Tonal => theme.tonal_fill(),
            ButtonKind::Plain => Color32::TRANSPARENT,
        };
        painter.rect_filled(visual_rect, 20.0, bg);
        let a = state_alpha(&resp);
        if a > 0 {
            painter.rect_filled(visual_rect, 20.0, with_alpha(fg, a));
        }
    }

    let mut x = visual_rect.left() + 18.0;
    if let Some(icon) = icon {
        icons::paint(painter, icon, pos2(x + 10.0, visual_rect.center().y), 18.0, fg);
        x += icon_w;
    }
    painter.galley(pos2(x, visual_rect.center().y - galley.size().y / 2.0), galley, fg);
    resp.on_hover_cursor(CursorIcon::PointingHand)
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

/// Пункт навигации (ширина = вся доступная, высота 48, форма «таблетка»).
/// `compact`: только иконка по центру (узкая панель), подпись показывается во всплывающей подсказке.
pub fn nav_item(ui: &mut Ui, theme: &Theme, icon: Icon, label: &str, selected: bool, enabled: bool, trailing: Option<&str>, compact: bool) -> Response {
    let p = &theme.palette;
    let size = vec2(ui.available_width(), 48.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let id = resp.id;
    let rect = if enabled { squish(ui, id, &resp, rect, 0.04, 0.10) } else { rect };
    let painter = ui.painter();

    // Фон реагирует только на нажатие; при наведении плавно меняется лишь цвет текста и иконки.
    // Фон уже выбранного пункта рисует rail.rs отдельной пилюлей.
    if enabled && resp.is_pointer_button_down_on() {
        painter.rect_filled(rect, 24.0, with_alpha(theme.hover_glide_color(), theme.hover_glide_alpha()));
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
    if compact {
        icons::paint(painter, icon, rect.center(), 24.0, fg);
    } else {
        icons::paint(painter, icon, pos2(rect.left() + 28.0, rect.center().y), 24.0, fg);
        painter.text(pos2(rect.left() + 52.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(15.0), fg);
        if let Some(t) = trailing {
            painter.text(pos2(rect.right() - 14.0, rect.center().y), Align2::RIGHT_CENTER, t, FontId::proportional(11.0), fg);
        }
    }
    let resp = if compact { resp.on_hover_text(label) } else { resp };
    if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

/// Результат ползунка: `value` = Some(доля 0..1), пока пользователь тянет или кликает.
pub struct BarOutput {
    pub response: Response,
    pub value: Option<f32>,
}

/// Ползунок для перемотки и громкости. В M3 ручка в виде вертикальной «таблетки» (Expressive),
/// в Liquid Glass круглая.
pub fn slider_bar(ui: &mut Ui, theme: &Theme, width: f32, frac: f32, enabled: bool) -> BarOutput {
    let p = &theme.palette;
    let sense = if enabled { Sense::click_and_drag() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 24.0), sense);
    let pad = 10.0;
    let (left, right) = (rect.left() + pad, rect.right() - pad);
    let cy = rect.center().y;
    let active = resp.hovered() || resp.dragged();
    let th = if active { 8.0 } else { 6.0 };
    let frac = frac.clamp(0.0, 1.0);
    let x = left + (right - left) * frac;

    let painter = ui.painter();
    let track_bg = with_alpha(p.on_surface, if p.dark { 50 } else { 40 });
    let fg = if enabled { p.primary } else { with_alpha(p.on_surface_variant, 120) };

    match theme.design() {
        Design::Material3 => {
            // Активная часть, зазор, ручка-«таблетка», неактивная часть.
            let gap = 6.0;
            let a_end = (x - gap).max(left);
            if a_end > left {
                painter.rect_filled(Rect::from_min_max(pos2(left, cy - th / 2.0), pos2(a_end, cy + th / 2.0)), th / 2.0, fg);
            }
            let i_start = (x + gap).min(right);
            if right > i_start {
                painter.rect_filled(Rect::from_min_max(pos2(i_start, cy - th / 2.0), pos2(right, cy + th / 2.0)), th / 2.0, track_bg);
            }
            let hw = if resp.dragged() { 3.0 } else { 4.0 };
            painter.rect_filled(Rect::from_center_size(pos2(x, cy), vec2(hw, 22.0)), hw / 2.0, fg);
        }
        Design::LiquidGlass => {
            painter.rect_filled(Rect::from_min_max(pos2(left, cy - th / 2.0), pos2(right, cy + th / 2.0)), th / 2.0, track_bg);
            if x > left {
                painter.rect_filled(Rect::from_min_max(pos2(left, cy - th / 2.0), pos2(x, cy + th / 2.0)), th / 2.0, fg);
            }
            let r = if active { 9.0 } else { 7.5 };
            painter.circle_filled(pos2(x, cy + 1.0), r, egui::Color32::from_black_alpha(50));
            painter.circle_filled(pos2(x, cy), r, egui::Color32::WHITE);
        }
    }

    let value = if resp.dragged() || resp.clicked() || resp.drag_stopped() {
        resp.interact_pointer_pos().map(|pos| ((pos.x - left) / (right - left)).clamp(0.0, 1.0))
    } else {
        None
    };
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
    icons::paint(painter, Icon::Note, rect.center(), rect.height() * 0.5, with_alpha(fg, 200));
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

/// Строка списка треков. Высота фиксированная (56), чтобы работала виртуализация списка.
/// Индекс выводится отдельной колонкой: он нужен для порядка в библиотеке и списка «Мои треки».
pub fn track_row(ui: &mut Ui, theme: &Theme, index: usize, track: &Track, is_current: bool, is_playing: bool, time: f64) -> Response {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
    // Подсветку под курсором рисует список (`glide_end`: она скользит между строками); здесь только сжатие при нажатии.
    if !ui.is_rect_visible(rect) {
        return resp; // не в кадре: ничего не считаем и не рисуем
    }
    let id = ui.scope_id().with(("track_row", &track.id));
    let rect = squish(ui, id, &resp, rect, 0.015, 0.07);
    let painter = ui.painter();

    if is_current {
        if theme.glass_active() {
            painter.rect_filled(
                Rect::from_min_size(pos2(rect.left() + 4.0, rect.top() + 12.0), vec2(2.5, 32.0)),
                1.25,
                with_alpha(p.primary, 210),
            );
        } else {
            painter.rect_filled(rect, 18.0, with_alpha(p.primary, if p.dark { 34 } else { 28 }));
        }
    }

    let number = format!("{}", index + 1);
    painter.text(
        pos2(rect.left() + 26.0, rect.center().y),
        Align2::RIGHT_CENTER,
        number,
        FontId::proportional(12.5),
        p.on_surface_variant,
    );
    let cover = Rect::from_min_size(pos2(rect.left() + 34.0, rect.center().y - 20.0), Vec2::splat(40.0));
    cover_tile(ui, theme, cover, &track.id, 12.0);
    let painter = ui.painter();
    if is_current {
        painter.rect_filled(cover, 12.0, egui::Color32::from_black_alpha(110));
        icons::paint_equalizer(painter, cover.center(), 18.0, egui::Color32::WHITE, time, is_playing);
    }

    let text_left = cover.right() + 14.0;
    let dur_x = rect.right() - 16.0;
    let clip = Rect::from_min_max(pos2(text_left, rect.top()), pos2(dur_x - 56.0, rect.bottom()));
    let clipped = painter.with_clip_rect(clip);
    let title_color = if is_current { p.primary } else { p.on_surface };
    clipped.text(pos2(text_left, rect.center().y - 9.0), Align2::LEFT_CENTER, &track.title, FontId::proportional(15.0), title_color);
    clipped.text(pos2(text_left, rect.center().y + 10.0), Align2::LEFT_CENTER, artists_text(track), FontId::proportional(12.5), p.on_surface_variant);
    painter.text(pos2(dur_x, rect.center().y), Align2::RIGHT_CENTER, fmt_time(track.duration_secs as u64), FontId::proportional(13.0), p.on_surface_variant);

    resp.on_hover_cursor(CursorIcon::PointingHand)
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

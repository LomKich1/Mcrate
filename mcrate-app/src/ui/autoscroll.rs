//! Прокрутка средней кнопкой мыши (СКМ), как в браузере.
//!
//! Нажали СКМ: в этой точке появляется значок, и чем дальше курсор от него по вертикали, тем быстрее едет страница
//! (вниз от значка: вниз, вверх: вверх). Два способа, как в Chrome:
//! - зажали, повели мышь и отпустили: прокрутка заканчивается при отпускании;
//! - быстро кликнули, не двигая мышь: прокрутка остаётся, пока не нажмёте любую кнопку мыши или Esc.
//!
//! Прокрутка заканчивается и при потере окном фокуса. Пока открыто меню или окно, она не начинается.
//!
//! Как это устроено: `AutoScroll` (чистая машина состояний, ниже с тестами) живёт в `McrateApp`. Раз за кадр
//! приложение публикует в память egui `Frame` (где значок и с какой скоростью ехать), а прокручиваемые области
//! (`layout::pane_scroll` и обычные `ScrollArea` через `apply_to_area`) забирают его, если значок лежит внутри них.
//! Номер кадра в `Frame` нужен Liquid Glass: он рисует экран ещё раз за кадром, с копией памяти, и без
//! номера прокрутка применилась бы дважды (скорость вдвое).

use egui::{Context, CursorIcon, Event, Id, LayerId, Order, Painter, PointerButton, Pos2, Shape, Stroke, Ui, vec2};

use crate::app::McrateApp;
use crate::theme::{Theme, with_alpha};

/// Мёртвая зона вокруг значка, пт: пока курсор в ней, страница стоит (и «зажали и повели» от «кликнули» не отличить).
pub(crate) const DEAD_ZONE: f32 = 12.0;
/// Скорость на каждую точку расстояния за мёртвой зоной, пт/с.
const SPEED_PER_POINT: f32 = 9.0;
/// Потолок скорости, пт/с: иначе у края окна страница пролетала бы список за долю секунды.
const MAX_SPEED: f32 = 5000.0;
/// Радиус круга значка и размер его стрелок, пт.
const MARKER_RADIUS: f32 = 15.0;
const ARROW_HALF_WIDTH: f32 = 4.5;
const ARROW_HEIGHT: f32 = 4.5;
const ARROW_GAP: f32 = 6.0;

/// Скорость прокрутки вниз в пт/с (минус: вверх) для курсора на высоте `pointer_y` при значке на `origin_y`.
pub(crate) fn scroll_speed(origin_y: f32, pointer_y: f32) -> f32 {
    let offset = pointer_y - origin_y;
    let beyond = (offset.abs() - DEAD_ZONE).max(0.0);
    (beyond * SPEED_PER_POINT).min(MAX_SPEED) * offset.signum()
}

/// Что произошло в кадре (собирается из событий egui, см. `McrateApp::step_autoscroll`).
#[derive(Clone, Copy, Default)]
pub(crate) struct Input {
    /// Где сейчас курсор (последнее известное место: за окном egui не видит его, а зажатая СКМ всё равно тянет).
    pub pointer: Option<Pos2>,
    /// СКМ нажали в этой точке.
    pub press: Option<Pos2>,
    /// СКМ отпустили.
    pub released: bool,
    /// Нажали другую кнопку мыши.
    pub other_pressed: bool,
    pub escape: bool,
    pub focus_lost: bool,
    /// Можно ли начинать (не открыто меню или окно).
    pub can_start: bool,
}

#[derive(Clone, Copy)]
struct Active {
    origin: Pos2,
    /// СКМ всё ещё зажата.
    holding: bool,
    /// Пока зажата, курсор уходил за мёртвую зону: значит, это «зажал и повёл», а не клик.
    dragged: bool,
}

/// Состояние прокрутки СКМ.
#[derive(Clone, Copy, Default)]
pub(crate) struct AutoScroll {
    active: Option<Active>,
}

impl AutoScroll {
    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Где значок (`None`, если прокрутки нет).
    pub(crate) fn origin(&self) -> Option<Pos2> {
        self.active.map(|a| a.origin)
    }

    /// Учитывает события кадра.
    pub(crate) fn update(&mut self, input: &Input) {
        if input.focus_lost || input.escape {
            self.active = None;
            return;
        }
        // В режиме «кликнули»: любое нажатие кнопки мыши заканчивает прокрутку (и не начинает новую).
        if let Some(a) = &self.active
            && !a.holding
            && (input.press.is_some() || input.other_pressed)
        {
            self.active = None;
            return;
        }
        if self.active.is_none()
            && input.can_start
            && let Some(origin) = input.press
        {
            self.active = Some(Active { origin, holding: true, dragged: false });
        }
        if let Some(a) = &mut self.active
            && a.holding
        {
            if input.pointer.is_some_and(|p| p.distance(a.origin) > DEAD_ZONE) {
                a.dragged = true;
            }
            if input.released {
                if a.dragged {
                    self.active = None; // «зажал, повёл, отпустил»: конец
                } else {
                    a.holding = false; // «кликнул»: едем дальше, пока не нажмут что-нибудь
                }
            }
        }
    }

    /// Скорость прокрутки вниз в пт/с для текущего положения курсора (0, если прокрутки нет).
    pub(crate) fn speed(&self, pointer: Option<Pos2>) -> f32 {
        match (self.active, pointer) {
            (Some(a), Some(p)) => scroll_speed(a.origin.y, p.y),
            _ => 0.0,
        }
    }
}

/// Что прокручиваемым областям нужно знать о текущем кадре.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    /// Где значок: область прокручивается, только если он внутри неё.
    pub origin: Pos2,
    /// Скорость вниз, пт/с.
    pub speed: f32,
    /// Номер прохода egui, в котором посчитан кадр (см. заголовок модуля: зачем).
    pub nr: u64,
}

fn frame_id() -> Id {
    Id::unique("autoscroll_frame")
}

/// Положить кадр в память egui (или убрать, если прокрутки нет).
fn publish(ctx: &Context, frame: Option<Frame>) {
    ctx.data_mut(|d| match frame {
        Some(frame) => {
            d.insert_temp(frame_id(), frame);
        }
        None => {
            d.remove::<Frame>(frame_id());
        }
    });
}

/// Текущий кадр прокрутки СКМ, если она идёт.
pub(crate) fn current(ctx: &Context) -> Option<Frame> {
    ctx.data(|d| d.get_temp::<Frame>(frame_id()))
}

/// Для обычной `ScrollArea` (не `pane_scroll`): вызвать первой строкой в её замыкании. Двигает область, если
/// значок лежит внутри её видимой части. Зовётся и в закадровом проходе Liquid Glass, поэтому смотрит номер кадра.
pub(crate) fn apply_to_area(ui: &mut Ui) {
    let Some(frame) = current(ui.ctx()) else { return };
    if !ui.clip_rect().contains(frame.origin) {
        return;
    }
    let key = ui.scope_id().with("autoscroll_applied");
    let done = ui.ctx().data(|d| d.get_temp::<u64>(key)) == Some(frame.nr);
    if done {
        return;
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, frame.nr));
    let dt = ui.input(|i| i.unstable_dt).clamp(0.0, 0.05);
    ui.scroll_with_delta(vec2(0.0, -frame.speed * dt));
}

impl McrateApp {
    /// Разбирает ввод кадра, двигает машину состояний и публикует кадр для областей прокрутки.
    /// Возвращает true, если Esc в этом кадре ушёл на остановку прокрутки (тогда он не должен ещё и вести «назад»).
    pub(crate) fn step_autoscroll(&mut self, ctx: &Context, can_start: bool) -> bool {
        let was_active = self.autoscroll.is_active();
        let mut input = ctx.input(|i| {
            let mut input = Input { pointer: i.pointer.latest_pos(), can_start, ..Input::default() };
            for event in &i.events {
                match *event {
                    Event::PointerButton { pos, button: PointerButton::Middle, pressed: true, .. } => input.press = Some(pos),
                    Event::PointerButton { button: PointerButton::Middle, pressed: false, .. } => input.released = true,
                    Event::PointerButton { pressed: true, .. } => input.other_pressed = true,
                    Event::Key { key: egui::Key::Escape, pressed: true, repeat: false, .. } => input.escape = true,
                    _ => {}
                }
            }
            input.focus_lost = i.viewport().focused == Some(false);
            input
        });
        // Начало прокрутки: курсор в точке нажатия.
        if input.pointer.is_none() {
            input.pointer = input.press;
        }
        self.autoscroll.update(&input);

        let frame = self.autoscroll.origin().map(|origin| Frame {
            origin,
            speed: self.autoscroll.speed(input.pointer),
            nr: ctx.cumulative_pass_nr(),
        });
        publish(ctx, frame);
        if frame.is_some() {
            ctx.set_cursor_icon(CursorIcon::AllScroll);
            ctx.request_repaint(); // едем каждый кадр, пока прокрутка идёт
        }
        was_active && input.escape
    }

    /// Рисует значок прокрутки поверх интерфейса (в конце кадра).
    pub(crate) fn paint_autoscroll_marker(&self, ctx: &Context, theme: &Theme) {
        let Some(origin) = self.autoscroll.origin() else { return };
        let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("autoscroll_marker")));
        paint_marker(&painter, theme, origin);
    }
}

/// Круг со стрелками вверх и вниз и точкой в центре, в цветах темы.
fn paint_marker(painter: &Painter, theme: &Theme, center: Pos2) {
    let p = &theme.palette;
    painter.circle(center, MARKER_RADIUS, with_alpha(p.surface_container, 235), Stroke::new(1.5, p.primary));
    painter.circle_filled(center, 2.5, p.primary);
    for sign in [-1.0_f32, 1.0] {
        let tip = center + vec2(0.0, sign * (ARROW_GAP + ARROW_HEIGHT));
        let base_y = center.y + sign * ARROW_GAP;
        let points = vec![tip, Pos2::new(center.x - ARROW_HALF_WIDTH, base_y), Pos2::new(center.x + ARROW_HALF_WIDTH, base_y)];
        painter.add(Shape::convex_polygon(points, p.primary, Stroke::NONE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    const ORIGIN: Pos2 = pos2(300.0, 300.0);
    const FAR: Pos2 = pos2(300.0, 380.0);

    fn press() -> Input {
        Input { press: Some(ORIGIN), pointer: Some(ORIGIN), can_start: true, ..Input::default() }
    }

    fn release_at(pointer: Pos2) -> Input {
        Input { released: true, pointer: Some(pointer), can_start: true, ..Input::default() }
    }

    fn state_after(inputs: &[Input]) -> AutoScroll {
        let mut s = AutoScroll::default();
        for i in inputs {
            s.update(i);
        }
        s
    }

    #[test]
    fn speed_is_zero_in_dead_zone_and_grows_with_distance() {
        assert_eq!(scroll_speed(100.0, 100.0), 0.0);
        assert_eq!(scroll_speed(100.0, 100.0 + DEAD_ZONE), 0.0);
        assert!(scroll_speed(100.0, 160.0) > 0.0, "ниже значка: вниз");
        assert!(scroll_speed(100.0, 40.0) < 0.0, "выше значка: вверх");
        assert!(scroll_speed(100.0, 200.0) > scroll_speed(100.0, 150.0));
        assert_eq!(scroll_speed(100.0, 160.0), -scroll_speed(100.0, 40.0), "вверх и вниз симметрично");
    }

    #[test]
    fn speed_has_a_ceiling() {
        assert_eq!(scroll_speed(0.0, 1.0e6), MAX_SPEED);
        assert_eq!(scroll_speed(0.0, -1.0e6), -MAX_SPEED);
    }

    #[test]
    fn middle_press_starts_scrolling_at_the_cursor() {
        let s = state_after(&[press()]);
        assert_eq!(s.origin(), Some(ORIGIN));
        assert_eq!(s.speed(Some(FAR)), scroll_speed(ORIGIN.y, FAR.y));
    }

    #[test]
    fn hold_drag_release_ends_scrolling() {
        let drag = Input { pointer: Some(FAR), can_start: true, ..Input::default() };
        let s = state_after(&[press(), drag, release_at(FAR)]);
        assert!(!s.is_active());
    }

    #[test]
    fn quick_click_keeps_scrolling_until_next_press() {
        let s = state_after(&[press(), release_at(ORIGIN)]);
        assert!(s.is_active(), "кликнули, не двигая: остаётся");
        let stop = Input { other_pressed: true, ..Input::default() };
        assert!(!state_after(&[press(), release_at(ORIGIN), stop]).is_active());
        let stop_by_middle = Input { press: Some(FAR), ..Input::default() };
        assert!(!state_after(&[press(), release_at(ORIGIN), stop_by_middle]).is_active(), "и повторный СКМ не начинает заново");
    }

    #[test]
    fn small_wobble_while_holding_still_counts_as_a_click() {
        let wobble = Input { pointer: Some(pos2(ORIGIN.x + 4.0, ORIGIN.y + 5.0)), ..Input::default() };
        assert!(state_after(&[press(), wobble, release_at(ORIGIN)]).is_active());
    }

    #[test]
    fn escape_and_focus_loss_stop_it() {
        let escape = Input { escape: true, ..Input::default() };
        let blur = Input { focus_lost: true, ..Input::default() };
        assert!(!state_after(&[press(), release_at(ORIGIN), escape]).is_active());
        assert!(!state_after(&[press(), blur]).is_active());
    }

    #[test]
    fn does_not_start_while_overlay_is_open() {
        let blocked = Input { can_start: false, ..press() };
        assert!(!state_after(&[blocked]).is_active());
    }

    #[test]
    fn no_pointer_means_no_speed() {
        let s = state_after(&[press()]);
        assert_eq!(s.speed(None), 0.0);
        assert_eq!(AutoScroll::default().speed(Some(FAR)), 0.0);
    }

    #[test]
    fn marker_geometry_fits_inside_circle() {
        assert!(ARROW_GAP + ARROW_HEIGHT < MARKER_RADIUS, "стрелки не вылезают за круг");
    }
}

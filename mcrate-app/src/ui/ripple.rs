//! Material 3: слой состояния (наведение / нажатие / фокус) в виде «кружка» от курсора.
//!
//! Раньше подсветка в M3 либо появлялась сразу на весь элемент (`state_layer`), либо скользила
//! между элементами (`glide_end`). Теперь она растёт кругом из точки, куда навели курсор, остаётся
//! кружком вокруг курсора и едет за ним. При нажатии круг разрастается до краёв элемента (как ripple
//! (волна-«рябь») в Material 3). Круг обрезается по форме элемента (скруглённый прямоугольник или круг).
//!
//! Liquid Glass этот модуль не трогает: там своя стеклянная подсветка (`glide_end_impl`, ветка `glass_active`).
//!
//! Геометрия: egui не умеет обрезать фигуру по скруглённому прямоугольнику (только по обычному `clip_rect`),
//! поэтому пересечение «круг ∩ скруглённый прямоугольник» считаем сами: оба фигуры выпуклые, берём
//! алгоритм Сазерленда-Ходжмана (Sutherland–Hodgman: многоугольник последовательно обрезается по каждой
//! стороне другого выпуклого многоугольника). Результат тоже выпуклый, его рисует `Shape::convex_polygon`
//! (края сглажены самим egui).

use egui::{Color32, Id, Painter, Pos2, Rect, Response, Shape, Ui, Vec2, layers::ShapeIdx, pos2};
use std::f32::consts::{FRAC_PI_2, TAU};

use crate::theme::{Theme, with_alpha};

/// Число сторон у многоугольника, которым приближается круг.
const DISC_SEGMENTS: usize = 56;
/// Число сторон на один скруглённый угол прямоугольника.
const CORNER_SEGMENTS: usize = 8;

/// Тот же цвет и плотность, что у hover-подсветки строк на экране настроек: on_surface / 8%.
const STATE_LAYER_HOVER_ALPHA: u8 = 20;
/// Material 3 pressed-state layer: 12% того же цвета.
const STATE_LAYER_PRESS_ALPHA: u8 = 30;

// ---------------------------------------------------------------------------------------------
// Геометрия
// ---------------------------------------------------------------------------------------------

/// Многоугольник скруглённого прямоугольника (обход по часовой стрелке в координатах экрана).
fn rounded_rect_polygon(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5).max(0.0);
    if r < 0.5 {
        return vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    }
    // (центр дуги, начальный угол): правый верх, правый низ, левый низ, левый верх.
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -FRAC_PI_2),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), FRAC_PI_2),
        (pos2(rect.left() + r, rect.top() + r), 2.0 * FRAC_PI_2),
    ];
    let mut pts = Vec::with_capacity(4 * (CORNER_SEGMENTS + 1));
    for (c, a0) in corners {
        for i in 0..=CORNER_SEGMENTS {
            let a = a0 + FRAC_PI_2 * (i as f32 / CORNER_SEGMENTS as f32);
            pts.push(pos2(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    pts
}

/// Многоугольник-круг.
fn disc_polygon(center: Pos2, radius: f32) -> Vec<Pos2> {
    (0..DISC_SEGMENTS)
        .map(|i| {
            let a = TAU * (i as f32 / DISC_SEGMENTS as f32);
            pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

/// Удвоенная ориентированная площадь (знак показывает направление обхода).
fn signed_area2(poly: &[Pos2]) -> f32 {
    let mut s = 0.0;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        s += a.x * b.y - b.x * a.y;
    }
    s
}

/// Обрезка выпуклого многоугольника `subject` выпуклым `clip` (Сазерленд-Ходжман).
fn clip_convex(subject: Vec<Pos2>, clip: &[Pos2]) -> Vec<Pos2> {
    let sign = if signed_area2(clip) >= 0.0 { 1.0 } else { -1.0 };
    let mut out = subject;
    for i in 0..clip.len() {
        if out.is_empty() {
            break;
        }
        let a = clip[i];
        let b = clip[(i + 1) % clip.len()];
        // Положительное значение: точка внутри (с той же стороны от ребра a→b, что и сам `clip`).
        let side = |p: Pos2| sign * ((b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x));
        let input = std::mem::take(&mut out);
        for j in 0..input.len() {
            let cur = input[j];
            let prev = input[(j + input.len() - 1) % input.len()];
            let (sc, sp) = (side(cur), side(prev));
            // Точка пересечения отрезка prev→cur с прямой ребра.
            let cross = || {
                let t = sp / (sp - sc);
                pos2(prev.x + (cur.x - prev.x) * t, prev.y + (cur.y - prev.y) * t)
            };
            if sc >= 0.0 {
                if sp < 0.0 {
                    out.push(cross());
                }
                out.push(cur);
            } else if sp >= 0.0 {
                out.push(cross());
            }
        }
    }
    out
}

/// Расстояние от точки до самого дальнего угла прямоугольника: круг такого радиуса закрывает весь элемент.
fn farthest_corner(rect: Rect, c: Pos2) -> f32 {
    let dx = (c.x - rect.left()).abs().max((c.x - rect.right()).abs());
    let dy = (c.y - rect.top()).abs().max((c.y - rect.bottom()).abs());
    (dx * dx + dy * dy).sqrt()
}

/// Единый радиус кружка при наведении для строк, пунктов и карточек.
/// Не зависит от размеров конкретного элемента: переход между соседними элементами выглядит цельно.
const HOVER_RADIUS: f32 = 44.0;

fn hover_radius(_rect: Rect) -> f32 {
    HOVER_RADIUS
}

/// Радиальная заливка: в точке курсора цвет максимально плотный, к границе круга
/// он непрерывно рассеивается до полной прозрачности. Градиент задаётся цветами вершин
/// треугольной сетки, а не стопкой полупрозрачных кругов.
fn radial_disc_in_rect(rect: Rect, corner: f32, center: Pos2, radius: f32, color: Color32, max_alpha: u8) -> Option<Shape> {
    if radius < 0.5 || rect.width() < 1.0 || rect.height() < 1.0 {
        return None;
    }
    let poly = clip_convex(disc_polygon(center, radius), &rounded_rect_polygon(rect, corner));
    if poly.len() < 3 {
        return None;
    }

    // Центр веера — центроид уже обрезанного выпуклого многоугольника.
    let inv_n = 1.0 / poly.len() as f32;
    let centroid = poly.iter().fold(pos2(0.0, 0.0), |sum, p| pos2(sum.x + p.x * inv_n, sum.y + p.y * inv_n));

    // Радиальный закон затухания:
    // alpha(d) = alpha_max * (1 - clamp(d / radius, 0, 1))^2.
    // Квадрат даёт яркое ядро и плавное, нелинейное рассеивание к краю.
    let alpha_at = |p: Pos2| -> u8 {
        let t = (1.0 - (p - center).length() / radius).clamp(0.0, 1.0);
        (max_alpha as f32 * t * t).round() as u8
    };

    let mut mesh = egui::Mesh::default();
    mesh.vertices.push(egui::epaint::Vertex {
        pos: centroid,
        uv: egui::epaint::WHITE_UV,
        color: with_alpha(color, alpha_at(centroid)),
    });
    for &p in &poly {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: p,
            uv: egui::epaint::WHITE_UV,
            color: with_alpha(color, alpha_at(p)),
        });
    }
    for i in 0..poly.len() {
        let next = (i + 1) % poly.len();
        mesh.indices.extend_from_slice(&[0, (i + 1) as u32, (next + 1) as u32]);
    }
    Some(Shape::mesh(mesh))
}

// ---------------------------------------------------------------------------------------------
// Состояние одного слоя
// ---------------------------------------------------------------------------------------------

/// Один кружок: где он, насколько вырос, насколько виден и насколько «нажат».
#[derive(Clone, Copy)]
struct Layer {
    key: u64,
    rect: Rect,
    center: Pos2,
    /// 0..1: рост кружка от точки входа до его радиуса.
    grow: f32,
    /// 0..1: прозрачность слоя (появление и исчезновение).
    fade: f32,
    /// 0..1: нажатие (кружок разрастается до краёв, слой становится плотнее).
    press: f32,
    live: bool,
}

impl Layer {
    fn new(key: u64, rect: Rect, center: Pos2) -> Self {
        Self { key, rect, center, grow: 0.0, fade: 0.0, press: 0.0, live: true }
    }

    /// Один шаг анимации. Возвращает true, пока что-то ещё движется (нужна перерисовка).
    /// `pointer`: позиция курсора (None: тянем кружок к центру элемента, как при фокусе с клавиатуры).
    fn step(&mut self, dt: f32, active: bool, pressed: bool, pointer: Option<Pos2>, rect: Rect, instant: bool) -> bool {
        // Экспоненциальное приближение к цели: быстрый старт и мягкое замедление, не зависит от частоты кадров.
        let k = |rate: f32| if instant { 1.0 } else { 1.0 - (-dt * rate).exp() };
        let mut moving = false;

        if active {
            self.rect = rect;
            let target = pointer.unwrap_or_else(|| rect.center());
            let follow = k(if pointer.is_some() { 26.0 } else { 12.0 });
            self.center += (target - self.center) * follow;
            if (target - self.center).length() > 0.3 {
                moving = true;
            }
            self.grow += (1.0 - self.grow) * k(9.0);
            self.fade += (1.0 - self.fade) * k(24.0);
            if self.grow > 0.995 {
                self.grow = 1.0;
            } else {
                moving = true;
            }
            if self.fade > 0.995 {
                self.fade = 1.0;
            } else {
                moving = true;
            }
        } else {
            // Курсор ушёл: кружок остаётся на месте и гаснет.
            self.fade -= self.fade * k(13.0);
            if self.fade < 0.01 || instant {
                self.fade = 0.0;
                self.grow = 0.0;
                self.press = 0.0;
                self.live = false;
                return false;
            }
            moving = true;
        }

        let press_target = if pressed { 1.0 } else { 0.0 };
        self.press += (press_target - self.press) * k(if pressed { 16.0 } else { 8.0 });
        if (self.press - press_target).abs() < 0.01 {
            self.press = press_target;
        } else {
            moving = true;
        }
        moving
    }

    /// Фигура слоя. `hover_alpha`/`press_alpha`: непрозрачность 0..255 при наведении и при нажатии.
    fn shape(&self, corner: f32, color: Color32, hover_alpha: u8, press_alpha: u8) -> Option<Shape> {
        if !self.live || self.fade <= 0.0 {
            return None;
        }
        let a = (hover_alpha as f32 + (press_alpha as f32 - hover_alpha as f32) * self.press) * self.fade;
        if a < 0.5 {
            return None;
        }
        let full = farthest_corner(self.rect, self.center) + 1.0;
        let target = hover_radius(self.rect) + (full - hover_radius(self.rect)).max(0.0) * self.press;
        let r = target * self.grow;
        // Радиальный градиент: яркое ядро в точке курсора рассеивается до нуля
        // на границе радиуса. Это настоящая vertex-color сетка, без наложения слоёв.
        let circle = radial_disc_in_rect(self.rect, corner, self.center, r, color, a.round() as u8);
        // Весь элемент получает тот же 8% state-layer, что и строка настроек.
        // Радиальный градиент поверх него усиливает центр, рассеиваясь к нулю по краю.
        let base_alpha = a.round() as u8;
        let mut shapes = Vec::with_capacity(2);
        if base_alpha > 0 {
            shapes.push(Shape::rect_filled(self.rect, corner, with_alpha(color, base_alpha)));
        }
        if let Some(circle) = circle {
            shapes.push(circle);
        }
        match shapes.len() {
            0 => None,
            1 => shapes.pop(),
            _ => Some(Shape::Vec(shapes)),
        }
    }
}

fn pointer_pos(ui: &Ui) -> Option<Pos2> {
    ui.input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()))
}

// ---------------------------------------------------------------------------------------------
// Публичный API
// ---------------------------------------------------------------------------------------------

/// Слой состояния M3 для одного виджета: растёт кружком от курсора при наведении, разрастается до краёв
/// при нажатии, при фокусе с клавиатуры показывается на весь элемент. Рисуется сразу `painter`-ом,
/// поэтому вызывать после заливки элемента и до его содержимого.
///
/// * `rect`: фактический (уже сжатый `squish`) прямоугольник элемента, `corner`: радиус его углов
///   (для круглой кнопки половина стороны).
/// * `hover_alpha`/`press_alpha`: 0..255 (M3: 8% ≈ 20 и 12% ≈ 31).
pub fn state_layer(ui: &Ui, theme: &Theme, painter: &Painter, resp: &Response, rect: Rect, corner: f32, _color: Color32, _hover_alpha: u8, _press_alpha: u8) {
    // Единый цвет для обычного выделения — как в settings_group(): on_surface с alpha=20 (8%).
    let color = theme.palette.on_surface;
    let hover_alpha = STATE_LAYER_HOVER_ALPHA;
    let press_alpha = STATE_LAYER_PRESS_ALPHA;
    let id: Id = resp.id.with("m3_ripple");
    let pressed = resp.is_pointer_button_down_on();
    let hovered = resp.hovered() || pressed;
    let focus_only = resp.has_focus() && !hovered;
    let active = hovered || focus_only;
    let mut layer: Option<Layer> = ui.ctx().data_mut(|d| d.get_temp(id));
    let pointer = if hovered { pointer_pos(ui) } else { None };

    if active && !layer.is_some_and(|l| l.live) {
        // Первый кадр наведения: кружок рождается в точке входа курсора (при фокусе с клавиатуры в центре).
        layer = Some(Layer::new(id.value(), rect, pointer.unwrap_or_else(|| rect.center())));
    }
    let Some(mut l) = layer else { return };
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.064);
    let reduce = theme.settings.reduce_motion;
    // Фокус с клавиатуры: плотность как при нажатии (M3: focus = 12%) и сразу весь элемент.
    let dense = pressed || focus_only;
    let moving = l.step(dt, active, dense, pointer, rect, reduce);
    if l.live {
        if let Some(shape) = l.shape(corner, color, hover_alpha, press_alpha) {
            painter.add(shape);
        }
    }
    if moving {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, l));
}

/// Параметры той же физической пружины, которая используется для glide-подложки в Mcrate.
/// F = -k * (x - target) - c * velocity.
const GLIDE_SPRING_STIFFNESS: f32 = 500.0;
const GLIDE_SPRING_DAMPING: f32 = 34.0;
const GLIDE_SPRING_EPSILON: f32 = 0.05;

/// Один шаг пружины. Скорость не сбрасывается при смене цели: новый импульс
/// разворачивает текущее движение, поэтому выделение не телепортируется.
fn glide_spring_step(value: &mut f32, velocity: &mut f32, target: f32, dt: f32) -> bool {
    if dt > 0.0 {
        let force = -GLIDE_SPRING_STIFFNESS * (*value - target) - GLIDE_SPRING_DAMPING * *velocity;
        *velocity += force * dt;
        *value += *velocity * dt;
    }

    if (*value - target).abs() < GLIDE_SPRING_EPSILON && (*velocity).abs() < GLIDE_SPRING_EPSILON {
        *value = target;
        *velocity = 0.0;
        false
    } else {
        true
    }
}

/// Сколько гаснущих кружков держим одновременно (при быстром движении курсора по строкам).
const MAX_FADING: usize = 4;

/// Состояние общей подсветки: кружок и прямоугольник подложки имеют независимую геометрию.
/// Прямоугольник двигается пружиной по четырём координатам, как glide в Mcrate.
#[derive(Clone)]
struct GlideRipple {
    cur: Option<Layer>,
    fading: Vec<Layer>,
    rect: Rect,
    rect_target: Rect,
    velocity_min: Vec2,
    velocity_max: Vec2,
    rect_initialized: bool,
    /// Был ли курсор над любым реагирующим элементом в прошлом кадре.
    was_active: bool,
}

// egui::Rect намеренно не реализует Default. Для неинициализированной геометрии
// используем нулевой прямоугольник; rect_initialized не даст отрисовать его как цель.
impl Default for GlideRipple {
    fn default() -> Self {
        Self {
            cur: None,
            fading: Vec::new(),
            rect: Rect::from_min_max(Pos2::ZERO, Pos2::ZERO),
            rect_target: Rect::from_min_max(Pos2::ZERO, Pos2::ZERO),
            velocity_min: Vec2::ZERO,
            velocity_max: Vec2::ZERO,
            rect_initialized: false,
            was_active: false,
        }
    }
}

/// Подсветка одна на список (как `glide_end`): общий прямоугольник пружинно скользит между
/// строками и карточками, а радиальный кружок остаётся привязан к курсору.
/// После выхода из всей группы прежний кружок гаснет; фигуры кладутся в слот `glide_begin`.
pub fn glide_ripple(
    ui: &Ui,
    theme: &Theme,
    slot: ShapeIdx,
    salt: Id,
    target: Option<(u64, Rect)>,
    corner: f32,
    _color: Color32,
    _hover_alpha: u8,
    _press_alpha: u8,
) {
    // Цвет и базовая плотность совпадают с настройками: p.on_surface, alpha=20 (8%).
    // При нажатии используется тот же цвет с M3-плотностью 12% (alpha=30).
    let color = theme.palette.on_surface;
    let hover_alpha = STATE_LAYER_HOVER_ALPHA;
    let press_alpha = STATE_LAYER_PRESS_ALPHA;
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.064);
    let reduce = theme.settings.reduce_motion;
    let pointer = pointer_pos(ui);
    let pressed = target.is_some() && ui.input(|i| i.pointer.primary_down());
    let mut st: GlideRipple = ui.ctx().data_mut(|d| d.get_temp(salt)).unwrap_or_default();
    let mut moving = false;

    if let Some((key, rect)) = target {
        if !st.was_active || !st.cur.is_some_and(|l| l.live) {
            // Новая сессия после выхода курсора: подложка появляется сразу под элементом.
            // Переход между соседними элементами ниже меняет только цель пружины.
            if let Some(old) = st.cur.take() {
                if old.live {
                    st.fading.push(old);
                    if st.fading.len() > MAX_FADING {
                        st.fading.remove(0);
                    }
                }
            }
            st.cur = Some(Layer::new(key, rect, pointer.unwrap_or_else(|| rect.center())));
            st.rect = rect;
            st.rect_target = rect;
            st.velocity_min = Vec2::ZERO;
            st.velocity_max = Vec2::ZERO;
            st.rect_initialized = true;
        } else {
            // Сохраняем текущее положение и скорость, меняем только конечный прямоугольник.
            st.rect_target = rect;
            if let Some(current) = st.cur.as_mut() {
                current.key = key;
            }
        }
    }

    let active = target.is_some();
    st.was_active = active;

    // Пружинно анимируем обе границы прямоугольника независимо — подложка одновременно
    // скользит и меняет размер. При reduce_motion геометрия сразу ставится в конечную точку.
    if st.rect_initialized && active {
        if reduce {
            st.rect = st.rect_target;
            st.velocity_min = Vec2::ZERO;
            st.velocity_max = Vec2::ZERO;
        } else {
            moving |= glide_spring_step(&mut st.rect.min.x, &mut st.velocity_min.x, st.rect_target.min.x, dt);
            moving |= glide_spring_step(&mut st.rect.min.y, &mut st.velocity_min.y, st.rect_target.min.y, dt);
            moving |= glide_spring_step(&mut st.rect.max.x, &mut st.velocity_max.x, st.rect_target.max.x, dt);
            moving |= glide_spring_step(&mut st.rect.max.y, &mut st.velocity_max.y, st.rect_target.max.y, dt);
        }
    }

    if let Some(l) = st.cur.as_mut() {
        let rect = if st.rect_initialized {
            st.rect
        } else {
            target.map(|(_, r)| r).unwrap_or(l.rect)
        };
        moving |= l.step(dt, active, pressed, pointer, rect, reduce);
        if !l.live {
            st.cur = None;
        }
    }
    for l in st.fading.iter_mut() {
        let r = l.rect;
        moving |= l.step(dt, false, false, None, r, reduce);
    }
    st.fading.retain(|l| l.live);

    let mut shapes = Vec::with_capacity(MAX_FADING + 1);
    // Гаснущие кружки рисуем первыми, текущий поверх.
    for l in st.fading.iter().chain(st.cur.iter()) {
        if let Some(s) = l.shape(corner, color, hover_alpha, press_alpha) {
            shapes.push(s);
        }
    }
    if !shapes.is_empty() {
        ui.painter().set(slot, Shape::Vec(shapes));
    }
    if moving {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(salt, st));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(poly: &[Pos2]) -> f32 {
        signed_area2(poly).abs() * 0.5
    }

    #[test]
    fn clip_inside_keeps_disc() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(400.0, 200.0));
        let disc = disc_polygon(pos2(200.0, 100.0), 40.0);
        let a0 = area(&disc);
        let clipped = clip_convex(disc, &rounded_rect_polygon(rect, 16.0));
        assert!((area(&clipped) - a0).abs() < 1.0);
    }

    #[test]
    fn clip_outside_is_empty() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(100.0, 50.0));
        let clipped = clip_convex(disc_polygon(pos2(500.0, 500.0), 30.0), &rounded_rect_polygon(rect, 10.0));
        assert!(clipped.len() < 3);
    }

    #[test]
    fn clip_half_disc_on_edge() {
        // Центр круга на левом краю широкого прямоугольника: остаётся ровно половина круга.
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(400.0, 400.0));
        let disc = disc_polygon(pos2(0.0, 200.0), 50.0);
        let half = area(&disc) * 0.5;
        let clipped = clip_convex(disc, &rounded_rect_polygon(rect, 0.0));
        assert!((area(&clipped) - half).abs() < 2.0);
    }

    #[test]
    fn corner_is_rounded() {
        // Круг в углу скруглённого прямоугольника обязан быть меньше, чем у прямого угла.
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(300.0, 300.0));
        let sharp = clip_convex(disc_polygon(pos2(2.0, 2.0), 60.0), &rounded_rect_polygon(rect, 0.0));
        let round = clip_convex(disc_polygon(pos2(2.0, 2.0), 60.0), &rounded_rect_polygon(rect, 24.0));
        assert!(area(&round) < area(&sharp) - 10.0);
    }

}

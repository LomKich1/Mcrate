//! Нажатие: ripple как у Material 3 на Android 12+ (порт шейдера `RippleShader` из AOSP на CPU).
//!
//! Подсветки при наведении курсора в приложении нет нигде: элемент реагирует только на нажатие. Эффект из
//! двух слоёв, как в Android: (1) мягкая «волна», круг растёт от точки нажатия, центр по пути уезжает к
//! центру элемента, слабая заливка; (2) «крапинки»: сетка мелких зёрен, каждое мерцает по шуму, видны только
//! в тонком мягком кольце на краю волны и в пятнах «турбулентности». Время жизни фиксированное: даже
//! быстрый тап проигрывает ripple целиком (как в Android), кнопка при этом может быть уже отпущена.
//! Случайность детерминирована положением зерна на сетке, поэтому крапинки не «пляшут» от кадра к кадру,
//! а мерцают только по времени. Всё обрезано по скруглённому прямоугольнику элемента.

use egui::{Color32, Id, Mesh, Painter, Pos2, Rect, Response, Shape, Ui, layers::ShapeIdx, pos2};
use std::f32::consts::{PI};

use crate::theme::{Theme, with_alpha};

/// Насколько видны крапинки на самом кольце (0..255): в Android цвет зёрен почти непрозрачный.
const SPARKLE_ALPHA: u8 = 190;
/// Заливка «волны» (0..255): в M3 слой нажатия 10% от цвета контента.
const WAVE_ALPHA: u8 = 10;
/// Ровная заливка при «уменьшить движение» (0..255), пока кнопка зажата: не зависит от `WAVE_ALPHA`.
const REDUCED_MOTION_ALPHA: u8 = 26;
/// Сколько секунд идёт весь ripple от нажатия до полного исчезновения.
const RIPPLE_SECS: f32 = 0.75;
/// Размер одной ячейки зёрен в пикселях (в Android около 2 dp).
const SPARKLE_CELL_PX: f32 = 1.0;
/// Сколько ячеек зёрен самое большее на один элемент: у больших карточек ячейка укрупняется, чтобы кадр не тормозил.
const SPARKLE_MAX_CELLS: f32 = 40_000.0;
/// Скорость мерцания зёрен (оборотов фазы шума в секунду): в Android фаза шума бежит быстрее прогресса.
const TWINKLE_RATE: f32 = 2.4;
/// Размытость кольца крапинок (доля радиуса, как `0.45` в шейдере Android).
const RING_BLUR: f32 = 0.45;
/// Размытость края волны (как `0.2` в шейдере Android).
const WAVE_BLUR: f32 = 0.2;
/// Толщина кольца крапинок: доля радиуса волны (как `0.05` в шейдере Android).
const RING_THICKNESS: f32 = 0.05;
/// Размер пятна «турбулентности» в пикселях: на таких пятнах крапинки то видны, то нет.
const TURBULENCE_SCALE_PX: f32 = 38.0;
/// Во сколько раз кольцо крапинок уходит дальше половины диагонали: чтобы оно дошло до самых краёв и углов.
const RING_REACH: f32 = 1.3;
/// Размер ячейки заливки волны в пикселях: крупнее зёрен, так как заливка гладкая.
const WAVE_CELL_PX: f32 = 0.5;
/// Сколько ячеек волны самое большее на элемент: у больших карточек ячейка укрупняется.
const WAVE_MAX_CELLS: f32 = 20_000.0;

// ---------------------------------------------------------------------------------------------
// Математика шейдера
// ---------------------------------------------------------------------------------------------

/// Как `smoothstep` в GLSL.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Дробная часть как `fract` в GLSL (для отрицательных тоже в 0..1).
fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// Часть пути анимации между `start` и `end`, нормированная в 0..1 (`subProgress` из шейдера).
fn sub_progress(start: f32, end: f32, progress: f32) -> f32 {
    (progress.clamp(start, end) - start) / (end - start)
}

/// Круг с мягким краем: 1 внутри, 0 снаружи, переход шириной `blur` радиуса (`softCircle` из шейдера).
fn soft_circle(dist: f32, radius: f32, blur: f32) -> f32 {
    if radius <= 0.0 {
        return 0.0;
    }
    let half = blur * 0.5;
    1.0 - smoothstep(1.0 - half, 1.0 + half, dist / radius)
}

/// Тонкое мягкое кольцо на краю растущей волны (`softRing` из шейдера): внешний круг минус внутренний.
fn soft_ring(dist: f32, max_radius: f32, progress: f32) -> f32 {
    let thickness = RING_THICKNESS * max_radius;
    let current = max_radius * progress;
    let outer = soft_circle(dist, current + thickness, RING_BLUR);
    let inner = soft_circle(dist, (current - thickness).max(0.0), RING_BLUR);
    (outer - inner).clamp(0.0, 1.0)
}

/// Шум с треугольным распределением в -1..1 по координате ячейки (`triangleNoise` из шейдера).
fn triangle_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x as i32, y as i32);
    lattice(ix, iy) + lattice(ix + 7919, iy + 104_729) - 1.0
}

/// Мерцание зерна: 0 или 1. В четырёх окнах шума зерно по очереди загорается, пока фаза `t` бежит.
fn sparkle(x: f32, y: f32, t: f32) -> f32 {
    let n = triangle_noise(x, y);
    let lit: f32 = (0..4)
        .map(|i| {
            let (low, high) = (i as f32 * 0.1, i as f32 * 0.1 + 0.05);
            let v = n + (PI * (t + 0.35 * i as f32)).sin();
            if v >= low && v < high { 1.0 } else { 0.0 }
        })
        .sum();
    lit.min(1.0)
}

/// Число 0..1 по целым координатам узла решётки (хэш без состояния).
fn lattice(ix: i32, iy: i32) -> f32 {
    let mut z = (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (iy as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 30)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 29;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// Плавный шум 0..1 (значения в узлах решётки, между ними сглаженная интерполяция).
fn value_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (smoothstep(0.0, 1.0, fract(x)), smoothstep(0.0, 1.0, fract(y)));
    let top = lattice(ix, iy) * (1.0 - fx) + lattice(ix + 1, iy) * fx;
    let bottom = lattice(ix, iy + 1) * (1.0 - fx) + lattice(ix + 1, iy + 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// Пятна, в которых крапинки заметнее (аналог `turbulence` в шейдере): медленно плывут со временем.
fn turbulence(p: Pos2, t: f32) -> f32 {
    let (x, y) = (p.x / TURBULENCE_SCALE_PX, p.y / TURBULENCE_SCALE_PX);
    let n = value_noise(x + t * 0.9, y - t * 0.6) * 0.65 + value_noise(x * 2.3 - t * 1.3, y * 2.3 + t * 0.8) * 0.35;
    ((n - 0.25) * 1.9).clamp(0.0, 1.0)
}

/// Лежит ли точка внутри прямоугольника со скруглёнными углами радиуса `radius`.
fn inside_rounded(p: Pos2, rect: Rect, radius: f32) -> bool {
    let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5).max(0.0);
    let dx = (rect.left() + r - p.x).max(p.x - (rect.right() - r)).max(0.0);
    let dy = (rect.top() + r - p.y).max(p.y - (rect.bottom() - r)).max(0.0);
    dx * dx + dy * dy <= r * r
}

/// Замедление к концу (Decelerate): кольцо быстро стартует и мягко подходит к краю.
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}

/// Где рисуется эффект: прямоугольник элемента и радиус его углов.
#[derive(Clone, Copy)]
pub(crate) struct LayerArea {
    /// Фактический (уже сжатый `squish`) прямоугольник элемента.
    pub(crate) rect: Rect,
    /// Радиус углов (для круглой кнопки половина стороны).
    pub(crate) corner: f32,
}

/// Цвет и плотность крапинок.
#[derive(Clone, Copy)]
pub(crate) struct StateTint {
    pub(crate) color: Color32,
    /// Плотность одной крапинки (0..255) на самом кольце.
    pub(crate) alpha: u8,
}

/// Нажатие на элемент: ripple (волна и крапинки) от точки нажатия. Рисуется сразу `painter`-ом, поэтому
/// вызывать после заливки элемента и до его содержимого. `rect`: фактический (уже сжатый `squish`)
/// прямоугольник элемента, `corner`: радиус его углов (для круглой кнопки половина стороны).
pub(crate) fn state_layer(ui: &Ui, theme: &Theme, painter: &Painter, resp: &Response, rect: Rect, corner: f32) {
    let tint = StateTint { color: Color32::WHITE, alpha: SPARKLE_ALPHA };
    state_layer_colored(ui, theme, painter, resp, LayerArea { rect, corner }, tint);
}

/// То же с заданным цветом крапинок. Нужен для элементов на инверсных поверхностях (кнопка закрытия snackbar),
/// где чёрный не виден.
pub(crate) fn state_layer_colored(ui: &Ui, theme: &Theme, painter: &Painter, resp: &Response, area: LayerArea, tint: StateTint) {
    if let Some(shape) = sparkle_shape(ui, theme, resp, area, tint) {
        painter.add(shape);
    }
}

/// Резервирует место под ripple в списке фигур: вызвать после фона элемента и до его содержимого, потом передать в
/// `press_sparkles`. Так крапинки лежат под текстом и значками, а не поверх них.
pub(crate) fn reserve_slot(ui: &Ui) -> ShapeIdx {
    ui.painter().add(Shape::Noop)
}

/// То же, но фигура кладётся в слот `slot` (его резервирует `glide_begin` перед элементом): крапинки оказываются
/// под текстом и значками строки.
pub(crate) fn press_sparkles(ui: &Ui, theme: &Theme, slot: ShapeIdx, resp: &Response, area: LayerArea) {
    let tint = StateTint { color: Color32::WHITE, alpha: SPARKLE_ALPHA };
    if let Some(shape) = sparkle_shape(ui, theme, resp, area, tint) {
        ui.painter().set(slot, shape);
    }
}

/// Нажатие, которое сейчас разыгрывается на элементе.
#[derive(Clone, Copy)]
struct Press {
    origin: Pos2,
    /// Время (секунды часов egui), когда нажатие началось. Возраст считается по часам, а не накоплением по кадрам:
    /// если элемент какое-то время не рисовался, ripple не «замирает», а к следующему показу уже закончен.
    start: f64,
}

/// Слои ripple в данный момент анимации (всё в 0..1, кроме `center`).
struct Frame {
    /// Центр волны: уезжает от точки нажатия к центру элемента за первую половину пути.
    center: Pos2,
    /// Радиус, до которого волна растёт: половина диагонали элемента, как в Android.
    max_radius: f32,
    /// Прогресс роста (после замедления).
    scale: f32,
    /// Общая видимость крапинок.
    sparkle_fade: f32,
    /// Общая видимость волны.
    wave_fade: f32,
    /// Время для мерцания.
    phase: f32,
}

/// Считает слои для возраста `age` (секунды) и точки нажатия `origin`.
fn frame_at(rect: Rect, origin: Pos2, age: f32) -> Frame {
    let progress = (age / RIPPLE_SECS).clamp(0.0, 1.0);
    let fade_in = sub_progress(0.0, 0.13, progress);
    let to_center = (progress * 2.0).clamp(0.0, 1.0);
    Frame {
        center: origin + (rect.center() - origin) * to_center,
        max_radius: (rect.width() * rect.width() + rect.height() * rect.height()).sqrt() * 0.5,
        scale: ease_out(progress),
        sparkle_fade: fade_in.min(1.0 - sub_progress(0.7, 0.95, progress)),
        wave_fade: fade_in.min(1.0 - sub_progress(0.4, 1.0, progress)),
        phase: age * TWINKLE_RATE,
    }
}

/// Заливка волны: сетка ячеек, цвет каждой считает `soft_circle` точно в её центре. Раньше волна строилась из колец
/// с цветом только в вершинах, и видеокарта растягивала мягкий край между ними лучами («колючками»).
fn wave_mesh(rect: Rect, corner: f32, f: &Frame, color: Color32, alpha: u8) -> Option<Mesh> {
    let radius = f.max_radius * f.scale;
    let strength = alpha as f32 * f.wave_fade;
    if strength < 0.5 || radius <= 0.0 {
        return None;
    }
    let cell = WAVE_CELL_PX.max((rect.width() * rect.height() / WAVE_MAX_CELLS).sqrt());
    let (x0, y0) = ((rect.left() / cell).floor() as i32, (rect.top() / cell).floor() as i32);
    let (x1, y1) = ((rect.right() / cell).ceil() as i32, (rect.bottom() / cell).ceil() as i32);
    let mut mesh = Mesh::default();
    for iy in y0..y1 {
        for ix in x0..x1 {
            let cell_rect = Rect::from_min_size(pos2(ix as f32 * cell, iy as f32 * cell), egui::vec2(cell, cell));
            let c = cell_rect.center();
            let a = (strength * soft_circle((c - f.center).length(), radius, WAVE_BLUR)).round() as u8;
            if a > 0 && inside_rounded(c, rect, corner) {
                mesh.add_colored_rect(cell_rect.intersect(rect), with_alpha(color, a));
            }
        }
    }
    (!mesh.is_empty()).then_some(mesh)
}

/// Крапинки: квадратики-зёрна сетки там, где зерно горит и попадает в кольцо волны.
fn sparkle_mesh(rect: Rect, corner: f32, f: &Frame, color: Color32, alpha: u8) -> Option<Mesh> {
    if f.sparkle_fade <= 0.0 {
        return None;
    }
    let cell = SPARKLE_CELL_PX.max((rect.width() * rect.height() / SPARKLE_MAX_CELLS).sqrt());
    let (x0, y0) = ((rect.left() / cell).floor() as i32, (rect.top() / cell).floor() as i32);
    let (x1, y1) = ((rect.right() / cell).ceil() as i32, (rect.bottom() / cell).ceil() as i32);
    let mut mesh = Mesh::default();
    for iy in y0..y1 {
        for ix in x0..x1 {
            let cell_rect = Rect::from_min_size(pos2(ix as f32 * cell, iy as f32 * cell), egui::vec2(cell, cell));
            let c = cell_rect.center();
            let ring = soft_ring((c - f.center).length(), f.max_radius * RING_REACH, f.scale);
            if ring < 0.004 || !inside_rounded(c, rect, corner) {
                continue;
            }
            let lit = sparkle(ix as f32, iy as f32, f.phase);
            let a = alpha as f32 * lit * ring * f.sparkle_fade * turbulence(c, f.phase / TWINKLE_RATE);
            if a >= 1.0 {
                mesh.add_colored_rect(cell_rect.intersect(rect), with_alpha(color, a.round() as u8));
            }
        }
    }
    (!mesh.is_empty()).then_some(mesh)
}

/// Шаг анимации и фигура для одного элемента; `None`, если рисовать нечего.
fn sparkle_shape(ui: &Ui, theme: &Theme, resp: &Response, area: LayerArea, tint: StateTint) -> Option<Shape> {
    let LayerArea { rect, corner } = area;
    let id: Id = resp.id.with("press_sparkles");
    let mut press: Option<Press> = ui.ctx().data_mut(|d| d.get_temp(id));
    let started = resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed());
    if started || (resp.clicked() && press.is_none()) {
        // Новое нажатие (и повторное, пока прежнее ещё идёт): ripple рождается в точке нажатия.
        // Тап без кадра с зажатой кнопкой (касание за один кадр) тоже получает ripple.
        let origin = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())).unwrap_or_else(|| rect.center());
        press = Some(Press { origin, start: ui.input(|i| i.time) });
    }
    let p = press?;
    let age = (ui.input(|i| i.time) - p.start) as f32;
    if age >= RIPPLE_SECS {
        ui.ctx().data_mut(|d| d.remove::<Press>(id));
        return None;
    }
    ui.ctx().request_repaint();
    ui.ctx().data_mut(|d| d.insert_temp(id, p));
    let held = resp.is_pointer_button_down_on();
    if theme.settings.reduce_motion {
        // «Уменьшить движение»: без бега и мерцания, только ровная заливка, пока кнопка зажата.
        return held.then(|| Shape::rect_filled(rect, corner, with_alpha(tint.color, REDUCED_MOTION_ALPHA)));
    }
    let frame = frame_at(rect, p.origin, age);
    let mut shapes = Vec::with_capacity(2);
    shapes.extend(wave_mesh(rect, corner, &frame, tint.color, WAVE_ALPHA).map(Shape::mesh));
    shapes.extend(sparkle_mesh(rect, corner, &frame, tint.color, tint.alpha).map(Shape::mesh));
    (!shapes.is_empty()).then(|| Shape::Vec(shapes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(400.0, 60.0))
    }

    #[test]
    fn triangle_noise_stays_in_range_and_is_deterministic() {
        for i in 0..40 {
            for j in 0..40 {
                let n = triangle_noise(i as f32, j as f32);
                assert!((-1.0..=1.0).contains(&n), "шум вне диапазона: {n}");
                assert_eq!(n, triangle_noise(i as f32, j as f32));
            }
        }
    }

    #[test]
    fn about_one_in_ten_grains_is_lit_at_a_time() {
        let lit: f32 = (0..100).flat_map(|i| (0..100).map(move |j| sparkle(i as f32, j as f32, 0.3))).sum();
        let share = lit / 10_000.0;
        assert!((0.04..0.2).contains(&share), "доля горящих зёрен: {share}");
    }

    #[test]
    fn grains_twinkle_over_time() {
        let changed = (0..60).filter(|&i| sparkle(i as f32, 7.0, 0.0) != sparkle(i as f32, 7.0, 0.5)).count();
        assert!(changed > 0, "за полфазы хоть одно зерно меняет состояние");
    }

    #[test]
    fn soft_circle_is_one_inside_zero_outside() {
        assert_eq!(soft_circle(0.0, 100.0, 0.2), 1.0);
        assert_eq!(soft_circle(200.0, 100.0, 0.2), 0.0);
        let edge = soft_circle(100.0, 100.0, 0.2);
        assert!((edge - 0.5).abs() < 1e-3, "на самом краю половина: {edge}");
        assert_eq!(soft_circle(1.0, 0.0, 0.2), 0.0, "нулевой радиус: пусто");
    }

    #[test]
    fn soft_ring_lives_at_the_edge_not_in_the_middle_or_far_outside() {
        let (max_r, progress) = (100.0, 0.6);
        let on_edge = soft_ring(60.0, max_r, progress);
        assert!(on_edge > 0.05, "на краю волны кольцо видно: {on_edge}");
        assert!(soft_ring(5.0, max_r, progress) < 0.01, "в середине пусто");
        assert_eq!(soft_ring(400.0, max_r, progress), 0.0, "далеко снаружи пусто");
    }

    #[test]
    fn sub_progress_maps_the_window_to_unit_range() {
        assert_eq!(sub_progress(0.4, 0.5, 0.2), 0.0);
        assert_eq!(sub_progress(0.4, 0.5, 0.9), 1.0);
        assert!((sub_progress(0.4, 0.5, 0.45) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn center_travels_from_touch_to_element_center_in_first_half() {
        let touch = pos2(10.0, 10.0);
        let start = frame_at(row(), touch, 0.0);
        let half = frame_at(row(), touch, RIPPLE_SECS * 0.5);
        assert_eq!(start.center, touch);
        assert_eq!(half.center, row().center());
        assert!(start.wave_fade == 0.0 && start.sparkle_fade == 0.0, "в первый миг ничего не видно");
    }

    #[test]
    fn ripple_fades_out_completely_by_the_end() {
        let end = frame_at(row(), pos2(50.0, 30.0), RIPPLE_SECS);
        assert_eq!(end.wave_fade, 0.0);
        assert_eq!(end.sparkle_fade, 0.0);
        assert!((end.max_radius - (400.0_f32 * 400.0 + 60.0 * 60.0).sqrt() * 0.5).abs() < 1e-3, "радиус: половина диагонали");
    }

    #[test]
    fn sparkles_stay_inside_the_rounded_rect_and_exist() {
        let f = frame_at(row(), pos2(50.0, 30.0), 0.25);
        for corner in [0.0, 12.0, 30.0] {
            let mesh = sparkle_mesh(row(), corner, &f, Color32::BLACK, 255).expect("на разгоне крапинки есть");
            let bounds = row().expand(0.01);
            assert!(mesh.vertices.iter().all(|v| bounds.contains(v.pos)), "зёрна не выходят за элемент");
        }
    }

    #[test]
    fn huge_elements_get_a_bounded_number_of_grains() {
        let big = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(3000.0, 2000.0));
        let f = frame_at(big, pos2(10.0, 10.0), 0.3);
        let mesh = sparkle_mesh(big, 0.0, &f, Color32::BLACK, 255).expect("крапинки есть");
        assert!(mesh.vertices.len() <= (SPARKLE_MAX_CELLS as usize) * 4, "вершин: {}", mesh.vertices.len());
    }

    #[test]
    fn wave_mesh_stays_inside_the_element_and_has_valid_indices() {
        let f = frame_at(row(), pos2(50.0, 30.0), 0.2);
        for corner in [0.0, 12.0, 30.0] {
            let mesh = wave_mesh(row(), corner, &f, Color32::BLACK, 255).expect("волна есть");
            let bounds = row().expand(0.01);
            assert!(mesh.vertices.iter().all(|v| bounds.contains(v.pos)));
            assert!(mesh.indices.iter().all(|&i| (i as usize) < mesh.vertices.len()));
            assert_eq!(mesh.indices.len() % 3, 0);
        }
        let end = frame_at(row(), pos2(50.0, 30.0), RIPPLE_SECS);
        assert!(wave_mesh(row(), 12.0, &end, Color32::BLACK, 255).is_none(), "в конце волны нет");
    }

    #[test]
    fn rounded_corner_cuts_the_corner_only() {
        let r = row();
        assert!(!inside_rounded(pos2(0.5, 0.5), r, 20.0), "угол срезан");
        assert!(inside_rounded(pos2(0.5, 0.5), r, 0.0), "без скругления угол на месте");
        assert!(inside_rounded(pos2(200.0, 30.0), r, 20.0));
        assert!(!inside_rounded(pos2(-1.0, 30.0), r, 20.0), "вне прямоугольника");
    }

    #[test]
    fn ease_out_goes_from_zero_to_one_without_overshoot() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.5) > 0.5, "быстрый старт");
    }
}

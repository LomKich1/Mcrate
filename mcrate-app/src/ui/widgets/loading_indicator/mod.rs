//! Индикатор загрузки Material 3 Expressive (`LoadingIndicator`): непрерывно морфирующая и вращающаяся
//! последовательность из семи фигур Material. Работает в обеих темах одинаково, цвет задаёт вызывающий
//! (в приложении это `primary`).
//!
//! Это порт индикатора из `frust-material` (`loading_indicator.rs`) на `egui`. Тот перенесён из Dart-пакета
//! `loading_indicator_m3e` / `material_3_expressive` (MIT), а он из AndroidX Compose Material3
//! `LoadingIndicator.kt` (Apache-2.0, © The Android Open Source Project). Тексты лицензий и ссылки
//! в `THIRD_PARTY_NOTICES.md`. Логика анимации (последовательность фигур, тайминги, пружина, вращение)
//! взята оттуда без изменений; отличия перечислены в конце этого комментария и в CLAUDE.md.
//!
//! # Как это устроено
//! - `geometry.rs`: `RoundedPolygon`, замкнутый контур из кубических Безье со скруглёнными углами.
//! - `morph.rs`: `Morph`, попарная интерполяция соответствующих кубик двух контуров.
//! - `shapes.rs`: семь фигур цикла и общий масштаб `shape_cycle_scale_factor`.
//! - здесь: тайминги, пружина, хореография вращения и рисование.
//!
//! # Хореография
//! Два независимых отсчёта от момента первого показа:
//! - **Морф.** Каждые `PER_SHAPE_PERIOD` (650 мс) идёт следующий морф цикла, прогресс 0..1 ведёт пружина
//!   (`LOADING_*`: жёсткость 380, демпфирование 0.8, масса 1, начальная скорость 5).
//! - **Фоновое вращение.** Полный оборот за `GLOBAL_ROTATION_PERIOD` (4666 мс).
//!
//! Угол кадра = `прогресс * 90° + цель + фоновый оборот * 360°`, где «цель» растёт на 90° за каждый морф
//! по модулю 360°. Как и в upstream, самый первый кадр уже идёт со второй фигуры цикла (`Cookie9Sided`) и
//! целью 180°: там `initState` вызывает `_startMorphCycle` до первой отрисовки.
//!
//! # Отличия от upstream (причины в комментариях рядом с кодом)
//! 1. Состояние не копится по кадрам: поза вычисляется из времени, прошедшего с первого показа. Совпадает с
//!    upstream при обычных кадрах, а при пропуске кадров догоняет, а не пропускает морфы.
//! 2. Размер можно менять: контур масштабируется пропорционально коробке (при 48 px ровно как в upstream).
//! 3. Заливка веером треугольников из центра плюс тонкий контур для сглаживания краёв (в `egui` нет надёжной
//!    заливки невыпуклых путей). Все фигуры цикла звёздчатые относительно центра.
//! 4. Прогресс морфа зажимается в 0..1 (как `value_clamped` у frust): пружина в рамках 1.2% перелёта,
//!    переключатель `CLAMP_MORPH_PROGRESS`.
//! 5. Без варианта `Contained` (подложка `primary_container`): в Mcrate он не нужен.

mod geometry;
mod morph;
mod shapes;

use std::sync::OnceLock;
use std::time::Duration;

use egui::{Color32, Context, Id, Mesh, Pos2, Response, Sense, Shape, Stroke, Ui, Vec2, Widget, pos2};

use geometry::{Cubic, Pt};
use morph::Morph;
use shapes::{SHAPE_CYCLE_LEN, polygons, shape_cycle_scale_factor};

/// Размер коробки индикатора по умолчанию, px (`containerWidth`/`containerHeight` темы upstream).
const LOADING_DIAMETER: f32 = 48.0;

/// Диаметр самих фигур внутри коробки в px при коробке `LOADING_DIAMETER` (`_activeSize` upstream, «по спецификации»).
const ACTIVE_INDICATOR_SIZE: f64 = 38.0;

/// Период одного морфа: сколько индикатор идёт к следующей фигуре (`_morphIntervalMs` upstream).
const PER_SHAPE_PERIOD: Duration = Duration::from_millis(650);

/// Период фонового вращения: один полный оборот (`_globalRotationDurationMs` upstream).
const GLOBAL_ROTATION_PERIOD: Duration = Duration::from_millis(4666);

/// Поворот за один морф, градусы: и размах самого морфа, и шаг «цели», копящийся каждый цикл (`_quarterRotation`).
const QUARTER_ROTATION_DEG: f64 = 90.0;

/// Полный оборот, градусы (`_fullRotation`).
const FULL_ROTATION_DEG: f64 = 360.0;

/// Начальная скорость пружины морфа (`SpringSimulation(spring, 0, 1, 5, snapToEnd: true)` upstream).
const MORPH_INITIAL_VELOCITY: f64 = 5.0;

/// Жёсткость пружины морфа (`M3EMotion.expressiveSpatialDefault`).
const SPRING_STIFFNESS: f64 = 380.0;

/// Коэффициент демпфирования пружины: 0.8 это недодемпфированная пружина с небольшим перелётом.
const SPRING_DAMPING_RATIO: f64 = 0.8;

/// Масса пружины (у всех пружин M3 равна 1).
const SPRING_MASS: f64 = 1.0;

/// Амплитуда колебаний пружины, ниже которой она считается остановившейся и значение «прилипает» к 1
/// (`snapToEnd` у Dart).
const SPRING_SETTLE_EPSILON: f64 = 1e-3;

/// Зажимать ли прогресс морфа в 0..1. Перелёт пружины не превышает 1.2%, но так же делает upstream
/// (`value_clamped` во frust, `AnimationController` во Flutter). `false` вернёт перелёт Compose.
const CLAMP_MORPH_PROGRESS: bool = true;

/// Цвет заливки по умолчанию, если вызывающий его не задал (`#3B82F6`, как `FALLBACK_FILL` upstream).
const FALLBACK_FILL: Color32 = Color32::from_rgb(0x3B, 0x82, 0xF6);

/// Сколько секунд индикатор мог не рисоваться, чтобы анимация продолжилась; дольше пауза значит
/// «показан заново» и отсчёт идёт с начала (как при пересоздании виджета в upstream).
const RESTART_GAP_SECS: f64 = 0.25;

/// Момент внутри цикла, в который замораживается кадр при «уменьшении движения»: пружина уже успокоилась.
const REDUCED_MOTION_TIME: f64 = 0.5;

/// Ширина контура-«сглаживателя» краёв, px. Заливка сетки сама края не сглаживает; тонкая обводка того же
/// цвета даёт им плавный переход (ценой ~0.5 px роста фигуры наружу).
const EDGE_STROKE_WIDTH: f32 = 1.0;

/// Не меньше и не больше столько отрезков на кубику при разбиении контура в ломаную: 6 при 48 px.
const FLATTEN_STEPS_RANGE: (usize, usize) = (4, 16);

/// Ключ данных egui для состояния индикатора (момент первого показа и последнего кадра).
const STATE_KEY: &str = "m3e_loading_indicator_clock";

/// Круговой список морфов `SHAPE_CYCLE[i] -> SHAPE_CYCLE[i + 1]` (последний замыкается на первую фигуру,
/// `circularSequence: true`). Строится один раз на первом кадре и общий для всех экземпляров: построение
/// морфа (измерение, сопоставление углов, разрезы) это дорогая часть, кадр лишь интерполирует готовые пары.
fn morph_sequence() -> &'static [Morph; SHAPE_CYCLE_LEN] {
    static CACHE: OnceLock<[Morph; SHAPE_CYCLE_LEN]> = OnceLock::new();
    CACHE.get_or_init(|| {
        let polys = polygons();
        std::array::from_fn(|i| Morph::new(&polys[i], &polys[(i + 1) % SHAPE_CYCLE_LEN]))
    })
}

/// Недодемпфированная пружина от 0 к 1 с начальной скоростью. Решение уравнения `x'' + 2ζω₀x' + ω₀²x = 0`
/// в замкнутом виде (то же, что считает `SpringSimulation` Flutter), поэтому значение берётся по времени,
/// а не копится шагами, и не зависит от частоты кадров.
struct Spring {
    /// Скорость затухания огибающей (`ζω₀`).
    decay: f64,
    /// Частота затухающих колебаний.
    omega: f64,
    c1: f64,
    c2: f64,
}

impl Spring {
    fn new(stiffness: f64, damping_ratio: f64, mass: f64, initial_velocity: f64) -> Spring {
        let damping = damping_ratio * 2.0 * (mass * stiffness).sqrt();
        let decay = damping / (2.0 * mass);
        let omega = (4.0 * mass * stiffness - damping * damping).sqrt() / (2.0 * mass);
        // Старт в 0, цель в 1: x(0) = 1 + c1 = 0, x'(0) = -decay*c1 + omega*c2 = initial_velocity.
        let c1 = -1.0;
        let c2 = (initial_velocity + decay * c1) / omega;
        Spring { decay, omega, c1, c2 }
    }

    /// Значение пружины через `t` секунд после запуска.
    fn value_at(&self, t: f64) -> f64 {
        let envelope = (-self.decay * t).exp();
        if envelope * self.c1.hypot(self.c2) < SPRING_SETTLE_EPSILON {
            return 1.0;
        }
        1.0 + envelope * (self.c1 * (self.omega * t).cos() + self.c2 * (self.omega * t).sin())
    }
}

/// Что рисовать в данный момент: какой морф цикла идёт, его прогресс и полный угол поворота.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    morph_index: usize,
    progress: f64,
    rotation_deg: f64,
}

/// Поза индикатора через `elapsed` секунд после первого показа. Чистая функция времени.
///
/// Номер цикла `n` растёт каждые `PER_SHAPE_PERIOD`. Upstream перед первым кадром уже один раз вызвал
/// `_startMorphCycle`, поэтому индекс морфа это `(1 + n) % 7`, а «цель» вращения `(180° + 90°·n) % 360°`.
fn pose_at(elapsed: f64) -> Pose {
    let elapsed = elapsed.max(0.0);
    let period = PER_SHAPE_PERIOD.as_secs_f64();
    let cycle = (elapsed / period).floor();
    let in_cycle = elapsed - cycle * period;
    let n = cycle as u64;

    let morph_index = ((1 + n) % SHAPE_CYCLE_LEN as u64) as usize;
    // Цель растёт на 90° за цикл по модулю 360°, то есть повторяется каждые 4 цикла.
    let rotation_target = (2.0 * QUARTER_ROTATION_DEG + QUARTER_ROTATION_DEG * (n % 4) as f64) % FULL_ROTATION_DEG;

    let spring = Spring::new(SPRING_STIFFNESS, SPRING_DAMPING_RATIO, SPRING_MASS, MORPH_INITIAL_VELOCITY);
    let raw = spring.value_at(in_cycle);
    let progress = if CLAMP_MORPH_PROGRESS { raw.clamp(0.0, 1.0) } else { raw };

    let global = GLOBAL_ROTATION_PERIOD.as_secs_f64();
    let global_rotation = (elapsed % global) / global * FULL_ROTATION_DEG;
    Pose { morph_index, progress, rotation_deg: progress * QUARTER_ROTATION_DEG + rotation_target + global_rotation }
}

/// Контур кадра в экранных координатах: кубики морфа масштабируются, центрируются по точным границам
/// и поворачиваются вокруг центра коробки (`_processPath` upstream: масштаб, центровка, поворот).
/// Масштаб пропорционален коробке: при `LOADING_DIAMETER` px он ровно upstream-овский `38 * scale_factor`.
fn outline_points(cubics: &[Cubic], center: Pos2, diameter: f32, rotation_deg: f64) -> Vec<Pos2> {
    let scale = f64::from(diameter) * (ACTIVE_INDICATOR_SIZE / f64::from(LOADING_DIAMETER)) * shape_cycle_scale_factor();
    let scaled: Vec<Cubic> = cubics.iter().map(|c| c.transformed_with(|p| Pt::new(p.x * scale, p.y * scale))).collect();

    // Центр точных границ (не опорных точек): фигура центрируется по тому, что реально нарисовано.
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for cubic in &scaled {
        let c = cubic.exact_bounds();
        b = [b[0].min(c[0]), b[1].min(c[1]), b[2].max(c[2]), b[3].max(c[3])];
    }
    let bounds_center = Pt::new((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    let (sin, cos) = rotation_deg.to_radians().sin_cos();
    let place = |p: Pt| -> Pos2 {
        let d = p - bounds_center;
        pos2(center.x + (d.x * cos - d.y * sin) as f32, center.y + (d.x * sin + d.y * cos) as f32)
    };

    let steps = ((diameter / 8.0).ceil() as usize).clamp(FLATTEN_STEPS_RANGE.0, FLATTEN_STEPS_RANGE.1);
    let mut points = Vec::with_capacity(scaled.len() * steps);
    for cubic in &scaled {
        for k in 0..steps {
            points.push(place(cubic.point_on_curve(k as f64 / steps as f64)));
        }
    }
    points
}

/// Рисует замкнутый контур: веер треугольников из `center` (`egui` не умеет надёжно заливать невыпуклые
/// пути, а все фигуры цикла звёздчатые относительно центра) и тонкая обводка для сглаживания краёв.
fn paint_outline(ui: &Ui, points: Vec<Pos2>, center: Pos2, color: Color32) {
    if points.len() < 3 {
        return;
    }
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, color);
    for p in &points {
        mesh.colored_vertex(*p, color);
    }
    let n = points.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    let painter = ui.painter();
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::closed_line(points, Stroke::new(EDGE_STROKE_WIDTH, color)));
}

/// Секунды с первого показа индикатора `id`. Момент старта лежит в памяти egui; если индикатор не рисовался
/// дольше `RESTART_GAP_SECS` (экран закрыли и открыли), отсчёт начинается заново.
fn elapsed_since_shown(ctx: &Context, id: Id, now: f64) -> f64 {
    let key = id.with(STATE_KEY);
    let (start, last_seen) = ctx.data_mut(|d| d.get_temp::<(f64, f64)>(key)).unwrap_or((now, now));
    let start = if now - last_seen > RESTART_GAP_SECS { now } else { start };
    ctx.data_mut(|d| d.insert_temp(key, (start, now)));
    now - start
}

/// Виджет индикатора загрузки M3 Expressive. Добавляется как любой виджет `egui`:
/// `ui.add(M3ELoadingIndicator::new().size(48.0).color(primary))`.
pub(crate) struct M3ELoadingIndicator {
    size: f32,
    color: Color32,
    reduce_motion: bool,
}

impl M3ELoadingIndicator {
    /// Индикатор 48 px цвета `#3B82F6`, с движением.
    pub(crate) fn new() -> Self {
        M3ELoadingIndicator { size: LOADING_DIAMETER, color: FALLBACK_FILL, reduce_motion: false }
    }

    /// Сторона квадратной коробки, px. Фигура занимает 38/48 коробки.
    pub(crate) fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Цвет заливки фигуры.
    pub(crate) fn color(mut self, color: Color32) -> Self {
        self.color = color;
        self
    }

    /// «Уменьшить анимацию»: вместо движения неподвижная фигура, перерисовку виджет не просит.
    pub(crate) fn reduce_motion(mut self, reduce_motion: bool) -> Self {
        self.reduce_motion = reduce_motion;
        self
    }
}

impl Default for M3ELoadingIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for M3ELoadingIndicator {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(self.size), Sense::hover());
        // Невидимый индикатор ничего не рисует и не просит перерисовку.
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let elapsed = if self.reduce_motion {
            REDUCED_MOTION_TIME
        } else {
            let now = ui.input(|i| i.time);
            elapsed_since_shown(ui.ctx(), response.id, now)
        };
        let pose = pose_at(elapsed);
        let cubics = morph_sequence()[pose.morph_index].cubics_at(pose.progress);
        let points = outline_points(&cubics, rect.center(), self.size, pose.rotation_deg);
        paint_outline(ui, points, rect.center(), self.color);
        if !self.reduce_motion {
            ui.ctx().request_repaint();
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::geometry::{CornerRounding, RoundedPolygon};
    use super::shapes::SHAPE_CYCLE;
    use super::*;

    /// Допуск для эталонных чисел upstream (Dart печатает кратчайшее представление `f64`, расхождение
    /// выше погрешности порядка операций значит настоящее расхождение портов).
    const GOLDEN_EPS: f64 = 1e-11;

    fn assert_cubic(got: &Cubic, want: [f64; 8], eps: f64, what: &str) {
        for (i, (g, w)) in got.p.iter().zip(want.iter()).enumerate() {
            assert!((g - w).abs() < eps, "{what}: координата {i}: ждали {w}, получили {g}");
        }
    }

    fn octagon() -> RoundedPolygon {
        RoundedPolygon::from_num_vertices(8, 1.0, Pt::ZERO, CornerRounding::UNROUNDED, None)
    }

    fn square() -> RoundedPolygon {
        RoundedPolygon::from_num_vertices(4, 1.0, Pt::ZERO, CornerRounding::UNROUNDED, None)
    }

    fn circle8() -> RoundedPolygon {
        RoundedPolygon::circle(8, 1.0, Pt::ZERO)
    }

    #[test]
    fn constants_match_upstream() {
        assert_eq!(PER_SHAPE_PERIOD, Duration::from_millis(650));
        assert_eq!(GLOBAL_ROTATION_PERIOD, Duration::from_millis(4666));
        assert_eq!(QUARTER_ROTATION_DEG, 90.0);
        assert_eq!(FULL_ROTATION_DEG, 360.0);
        assert_eq!(MORPH_INITIAL_VELOCITY, 5.0);
        assert_eq!(SPRING_STIFFNESS, 380.0);
        assert_eq!(SPRING_DAMPING_RATIO, 0.8);
        assert_eq!(SPRING_MASS, 1.0);
        assert_eq!(ACTIVE_INDICATOR_SIZE, 38.0);
        assert_eq!(LOADING_DIAMETER, 48.0);
        assert_eq!(FALLBACK_FILL, Color32::from_rgb(0x3B, 0x82, 0xF6));
        assert_eq!(SHAPE_CYCLE_LEN, 7);
        assert_eq!(SHAPE_CYCLE.len(), 7);
    }

    /// Эталон `CIRCLE_8` из тестов `frust-material` (числа из Dart-реализации).
    #[test]
    fn circle_matches_upstream_golden() {
        let circle = circle8();
        assert_eq!(circle.cubics.len(), 9);
        assert_cubic(
            &circle.cubics[0],
            [
                1.0,
                1.3877787807814457e-17,
                1.0,
                0.1300846945207343,
                0.9746265108370957,
                0.2601693890414686,
                0.9238795325112868,
                0.3826834323650898,
            ],
            1e-12,
            "circle[0]",
        );
    }

    /// Эталон `PILL` (пилюля 2×1, скругление 0.5 при сглаживании 0.3): проверяет сглаживание и разрезание сторон.
    #[test]
    fn rounded_pill_matches_upstream_golden() {
        let (w, h) = (2.0, 1.0);
        let vertices = [Pt::new(w / 2.0, h / 2.0), Pt::new(-w / 2.0, h / 2.0), Pt::new(-w / 2.0, -h / 2.0), Pt::new(w / 2.0, -h / 2.0)];
        let rounding = CornerRounding { radius: 0.5, smoothing: 0.3 };
        let pill = RoundedPolygon::from_vertices(&vertices, rounding, None, Some(Pt::ZERO));
        assert_eq!(pill.cubics.len(), 11);
        assert_cubic(
            &pill.cubics[0],
            [
                0.8830446015004401,
                0.3213671315821969,
                0.8096338575801304,
                0.4088670230999255,
                0.7063570243187614,
                0.47130983399174997,
                0.5868926669545238,
                0.4923917794089684,
            ],
            1e-12,
            "pill[0]",
        );
    }

    /// Эталоны морфов из тестов upstream: круг -> квадрат, треугольник -> восьмиугольник, звезда -> круг (вогнутые углы).
    #[test]
    fn morphs_match_upstream_goldens() {
        let m = Morph::new(&circle8(), &square()).cubics_at(0.5);
        assert_eq!(m.len(), 12);
        assert_cubic(
            &m[0],
            [
                0.8369397662556435,
                -0.31634171618254486,
                0.9039799220852145,
                -0.2134180278540675,
                0.9583333333333333,
                -0.10670901392703358,
                1.0,
                3.0878077872387166e-16,
            ],
            GOLDEN_EPS,
            "circle->square",
        );

        let triangle = RoundedPolygon::from_num_vertices(3, 1.0, Pt::ZERO, CornerRounding::UNROUNDED, None);
        let m = Morph::new(&triangle, &octagon()).cubics_at(0.5);
        assert_eq!(m.len(), 8);
        assert_cubic(
            &m[0],
            [
                1.0,
                0.0,
                0.867851130197758,
                0.16596365263022672,
                0.7357022603955158,
                0.33192730526045344,
                0.6035533905932737,
                0.4978909578906803,
            ],
            GOLDEN_EPS,
            "triangle->octagon",
        );

        let star = RoundedPolygon::star(5, 1.0, 0.5, CornerRounding::radius(0.2), Pt::ZERO);
        let m = Morph::new(&star, &circle8()).cubics_at(0.5);
        assert_eq!(m.len(), 28);
        let (a0, a7) = (m[0].anchor0(), m[7].anchor0());
        assert!((a0.x - 0.785330199718972).abs() < GOLDEN_EPS && (a0.y + 0.2682354889504666).abs() < GOLDEN_EPS);
        assert!((a7.x + 0.0776730888836798).abs() < 1e-10 && (a7.y - 0.8221309185364376).abs() < 1e-10);
    }

    /// Все семь морфов цикла строятся, их число кривых совпадает с эталонным портом, контуры замкнуты и конечны.
    #[test]
    fn whole_cycle_builds_and_is_well_formed() {
        let polys = polygons();
        assert_eq!(polys.iter().map(|p| p.cubics.len()).collect::<Vec<_>>(), [41, 19, 11, 13, 33, 17, 9]);
        let expected_pairs = [58, 28, 22, 44, 48, 24, 48];
        for (i, morph) in morph_sequence().iter().enumerate() {
            for progress in [0.0, 0.25, 0.5, 1.0] {
                let cubics = morph.cubics_at(progress);
                assert_eq!(cubics.len(), expected_pairs[i], "морф {i}");
                for (j, c) in cubics.iter().enumerate() {
                    assert!(c.p.iter().all(|v| v.is_finite()), "морф {i}, кривая {j}: не конечное число");
                    let next = cubics[(j + 1) % cubics.len()].anchor0();
                    assert!((c.anchor1() - next).hypot() < 1e-9, "морф {i}, кривая {j}: контур разорван");
                }
            }
        }
    }

    /// Общий масштаб цикла: наихудшее отношение границ к границам при любом повороте (у овала).
    #[test]
    fn scale_factor_matches_reference() {
        assert!((shape_cycle_scale_factor() - 0.865584223342794).abs() < 1e-9);
    }

    #[test]
    fn spring_is_underdamped_with_small_overshoot() {
        let s = Spring::new(SPRING_STIFFNESS, SPRING_DAMPING_RATIO, SPRING_MASS, MORPH_INITIAL_VELOCITY);
        assert_eq!(s.value_at(0.0), 0.0);
        assert!((s.value_at(0.1) - 0.74257).abs() < 1e-4);
        assert!((s.value_at(0.2) - 1.00193).abs() < 1e-4);
        assert!(s.value_at(0.3) > 1.0, "перелёт есть");
        assert!(s.value_at(0.3) < 1.02, "перелёт небольшой");
        assert_eq!(s.value_at(1.0), 1.0);
    }

    #[test]
    fn pose_replays_the_initial_cycle_and_wraps() {
        // Первый кадр уже на второй фигуре цикла и с целью 180° (upstream вызвал `_startMorphCycle` в initState).
        let p = pose_at(0.0);
        assert_eq!((p.morph_index, p.progress, p.rotation_deg), (1, 0.0, 180.0));
        // После одного периода: следующий морф и цель 270°.
        let p = pose_at(0.651);
        assert_eq!(p.morph_index, 2);
        assert!((p.rotation_deg - (QUARTER_ROTATION_DEG * p.progress + 270.0 + 0.651 / 4.666 * 360.0)).abs() < 1e-9);
        // Через шесть периодов индекс замыкается на 0, цель (180 + 540) % 360 = 0.
        let p = pose_at(6.0 * 0.65 + 0.01);
        assert_eq!(p.morph_index, 0);
        let global = (6.0 * 0.65 + 0.01) / 4.666 * 360.0;
        assert!((p.rotation_deg - (p.progress * 90.0 + 0.0 + global)).abs() < 1e-9);
        // Отрицательное время не ломает расчёт.
        assert_eq!(pose_at(-1.0), pose_at(0.0));
    }

    #[test]
    fn painted_outline_stays_inside_the_box() {
        let center = pos2(24.0, 24.0);
        for step in 0..200 {
            let pose = pose_at(f64::from(step) * 0.037);
            let cubics = morph_sequence()[pose.morph_index].cubics_at(pose.progress);
            let points = outline_points(&cubics, center, LOADING_DIAMETER, pose.rotation_deg);
            for p in &points {
                assert!((p.x - 24.0).abs() <= 24.0 && (p.y - 24.0).abs() <= 24.0, "кадр {step}: {p:?} вышла из коробки 48 px");
            }
        }
    }
}

//! Каталог фигур индикатора: семь фигур Material Shapes из `SHAPE_CYCLE` и их общие вычисления.
//!
//! Перенос из `frust-material` (`shapes/catalog.rs`), тот из `material_new_shapes` 1.0.0 (MIT, Agbama Gifted)
//! и AndroidX Compose Material3 `MaterialShapes` (Apache-2.0). Координаты вершин и скругления скопированы
//! без изменений и проходят тот же конвейер «повтор, зеркало, поворот, нормализация». Из полного каталога
//! в 35 фигур портированы только семь нужных индикатору.
//!
//! Многие фигуры строятся не готовыми конструкторами, а из короткого списка вершин, который `repeat_points`
//! повторяет `reps` раз вокруг центра (с зеркалом или без): так из несимметричной затравки выходит
//! симметричный лепесток или «печенье».

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};
use std::sync::OnceLock;

use super::geometry::{CornerRounding, Pt, RoundedPolygon};

/// Сколько фигур в цикле (`_defaultPolygons.length`).
pub(super) const SHAPE_CYCLE_LEN: usize = 7;

/// Фигуры цикла индикатора, в порядке показа.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShapeKind {
    SoftBurst,
    Cookie9Sided,
    Pentagon,
    Pill,
    Sunny,
    Cookie4Sided,
    Oval,
}

/// Последовательность фигур, как `_defaultPolygons` в upstream и `IndeterminateIndicatorPolygons` в Compose.
pub(super) const SHAPE_CYCLE: [ShapeKind; SHAPE_CYCLE_LEN] = [
    ShapeKind::SoftBurst,
    ShapeKind::Cookie9Sided,
    ShapeKind::Pentagon,
    ShapeKind::Pill,
    ShapeKind::Sunny,
    ShapeKind::Cookie4Sided,
    ShapeKind::Oval,
];

/// Центр, вокруг которого строятся фигуры не от нуля (`center` по умолчанию в `_customPolygon`).
const CENTER: Pt = Pt::new(0.5, 0.5);

/// Круговое скругление 100% (`_cornerRound100`).
const ROUND_100: CornerRounding = CornerRounding::radius(1.0);

/// Нормализованные полигоны цикла. Строятся один раз на первом обращении: скругление считается
/// по каждой вершине и недёшево.
pub(super) fn polygons() -> &'static [RoundedPolygon; SHAPE_CYCLE_LEN] {
    static CACHE: OnceLock<[RoundedPolygon; SHAPE_CYCLE_LEN]> = OnceLock::new();
    CACHE.get_or_init(|| SHAPE_CYCLE.map(build))
}

fn build(kind: ShapeKind) -> RoundedPolygon {
    match kind {
        ShapeKind::SoftBurst => soft_burst(),
        ShapeKind::Cookie9Sided => cookie_9_sided(),
        ShapeKind::Pentagon => pentagon(),
        ShapeKind::Pill => pill(),
        ShapeKind::Sunny => sunny(),
        ShapeKind::Cookie4Sided => cookie_4_sided(),
        ShapeKind::Oval => oval(),
    }
}

/// Вершина фигуры, заданной вручную, и скругление в ней (`_PointNRound`).
type PointNRound = (Pt, CornerRounding);

/// Полигон из короткого списка вершин, повторённого `reps` раз вокруг `center` (`_customPolygon`).
fn custom_polygon(points: &[PointNRound], reps: usize, center: Pt, mirroring: bool) -> RoundedPolygon {
    let actual = repeat_points(points, reps, center, mirroring);
    let vertices: Vec<Pt> = actual.iter().map(|p| p.0).collect();
    let rounding: Vec<CornerRounding> = actual.iter().map(|p| p.1).collect();
    RoundedPolygon::from_vertices(&vertices, CornerRounding::UNROUNDED, Some(&rounding), Some(center))
}

/// Повторяет список вершин вокруг `center` (`_doRepeat`). Без зеркала список просто поворачивается на
/// `360/reps` градусов `reps` раз. С зеркалом каждая нечётная копия отражена (идёт в обратном порядке без
/// первой точки), поэтому соседние копии читаются как отражения, а не как повороты.
fn repeat_points(points: &[PointNRound], reps: usize, center: Pt, mirroring: bool) -> Vec<PointNRound> {
    let mut result = Vec::new();
    if mirroring {
        // Полярные координаты затравки относительно центра: считаются один раз, а не на каждой копии.
        let measures: Vec<(f64, f64)> = points
            .iter()
            .map(|p| {
                let off = p.0 - center;
                (off.y.atan2(off.x), off.hypot())
            })
            .collect();
        let actual_reps = reps * 2;
        let section_angle = TAU / actual_reps as f64;
        for r in 0..actual_reps {
            for index in 0..points.len() {
                let i = if r % 2 == 0 { index } else { points.len() - 1 - index };
                // Отражённая копия пропускает первую точку: ею кончилась предыдущая копия.
                if i > 0 || r % 2 == 0 {
                    let angle = section_angle * r as f64
                        + if r % 2 == 0 { measures[i].0 } else { section_angle - measures[i].0 + 2.0 * measures[0].0 };
                    let point = center + Pt::new(angle.cos(), angle.sin()) * measures[i].1;
                    result.push((point, points[i].1));
                }
            }
        }
    } else {
        let n = points.len();
        for i in 0..(n * reps) {
            let degrees = (i / n) as f64 * 360.0 / reps as f64;
            result.push((rotate_around(points[i % n].0, degrees.to_radians(), center), points[i % n].1));
        }
    }
    result
}

/// Поворот точки на `radians` вокруг `center`.
fn rotate_around(p: Pt, radians: f64, center: Pt) -> Pt {
    let off = p - center;
    let (sin, cos) = radians.sin_cos();
    center + Pt::new(off.x * cos - off.y * sin, off.x * sin + off.y * cos)
}

/// Поворот вокруг начала координат (`..rotateZ(a)` в цепочке `Matrix4` upstream).
fn rotate_z(radians: f64, p: Pt) -> Pt {
    let (sin, cos) = radians.sin_cos();
    Pt::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos)
}

/// Мягкий всплеск: 10 повторов зубца (`SoftBurst`, `material_shapes.dart:369-381`).
fn soft_burst() -> RoundedPolygon {
    custom_polygon(
        &[(Pt::new(0.193, 0.277), CornerRounding::radius(0.053)), (Pt::new(0.176, 0.055), CornerRounding::radius(0.053))],
        10,
        CENTER,
        false,
    )
    .normalized()
}

/// Девятиугольное «печенье»: звезда 9 на 0.8, скругление 0.5, поворот на -90° (`Cookie9Sided`, `:307-315`).
fn cookie_9_sided() -> RoundedPolygon {
    RoundedPolygon::star(9, 1.0, 0.8, CornerRounding::radius(0.5), Pt::ZERO).transformed_with(|p| rotate_z(-FRAC_PI_2, p)).normalized()
}

/// Пятиугольник с зеркалом (`Pentagon`, `:200-217`).
fn pentagon() -> RoundedPolygon {
    custom_polygon(
        &[
            (Pt::new(0.5, -0.009), CornerRounding::radius(0.172)),
            (Pt::new(1.03, 0.365), CornerRounding::radius(0.164)),
            (Pt::new(0.828, 0.97), CornerRounding::radius(0.169)),
        ],
        1,
        CENTER,
        true,
    )
    .normalized()
}

/// Пилюля (`Pill`, `:94-108`).
fn pill() -> RoundedPolygon {
    custom_polygon(
        &[
            (Pt::new(0.961, 0.039), CornerRounding::radius(0.426)),
            (Pt::new(1.001, 0.428), CornerRounding::UNROUNDED),
            (Pt::new(1.0, 0.609), ROUND_100),
        ],
        2,
        CENTER,
        true,
    )
    .normalized()
}

/// «Солнышко»: звезда 8 на 0.8, скругление 0.15 (`Sunny`, `:244-248`).
fn sunny() -> RoundedPolygon {
    RoundedPolygon::star(8, 1.0, 0.8, CornerRounding::radius(0.15), Pt::ZERO).normalized()
}

/// Четырёхугольное «печенье» (`Cookie4Sided`, `:266-278`).
fn cookie_4_sided() -> RoundedPolygon {
    custom_polygon(
        &[(Pt::new(1.237, 1.236), CornerRounding::radius(0.258)), (Pt::new(0.5, 0.918), CornerRounding::radius(0.233))],
        4,
        CENTER,
        false,
    )
    .normalized()
}

/// Овал: окружность на 8 вершин, сжатая по y до 0.64 и повёрнутая на -45°; масштаб применяется первым,
/// потом поворот (цепочка `..rotateZ(a)..scale(sx, sy)` у `Matrix4` умножает справа) (`Oval`, `:84-91`).
fn oval() -> RoundedPolygon {
    RoundedPolygon::circle(8, 1.0, Pt::ZERO).transformed_with(|p| rotate_z(-FRAC_PI_4, Pt::new(p.x, p.y * 0.64))).normalized()
}

/// Общий масштаб нормализованных контуров цикла, при котором фигура не вылезает из коробки при вращении
/// (`_calculateScaleFactor`). Для вращающейся фигуры мало вписать её в свои неповёрнутые границы: берётся
/// наихудшее по всему циклу отношение осевых границ к границам при любом повороте.
pub(super) fn shape_cycle_scale_factor() -> f64 {
    static CACHE: OnceLock<f64> = OnceLock::new();
    *CACHE.get_or_init(|| {
        polygons().iter().fold(1.0_f64, |acc, polygon| {
            let b = polygon.bounds();
            let m = polygon.max_bounds();
            let scale_x = (b[2] - b[0]) / (m[2] - m[0]);
            let scale_y = (b[3] - b[1]) / (m[3] - m[1]);
            acc.min(scale_x.max(scale_y))
        })
    })
}

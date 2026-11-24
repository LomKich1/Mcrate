//! Морфинг между двумя `RoundedPolygon`: измерение контура, сопоставление углов и сам `Morph`.
//!
//! Перенос из `frust-material` (`shapes/morph.rs`), тот из `material_new_shapes` и AndroidX `graphics-shapes`.
//! Конвейер построения (всё в `Morph::new`, поэтому морф строят один раз и хранят):
//! 1. **Измерение.** Каждой кубике считается диапазон «прогресса вдоль контура» (доля периметра по длине дуги,
//!    три хорды на кубику). Кривые нулевой длины (каждый острый угол) выпадают.
//! 2. **Сопоставление углов.** Углы двух фигур парами по близости, ближайшие первыми; выпуклый только с
//!    выпуклым, вогнутый только с вогнутым; пересечений нет. Пары задают кусочно-линейное отображение
//!    прогресса одной фигуры в прогресс другой (`DoubleMapper`).
//! 3. **Разрез и сдвиг.** Нулевой прогресс первой фигуры лежит где-то на второй: вторую перерезаем так,
//!    чтобы обе обходились с нуля.
//! 4. **Сшивка.** Идём по обеим; кривая, которая кончается первой, задаёт границу, перебежавшая режется.
//!    Получается список пар (кривая начала, кривая конца), покрывающий оба контура точно.
//!
//! Кадр морфа это поэлементная интерполяция каждой пары (`Morph::cubics_at`).

use super::geometry::{Cubic, DISTANCE_EPSILON, Feature, FeatureKind, Pt, RoundedPolygon, lerp};

/// Разница прогресса, ниже которой две позиции на контуре считаются одной и кривую не режем (`angleEpsilon`).
const ANGLE_EPSILON: f64 = 1e-6;

/// На сколько хорд приближается длина дуги одной кубики (`LengthMeasurer._segments`). Три хорды дают
/// ошибку до 1,5% на круговой дуге; морф только сравнивает измерения друг с другом, поэтому занижение не важно.
const MEASURE_SEGMENTS: usize = 3;

/// `value` по модулю `modulus`, всегда в `0..modulus`. `rem_euclid` для крошечного отрицательного даёт
/// сам модуль, что нарушает контракт прогресса `[0, 1)`; лишнее сравнение даёт то, что у Dart `%`.
fn positive_modulo(value: f64, modulus: f64) -> f64 {
    let remainder = value.rem_euclid(modulus);
    if remainder >= modulus { 0.0 } else { remainder }
}

/// Расстояние между двумя прогрессами коротким путём по кругу: 0.99 и 0.0 отстоят на 0.01.
fn progress_distance(p1: f64, p2: f64) -> f64 {
    let diff = (p1 - p2).abs();
    diff.min(1.0 - diff)
}

/// Лежит ли `progress` в диапазоне от `from` до `to`. Прогресс замкнут в круг: диапазон с `to < from`
/// переходит через 0 (`0.7..0.2` содержит 0.8 и 0.1, но не 0.5).
fn progress_in_range(progress: f64, from: f64, to: f64) -> bool {
    if to >= from { progress >= from && progress <= to } else { progress >= from || progress <= to }
}

/// Интерполирует `x` по кусочно-линейному замкнутому отображению `x_values[i] -> y_values[i]`.
/// Для проверенного отображения отрезки покрывают весь круг, поэтому отрезок всегда находится;
/// иначе (недостижимо) возвращаем `x`, чтобы не ронять окно из-за декоративного индикатора.
fn linear_map(x_values: &[f64], y_values: &[f64], x: f64) -> f64 {
    let n = x_values.len();
    let Some(start) = (0..n).find(|&i| progress_in_range(x, x_values[i], x_values[(i + 1) % n])) else {
        return x;
    };
    let end = (start + 1) % n;
    let segment_size_x = positive_modulo(x_values[end] - x_values[start], 1.0);
    let segment_size_y = positive_modulo(y_values[end] - y_values[start], 1.0);
    // Слишком короткий отрезок: берём середину, а не усиливаем погрешность делением.
    let position = if segment_size_x < 0.001 { 0.5 } else { positive_modulo(x - x_values[start], 1.0) / segment_size_x };
    positive_modulo(y_values[start] + segment_size_y * position, 1.0)
}

/// Биекция между двумя пространствами прогресса `0..1`, заданная конечным списком соответствий
/// и продолженная линейно с заворотом.
struct DoubleMapper {
    source: Vec<f64>,
    target: Vec<f64>,
}

impl DoubleMapper {
    fn new(mappings: &[(f64, f64)]) -> DoubleMapper {
        DoubleMapper { source: mappings.iter().map(|m| m.0).collect(), target: mappings.iter().map(|m| m.1).collect() }
    }

    /// Значение в целевом пространстве для `x` из исходного.
    fn map(&self, x: f64) -> f64 {
        linear_map(&self.source, &self.target, x)
    }

    /// Значение в исходном пространстве для `x` из целевого.
    fn map_back(&self, x: f64) -> f64 {
        linear_map(&self.target, &self.source, x)
    }
}

/// За один обход кубики хордами возвращает (параметр, на котором набрана длина `threshold`; саму `threshold`),
/// а если кривая короче, то `(1, её длина)`. С `INFINITY` порог недостижим, и результат это полная длина.
fn closest_progress_to(cubic: &Cubic, threshold: f64) -> (f64, f64) {
    let segments = MEASURE_SEGMENTS as f64;
    let mut total = 0.0;
    let mut remainder = threshold;
    let mut previous = cubic.anchor0();
    for i in 0..=MEASURE_SEGMENTS {
        let progress = i as f64 / segments;
        let point = cubic.point_on_curve(progress);
        let segment = (point - previous).hypot();
        if segment >= remainder {
            // Хорда нулевой длины «достигается» только при нулевом остатке, и тогда разрез ровно здесь
            // (upstream делит 0/0 и получает NaN).
            let covered = if segment > 0.0 { remainder / segment } else { 1.0 };
            return (progress - (1.0 - covered) / segments, threshold);
        }
        remainder -= segment;
        total += segment;
        previous = point;
    }
    (1.0, total)
}

fn measure_cubic(cubic: &Cubic) -> f64 {
    closest_progress_to(cubic, f64::INFINITY).1
}

/// Параметр, на котором кубика прошла длину `measure` (не больше полной длины).
fn find_cubic_cut_point(cubic: &Cubic, measure: f64) -> f64 {
    closest_progress_to(cubic, measure).0
}

/// Кубика контура и участок контура, который она покрывает (доли периметра).
#[derive(Clone, Copy, Debug)]
struct MeasuredCubic {
    cubic: Cubic,
    measured_size: f64,
    start_outline_progress: f64,
    end_outline_progress: f64,
}

impl MeasuredCubic {
    fn new(cubic: Cubic, start_outline_progress: f64, end_outline_progress: f64) -> MeasuredCubic {
        MeasuredCubic { cubic, measured_size: measure_cubic(&cubic), start_outline_progress, end_outline_progress }
    }

    /// Делит кривую по прогрессу контура на часть до и часть после. Разрез сначала зажимается в диапазон
    /// самой кривой: накопленная ошибка хуже, чем разрез ровно на краю.
    fn cut_at_progress(&self, cut_outline_progress: f64) -> (MeasuredCubic, MeasuredCubic) {
        let bounded = cut_outline_progress.clamp(self.start_outline_progress, self.end_outline_progress);
        let progress_size = self.end_outline_progress - self.start_outline_progress;
        // Пустые кривые отфильтрованы до любого разреза, деления на ноль нет.
        let relative_progress = (bounded - self.start_outline_progress) / progress_size;
        let t = find_cubic_cut_point(&self.cubic, relative_progress * self.measured_size).clamp(0.0, 1.0);
        let (before, after) = self.cubic.split(t);
        (MeasuredCubic::new(before, self.start_outline_progress, bounded), MeasuredCubic::new(after, bounded, self.end_outline_progress))
    }
}

/// Угол полигона, сведённый к нужному для сопоставления: где он на контуре, где в пространстве, куда вдавлен.
#[derive(Clone, Copy, Debug)]
struct ProgressableFeature {
    progress: f64,
    representative: Pt,
    kind: FeatureKind,
}

/// Точка-представитель черты: середина между первым и последним якорем.
fn feature_representative_point(feature: &Feature) -> Pt {
    let first = feature.cubics[0].anchor0();
    let last = feature.cubics[feature.cubics.len() - 1].anchor1();
    Pt::new((first.x + last.x) / 2.0, (first.y + last.y) / 2.0)
}

/// Квадрат расстояния между чертами или `None`, если их нельзя сопоставлять: внешняя вершина звезды
/// с внутренней вершиной другой выворачивает морф наизнанку.
fn feature_dist_squared(f1: &ProgressableFeature, f2: &ProgressableFeature) -> Option<f64> {
    if f1.kind != f2.kind {
        return None;
    }
    Some((f1.representative - f2.representative).hypot().powi(2))
}

/// Контур, нарезанный на кубики, каждая из которых знает своё место на нём, плюс прогресс каждого угла.
#[derive(Clone, Debug)]
struct MeasuredPolygon {
    cubics: Vec<MeasuredCubic>,
    features: Vec<ProgressableFeature>,
}

impl MeasuredPolygon {
    /// Измеряет контур полигона. Кривые берутся из черт, а не из плоского списка, потому что прогресс угла
    /// считается по кубикам его черты; оба описывают один замкнутый контур.
    fn measure(polygon: &RoundedPolygon) -> MeasuredPolygon {
        let mut cubics: Vec<Cubic> = Vec::new();
        // Угол представляет кубика в середине его серии: внутренняя дуга скруглённого угла.
        let mut corner_cubic_indices: Vec<(usize, &Feature)> = Vec::new();
        for feature in &polygon.features {
            let representative = feature.cubics.len() / 2;
            for (i, cubic) in feature.cubics.iter().enumerate() {
                if feature.kind != FeatureKind::Edge && i == representative {
                    corner_cubic_indices.push((cubics.len(), feature));
                }
                cubics.push(*cubic);
            }
        }

        let mut measures = vec![0.0; cubics.len() + 1];
        let mut total_measure = 0.0;
        for (i, cubic) in cubics.iter().enumerate() {
            total_measure += measure_cubic(cubic);
            measures[i + 1] = total_measure;
        }
        let outline_progress: Vec<f64> = measures.iter().map(|m| m / total_measure).collect();
        let features = corner_cubic_indices
            .iter()
            .map(|&(index, feature)| ProgressableFeature {
                progress: positive_modulo((outline_progress[index] + outline_progress[index + 1]) / 2.0, 1.0),
                representative: feature_representative_point(feature),
                kind: feature.kind,
            })
            .collect();
        MeasuredPolygon::assemble(features, &cubics, &outline_progress)
    }

    /// Собирает измеренный контур из кривых и границ прогресса между ними, выбрасывая пустые кривые.
    fn assemble(features: Vec<ProgressableFeature>, cubics: &[Cubic], outline_progress: &[f64]) -> MeasuredPolygon {
        let mut measured: Vec<MeasuredCubic> = Vec::with_capacity(cubics.len());
        let mut start = 0.0;
        for (i, cubic) in cubics.iter().enumerate() {
            if outline_progress[i + 1] - outline_progress[i] > DISTANCE_EPSILON {
                measured.push(MeasuredCubic::new(*cubic, start, outline_progress[i + 1]));
                // Следующая начинается ровно там, где кончилась эта: закрываем щели от выброшенных кривых.
                start = outline_progress[i + 1];
            }
        }
        // Хвостовые кривые могли выпасть, а контур обязан кончаться на 1.
        if let Some(last) = measured.last_mut() {
            last.end_outline_progress = 1.0;
        }
        MeasuredPolygon { cubics: measured, features }
    }

    /// Тот же контур, перерезанный так, чтобы начинаться в `cutting_point`.
    fn cut_and_shift(self, cutting_point: f64) -> MeasuredPolygon {
        if cutting_point < DISTANCE_EPSILON {
            return self;
        }
        let Some(target_index) =
            self.cubics.iter().position(|c| cutting_point >= c.start_outline_progress && cutting_point <= c.end_outline_progress)
        else {
            return self;
        };
        let (before, after) = self.cubics[target_index].cut_at_progress(cutting_point);

        // Часть целевой кривой после разреза, затем всё от неё по кругу до целевой, затем часть до разреза.
        let count = self.cubics.len();
        let mut cubics = Vec::with_capacity(count + 2);
        cubics.push(after.cubic);
        for i in 1..count {
            cubics.push(self.cubics[(i + target_index) % count].cubic);
        }
        cubics.push(before.cubic);

        // Каждая граница сдвигается назад на разрез с заворотом; концы это сам разрез.
        let mut outline_progress = Vec::with_capacity(cubics.len() + 1);
        for i in 0..=cubics.len() {
            if i == 0 {
                outline_progress.push(0.0);
            } else if i == cubics.len() {
                outline_progress.push(1.0);
            } else {
                let index = (target_index + i - 1) % count;
                outline_progress.push(positive_modulo(self.cubics[index].end_outline_progress - cutting_point, 1.0));
            }
        }
        let features = self
            .features
            .iter()
            .map(|f| ProgressableFeature { progress: positive_modulo(f.progress - cutting_point, 1.0), ..*f })
            .collect();
        MeasuredPolygon::assemble(features, &cubics, &outline_progress)
    }

    fn get(&self, index: usize) -> Option<MeasuredCubic> {
        self.cubics.get(index).copied()
    }
}

/// Отображение прогресса между двумя фигурами, построенное только по их углам.
fn feature_mapper(features1: &[ProgressableFeature], features2: &[ProgressableFeature]) -> DoubleMapper {
    let corners = |features: &[ProgressableFeature]| -> Vec<ProgressableFeature> {
        features.iter().copied().filter(|f| f.kind != FeatureKind::Edge).collect()
    };
    DoubleMapper::new(&do_mapping(&corners(features1), &corners(features2)))
}

/// Пары углов двух фигур (прогресс начала, прогресс конца), отсортированные по первому элементу.
/// Все возможные пары оцениваются по расстоянию и берутся по возрастанию; пропускаются те, что трогают
/// уже сопоставленный угол, стоят ближе `DISTANCE_EPSILON` к существующей паре или пересекают её.
fn do_mapping(features1: &[ProgressableFeature], features2: &[ProgressableFeature]) -> Vec<(f64, f64)> {
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for (i1, f1) in features1.iter().enumerate() {
        for (i2, f2) in features2.iter().enumerate() {
            if let Some(distance) = feature_dist_squared(f1, f2) {
                candidates.push((distance, i1, i2));
            }
        }
    }
    // Сортировка устойчивая: при равных расстояниях порядок перебора сохраняется (у Dart она неустойчива).
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));

    // Опереться не на что: прогресс переносится как есть.
    let Some(&(_, first1, first2)) = candidates.first() else {
        return vec![(0.0, 0.0), (0.5, 0.5)];
    };
    // Одна пара задаёт сдвиг, но не масштаб: масштаб фиксирует противоположная точка.
    if candidates.len() == 1 {
        let (p1, p2) = (features1[first1].progress, features2[first2].progress);
        return vec![(p1, p2), ((p1 + 0.5) % 1.0, (p2 + 0.5) % 1.0)];
    }

    let mut helper = MappingHelper { mapping: Vec::new(), used1: vec![false; features1.len()], used2: vec![false; features2.len()] };
    for &(_, i1, i2) in &candidates {
        helper.add(features1[i1].progress, features2[i2].progress, i1, i2);
    }
    helper.mapping
}

/// Копит пары углов, держа список отсортированным по прогрессу начала и без пересечений.
struct MappingHelper {
    mapping: Vec<(f64, f64)>,
    used1: Vec<bool>,
    used2: Vec<bool>,
}

impl MappingHelper {
    fn add(&mut self, p1: f64, p2: f64, i1: usize, i2: usize) {
        // Угол сопоставляется один раз.
        if self.used1[i1] || self.used2[i2] {
            return;
        }
        let insertion = self.mapping.partition_point(|m| m.0 < p1);
        if self.mapping.get(insertion).is_some_and(|m| m.0 == p1) {
            return;
        }
        let n = self.mapping.len();
        if n >= 1 {
            let (before1, before2) = self.mapping[(insertion + n - 1) % n];
            let (after1, after2) = self.mapping[insertion % n];
            // Слишком тесные пары делают отображение неустойчивым.
            if progress_distance(p1, before1) < DISTANCE_EPSILON
                || progress_distance(p1, after1) < DISTANCE_EPSILON
                || progress_distance(p2, before2) < DISTANCE_EPSILON
                || progress_distance(p2, after2) < DISTANCE_EPSILON
            {
                return;
            }
            // При двух и более готовых парах новая должна лечь между соседями и на целевой стороне,
            // иначе контуры пересекутся в ходе морфа.
            if n > 1 && !progress_in_range(p2, before2, after2) {
                return;
            }
        }
        self.mapping.insert(insertion, (p1, p2));
        self.used1[i1] = true;
        self.used2[i2] = true;
    }
}

/// Интерполяция между двумя полигонами: при прогрессе 0 первая фигура, при 1 вторая.
/// Строится дорого (измерение, сопоставление, разрезы), поэтому один раз и надолго; за кадр
/// вызывается только `cubics_at`, которая лишь интерполирует готовые пары.
pub(super) struct Morph {
    /// Контуры, разрезанные на попарно соответствующие кривые; кадр это интерполяция каждой пары одним прогрессом.
    pairs: Vec<(Cubic, Cubic)>,
}

impl Morph {
    pub(super) fn new(start: &RoundedPolygon, end: &RoundedPolygon) -> Morph {
        Morph { pairs: Morph::match_polygons(start, end) }
    }

    /// Сшивает два контура кривая в кривую, разрезая там, где они расходятся.
    fn match_polygons(p1: &RoundedPolygon, p2: &RoundedPolygon) -> Vec<(Cubic, Cubic)> {
        let measured1 = MeasuredPolygon::measure(p1);
        let measured2 = MeasuredPolygon::measure(p2);
        let mapper = feature_mapper(&measured1.features, &measured2.features);

        // Прогресс 0 первой фигуры лежит в этой точке второй: с неё вторая и начинается.
        let cut_point = mapper.map(0.0);
        let bs1 = measured1;
        let bs2 = measured2.cut_and_shift(cut_point);

        let mut pairs: Vec<(Cubic, Cubic)> = Vec::new();
        // Ждущая сопоставления кривая каждой фигуры и индекс той, что придёт за ней.
        let mut curve1 = bs1.get(0);
        let mut curve2 = bs2.get(0);
        let mut i1 = 1;
        let mut i2 = 1;
        while let (Some(b1), Some(b2)) = (curve1, curve2) {
            // Где кончается каждая кривая в пространстве прогресса ПЕРВОЙ фигуры. Последняя кривая
            // кончается на 1 по определению, что бы ни говорило округление.
            let b1_end = if i1 == bs1.cubics.len() { 1.0 } else { b1.end_outline_progress };
            let b2_end =
                if i2 == bs2.cubics.len() { 1.0 } else { mapper.map_back(positive_modulo(b2.end_outline_progress + cut_point, 1.0)) };
            // Граница там, где кончается первая; перебежавшая кривая режется, остаток встретится в следующем круге.
            let boundary = b1_end.min(b2_end);

            let (segment1, remainder1) = if b1_end > boundary + ANGLE_EPSILON {
                let (cut, rest) = b1.cut_at_progress(boundary);
                (cut, Some(rest))
            } else {
                let next = bs1.get(i1);
                i1 += 1;
                (b1, next)
            };
            let (segment2, remainder2) = if b2_end > boundary + ANGLE_EPSILON {
                let (cut, rest) = b2.cut_at_progress(positive_modulo(mapper.map(boundary) - cut_point, 1.0));
                (cut, Some(rest))
            } else {
                let next = bs2.get(i2);
                i2 += 1;
                (b2, next)
            };
            pairs.push((segment1.cubic, segment2.cubic));
            curve1 = remainder1;
            curve2 = remainder2;
        }
        pairs
    }

    /// Контур морфа при прогрессе `progress` списком кривых. Прогресс не зажимается: пружина может
    /// слегка выходить за 0..1. Последняя кривая пересобирается так, чтобы кончаться ровно в начале
    /// первой: щель в доли пикселя при заливке видна как шов.
    pub(super) fn cubics_at(&self, progress: f64) -> Vec<Cubic> {
        let Some((first_start, first_end)) = self.pairs.first() else {
            return Vec::new();
        };
        let first_anchor = interpolate(first_start, first_end, progress).anchor0();
        let last = self.pairs.len() - 1;
        self.pairs
            .iter()
            .enumerate()
            .map(|(i, (start, end))| {
                let cubic = interpolate(start, end, progress);
                if i == last { Cubic::new(cubic.anchor0(), cubic.control0(), cubic.control1(), first_anchor) } else { cubic }
            })
            .collect()
    }
}

/// Поэлементная интерполяция двух кривых в плоской раскладке. Форма `a*(1-t) + b*t` даёт точные концы:
/// при 0 все члены второй кривой умножены на ноль, при 1 все члены первой.
fn interpolate(start: &Cubic, end: &Cubic, progress: f64) -> Cubic {
    let mut p = [0.0_f64; 8];
    for (i, v) in p.iter_mut().enumerate() {
        *v = lerp(start.p[i], end.p[i], progress);
    }
    Cubic { p }
}

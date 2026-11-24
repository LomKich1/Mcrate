//! Геометрия `RoundedPolygon`: замкнутый контур из кубических кривых Безье со скруглёнными углами.
//!
//! Перенос из `frust-material` (`shapes/rounded_polygon.rs`), тот в свою очередь порт
//! `material_new_shapes` (Dart, MIT) и AndroidX `graphics-shapes` (Apache-2.0). Формулы, порядок
//! операций и константы те же, чтобы совпадали числа (сверено с эталонными числами upstream, см. тесты).
//! Всё считается в `f64` в собственной системе координат (не экранной), в `f32` и пиксели
//! контур переводится только при рисовании (`mod.rs`). Внешних зависимостей (`kurbo`) нет.
//!
//! Портирована только та часть, что нужна индикатору: конструкторы `from_vertices`, `circle`, `star`,
//! `custom_polygon` (с повтором и зеркалом), `normalized`, границы. Без `pill`, `rectangle`, `pill_star`.

use std::f64::consts::PI;
use std::ops::{Add, Div, Mul, Sub};

/// Расстояние, меньше которого две точки считаются одной (`distanceEpsilon` в upstream).
pub(super) const DISTANCE_EPSILON: f64 = 1e-5;

/// Синус угла в вершине, ниже которого стороны считаются лежащими на одной прямой
/// и вершина не скругляется (inline `1e-3` в `_RoundedCorner` upstream).
const COLLINEAR_SIN_EPSILON: f64 = 1e-3;

/// Точка или вектор на плоскости (`f64`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pt {
    pub(super) x: f64,
    pub(super) y: f64,
}

impl Pt {
    pub(super) const ZERO: Pt = Pt { x: 0.0, y: 0.0 };

    pub(super) const fn new(x: f64, y: f64) -> Pt {
        Pt { x, y }
    }

    /// Длина вектора.
    pub(super) fn hypot(self) -> f64 {
        self.x.hypot(self.y)
    }

    fn dot(self, o: Pt) -> f64 {
        self.x * o.x + self.y * o.y
    }

    fn cross(self, o: Pt) -> f64 {
        self.x * o.y - self.y * o.x
    }

    /// Вектор, повёрнутый на четверть оборота против часовой стрелки.
    fn rot90(self) -> Pt {
        Pt::new(-self.y, self.x)
    }

    /// Единичный вектор того же направления (нулевой вектор направления не имеет).
    fn direction(self) -> Pt {
        self / self.hypot()
    }
}

impl Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f64> for Pt {
    type Output = Pt;
    fn mul(self, k: f64) -> Pt {
        Pt::new(self.x * k, self.y * k)
    }
}

impl Div<f64> for Pt {
    type Output = Pt;
    fn div(self, k: f64) -> Pt {
        Pt::new(self.x / k, self.y / k)
    }
}

/// Линейная интерполяция именно в форме upstream (`a*(1-f) + b*f`, не `a + (b-a)*f`):
/// форма меняет округление, а числа должны совпадать с эталоном.
pub(super) fn lerp(a: f64, b: f64, f: f64) -> f64 {
    a * (1.0 - f) + b * f
}

fn lerp_pt(a: Pt, b: Pt, f: f64) -> Pt {
    Pt::new(lerp(a.x, b.x, f), lerp(a.y, b.y, f))
}

/// Точка на расстоянии `radius` от начала координат под углом `angle` (радианы).
fn radial(radius: f64, angle: f64) -> Pt {
    Pt::new(angle.cos(), angle.sin()) * radius
}

/// Выпуклый ли угол в `current` (знак векторного произведения; для вырожденных данных ненадёжен, как и в upstream).
fn convex(previous: Pt, current: Pt, next: Pt) -> bool {
    (current - previous).cross(next - current) > 0.0
}

/// Скругление одной вершины: радиус и «сглаживание» (0 = круговая дуга, 1 = дуга растянута до краёв).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CornerRounding {
    pub(super) radius: f64,
    pub(super) smoothing: f64,
}

impl CornerRounding {
    /// Острый угол: сама вершина, без дуги.
    pub(super) const UNROUNDED: CornerRounding = CornerRounding { radius: 0.0, smoothing: 0.0 };

    /// Круговое скругление радиуса `radius` без сглаживания.
    pub(super) const fn radius(radius: f64) -> CornerRounding {
        CornerRounding { radius, smoothing: 0.0 }
    }
}

/// Одна кубическая кривая Безье в плоской раскладке `[a0x, a0y, c0x, c0y, c1x, c1y, a1x, a1y]`
/// (так же хранит upstream: поэлементная интерполяция морфа работает прямо по этому массиву).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Cubic {
    pub(super) p: [f64; 8],
}

impl Cubic {
    pub(super) const fn new(a0: Pt, c0: Pt, c1: Pt, a1: Pt) -> Cubic {
        Cubic { p: [a0.x, a0.y, c0.x, c0.y, c1.x, c1.y, a1.x, a1.y] }
    }

    /// Прямая между якорями, контрольные точки на трети и двух третях.
    pub(super) fn straight_line(a: Pt, b: Pt) -> Cubic {
        Cubic::new(a, lerp_pt(a, b, 1.0 / 3.0), lerp_pt(a, b, 2.0 / 3.0), b)
    }

    /// Кубика, приближающая кратчайшую дугу окружности с центром `center` от `p0` до `p1`.
    /// Концы должны быть на одном расстоянии от центра; почти совпавшие концы дают прямую.
    pub(super) fn circular_arc(center: Pt, p0: Pt, p1: Pt) -> Cubic {
        let p0d = (p0 - center).direction();
        let p1d = (p1 - center).direction();
        let rotated_p0 = p0d.rot90();
        let rotated_p1 = p1d.rot90();
        let clockwise = rotated_p0.dot(p1 - center) >= 0.0;
        let cosa = p0d.dot(p1d);
        if cosa > 0.999 {
            return Cubic::straight_line(p0, p1);
        }
        let k = (p0 - center).hypot() * 4.0 / 3.0 * ((2.0 * (1.0 - cosa)).sqrt() - (1.0 - cosa * cosa).sqrt()) / (1.0 - cosa)
            * if clockwise { 1.0 } else { -1.0 };
        Cubic::new(p0, p0 + rotated_p0 * k, p1 - rotated_p1 * k, p1)
    }

    pub(super) fn anchor0(&self) -> Pt {
        Pt::new(self.p[0], self.p[1])
    }

    pub(super) fn control0(&self) -> Pt {
        Pt::new(self.p[2], self.p[3])
    }

    pub(super) fn control1(&self) -> Pt {
        Pt::new(self.p[4], self.p[5])
    }

    pub(super) fn anchor1(&self) -> Pt {
        Pt::new(self.p[6], self.p[7])
    }

    /// Точка кривой в параметре `t` (0 = первый якорь, 1 = второй).
    pub(super) fn point_on_curve(&self, t: f64) -> Pt {
        let u = 1.0 - t;
        let at = |a: f64, c0: f64, c1: f64, b: f64| a * (u * u * u) + c0 * (3.0 * t * u * u) + c1 * (3.0 * t * t * u) + b * (t * t * t);
        Pt::new(at(self.p[0], self.p[2], self.p[4], self.p[6]), at(self.p[1], self.p[3], self.p[5], self.p[7]))
    }

    /// Оба якоря в одной точке (с точностью `DISTANCE_EPSILON`): такая кривая ничего не рисует.
    pub(super) fn zero_length(&self) -> bool {
        (self.p[0] - self.p[6]).abs() < DISTANCE_EPSILON && (self.p[1] - self.p[7]).abs() < DISTANCE_EPSILON
    }

    /// Делит кривую в параметре `t` на две, покрывающие тот же путь.
    pub(super) fn split(&self, t: f64) -> (Cubic, Cubic) {
        let u = 1.0 - t;
        let point = self.point_on_curve(t);
        let (a0, c0, c1, a1) = (self.anchor0(), self.control0(), self.control1(), self.anchor1());
        (
            Cubic::new(
                a0,
                Pt::new(a0.x * u + c0.x * t, a0.y * u + c0.y * t),
                Pt::new(a0.x * (u * u) + c0.x * (2.0 * u * t) + c1.x * (t * t), a0.y * (u * u) + c0.y * (2.0 * u * t) + c1.y * (t * t)),
                point,
            ),
            Cubic::new(
                point,
                Pt::new(c0.x * (u * u) + c1.x * (2.0 * u * t) + a1.x * (t * t), c0.y * (u * u) + c1.y * (2.0 * u * t) + a1.y * (t * t)),
                Pt::new(c1.x * u + a1.x * t, c1.y * u + a1.y * t),
                a1,
            ),
        )
    }

    /// Та же кривая в обратном направлении.
    pub(super) fn reversed(&self) -> Cubic {
        Cubic::new(self.anchor1(), self.control1(), self.control0(), self.anchor0())
    }

    /// Кривая с каждым якорем и контрольной точкой, пропущенными через `f`.
    pub(super) fn transformed_with(&self, f: impl Fn(Pt) -> Pt) -> Cubic {
        Cubic::new(f(self.anchor0()), f(self.control0()), f(self.control1()), f(self.anchor1()))
    }

    /// Рамка по четырём опорным точкам (дёшево, не меньше точной). Вид: `(min_x, min_y, max_x, max_y)`.
    pub(super) fn approximate_bounds(&self) -> [f64; 4] {
        let xs = [self.p[0], self.p[2], self.p[4], self.p[6]];
        let ys = [self.p[1], self.p[3], self.p[5], self.p[7]];
        [
            xs.iter().copied().fold(f64::INFINITY, f64::min),
            ys.iter().copied().fold(f64::INFINITY, f64::min),
            xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        ]
    }

    /// Точная рамка кривой: экстремумы по каждой оси ищутся из корней производной (квадратного уравнения).
    pub(super) fn exact_bounds(&self) -> [f64; 4] {
        if self.zero_length() {
            let p = self.anchor0();
            return [p.x, p.y, p.x, p.y];
        }
        let (a0, a1) = (self.anchor0(), self.anchor1());
        let mut min = [a0.x.min(a1.x), a0.y.min(a1.y)];
        let mut max = [a0.x.max(a1.x), a0.y.max(a1.y)];
        for axis in 0..2 {
            let (p0, p1, p2, p3) = (self.p[axis], self.p[2 + axis], self.p[4 + axis], self.p[6 + axis]);
            let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
            let b = 2.0 * p0 - 4.0 * p1 + 2.0 * p2;
            let c = -p0 + p1;
            let mut roots = [None, None];
            if a.abs() < DISTANCE_EPSILON {
                // При a ≈ 0 корень один (метод Мюллера из upstream).
                if b != 0.0 {
                    roots[0] = Some(2.0 * c / (-2.0 * b));
                }
            } else {
                let d = b * b - 4.0 * a * c;
                if d >= 0.0 {
                    roots[0] = Some((-b + d.sqrt()) / (2.0 * a));
                    roots[1] = Some((-b - d.sqrt()) / (2.0 * a));
                }
            }
            for t in roots.into_iter().flatten() {
                if !(0.0..=1.0).contains(&t) {
                    continue;
                }
                let q = self.point_on_curve(t);
                let v = if axis == 0 { q.x } else { q.y };
                min[axis] = min[axis].min(v);
                max[axis] = max[axis].max(v);
            }
        }
        [min[0], min[1], max[0], max[1]]
    }
}

/// Чем является черта контура. Морфинг сопоставляет только углы (выпуклый с выпуклым, вогнутый с вогнутым).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FeatureKind {
    /// Отрезок между углами, сам по себе не вдавлен; в сопоставлении не участвует.
    Edge,
    /// Угол наружу.
    ConvexCorner,
    /// Угол внутрь (внутренние вершины звезды).
    ConcaveCorner,
}

/// Подряд идущие кривые, собранные в одну смысловую единицу контура (угол или ребро).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Feature {
    pub(super) cubics: Vec<Cubic>,
    pub(super) kind: FeatureKind,
}

impl Feature {
    fn transformed_with(&self, f: impl Fn(Pt) -> Pt + Copy) -> Feature {
        Feature { cubics: self.cubics.iter().map(|c| c.transformed_with(f)).collect(), kind: self.kind }
    }
}

/// Замкнутый контур с (необязательно) скруглёнными углами: список черт и тот же контур плоским списком кубик.
/// Координаты не экранные: полигон радиуса 1 живёт в коробке (-1,-1)..(1,1), перед рисованием его
/// нужно нормализовать (`normalized`) и промасштабировать.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RoundedPolygon {
    pub(super) features: Vec<Feature>,
    pub(super) center: Pt,
    pub(super) cubics: Vec<Cubic>,
}

impl RoundedPolygon {
    /// Полигон из черт, уже образующих замкнутый контур. Без `center` центр это среднее начальных якорей.
    fn from_features(features: Vec<Feature>, center: Option<Pt>) -> RoundedPolygon {
        let center = center.unwrap_or_else(|| {
            let anchors: Vec<Pt> = features.iter().flat_map(|f| f.cubics.iter().map(Cubic::anchor0)).collect();
            average(&anchors)
        });
        let cubics = init_cubics(&features, center);
        RoundedPolygon { features, center, cubics }
    }

    /// Полигон по упорядоченным вершинам; `per_vertex` (если задан) заменяет `rounding` для каждой вершины.
    /// Скругления соседних углов делят одну сторону: сначала им отдаётся круговая часть, остаток идёт на сглаживание.
    /// Функция линейная (без ветвления по смыслу), поэтому длиннее 60 строк: резать её на куски значит разорвать
    /// два прохода по сторонам, которые тут идут подряд.
    pub(super) fn from_vertices(
        vertices: &[Pt],
        rounding: CornerRounding,
        per_vertex: Option<&[CornerRounding]>,
        center: Option<Pt>,
    ) -> RoundedPolygon {
        let n = vertices.len();
        let corners: Vec<RoundedCorner> = (0..n)
            .map(|i| {
                let r = per_vertex.map_or(rounding, |pv| pv[i]);
                RoundedCorner::new(vertices[(i + n - 1) % n], vertices[i], vertices[(i + 1) % n], r)
            })
            .collect();

        // Для стороны от угла i к i+1: (какая доля круговой части влезает, какая доля сглаживания влезает).
        let cut_adjusts: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let next = (i + 1) % n;
                let expected_round_cut = corners[i].expected_round_cut + corners[next].expected_round_cut;
                let expected_cut = corners[i].expected_cut() + corners[next].expected_cut();
                let side_size = (vertices[i] - vertices[next]).hypot();
                if expected_round_cut > side_size {
                    (side_size / expected_round_cut, 0.0)
                } else if expected_cut > side_size {
                    (1.0, (side_size - expected_round_cut) / (expected_cut - expected_round_cut))
                } else {
                    (1.0, 1.0)
                }
            })
            .collect();

        let corner_cubics: Vec<Vec<Cubic>> = (0..n)
            .map(|i| {
                // allowed[0] относится к стороне от предыдущего угла, allowed[1] к стороне к следующему.
                let mut allowed = [0.0_f64; 2];
                for (delta, cut) in allowed.iter_mut().enumerate() {
                    let (round_ratio, cut_ratio) = cut_adjusts[(i + n - 1 + delta) % n];
                    *cut = corners[i].expected_round_cut * round_ratio
                        + (corners[i].expected_cut() - corners[i].expected_round_cut) * cut_ratio;
                }
                corners[i].cubics(allowed[0], allowed[1])
            })
            .collect();

        // Кривые углов и прямые рёбра между ними, в порядке обхода контура.
        let mut features = Vec::with_capacity(2 * n);
        for i in 0..n {
            let next = (i + 1) % n;
            let cvx = convex(vertices[(i + n - 1) % n], vertices[i], vertices[next]);
            let edge = Cubic::straight_line(corner_cubics[i][corner_cubics[i].len() - 1].anchor1(), corner_cubics[next][0].anchor0());
            let kind = if cvx { FeatureKind::ConvexCorner } else { FeatureKind::ConcaveCorner };
            features.push(Feature { cubics: corner_cubics[i].clone(), kind });
            features.push(Feature { cubics: vec![edge], kind: FeatureKind::Edge });
        }
        let center = center.unwrap_or_else(|| average(vertices));
        RoundedPolygon::from_features(features, Some(center))
    }

    /// Правильный многоугольник: `n` вершин на окружности радиуса `radius` вокруг `center`.
    pub(super) fn from_num_vertices(
        n: usize,
        radius: f64,
        center: Pt,
        rounding: CornerRounding,
        per_vertex: Option<&[CornerRounding]>,
    ) -> RoundedPolygon {
        let vertices: Vec<Pt> = (0..n).map(|i| center + radial(radius, PI / n as f64 * 2.0 * i as f64)).collect();
        RoundedPolygon::from_vertices(&vertices, rounding, per_vertex, Some(center))
    }

    /// Окружность радиуса `radius`: `n`-угольник, у которого скругление съедает каждую сторону.
    pub(super) fn circle(n: usize, radius: f64, center: Pt) -> RoundedPolygon {
        // Радиус описанного многоугольника больше радиуса круга на косинус половины угла между вершинами.
        let polygon_radius = radius / (PI / n as f64).cos();
        RoundedPolygon::from_num_vertices(n, polygon_radius, center, CornerRounding::radius(radius), None)
    }

    /// Звезда: вершины чередуются между внешним и внутренним радиусом.
    pub(super) fn star(per_radius: usize, radius: f64, inner_radius: f64, rounding: CornerRounding, center: Pt) -> RoundedPolygon {
        let n = per_radius as f64;
        let mut vertices = Vec::with_capacity(per_radius * 2);
        for i in 0..per_radius {
            vertices.push(center + radial(radius, PI / n * 2.0 * i as f64));
            vertices.push(center + radial(inner_radius, PI / n * (2.0 * i as f64 + 1.0)));
        }
        RoundedPolygon::from_vertices(&vertices, rounding, None, Some(center))
    }

    /// Полигон с каждой точкой, пропущенной через `f`.
    pub(super) fn transformed_with(&self, f: impl Fn(Pt) -> Pt + Copy) -> RoundedPolygon {
        RoundedPolygon::from_features(self.features.iter().map(|feat| feat.transformed_with(f)).collect(), Some(f(self.center)))
    }

    /// Полигон, сдвинутый и промасштабированный в коробку (0,0)..(1,1); по короткой оси центрируется.
    pub(super) fn normalized(&self) -> RoundedPolygon {
        let [x0, y0, x1, y1] = self.bounds();
        let (width, height) = (x1 - x0, y1 - y0);
        let side = width.max(height);
        let offset_x = (side - width) / 2.0 - x0;
        let offset_y = (side - height) / 2.0 - y0;
        self.transformed_with(|p| Pt::new((p.x + offset_x) / side, (p.y + offset_y) / side))
    }

    /// Рамка по всем якорям и контрольным точкам (дёшево, не уже точной). Вид: `[min_x, min_y, max_x, max_y]`.
    pub(super) fn bounds(&self) -> [f64; 4] {
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for cubic in &self.cubics {
            let c = cubic.approximate_bounds();
            b = [b[0].min(c[0]), b[1].min(c[1]), b[2].max(c[2]), b[3].max(c[3])];
        }
        b
    }

    /// Наименьший квадрат с центром в `center`, вмещающий фигуру при любом повороте (по расстоянию
    /// от центра до начала и середины каждой кривой, то есть приближённо, как в upstream).
    pub(super) fn max_bounds(&self) -> [f64; 4] {
        let mut max_dist_sq = 0.0_f64;
        for cubic in &self.cubics {
            let anchor = (cubic.anchor0() - self.center).hypot().powi(2);
            let middle = (cubic.point_on_curve(0.5) - self.center).hypot().powi(2);
            max_dist_sq = max_dist_sq.max(anchor.max(middle));
        }
        let d = max_dist_sq.sqrt();
        [self.center.x - d, self.center.y - d, self.center.x + d, self.center.y + d]
    }
}

/// Среднее арифметическое точек.
fn average(points: &[Pt]) -> Pt {
    let sum = points.iter().fold(Pt::ZERO, |acc, p| acc + *p);
    sum / points.len() as f64
}

/// Склеивает черты в плоский замкнутый список кубик.
/// Кривые нулевой длины выбрасываются (они ничего не рисуют и дают артефакты), но их конечный якорь
/// переносится в предыдущую кривую: иначе цепочка почти нулевых кривых накопит видимый разрыв.
/// Если первая черта это угол из трёх кривых, контур начинается с середины её средней кривой: так
/// замыкающую кривую можно пересобрать, чтобы она попала ТОЧНО в начальный якорь (щель в доли пикселя
/// при заливке видна как шов).
fn init_cubics(features: &[Feature], center: Pt) -> Vec<Cubic> {
    let mut cubics: Vec<Cubic> = Vec::new();
    let mut first_cubic: Option<Cubic> = None;
    let mut last_cubic: Option<Cubic> = None;
    let mut split_start: Option<[Cubic; 2]> = None;
    let mut split_end: Option<[Cubic; 2]> = None;

    if let Some(first) = features.first()
        && first.cubics.len() == 3
    {
        let (start, end) = first.cubics[1].split(0.5);
        split_start = Some([first.cubics[0], start]);
        split_end = Some([end, first.cubics[2]]);
    }

    // На один шаг больше числа черт: в конце дописывается первая половина разрезанной первой черты.
    for i in 0..=features.len() {
        let list: &[Cubic] = if i == features.len() {
            match split_start.as_ref() {
                Some(s) => &s[..],
                None => break,
            }
        } else if i == 0 {
            match split_end.as_ref() {
                Some(s) => &s[..],
                None => &features[0].cubics[..],
            }
        } else {
            &features[i].cubics[..]
        };

        for cubic in list {
            if cubic.zero_length() {
                if let Some(last) = last_cubic.as_mut() {
                    last.p[6] = cubic.p[6];
                    last.p[7] = cubic.p[7];
                }
            } else {
                if let Some(last) = last_cubic {
                    cubics.push(last);
                }
                last_cubic = Some(*cubic);
                first_cubic.get_or_insert(*cubic);
            }
        }
    }

    match (last_cubic, first_cubic) {
        (Some(last), Some(first)) => cubics.push(Cubic::new(last.anchor0(), last.control0(), last.control1(), first.anchor0())),
        // Пустой полигон: одна вырожденная кривая в центре.
        _ => cubics.push(Cubic::new(center, center, center, center)),
    }
    cubics
}

/// Состояние одной вершины между двумя проходами: сначала угол говорит, сколько хочет срезать
/// с каждой стороны, потом ему сообщают, сколько разрешено (соседи делят сторону), и он строит кривые.
struct RoundedCorner {
    p0: Pt,
    p1: Pt,
    p2: Pt,
    /// Единичный вектор от `p1` к `p0`.
    d1: Pt,
    /// Единичный вектор от `p1` к `p2`.
    d2: Pt,
    corner_radius: f64,
    smoothing: f64,
    /// Сколько вдоль каждой стороны хочет срезать одно только скругление.
    expected_round_cut: f64,
}

impl RoundedCorner {
    fn new(p0: Pt, p1: Pt, p2: Pt, rounding: CornerRounding) -> RoundedCorner {
        let v01 = p0 - p1;
        let v21 = p2 - p1;
        let (d01, d21) = (v01.hypot(), v21.hypot());
        if d01 <= 0.0 || d21 <= 0.0 {
            // Сторона нулевой длины: скруглять нечего.
            return RoundedCorner { p0, p1, p2, d1: Pt::ZERO, d2: Pt::ZERO, corner_radius: 0.0, smoothing: 0.0, expected_round_cut: 0.0 };
        }
        let d1 = v01 / d01;
        let d2 = v21 / d21;
        // cos угла в p1 это скалярное произведение единичных векторов, sin из sin² + cos² = 1;
        // длина среза из tan(A/2) = sinA / (1 + cosA) = radius / cut.
        let cos_angle = d1.dot(d2);
        let sin_angle = (1.0 - cos_angle * cos_angle).sqrt();
        let expected_round_cut = if sin_angle > COLLINEAR_SIN_EPSILON { rounding.radius * (cos_angle + 1.0) / sin_angle } else { 0.0 };
        RoundedCorner { p0, p1, p2, d1, d2, corner_radius: rounding.radius, smoothing: rounding.smoothing, expected_round_cut }
    }

    /// Полный желаемый срез: сглаживание растягивает его (0 оставляет круговой, 1 удваивает).
    fn expected_cut(&self) -> f64 {
        (1.0 + self.smoothing) * self.expected_round_cut
    }

    /// Кривые угла при разрешённых срезах `allowed_cut0`/`allowed_cut1` по двум сторонам:
    /// фланговая кривая, дуга, фланговая кривая (или одна нулевая кривая в самой вершине).
    fn cubics(&self, allowed_cut0: f64, allowed_cut1: f64) -> Vec<Cubic> {
        let allowed_cut = allowed_cut0.min(allowed_cut1);
        if self.expected_round_cut < DISTANCE_EPSILON || allowed_cut < DISTANCE_EPSILON || self.corner_radius < DISTANCE_EPSILON {
            return vec![Cubic::straight_line(self.p1, self.p1)];
        }
        let actual_round_cut = allowed_cut.min(self.expected_round_cut);
        // Скруглению место отдаётся первым, сглаживанию достаётся остаток.
        let smoothing0 = self.actual_smoothing(allowed_cut0);
        let smoothing1 = self.actual_smoothing(allowed_cut1);
        let actual_r = self.corner_radius * actual_round_cut / self.expected_round_cut;
        let center_distance = (actual_r * actual_r + actual_round_cut * actual_round_cut).sqrt();
        let center = self.p1 + ((self.d1 + self.d2) / 2.0).direction() * center_distance;
        let intersection0 = self.p1 + self.d1 * actual_round_cut;
        let intersection2 = self.p1 + self.d2 * actual_round_cut;

        let flanking0 = self.flanking_curve(actual_round_cut, smoothing0, self.p0, intersection0, intersection2, center, actual_r);
        let flanking2 =
            self.flanking_curve(actual_round_cut, smoothing1, self.p2, intersection2, intersection0, center, actual_r).reversed();
        vec![flanking0, Cubic::circular_arc(center, flanking0.anchor1(), flanking2.anchor0()), flanking2]
    }

    /// Сколько сглаживания уцелело при разрешённом срезе: всё, если места хватает; доля, если места
    /// чуть больше круговой части; ничего, если скругление уже заняло сторону.
    fn actual_smoothing(&self, allowed_cut: f64) -> f64 {
        let expected_cut = self.expected_cut();
        if allowed_cut > expected_cut {
            self.smoothing
        } else if allowed_cut > self.expected_round_cut {
            self.smoothing * (allowed_cut - self.expected_round_cut) / (expected_cut - self.expected_round_cut)
        } else {
            0.0
        }
    }

    /// Кривая от прямой стороны к дуге скругления; касается окружности, поэтому стык гладкий.
    /// Аргументов много, потому что это дословный `_computeFlankingCurve` upstream (минус два, что лежат в `self`).
    #[allow(clippy::too_many_arguments)]
    fn flanking_curve(
        &self,
        actual_round_cut: f64,
        actual_smoothing: f64,
        side_start: Pt,
        circle_intersection: Pt,
        other_circle_intersection: Pt,
        circle_center: Pt,
        actual_r: f64,
    ) -> Cubic {
        let side_direction = (side_start - self.p1).direction();
        let curve_start = self.p1 + side_direction * actual_round_cut * (1.0 + actual_smoothing);
        // Срезаем долю круговой части, пропорциональную (1 - smoothing): ничего при 0, всё при 1.
        let midpoint = lerp_pt(circle_intersection, other_circle_intersection, 0.5);
        let p = lerp_pt(circle_intersection, midpoint, actual_smoothing);
        let curve_end = circle_center + (p - circle_center).direction() * actual_r;
        // Якорь у окружности: где касательная в конце пересекает прямую сторону.
        let circle_tangent = (curve_end - circle_center).rot90();
        let anchor_end = line_intersection(side_start, side_direction, curve_end, circle_tangent).unwrap_or(circle_intersection);
        // Остаток: две трети пути к тому якорю (пропорция, к которой сходятся дизайн-инструменты).
        let anchor_start = (curve_start + anchor_end * 2.0) / 3.0;
        Cubic::new(curve_start, anchor_start, anchor_end, curve_end)
    }
}

/// Пересечение прямых `p0 + t*d0` и `p1 + t*d1`; `None`, если они почти параллельны.
fn line_intersection(p0: Pt, d0: Pt, p1: Pt, d1: Pt) -> Option<Pt> {
    let rotated_d1 = d1.rot90();
    let den = d0.dot(rotated_d1);
    if den.abs() < DISTANCE_EPSILON {
        return None;
    }
    let num = (p1 - p0).dot(rotated_d1);
    if den.abs() < DISTANCE_EPSILON * num.abs() {
        return None;
    }
    Some(p0 + d0 * (num / den))
}

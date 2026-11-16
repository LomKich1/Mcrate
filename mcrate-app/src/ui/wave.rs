//! «Моя волна»: три ленты из точек. Каждая лента — скрученная полоса: у неё есть линия-хребет,
//! ширина и угол поворота. Когда полоса повёрнута ребром к зрителю, точки сжимаются в яркую
//! складку (как на референсе); когда лицом — растекаются полупрозрачным полотном.
//!
//! Что волна «слышит» (всё из UI, без обращения к звуковому движку):
//!   • воспроизведение: пауза = спокойный медленный дрейф и тусклые точки; игра = быстрее, выше, ярче.
//!     Переход плавный, а нажатие ▶/⏸ пускает от центра волну-импульс;
//!   • смена трека: импульс от левого края и плавная «перелепка» формы лент;
//!   • позиция в треке: мягкий блик на ленте идёт слева направо вместе с прогрессом;
//!   • курсор: точки расступаются кругом («дыра»), движение мыши подталкивает их и будоражит волну;
//!   • клик/нажатие: от точки нажатия бежит импульс по лентам (они вздуваются) и расходится кольцо.
//! К громкости и спектру музыки волна пока НЕ привязана: спектр должен отдавать аудиодвижок
//! (дорожка A), а UI-задачи его не правят (см. CLAUDE.md, «Правила для дорожки B»).

use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use egui::{Color32, Mesh, Painter, Rect, Shape, Vec2, vec2};

use crate::theme::with_alpha;

pub const RIBBONS: usize = 3; // сколько лент
const DOTS_PER_COL: usize = 12; // точек на ленту в одной колонке
pub const COL_STEP_PX: f32 = 2.4; // шаг колонок по X, px
const MAX_PULSES: usize = 8; // одновременно живущих импульсов
const PULSE_TTL: f32 = 3.0; // время жизни импульса, с
const FRONT_SPEED: f32 = 520.0; // скорость фронта импульса вдоль лент, px/с
const RING_SPEED: f32 = 420.0; // скорость кольца от клика, px/с
const IDLE_SPEED: f32 = 0.12; // скорость фазы на паузе
const PLAY_SPEED: f32 = 0.62; // скорость фазы при воспроизведении

/// Что UI сообщает волне на этот кадр.
pub struct WaveInput<'a> {
    /// Время кадра (с). Повторный вызов с тем же временем (второй проход стекла) ничего не меняет.
    pub now: f64,
    pub playing: bool,
    /// Идентификатор текущего трека; смена запускает импульс и перелепку формы.
    pub track_id: Option<&'a str>,
    /// Позиция в треке 0..1 (None, если трека нет).
    pub progress: Option<f32>,
    /// «Уменьшить анимацию»: волна замирает, на курсор и клики не реагирует.
    pub reduce_motion: bool,
    /// Курсор в координатах холста (None, если он вне холста).
    pub pointer: Option<Vec2>,
    /// Кнопка мыши зажата, и нажатие началось на холсте.
    pub pressed: bool,
    /// Размер холста, px.
    pub size: Vec2,
}

/// Импульс: бежит по лентам от точки (x, y) и расходит кольцо.
#[derive(Clone, Copy)]
struct Pulse {
    x: f32,
    y: f32,
    age: f32,
    strength: f32,
}

/// Состояние волны между кадрами (хранится в памяти egui).
#[derive(Clone, Default)]
pub struct WaveSim {
    t_prev: f64,
    started: bool,
    /// Накопленная фаза. Именно накопленная (+= скорость * dt), а не «время * скорость»:
    /// иначе при смене скорости форма скачет.
    phase: f64,
    /// 0..1: насколько «играет» (плавно).
    play: f32,
    /// 0..1: затухающее возбуждение (клики, быстрая мышь, смена состояния).
    excite: f32,
    /// Сдвиг формы лент: плавно догоняет `morph_target`, который растёт при смене трека.
    morph: f32,
    morph_target: f32,
    track_key: u64,
    was_playing: bool,
    was_pressed: bool,
    has_progress: bool,
    progress: f32,
    hover: f32,
    cursor: Vec2,
    vel: Vec2,
    press: f32,
    pulses: Vec<Pulse>,
}

/// Хеш строки (FNV-1a): из id трека получаем число, чтобы заметить смену. 0 зарезервирован под «трека нет».
fn key_of(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h | 1
}

fn smooth(dt: f32, rate: f32) -> f32 {
    1.0 - (-dt * rate).exp() // доля пути до цели за dt при экспоненциальном сглаживании
}

impl WaveSim {
    fn push_pulse(&mut self, x: f32, y: f32, strength: f32) {
        if self.pulses.len() >= MAX_PULSES {
            self.pulses.remove(0);
        }
        self.pulses.push(Pulse { x, y, age: 0.0, strength });
    }

    /// Нужна ли частая перерисовка (60 к/с). В покое хватает ~30 к/с дрейфа.
    pub fn busy(&self) -> bool {
        self.play > 0.02 || self.excite > 0.01 || self.hover > 0.002 || self.press > 0.002 || !self.pulses.is_empty()
    }

    /// Один шаг симуляции.
    pub fn step(&mut self, inp: &WaveInput) {
        if inp.reduce_motion {
            // Статичная картинка: без дрейфа, импульсов и реакции на курсор.
            *self = WaveSim { play: if inp.playing { 0.8 } else { 0.4 }, ..WaveSim::default() };
            return;
        }
        let dt = ((inp.now - self.t_prev) as f32).clamp(0.0, 0.05);
        if !self.started {
            // Первый кадр: запоминаем исходное состояние, чтобы не пускать лишний импульс.
            self.started = true;
            self.t_prev = inp.now;
            self.was_playing = inp.playing;
            self.track_key = inp.track_id.map_or(0, key_of);
            self.play = if inp.playing { 1.0 } else { 0.0 };
            self.has_progress = inp.progress.is_some();
            self.progress = inp.progress.unwrap_or(0.0);
            return;
        }
        self.t_prev = inp.now;
        if dt <= 0.0 {
            return;
        }
        let (w, h) = (inp.size.x, inp.size.y);

        // 1. Воспроизведение: импульс от центра и плавный разгон/затухание.
        if inp.playing != self.was_playing {
            self.was_playing = inp.playing;
            if inp.playing {
                self.push_pulse(w * 0.5, h * 0.5, 1.0);
                self.excite = self.excite.max(0.8);
            } else {
                self.push_pulse(w * 0.5, h * 0.5, 0.45);
            }
        }
        let target = if inp.playing { 1.0 } else { 0.0 };
        let rate = if inp.playing { 2.0 } else { 1.1 }; // разгоняется быстрее, чем затихает
        self.play += (target - self.play) * smooth(dt, rate);

        // 2. Смена трека: импульс слева и перелепка формы.
        let key = inp.track_id.map_or(0, key_of);
        if key != self.track_key {
            self.track_key = key;
            if key != 0 {
                self.push_pulse(0.0, h * 0.5, 0.9);
                self.morph_target += 1.9;
                self.excite = self.excite.max(0.7);
            }
        }
        self.morph += (self.morph_target - self.morph) * smooth(dt, 1.6);

        // 3. Фаза: скорость растёт от паузы к игре и от возбуждения.
        let speed = IDLE_SPEED + (PLAY_SPEED - IDLE_SPEED) * self.play + 0.9 * self.excite;
        self.phase += (speed * dt) as f64;

        // 4. Прогресс (сглажен, чтобы блик не прыгал при перемотке).
        self.has_progress = inp.progress.is_some();
        if let Some(p) = inp.progress {
            self.progress += (p - self.progress) * smooth(dt, 10.0);
        }

        // 5. Курсор.
        match inp.pointer {
            Some(p) => {
                if self.hover < 0.02 {
                    self.cursor = p; // только вошёл: не тянем волну через весь холст
                    self.vel = Vec2::ZERO;
                }
                let prev = self.cursor;
                self.cursor += (p - self.cursor) * smooth(dt, 14.0);
                let v = (self.cursor - prev) / dt; // px/с
                self.vel += (v - self.vel) * smooth(dt, 10.0);
                self.hover += (1.0 - self.hover) * smooth(dt, 8.0);
                self.excite = self.excite.max((self.vel.length() / 1400.0).min(0.5));
            }
            None => {
                self.hover -= self.hover * smooth(dt, 4.0);
                self.vel *= (-dt * 6.0).exp();
            }
        }
        let pressing = inp.pressed && inp.pointer.is_some();
        self.press += ((if pressing { 1.0 } else { 0.0 }) - self.press) * smooth(dt, 14.0);
        if inp.pressed && !self.was_pressed {
            if let Some(p) = inp.pointer {
                self.push_pulse(p.x, p.y, 1.0); // клик: импульс из точки нажатия
                self.excite = self.excite.max(0.75);
            }
        }
        self.was_pressed = inp.pressed;

        // 6. Старение.
        self.excite *= (-dt * 1.1).exp();
        for p in &mut self.pulses {
            p.age += dt;
        }
        self.pulses.retain(|p| p.age < PULSE_TTL);
    }
}

// ---------------------------------------------------------------------------------------------
// Таблица точек (то, что не меняется от кадра к кадру)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Dot {
    ribbon: u8,
    col: u32,
    v: f32,  // положение поперёк ленты, -1..1
    jx: f32, // сдвиг по X внутри колонки, -1..1
    a: f32,  // «яркость» и размер точки, 0..1
}

pub struct WaveTable {
    cols: usize,
    dots: Vec<Dot>,
}

/// Псевдослучайное число 0..1 из (seed, i): один и тот же вход всегда даёт один и тот же результат.
fn hash01(seed: u64, i: u64) -> f64 {
    let mut z = seed.wrapping_add(i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

impl WaveTable {
    pub fn build(cols: usize) -> Self {
        let cols = cols.max(2);
        let mut dots = Vec::with_capacity(RIBBONS * cols * DOTS_PER_COL);
        for k in 0..RIBBONS {
            for col in 0..cols {
                for j in 0..DOTS_PER_COL {
                    let i = ((k * cols + col) * DOTS_PER_COL + j) as u64;
                    let (r1, r2, r3, r4) = (hash01(11, i), hash01(23, i), hash01(37, i), hash01(53, i));
                    // Чаще равномерно по ширине, в четверти случаев плотнее к середине.
                    let v = if r4 < 0.25 { r1 + r2 - 1.0 } else { r1 * 2.0 - 1.0 };
                    dots.push(Dot { ribbon: k as u8, col: col as u32, v: v as f32, jx: (r3 * 2.0 - 1.0) as f32, a: r2 as f32 });
                }
            }
        }
        Self { cols, dots }
    }
}

/// Таблица для `cols` колонок. Лежит в памяти egui; пересобирается только при смене `cols` (размера окна).
pub fn wave_table(ctx: &egui::Context, cols: usize) -> Arc<WaveTable> {
    let id = egui::Id::NULL.with("wave_table_v2");
    if let Some(t) = ctx.data_mut(|d| d.get_temp::<Arc<WaveTable>>(id)) {
        if t.cols == cols {
            return t;
        }
    }
    let t = Arc::new(WaveTable::build(cols));
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

// ---------------------------------------------------------------------------------------------
// Рисование
// ---------------------------------------------------------------------------------------------

/// Одна колонка одной ленты на этот кадр.
#[derive(Clone, Copy, Default)]
struct Col {
    y: f32,     // центр ленты, px от верха холста
    hw: f32,    // полуширина ленты, px
    cos_t: f32, // cos угла поворота: 1 = лицом, 0 = ребром (складка)
    sin_t: f32,
    env: f32, // 0..1: сужение лент к краям
}

/// Приводит фазу к 0..2π (в f64: время растёт неограниченно), дальше считаем в f32.
fn wrap(x: f64) -> f32 {
    (x % std::f64::consts::TAU) as f32
}

/// Считает колонки одной ленты. Форма: три синусоиды для хребта, скрутка и «дыхание» ширины.
/// Импульсы поднимают хребет бегущей волной и раздувают ленту.
fn ribbon_columns(k: usize, cols: usize, sim: &WaveSim, w: f32, h: f32, amp: f32) -> Vec<Col> {
    let kf = k as f64;
    let (ph, m) = (sim.phase, sim.morph as f64);
    let p1 = wrap(ph * 0.9 + kf * 1.7 + m * 0.8);
    let p2 = wrap(-ph * 0.6 + kf * 0.9 + m * 1.3);
    let p3 = wrap(ph * 0.4 + kf * 2.1 + m * 0.5);
    let pt = wrap(ph * 0.7 + kf * 2.4 + m * 1.1);
    let pw = wrap(ph * 0.35 + kf * 1.3 + m * 0.4);
    let cy = h * 0.5;
    let parallax = 1.0 + 0.25 * (k as f32 - 1.0); // ленты реагируют на импульс чуть по-разному
    let mut out = Vec::with_capacity(cols);
    for i in 0..cols {
        let u = i as f32 / (cols - 1) as f32;
        let x = u * w;
        let env = (PI * u).sin().max(0.0).powf(0.7);
        let spine = (0.50 * (u * TAU * 1.1 + p1).sin() + 0.22 * (u * TAU * 2.3 + p2).sin() + 0.35 * (u * TAU * 0.55 + p3).sin()) / 1.07;
        let twist = u * TAU * 1.15 + pt + 0.9 * (u * TAU * 0.7 - p2).sin();
        let (sin_t, cos_t) = twist.sin_cos();
        let mut hw = (0.42 + 0.26 * (u * TAU * 0.8 + pw).sin()) * env.powf(0.6) * h * 0.22 * amp;
        let mut y = cy + spine * env * h * 0.27 * amp;

        for pu in &sim.pulses {
            let (d, r) = ((x - pu.x).abs(), FRONT_SPEED * pu.age);
            let z = (d - r) / (80.0 + 70.0 * pu.age);
            if z.abs() < 3.0 {
                let g = (-z * z).exp();
                let power = pu.strength * (-1.3 * pu.age).exp();
                y += power * g * ((d - r) * 0.04).sin() * h * 0.22 * parallax;
                hw *= 1.0 + 0.5 * g * power;
            }
        }
        out.push(Col { y, hw, cos_t, sin_t, env });
    }
    out
}

/// Кольцо импульса, подготовленное к кадру (чтобы не считать exp и границы для каждой точки).
struct Ring {
    x: f32,
    y: f32,
    r: f32,
    lo2: f32,
    hi2: f32,
    power: f32,
}

/// Рисует волну в `rect`. `step`: брать каждую `step`-ю точку (1 = все; в закадровом проходе стекла 3).
pub fn paint(painter: &Painter, rect: Rect, sim: &WaveSim, table: &WaveTable, base: Color32, accent: Color32, step: usize) {
    let (w, h) = (rect.width(), rect.height());
    let cols = table.cols;
    // Размах: пауза спокойнее, игра и возбуждение выше. Верхняя граница держит ленты в холсте.
    let amp = ((0.78 + 0.22 * sim.play) * (1.0 + 0.25 * sim.excite)).min(1.2);
    let bright = 0.62 + 0.38 * sim.play + 0.25 * sim.excite;

    let columns: Vec<Vec<Col>> = (0..RIBBONS).map(|k| ribbon_columns(k, cols, sim, w, h, amp)).collect();

    let rings: Vec<Ring> = sim
        .pulses
        .iter()
        .map(|p| {
            let r = RING_SPEED * p.age;
            let (lo, hi) = ((r - 90.0).max(0.0), r + 90.0);
            Ring { x: p.x, y: p.y, r, lo2: lo * lo, hi2: hi * hi, power: p.strength * (-1.5 * p.age).exp() }
        })
        .collect();

    let hover_on = sim.hover > 0.002;
    let sigma = 62.0 + 40.0 * sim.press;
    let cur = sim.cursor;
    let vl = sim.vel.length();
    let vn = (if vl > 1100.0 { sim.vel * (1100.0 / vl) } else { sim.vel }) / 1100.0; // скорость курсора, |vn| <= 1
    let playhead_x = sim.progress * w;
    let col_w = w / (cols - 1) as f32;
    let size_base = if step == 1 { 1.15 } else { 2.4 }; // при step=3 точки крупнее, чтобы пятно не «рассыпалось»

    let mut mesh = Mesh::default();
    mesh.reserve_triangles(table.dots.len() / step * 2 + 2);
    mesh.reserve_vertices(table.dots.len() / step * 4 + 4);
    for dot in table.dots.iter().step_by(step) {
        let c = columns[dot.ribbon as usize][dot.col as usize];
        let mut x = (dot.col as f32 + dot.jx * 0.5) * col_w;
        let mut y = c.y + dot.v * c.hw * c.cos_t;

        // Освещение: «лицевая» сторона ярче, складки (ребром к зрителю) подсвечены.
        let shade = 0.7 + 0.3 * dot.v * c.sin_t;
        let fold = 1.0 + 0.9 * (1.0 - c.cos_t.abs()).powi(4);
        let mut alpha = (28.0 + 140.0 * dot.a) * shade * fold * c.env.sqrt() * bright;
        let mut tint = 0.0_f32;

        if hover_on {
            let (dx, dy) = (x - cur.x, y - cur.y);
            let d2 = dx * dx + dy * dy;
            if d2 < (3.0 * sigma) * (3.0 * sigma) {
                let d = d2.sqrt().max(0.001);
                let infl = (-d2 / (2.0 * sigma * sigma)).exp() * sim.hover;
                let push = infl * (24.0 + 34.0 * sim.press); // «дыра» вокруг курсора
                x += dx / d * push + vn.x * infl * 16.0; // и снос по движению мыши
                y += dy / d * push + vn.y * infl * 16.0;
                tint = infl;
            }
        }
        for ring in &rings {
            let (dx, dy) = (x - ring.x, y - ring.y);
            let d2 = dx * dx + dy * dy;
            if d2 < ring.lo2 || d2 > ring.hi2 {
                continue;
            }
            let d = d2.sqrt().max(0.001);
            let band = ((d - ring.r) / 34.0).powi(2);
            let k = (-band).exp() * ring.power;
            x += dx / d * k * 20.0;
            y += dy / d * k * 20.0;
            tint = tint.max(k.min(1.0));
        }
        if sim.has_progress && sim.play > 0.02 {
            let s = (-((x - playhead_x) / 50.0).powi(2)).exp() * sim.play * 0.55; // блик прогресса
            tint = tint.max(s);
            alpha *= 1.0 + 0.8 * s;
        }

        let alpha = (alpha * (1.0 + 0.9 * tint)).clamp(0.0, 255.0) as u8;
        if alpha < 3 {
            continue;
        }
        let color = if tint > 0.003 { base.lerp_to_gamma(accent, tint.min(1.0)) } else { base };
        let size = size_base + 1.5 * tint + 0.5 * dot.a;
        mesh.add_colored_rect(Rect::from_center_size(rect.min + vec2(x, y), vec2(size, size)), with_alpha(color, alpha));
    }
    painter.with_clip_rect(rect).add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(now: f64, playing: bool, track: Option<&'a str>) -> WaveInput<'a> {
        WaveInput { now, playing, track_id: track, progress: None, reduce_motion: false, pointer: None, pressed: false, size: vec2(1000.0, 200.0) }
    }

    fn run(sim: &mut WaveSim, frames: usize, playing: bool) {
        for i in 0..frames {
            sim.step(&input(1.0 + i as f64 / 60.0, playing, None));
        }
    }

    #[test]
    fn playing_runs_faster_than_paused() {
        let (mut idle, mut play) = (WaveSim::default(), WaveSim::default());
        run(&mut idle, 120, false);
        run(&mut play, 120, true);
        assert!(play.phase > idle.phase * 2.0, "{} vs {}", play.phase, idle.phase);
    }

    #[test]
    fn play_toggle_spawns_pulse() {
        let mut sim = WaveSim::default();
        sim.step(&input(1.0, false, None)); // инициализация
        sim.step(&input(1.016, true, None));
        assert_eq!(sim.pulses.len(), 1);
        assert!(sim.excite > 0.5);
    }

    #[test]
    fn press_spawns_pulse_at_pointer_only_once() {
        let mut sim = WaveSim::default();
        sim.step(&input(1.0, false, None));
        for i in 1..4 {
            let mut inp = input(1.0 + i as f64 * 0.016, false, None);
            inp.pointer = Some(vec2(100.0, 50.0));
            inp.pressed = true;
            sim.step(&inp);
        }
        assert_eq!(sim.pulses.len(), 1);
        assert!((sim.pulses[0].x - 100.0).abs() < 1e-3);
    }

    #[test]
    fn track_change_morphs_shape() {
        let mut sim = WaveSim::default();
        sim.step(&input(1.0, true, Some("a")));
        sim.step(&input(1.016, true, Some("a")));
        assert!(sim.pulses.is_empty());
        sim.step(&input(1.032, true, Some("b")));
        assert_eq!(sim.pulses.len(), 1);
        assert!(sim.morph_target > 1.0);
    }

    #[test]
    fn reduce_motion_freezes_everything() {
        let mut sim = WaveSim::default();
        sim.step(&input(1.0, false, None));
        sim.step(&input(1.016, true, None));
        let mut inp = input(1.032, true, None);
        inp.reduce_motion = true;
        sim.step(&inp);
        assert!(sim.pulses.is_empty());
        assert_eq!(sim.phase, 0.0);
    }

    #[test]
    fn table_has_expected_size_and_range() {
        let t = WaveTable::build(100);
        assert_eq!(t.dots.len(), RIBBONS * 100 * DOTS_PER_COL);
        assert!(t.dots.iter().all(|d| d.v >= -1.0 && d.v <= 1.0 && d.jx >= -1.0 && d.jx <= 1.0));
    }
}

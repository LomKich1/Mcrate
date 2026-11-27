//! Анимация скачивания на обложке строки: затемнение и значок.
//!
//! Сначала стрелка падает в лоток. Если скачивание идёт дольше `SPIN_DELAY_SECS`, лоток под стрелкой плавно
//! загибается в незамкнутое кольцо вокруг неё и начинает вращаться. После конца загрузки значок гаснет.
//! При «уменьшении движения» кольцо появляется сразу и стоит на месте.
//!
//! Лоток и кольцо это одна ломаная: у каждой точки есть доля пути `u` (0..1), и при превращении точка едет по
//! прямой от своего места в лотке к своему месту на кольце.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::{Duration, Instant};

use egui::{Color32, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};

use crate::controller::{DOWNLOAD_ICON_DROP, DOWNLOAD_ICON_FADE, DownloadVisual};
use crate::theme::{Theme, with_alpha};
use crate::ui::anim::{EMPHASIZED_DECELERATE, STANDARD, cubic_bezier};

/// Сколько секунд скачивание должно длиться, чтобы лоток превратился в кольцо.
const SPIN_DELAY_SECS: f32 = 1.0;
/// Длина превращения лотка в кольцо, с. За это же время вращение разгоняется.
const MORPH_SECS: f32 = 0.5;
/// Один оборот кольца, с.
const LAP_SECS: f32 = 1.1;
/// Сторона значка, доля стороны обложки.
const ICON_RATIO: f32 = 0.52;
/// Радиус кольца, доля стороны значка.
const RING_RADIUS: f32 = 0.56;
/// Дуга кольца в радианах (285°): без разрыва вращения не было бы видно.
const RING_SWEEP: f32 = TAU * 285.0 / 360.0;
/// Половина ширины лотка, высота его верха и низа от центра, доли стороны значка.
const TRAY_HALF_WIDTH: f32 = 0.34;
const TRAY_TOP: f32 = 0.06;
const TRAY_BOTTOM: f32 = 0.34;
/// На столько ниже центра значка лежит середина упавшей стрелки; в кольце она поднята в самый центр.
const ARROW_OFF_CENTER: f32 = 0.065;
/// На сколько равных отрезков делим ломаную.
const SEGMENTS: usize = 48;

/// Затемнение обложки и значок скачивания.
pub(super) fn paint_download_overlay(ui: &Ui, theme: &Theme, cover: Rect, radius: f32, download: DownloadVisual) {
    let now = Instant::now();
    let elapsed = now.saturating_duration_since(download.started_at).as_secs_f32();
    let finished = download.finished_at.map(|at| at.saturating_duration_since(download.started_at).as_secs_f32());
    let reduce = theme.settings.reduce_motion;
    let drop = if reduce { 1.0 } else { (elapsed / DOWNLOAD_ICON_DROP.as_secs_f32()).clamp(0.0, 1.0) };
    let ring = RingState::at(elapsed, finished, reduce);
    request_next_frame(ui, download, elapsed, reduce);
    ui.painter().rect_filled(cover, radius, Color32::from_black_alpha(super::track_row::COVER_SCRIM_ALPHA));
    let icon = IconPaint { center: cover.center(), size: cover.height() * ICON_RATIO, alpha: fade_alpha(download, now) };
    icon.paint(ui, drop, ring);
}

/// Прозрачность значка: 1, пока идёт загрузка и падает стрелка, потом гаснет до 0.
fn fade_alpha(download: DownloadVisual, now: Instant) -> f32 {
    let Some(finished_at) = download.finished_at else { return 1.0 };
    let fade_start = (download.started_at + DOWNLOAD_ICON_DROP).max(finished_at);
    (1.0 - now.saturating_duration_since(fade_start).as_secs_f32() / DOWNLOAD_ICON_FADE.as_secs_f32()).clamp(0.0, 1.0)
}

/// Просит следующий кадр: пока что-то движется, сразу; пока ждём начала кольца, к этому моменту.
fn request_next_frame(ui: &Ui, download: DownloadVisual, elapsed: f32, reduce_motion: bool) {
    let falling = elapsed < DOWNLOAD_ICON_DROP.as_secs_f32() && !reduce_motion;
    let spinning = elapsed >= SPIN_DELAY_SECS && !reduce_motion;
    if falling || spinning || download.finished_at.is_some() {
        ui.ctx().request_repaint();
    } else if elapsed < SPIN_DELAY_SECS {
        ui.ctx().request_repaint_after(Duration::from_secs_f32(SPIN_DELAY_SECS - elapsed));
    }
}

/// Насколько лоток превратился в кольцо и куда оно повёрнуто.
#[derive(Clone, Copy, PartialEq, Debug)]
struct RingState {
    /// 0 = лоток, 1 = кольцо.
    morph: f32,
    /// Поворот кольца по часовой стрелке, радианы.
    turn: f32,
}

impl RingState {
    /// `elapsed`: секунд от начала скачивания; `finished`: на какой секунде оно закончилось.
    /// Скачивание, уложившееся в `SPIN_DELAY_SECS`, остаётся с лотком.
    fn at(elapsed: f32, finished: Option<f32>, reduce_motion: bool) -> Self {
        let since = elapsed - SPIN_DELAY_SECS;
        if since <= 0.0 || finished.is_some_and(|at| at <= SPIN_DELAY_SECS) {
            return Self { morph: 0.0, turn: 0.0 };
        }
        if reduce_motion {
            return Self { morph: 1.0, turn: 0.0 };
        }
        let speed = TAU / LAP_SECS;
        // Разгон с нуля за время превращения, дальше ровное вращение: угол и скорость без скачков.
        let turn = if since < MORPH_SECS { speed * since * since / (2.0 * MORPH_SECS) } else { speed * (since - MORPH_SECS / 2.0) };
        Self { morph: cubic_bezier(STANDARD, since / MORPH_SECS), turn: turn % TAU }
    }
}

/// Где и каким рисуется значок.
#[derive(Clone, Copy)]
struct IconPaint {
    center: Pos2,
    size: f32,
    alpha: f32,
}

impl IconPaint {
    fn paint(self, ui: &Ui, drop: f32, ring: RingState) {
        let color = with_alpha(Color32::WHITE, (255.0 * self.alpha).round() as u8);
        let stroke = Stroke::new((self.size * 0.085).max(1.5), color);
        ui.painter().add(Shape::line(base_points(self.center, self.size, ring), stroke));
        self.paint_arrow(ui, stroke, drop, ring.morph);
    }

    /// Стрелка падает строго по вертикали по кубической траектории; при появлении кольца поднимается в его центр.
    fn paint_arrow(self, ui: &Ui, stroke: Stroke, drop: f32, morph: f32) {
        let Self { center, size, .. } = self;
        let t = cubic_bezier(EMPHASIZED_DECELERATE, drop);
        // Высоты острия (доли значка): старт и две опорные точки кривой Безье, конец.
        let [start, a, b, end] = [-0.50, -0.30, -0.08, 0.15];
        let u = 1.0 - t;
        let fall = u * u * u * start + 3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t * end;
        let tip = pos2(center.x, center.y + size * (fall + ARROW_OFF_CENTER * morph));
        let painter = ui.painter();
        painter.line_segment([tip - vec2(0.0, size * 0.43), tip], stroke);
        painter.line_segment([tip + vec2(-size * 0.20, -size * 0.20), tip], stroke);
        painter.line_segment([tip, tip + vec2(size * 0.20, -size * 0.20)], stroke);
    }
}

/// Точки ломаной под стрелкой: от левого верхнего конца через низ к правому.
fn base_points(center: Pos2, size: f32, ring: RingState) -> Vec<Pos2> {
    path_stops().into_iter().map(|u| tray_point(center, size, u).lerp(ring_point(center, size, u, ring.turn), ring.morph)).collect()
}

/// Доли пути, в которых ставим точки: равномерно плюс ровно два угла лотка (иначе они срезались бы).
fn path_stops() -> Vec<f32> {
    let leg = TRAY_BOTTOM - TRAY_TOP;
    let corner = leg / (2.0 * leg + 2.0 * TRAY_HALF_WIDTH);
    let mut stops: Vec<f32> = (0..=SEGMENTS).map(|i| i as f32 / SEGMENTS as f32).chain([corner, 1.0 - corner]).collect();
    stops.sort_by(f32::total_cmp);
    stops
}

/// Точка лотка на доле пути `u`.
fn tray_point(center: Pos2, size: f32, u: f32) -> Pos2 {
    let (leg, width) = (TRAY_BOTTOM - TRAY_TOP, 2.0 * TRAY_HALF_WIDTH);
    let s = u * (2.0 * leg + width);
    let (x, y) = if s < leg {
        (-TRAY_HALF_WIDTH, TRAY_TOP + s)
    } else if s < leg + width {
        (-TRAY_HALF_WIDTH + (s - leg), TRAY_BOTTOM)
    } else {
        (TRAY_HALF_WIDTH, TRAY_BOTTOM - (s - leg - width))
    };
    center + vec2(x, y) * size
}

/// Точка кольца на доле пути `u`: середина пути внизу, концы сходятся к разрыву наверху.
fn ring_point(center: Pos2, size: f32, u: f32, turn: f32) -> Pos2 {
    let angle = FRAC_PI_2 + RING_SWEEP * (0.5 - u) + turn; // ось y смотрит вниз: π/2 это низ
    center + vec2(angle.cos(), angle.sin()) * (RING_RADIUS * size)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CENTER: Pos2 = pos2(24.0, 24.0);
    const SIZE: f32 = 25.0;

    #[test]
    fn quick_download_keeps_the_tray() {
        assert_eq!(RingState::at(0.9, None, false).morph, 0.0);
        assert_eq!(RingState::at(1.15, Some(0.8), false).morph, 0.0);
    }

    #[test]
    fn long_download_turns_into_a_spinning_ring() {
        let middle = RingState::at(SPIN_DELAY_SECS + MORPH_SECS / 2.0, None, false);
        assert!(middle.morph > 0.0 && middle.morph < 1.0);
        let a = RingState::at(2.0, None, false);
        let b = RingState::at(2.1, None, false);
        assert_eq!(a.morph, 1.0);
        assert!(a.turn != b.turn);
    }

    #[test]
    fn animation_has_no_jumps() {
        let frame = |i: u16| base_points(CENTER, SIZE, RingState::at(f32::from(i) / 120.0, None, false));
        for i in 0..600 {
            for (a, b) in frame(i).iter().zip(frame(i + 1)) {
                assert!(a.distance(b) < 2.0, "скачок точки в кадре {i}");
            }
        }
    }

    #[test]
    fn reduced_motion_shows_a_still_ring() {
        assert_eq!(RingState::at(3.0, None, true), RingState { morph: 1.0, turn: 0.0 });
    }

    #[test]
    fn tray_keeps_its_corners_and_ring_is_round() {
        let tray = base_points(CENTER, SIZE, RingState { morph: 0.0, turn: 0.0 });
        let corner = CENTER + vec2(-TRAY_HALF_WIDTH, TRAY_BOTTOM) * SIZE;
        assert!(tray.iter().any(|p| p.distance(corner) < 0.01));
        for p in base_points(CENTER, SIZE, RingState { morph: 1.0, turn: 1.0 }) {
            assert!((p.distance(CENTER) - RING_RADIUS * SIZE).abs() < 0.01);
        }
    }
}

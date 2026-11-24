//! Обложка-заглушка и контур загрузки вокруг неё.

use crate::icons::{self, Icon};
use crate::settings::Design;
use crate::theme::{Theme, with_alpha};
use egui::{Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};

/// Цветная плитка-заглушка вместо обложки (обложек в ядре пока нет).
pub(crate) fn cover_tile(ui: &mut Ui, theme: &Theme, rect: Rect, seed: &str, radius: f32) {
    let p = &theme.palette;
    let h = seed.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let (mut bg, fg) = match h % 3 {
        0 => (p.primary_container, p.on_primary_container),
        1 => (p.secondary_container, p.on_secondary_container),
        _ => (p.tertiary_container, if theme.design() == Design::Material3 { p.on_tertiary_container } else { p.on_surface }),
    };
    // Эти цветные плитки — временная замена обложек, а не M3-контейнеры.
    // В Liquid Glass слегка приглушаем тон, чтобы заглушки не спорили с альбомным рядом.
    if theme.glass_active() {
        bg = bg.lerp_to_gamma(p.surface_container_lowest, 0.26);
    }
    let painter = ui.painter();
    painter.rect_filled(rect, radius, bg);
    icons::paint(theme, painter, Icon::Note, rect.center(), rect.height() * 0.5, with_alpha(fg, 200));
}

/// Замкнутая ломаная по скруглённому прямоугольнику, по часовой стрелке (ось y экрана смотрит вниз).
/// Последняя точка не повторяет первую.
fn rounded_rect_ring(rect: Rect, r: f32) -> Vec<Pos2> {
    const STEPS: usize = 6; // сегментов на каждый скруглённый угол
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0_f32),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    let mut pts: Vec<Pos2> = Vec::with_capacity(4 * (STEPS + 1) + 1);
    for (center, start) in corners {
        for i in 0..=STEPS {
            let a = (start + 90.0 * i as f32 / STEPS as f32).to_radians();
            pts.push(center + vec2(a.cos(), a.sin()) * r);
        }
    }
    pts
}

/// Контур загрузки: яркий отрезок с затухающим хвостом бежит по периметру обложки, пока трек грузится.
/// Чисел нет. Контур рисуется чуть внутри обложки (`rect`), чтобы его не обрезала область панели.
/// При включённом «уменьшении движения» вместо бега показывается неподвижная рамка.
pub(crate) fn loading_outline(ui: &Ui, theme: &Theme, cover: Rect, radius: f32, time: f64) {
    const TRAIL: f32 = 0.30; // длина бегущего отрезка, доля периметра
    const LAP_SECS: f64 = 1.4; // один круг по периметру
    const SEGMENTS: usize = 28; // на сколько кусочков режем отрезок для затухания хвоста

    let p = &theme.palette;
    let rect = cover.shrink(1.5);
    let r = (radius - 1.5).clamp(0.0, rect.width().min(rect.height()) * 0.5);

    let ring = rounded_rect_ring(rect, r);
    let mut pts = ring.clone();
    pts.push(pts[0]); // замыкаем, чтобы последний отрезок периметра тоже считался

    // Накопленная длина вдоль периметра: по ней находим точку на расстоянии `s` от старта.
    let mut cum: Vec<f32> = Vec::with_capacity(pts.len());
    cum.push(0.0);
    for w in pts.windows(2) {
        let last = *cum.last().unwrap_or(&0.0);
        cum.push(last + (w[1] - w[0]).length());
    }
    let total = *cum.last().unwrap_or(&0.0);
    if total <= f32::EPSILON {
        return;
    }
    let point_at = |s: f32| -> Pos2 {
        let s = s.rem_euclid(total); // за пределами круга заворачиваем в начало
        let i = cum.partition_point(|&c| c <= s).saturating_sub(1).min(pts.len() - 2);
        let seg = cum[i + 1] - cum[i];
        if seg <= f32::EPSILON { pts[i] } else { pts[i].lerp(pts[i + 1], (s - cum[i]) / seg) }
    };

    let painter = ui.painter();
    if theme.settings.reduce_motion {
        let _ = painter.add(Shape::closed_line(ring, Stroke::new(2.0, with_alpha(p.primary, 170))));
        return;
    }
    // Тусклая дорожка по всему периметру, чтобы было видно, где бежит отрезок.
    let _ = painter.add(Shape::closed_line(ring, Stroke::new(1.5, with_alpha(p.primary, 60))));
    let head = ((time / LAP_SECS).fract() as f32) * total;
    let trail = total * TRAIL;
    for k in 0..SEGMENTS {
        let a = k as f32 / SEGMENTS as f32;
        let b = (k + 1) as f32 / SEGMENTS as f32;
        let alpha = (255.0 * b).round() as u8; // хвост тает, голова самая яркая
        painter.line_segment(
            [point_at(head - trail * (1.0 - a)), point_at(head - trail * (1.0 - b))],
            Stroke::new(2.5, with_alpha(p.primary, alpha)),
        );
    }
    ui.ctx().request_repaint(); // анимация идёт сама, без движения мыши
}

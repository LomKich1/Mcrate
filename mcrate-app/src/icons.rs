//! Иконки, нарисованные кодом (линии, многоугольники). Так не нужны файлы и шрифты с иконками,
//! а размер и цвет меняются свободно. Сетка 24x24, центр в (0, 0).

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Play,
    Pause,
    Next,
    Prev,
    Shuffle,
    Repeat,
    RepeatOne,
    Volume,
    VolumeMute,
    Note,
    Queue,
    Settings,
    Plus,
    Folder,
}

/// Нарисовать иконку с центром в `center`; `size` = сторона квадрата в пикселях.
pub fn paint(painter: &Painter, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let u = size / 24.0; // масштаб: 1 «единица сетки» в пикселях
    let p = |x: f32, y: f32| center + vec2(x * u, y * u); // точка сетки -> точка на экране
    let stroke = Stroke::new((2.0 * u).max(1.2), color);
    let fill_poly = |pts: Vec<Pos2>| Shape::convex_polygon(pts, color, Stroke::NONE);
    let bar = |x0: f32, y0: f32, x1: f32, y1: f32| {
        painter.rect_filled(Rect::from_min_max(p(x0, y0), p(x1, y1)), 1.5 * u, color);
    };

    match icon {
        Icon::Play => {
            painter.add(fill_poly(vec![p(-6.0, -9.0), p(-6.0, 9.0), p(9.0, 0.0)]));
        }
        Icon::Pause => {
            bar(-7.0, -8.0, -2.0, 8.0);
            bar(2.0, -8.0, 7.0, 8.0);
        }
        Icon::Next => {
            painter.add(fill_poly(vec![p(-8.0, -7.0), p(-8.0, 7.0), p(4.0, 0.0)]));
            bar(5.5, -7.0, 8.0, 7.0);
        }
        Icon::Prev => {
            painter.add(fill_poly(vec![p(8.0, -7.0), p(8.0, 7.0), p(-4.0, 0.0)]));
            bar(-8.0, -7.0, -5.5, 7.0);
        }
        Icon::Shuffle => {
            painter.add(Shape::line(vec![p(-9.0, -6.0), p(-3.0, -6.0), p(3.0, 6.0), p(7.0, 6.0)], stroke));
            painter.add(Shape::line(vec![p(-9.0, 6.0), p(-3.0, 6.0), p(3.0, -6.0), p(7.0, -6.0)], stroke));
            painter.add(fill_poly(vec![p(6.0, 3.0), p(6.0, 9.0), p(11.0, 6.0)]));
            painter.add(fill_poly(vec![p(6.0, -9.0), p(6.0, -3.0), p(11.0, -6.0)]));
        }
        Icon::Repeat | Icon::RepeatOne => {
            painter.rect_stroke(Rect::from_min_max(p(-9.0, -5.0), p(9.0, 5.0)), 4.0 * u, stroke, StrokeKind::Middle);
            painter.add(fill_poly(vec![p(5.0, -9.5), p(5.0, -0.5), p(11.0, -5.0)]));
            if icon == Icon::RepeatOne {
                painter.text(center, Align2::CENTER_CENTER, "1", FontId::proportional(9.0 * u * 1.2), color);
            }
        }
        Icon::Volume | Icon::VolumeMute => {
            painter.rect_filled(Rect::from_min_max(p(-10.0, -3.0), p(-6.0, 3.0)), 1.0 * u, color);
            painter.add(fill_poly(vec![p(-6.0, -3.0), p(-1.0, -8.0), p(-1.0, 8.0), p(-6.0, 3.0)]));
            if icon == Icon::Volume {
                for r in [5.0_f32, 9.0] {
                    // Дуга: точки на окружности радиуса r с центром (-1, 0), углы от -50° до +50°.
                    let pts: Vec<Pos2> = (0..=8)
                        .map(|i| {
                            let a = (-50.0_f32 + 12.5 * i as f32).to_radians();
                            p(-1.0 + r * a.cos(), r * a.sin())
                        })
                        .collect();
                    painter.add(Shape::line(pts, stroke));
                }
            } else {
                painter.line_segment([p(3.0, -4.0), p(10.0, 4.0)], stroke);
                painter.line_segment([p(10.0, -4.0), p(3.0, 4.0)], stroke);
            }
        }
        Icon::Note => {
            painter.circle_filled(p(-3.5, 6.0), 3.8 * u, color);
            painter.line_segment([p(0.0, 6.0), p(0.0, -8.0)], stroke);
            painter.add(Shape::line(vec![p(0.0, -8.0), p(7.0, -5.5), p(7.0, -1.0)], stroke));
        }
        Icon::Queue => {
            painter.line_segment([p(-10.0, -6.0), p(3.0, -6.0)], stroke);
            painter.line_segment([p(-10.0, 0.0), p(3.0, 0.0)], stroke);
            painter.line_segment([p(-10.0, 6.0), p(-2.0, 6.0)], stroke);
            painter.add(fill_poly(vec![p(4.0, 2.5), p(4.0, 9.5), p(10.0, 6.0)]));
        }
        Icon::Settings => {
            painter.line_segment([p(-10.0, -5.0), p(10.0, -5.0)], stroke);
            painter.line_segment([p(-10.0, 5.0), p(10.0, 5.0)], stroke);
            painter.circle_filled(p(3.5, -5.0), 3.2 * u, color);
            painter.circle_filled(p(-3.5, 5.0), 3.2 * u, color);
        }
        Icon::Plus => {
            painter.line_segment([p(-8.0, 0.0), p(8.0, 0.0)], stroke);
            painter.line_segment([p(0.0, -8.0), p(0.0, 8.0)], stroke);
        }
        Icon::Folder => {
            painter.rect_stroke(Rect::from_min_max(p(-10.0, -4.0), p(10.0, 8.0)), 2.5 * u, stroke, StrokeKind::Middle);
            painter.add(Shape::line(vec![p(-10.0, -4.0), p(-10.0, -8.0), p(-3.0, -8.0), p(-1.0, -4.0)], stroke));
        }
    }
}

/// Анимированный «эквалайзер» для играющего трека. `t` = время в секундах.
pub fn paint_equalizer(painter: &Painter, center: Pos2, size: f32, color: Color32, t: f64, animate: bool) {
    let w = size * 0.18;
    let gap = size * 0.12;
    for i in 0..3 {
        let phase = if animate { ((t * 5.0 + i as f64 * 1.7).sin() * 0.5 + 0.5) as f32 } else { 0.5 };
        let h = size * (0.3 + 0.7 * phase);
        let x = center.x + (i as f32 - 1.0) * (w + gap);
        let r = Rect::from_min_max(pos2(x - w / 2.0, center.y + size / 2.0 - h), pos2(x + w / 2.0, center.y + size / 2.0));
        painter.rect_filled(r, w / 2.0, color);
    }
}

//! Сегментированный список Material 3: каждая строка лежит на своей плашке, между плашками маленький зазор.
//! Углы плашек: у первой сверху и у последней снизу крупное скругление, на стыках мелкое; единственная строка
//! скруглена со всех сторон. Только в теме Material 3, Liquid Glass рисует списки по-прежнему.

use crate::theme::{ShapeRole, Theme};
use egui::{Color32, CornerRadius, Rect, Shape, Ui};

/// Зазор между плашками соседних строк.
pub(crate) const SEGMENT_GAP: f32 = 2.1;
/// Радиус угла на стыке плашек.
const SEGMENT_INNER_RADIUS: u8 = 4;

/// Место строки в списке: номер (с нуля) и сколько всего строк.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SegmentPos {
    pub(crate) index: usize,
    pub(crate) count: usize,
}

/// Углы плашки для строки `pos`: внешний радиус у краёв группы, `SEGMENT_INNER_RADIUS` на стыках.
pub(crate) fn segment_radius(theme: &Theme, pos: SegmentPos) -> CornerRadius {
    radii(theme.shape_radius(ShapeRole::ExtraLarge), pos)
}

fn radii(outer: u8, pos: SegmentPos) -> CornerRadius {
    let top = if pos.index == 0 { outer } else { SEGMENT_INNER_RADIUS };
    let bottom = if pos.index + 1 >= pos.count { outer } else { SEGMENT_INNER_RADIUS };
    CornerRadius { nw: top, ne: top, sw: bottom, se: bottom }
}

/// Плашка строки.
pub(crate) fn segment_shape(rect: Rect, radius: CornerRadius, fill: Color32) -> Shape {
    Shape::rect_filled(rect, radius, fill)
}

/// Радиус для волны нажатия (она знает один радиус на все углы): берём самый большой, чтобы она не вылезала за внешние углы.
pub(crate) fn ripple_corner(radius: CornerRadius) -> f32 {
    f32::from(radius.nw.max(radius.ne).max(radius.sw).max(radius.se))
}

/// Плашка строки трека внутри её прямоугольника: уже по высоте на `SEGMENT_GAP`, поровну сверху и снизу. Зазор между
/// плашками так заложен в саму строку и не зависит от отступа между элементами `ui`, который в разных местах разный.
pub(crate) fn track_plate(rect: Rect) -> Rect {
    rect.shrink2(egui::vec2(0.0, SEGMENT_GAP / 2.0))
}

/// Настраивает отступ между строками списка треков. Вызывать один раз для `ui` списка и до `rows_in_view`: он считает
/// шаг строк по этому значению.
///
/// * Material 3: отступа нет, зазор уже внутри строки (`track_plate`), шаг равен высоте строки.
/// * Liquid Glass: отступ меньше заданного на 10 px (строки чуть заходят друг на друга, как и выглядели раньше).
pub(crate) fn apply_list_gap(ui: &mut Ui, theme: &Theme) {
    let gap = &mut ui.spacing_mut().item_spacing.y;
    if theme.glass_active() {
        *gap -= 10.0;
    } else {
        *gap = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTER: u8 = 28;

    fn at(index: usize, count: usize) -> CornerRadius {
        radii(OUTER, SegmentPos { index, count })
    }

    #[test]
    fn single_row_is_rounded_everywhere() {
        assert_eq!(at(0, 1), CornerRadius::same(OUTER));
    }

    #[test]
    fn group_edges_are_big_and_joints_are_small() {
        let (first, middle, last) = (at(0, 3), at(1, 3), at(2, 3));
        assert_eq!((first.nw, first.ne, first.sw, first.se), (OUTER, OUTER, SEGMENT_INNER_RADIUS, SEGMENT_INNER_RADIUS));
        assert_eq!(middle, CornerRadius::same(SEGMENT_INNER_RADIUS));
        assert_eq!((last.nw, last.ne, last.sw, last.se), (SEGMENT_INNER_RADIUS, SEGMENT_INNER_RADIUS, OUTER, OUTER));
    }

    #[test]
    fn ripple_uses_the_biggest_corner() {
        assert_eq!(ripple_corner(at(0, 3)), f32::from(OUTER));
        assert_eq!(ripple_corner(at(1, 3)), f32::from(SEGMENT_INNER_RADIUS));
    }
}

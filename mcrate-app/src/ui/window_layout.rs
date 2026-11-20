//! Геометрия каркаса окна: рейка слева, контентная панель, плавающий мини-плеер.
//! Общая для обеих раскладок (Material 3 и Liquid Glass): все функции чистые, без egui-состояния,
//! поэтому обе раскладки получают одни и те же прямоугольники, а расчёт покрыт тестами.

use egui::{Rect, pos2};

/// Внешний отступ плавающих панелей от края окна, px.
pub(crate) const EDGE: f32 = 12.0;
/// Половина зазора между рейкой и контентной панелью, px: каждая отступает от общей границы на столько.
pub(crate) const PANE_HALF_GAP: f32 = 4.0;
/// Рейка сворачивается до иконок, пока её ширина меньше этого значения (подписи проявляются ближе к концу раскрытия).
pub(crate) const RAIL_COMPACT_BELOW: f32 = 208.0;
/// Рамка `Surface::Content` в Material 3 даёт столько px со всех сторон (см. theme/material3.rs); столько же `Bleed`.
pub(crate) const M3_CONTENT_INSET: f32 = 20.0;

/// Высота плавающей панели плеера: содержимое 80 (52+4+24) + отступы 12 сверху и снизу.
pub(crate) const BAR_H: f32 = 104.0;
/// Боковой отступ плеера от краёв контентной панели.
const BAR_SIDE_INSET: f32 = 16.0;
/// Расстояние от нижнего края ОКНА до нижнего края плеера. Было `EDGE` (12): тогда низ плеера
/// совпадал с низом рейки. Больше число = плеер выше.
const BAR_BOTTOM_GAP: f32 = 28.0;
/// Плеер в 1,5 раза уже полной ширины панели (за вычетом боковых отступов).
const BAR_WIDTH_DIVISOR: f32 = 1.5;
/// Нижняя граница ширины плеера: уже колонки плеера вырождаются. В узком окне плеер занимает всю ширину.
const BAR_MIN_WIDTH: f32 = 640.0;
/// Запас к высоте, которую плеер закрывает от страницы, px.
const BAR_OVERLAP_MARGIN: f32 = 4.0;

/// Отступ содержимого страницы от краёв контентной панели в Liquid Glass. Содержимое прокрутки
/// при этом уходит под край панели, а не режется в этих 20 px (см. `Bleed`).
pub(crate) const GLASS_PAGE_PAD: f32 = 20.0;

/// Контентная панель: от рейки (с зазором) до отступов `EDGE` сверху, справа и снизу.
pub(crate) fn content_pane_rect(full: Rect, rail_width: f32) -> Rect {
    let pane = full.shrink(EDGE);
    Rect::from_min_max(pos2(full.left() + rail_width + PANE_HALF_GAP, pane.top()), pane.right_bottom())
}

/// Плавающая рейка Liquid Glass: те же отступы `EDGE` сверху, слева и снизу, что и у контентной
/// панели, поэтому верх и низ у них на одной линии, независимо от положения плеера.
pub(crate) fn glass_rail_rect(full: Rect, rail_width: f32) -> Rect {
    let pane = full.shrink(EDGE);
    Rect::from_min_max(pane.left_top(), pos2(full.left() + rail_width - PANE_HALF_GAP, pane.bottom()))
}

/// Область страницы внутри контентной панели Liquid Glass: отступ со всех сторон, кроме нижней
/// (страница доходит до низа панели, а над ней лежит плеер).
pub(crate) fn glass_page_rect(content_pane: Rect) -> Rect {
    Rect::from_min_max(
        pos2(content_pane.left() + GLASS_PAGE_PAD, content_pane.top() + GLASS_PAGE_PAD),
        pos2(content_pane.right() - GLASS_PAGE_PAD, content_pane.bottom()),
    )
}

/// Мини-плеер по центру контентной панели; `window_bottom` = нижний край окна.
pub(crate) fn mini_bar_rect(content_pane: Rect, window_bottom: f32) -> Rect {
    let full_w = content_pane.width() - BAR_SIDE_INSET * 2.0;
    let w = (full_w / BAR_WIDTH_DIVISOR).max(BAR_MIN_WIDTH.min(full_w));
    let cx = content_pane.center().x;
    let bottom = window_bottom - BAR_BOTTOM_GAP;
    Rect::from_min_max(pos2(cx - w * 0.5, bottom - BAR_H), pos2(cx + w * 0.5, bottom))
}

/// Сколько снизу страница закрыта плеером: от верха плеера до низа панели. Прокручиваемые экраны
/// добавляют это как нижний отступ, чтобы последний элемент можно было прокрутить выше плеера.
pub(crate) fn bottom_inset(content_pane: Rect, bar: Rect) -> f32 {
    content_pane.bottom() - bar.top() + BAR_OVERLAP_MARGIN
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(w, h))
    }

    #[test]
    fn content_pane_matches_the_rail_gap_in_both_layouts() {
        for rail_width in [72.0, 150.5, 208.0, 280.0] {
            let full = window(1280.0, 800.0);
            let content = content_pane_rect(full, rail_width);
            let rail = glass_rail_rect(full, rail_width);
            // зазор между рейкой и контентом = две половины, как и в раскладке Material 3
            assert!((content.left() - rail.right() - 2.0 * PANE_HALF_GAP).abs() < 1e-3, "рейка {rail_width}");
            assert_eq!((content.top(), content.bottom()), (rail.top(), rail.bottom()), "верх и низ на одной линии");
            assert_eq!(content.right(), full.right() - EDGE);
            assert_eq!(content.top(), EDGE);
        }
    }

    #[test]
    fn mini_bar_is_centered_and_floats_above_the_window_bottom() {
        let full = window(1600.0, 900.0);
        let content = content_pane_rect(full, 240.0);
        let bar = mini_bar_rect(content, full.bottom());
        assert!((bar.center().x - content.center().x).abs() < 1e-3);
        assert_eq!(bar.height(), BAR_H);
        assert_eq!(bar.bottom(), full.bottom() - BAR_BOTTOM_GAP);
        assert!((bar.width() - (content.width() - 32.0) / 1.5).abs() < 1e-3, "в широком окне ширина = 1/1.5");
    }

    #[test]
    fn mini_bar_never_gets_narrower_than_the_minimum_or_wider_than_the_pane() {
        // средняя ширина: 1/1.5 дало бы меньше 640, поэтому плеер держит минимум
        let content = content_pane_rect(window(1000.0, 700.0), 240.0);
        let bar = mini_bar_rect(content, 700.0);
        assert_eq!(bar.width(), BAR_MIN_WIDTH);
        // узкое окно: места меньше минимума, плеер занимает всю доступную ширину
        let narrow = content_pane_rect(window(520.0, 700.0), 72.0);
        let bar = mini_bar_rect(narrow, 700.0);
        assert!((bar.width() - (narrow.width() - 32.0)).abs() < 1e-3);
        assert!(bar.left() >= narrow.left() && bar.right() <= narrow.right());
    }

    #[test]
    fn bottom_inset_covers_the_player_overlap() {
        let full = window(1280.0, 800.0);
        let content = content_pane_rect(full, 240.0);
        let bar = mini_bar_rect(content, full.bottom());
        // плеер стоит выше низа панели на (BAR_BOTTOM_GAP - EDGE), и сама высота плеера тоже закрыта
        let expected = BAR_H + (BAR_BOTTOM_GAP - EDGE) + BAR_OVERLAP_MARGIN;
        assert!((bottom_inset(content, bar) - expected).abs() < 1e-3);
    }

    #[test]
    fn glass_page_has_no_bottom_padding() {
        let content = Rect::from_min_max(pos2(100.0, 12.0), pos2(1000.0, 700.0));
        let page = glass_page_rect(content);
        assert_eq!(page.bottom(), content.bottom());
        assert_eq!(page.left() - content.left(), GLASS_PAGE_PAD);
        assert_eq!(page.top() - content.top(), GLASS_PAGE_PAD);
    }
}

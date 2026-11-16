//! Адаптивная раскладка: размеры, которые зависят от ширины окна.
//! Всё в «точках» egui (логических пикселях), поэтому масштаб Windows (125%, 150%) учитывается сам.
//! Правила собраны здесь, а не размазаны по экранам: так их проще менять и проверять тестами.

use egui::{Align, Layout, Margin, Rect, ScrollArea, Ui, UiBuilder, pos2, vec2};

/// Размерный класс окна (в Material 3 это называется window size class).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Breakpoint {
    /// Узкое окно: навигация сворачивается в одни иконки.
    Compact,
    /// Обычное окно (стартовый размер 1180 px попадает сюда).
    Medium,
    /// Широкое окно, например fullHD на весь экран.
    Expanded,
}

impl Breakpoint {
    pub fn from_width(window_width: f32) -> Self {
        if window_width < 1000.0 {
            Self::Compact
        } else if window_width < 1400.0 {
            Self::Medium
        } else {
            Self::Expanded
        }
    }

    /// Ширина левой панели навигации (вместе с отступами).
    pub fn rail_width(self) -> f32 {
        match self {
            Self::Compact => 88.0,
            Self::Medium => 232.0,
            Self::Expanded => 272.0,
        }
    }
}

/// Ширины колонок нижней панели плеера.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarColumns {
    /// Ширина левой и правой колонки (они всегда равны).
    pub side: f32,
    /// Ширина центральной колонки с кнопками и перемоткой.
    pub center: f32,
}

/// Центр занимает половину панели, но не уже 360 и не шире 720 px (на широком экране кнопки
/// не должны разъезжаться по всей панели). Боковые колонки делят остаток поровну, поэтому
/// центр всегда точно посередине плавающего плеера.
pub fn bar_columns(total: f32, gap: f32) -> BarColumns {
    let center = (total * 0.5).clamp(360.0, 720.0);
    let side = ((total - center - gap * 2.0) / 2.0).max(0.0);
    BarColumns { side, center }
}

/// Сторона обложки на экране «Сейчас играет». В компактной высоте допускаем меньшую обложку,
/// чтобы название и метаданные оставались видны без прокрутки.
pub fn cover_side(avail_w: f32, avail_h: f32, compact_height: bool) -> f32 {
    let minimum = if compact_height { 112.0 } else { 160.0 };
    let preferred = if compact_height { 0.44 } else { 0.50 };
    (avail_h * preferred).min(avail_w * 0.5).clamp(minimum, 440.0)
}

/// Создаёт колонку с ограниченной максимальной шириной и центрирует её в доступной области.
/// Используется для списков, настроек и текстовых страниц на широких окнах.
pub fn centered_column<R>(ui: &mut Ui, max_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let available = ui.available_rect_before_wrap();
    let width = available.width().min(max_width.max(1.0));
    let rect = Rect::from_min_size(pos2(available.center().x - width * 0.5, available.top()), vec2(width, available.height()));
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)), add).inner
}

/// «Вылет» (bleed): на сколько пикселей содержимое страницы может выходить за её отступы
/// к самым краям панели. Страница рисуется с отступом (20 px) от краёв панели, а прокрутка
/// (`ScrollArea`) обрезает содержимое по своей рамке (clip rect), поэтому раньше карточки
/// и заголовки резались жёсткой линией в 20 px ОТ края, а полоса прокрутки сидела внутри.
/// Здесь хранится, сколько отступа лежит с каждой стороны. Liquid Glass: снизу 0 (панель
/// заканчивается там, где плеер), M3: 20 со всех сторон (поля рамки `Surface::Content`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bleed {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Bleed {
    pub const fn same(v: f32) -> Self {
        Self { left: v, top: v, right: v, bottom: v }
    }

    /// Только левая и правая сторона: для экранов, у которых заголовок лежит НАД прокруткой
    /// (иначе прокручиваемое содержимое наезжало бы на заголовок).
    pub fn sides(self) -> Self {
        Self { top: 0.0, bottom: 0.0, ..self }
    }

    /// Без верхней стороны: заголовок лежит над прокруткой, но снизу и по бокам вылет нужен.
    pub fn below_header(self) -> Self {
        Self { top: 0.0, ..self }
    }

    /// Прямоугольник, расширенный на вылет.
    pub fn expand(self, r: Rect) -> Rect {
        Rect::from_min_max(r.min - vec2(self.left, self.top), r.max + vec2(self.right, self.bottom))
    }
}

/// Расширяет область отсечения (clip rect) до краёв панели. Нужно для содержимого, которое
/// не лежит прямо в `pane_scroll` (например, горизонтальная полка внутри страницы): раскладка
/// остаётся прежней, меняется только то, докуда доходит рисование и наведение мыши.
pub fn bleed_clip(ui: &mut Ui, bleed: Bleed) {
    let c = ui.clip_rect();
    ui.set_clip_rect(bleed.expand(c));
}

/// Прокрутка, растянутая до краёв панели: рамка `ScrollArea` и её полоса прокрутки лежат на самом
/// краю панели, а прежний отступ возвращён полем содержимого (`content_margin`). Положение
/// виджетов не меняется, но при прокрутке они уходят под край панели, а не обрезаются в отступе.
/// Закрытие получает готовую `ScrollArea` и вызывает у неё `show` или `show_rows`.
pub fn pane_scroll<R>(ui: &mut Ui, bleed: Bleed, area: ScrollArea, show: impl FnOnce(ScrollArea, &mut Ui) -> R) -> R {
    let wide = bleed.expand(ui.available_rect_before_wrap());
    // Нижнее поле не нужно: конец страницы даёт `scroll_end_padding`. `as i8`: Margin хранит i8.
    let margin = Margin { left: bleed.left as i8, right: bleed.right as i8, top: bleed.top as i8, bottom: 0 };
    ui.scope_builder(UiBuilder::new().max_rect(wide).layout(Layout::top_down(Align::Min)), |ui| {
        show(area.content_margin(margin), ui)
    })
    .inner
}

/// Единый запас в конце прокручиваемых страниц. В Material 3 это небольшой край,
/// в Liquid Glass к нему добавляется область, перекрываемая плавающим плеером.
pub fn scroll_end_padding(ui: &mut Ui, bottom_inset: f32) {
    ui.add_space(16.0 + bottom_inset.max(0.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints() {
        assert_eq!(Breakpoint::from_width(880.0), Breakpoint::Compact);
        assert_eq!(Breakpoint::from_width(1180.0), Breakpoint::Medium);
        assert_eq!(Breakpoint::from_width(1920.0), Breakpoint::Expanded);
    }

    #[test]
    fn player_bar_is_symmetric_and_fills_width_at_any_size() {
        let gap = 8.0;
        // От минимального окна (880 px) до 4K.
        for total in [800.0, 830.0, 1000.0, 1130.0, 1536.0, 1880.0, 2560.0, 3800.0] {
            let c = bar_columns(total, gap);
            // Сумма колонок + зазоры = вся ширина панели.
            assert!((c.side * 2.0 + c.center + gap * 2.0 - total).abs() < 0.01, "{total}");
            // Центр ровно посередине: левая и правая колонки равны.
            assert!(c.center >= 360.0 && c.center <= 720.0, "{total}");
            // Левая колонка (обложка 52 + текст) и правая (иконка + ползунок) не вырождаются.
            assert!(c.side >= 190.0, "боковая колонка слишком узкая при {total}: {}", c.side);
        }
    }

    #[test]
    fn glass_player_columns_fit_narrow_content_area() {
        // Минимальное окно 880 px, компактная рейка и симметричные отступы
        // оставляют около 696 px под содержимое плеера после внутренних полей Glass.
        let c = bar_columns(696.0, 8.0);
        assert_eq!(c.center, 360.0);
        assert_eq!(c.side, 160.0);
    }

    #[test]
    fn bleed_expands_rect_to_pane_edges() {
        let page = Rect::from_min_max(pos2(20.0, 20.0), pos2(780.0, 600.0));
        // Liquid Glass: отступ слева, сверху и справа, снизу нет.
        let glass = Bleed { left: 20.0, top: 20.0, right: 20.0, bottom: 0.0 };
        assert_eq!(glass.expand(page), Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0)));
        // Экран с заголовком над прокруткой не должен вылезать вверх.
        assert_eq!(glass.below_header().expand(page).top(), 20.0);
        assert_eq!(Bleed::same(20.0).sides().expand(page).bottom(), 600.0);
    }

    #[test]
    fn cover_scales_but_stays_in_range() {
        assert_eq!(cover_side(300.0, 300.0, false), 160.0);
        assert_eq!(cover_side(1600.0, 800.0, false), 400.0);
        assert_eq!(cover_side(3000.0, 3000.0, false), 440.0);
        assert_eq!(cover_side(500.0, 360.0, true), 158.4);
    }
}

//! Адаптивная раскладка: размеры, которые зависят от ширины окна.
//! Всё в «точках» egui (логических пикселях), поэтому масштаб Windows (125%, 150%) учитывается сам.
//! Правила собраны здесь, а не размазаны по экранам: так их проще менять и проверять тестами.

use egui::epaint::{Mesh, Vertex};
use egui::{Align, Color32, Layout, Margin, Rect, ScrollArea, Ui, UiBuilder, Shape, pos2, vec2};

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

/// Геометрия прокрутки внутри поверхности. Содержимое сохраняет прежнее начало, а область
/// ScrollArea занимает поверхность целиком, до всех четырёх краёв (раньше она отступала на 20 px
/// сверху и снизу, и прокручиваемое содержимое обрезалось жёсткой линией внутри панели).
/// Если понадобится вернуть отступ бегунка от скруглений, это константа ниже.
const SCROLLBAR_END_INSET: f32 = 0.0;

fn pane_scroll_geometry(page: Rect, bleed: Bleed) -> (Rect, Margin) {
    let expanded = bleed.expand(page);
    let end_inset = SCROLLBAR_END_INSET.min((expanded.height() * 0.5).max(0.0));
    let scroll_rect = Rect::from_min_max(
        pos2(expanded.left(), expanded.top() + end_inset),
        pos2(expanded.right(), expanded.bottom() - end_inset),
    );

    // Компенсируем верхний inset, чтобы первый виджет не сдвинулся вниз.
    // `Margin` хранит i8, а величины Bleed заданы в целых логических пикселях.
    let top = (bleed.top - end_inset).max(0.0) as i8;
    let margin = Margin {
        left: bleed.left.max(0.0) as i8,
        right: bleed.right.max(0.0) as i8,
        top,
        bottom: 0,
    };
    (scroll_rect, margin)
}

/// Область прокрутки занимает поверхность целиком (до всех четырёх краёв), поэтому содержимое
/// можно прокручивать под самый край панели. Бегунок при этом остаётся ВНУТРИ скруглений:
/// его дорожка короче области прокрутки на `corner` px сверху и снизу (`scroll_bar_rect`).
/// Если задан `mask`, в четырёх скруглённых углах поверх содержимого рисуется заливка цветом фона
/// за панелью: прямоугольная область прокрутки иначе показывала бы «квадратные» углы у карточек,
/// которые доходят до краёв. Закрытие получает готовую `ScrollArea` и вызывает у неё `show` или `show_rows`.
pub fn pane_scroll<R>(
    ui: &mut Ui,
    bleed: Bleed,
    corner: f32,
    mask: Option<Color32>,
    area: ScrollArea,
    show: impl FnOnce(ScrollArea, &mut Ui) -> R,
) -> R {
    let (scroll_rect, margin) = pane_scroll_geometry(ui.available_rect_before_wrap(), bleed);
    let bar_rect = Rect::from_min_max(
        pos2(scroll_rect.left(), scroll_rect.top() + corner),
        pos2(scroll_rect.right(), scroll_rect.bottom() - corner),
    );
    let out = ui
        .scope_builder(UiBuilder::new().max_rect(scroll_rect).layout(Layout::top_down(Align::Min)), |ui| {
            show(area.content_margin(margin).scroll_bar_rect(bar_rect), ui)
        })
        .inner;
    if let Some(color) = mask {
        paint_corner_masks(&ui.painter().with_clip_rect(scroll_rect), scroll_rect, corner, color);
    }
    out
}

/// Заливает «пустоты» между углом прямоугольника и дугой скругления радиуса `r` цветом `color`.
/// В каждом углу пустота это фигура между вершиной угла и четвертью окружности; её можно
/// разбить веером треугольников из вершины угла по точкам дуги.
pub fn paint_corner_masks(painter: &egui::Painter, rect: Rect, r: f32, color: Color32) {
    const STEPS: usize = 12;
    if r < 1.0 {
        return;
    }
    // (вершина угла, центр окружности, начальный угол дуги). Углы считаются по часовой стрелке в экранных осях.
    let corners = [
        (rect.left_top(), rect.left_top() + vec2(r, r), std::f32::consts::PI),
        (rect.right_top(), rect.right_top() + vec2(-r, r), std::f32::consts::PI * 1.5),
        (rect.right_bottom(), rect.right_bottom() + vec2(-r, -r), 0.0),
        (rect.left_bottom(), rect.left_bottom() + vec2(r, -r), std::f32::consts::PI * 0.5),
    ];
    let mut mesh = Mesh::default();
    for (corner, center, start) in corners {
        let base = mesh.vertices.len() as u32;
        mesh.vertices.push(Vertex { pos: corner, uv: egui::epaint::WHITE_UV, color });
        for i in 0..=STEPS {
            let a = start + std::f32::consts::FRAC_PI_2 * i as f32 / STEPS as f32;
            let pos = center + vec2(a.cos(), a.sin()) * r;
            mesh.vertices.push(Vertex { pos, uv: egui::epaint::WHITE_UV, color });
        }
        for i in 0..STEPS as u32 {
            mesh.indices.extend_from_slice(&[base, base + 1 + i, base + 2 + i]);
        }
    }
    painter.add(Shape::mesh(mesh));
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
    fn pane_scroll_fills_surface_without_shifting_content() {
        let page = Rect::from_min_max(pos2(20.0, 20.0), pos2(780.0, 580.0));
        let (scroll, margin) = pane_scroll_geometry(page, Bleed::same(20.0));

        // Область прокрутки доходит до краёв панели, а содержимое начинается там же, где и раньше.
        assert_eq!(scroll, Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0)));
        assert_eq!(scroll.top() + margin.top as f32, page.top());
        assert_eq!(margin.left, 20);
        assert_eq!(margin.right, 20);

        // В Liquid Glass нижнего Bleed нет: прокрутка заканчивается на низу страницы.
        let glass = Bleed { left: 20.0, top: 20.0, right: 20.0, bottom: 0.0 };
        let (glass_scroll, glass_margin) = pane_scroll_geometry(page, glass);
        assert_eq!(glass_scroll.top() + glass_margin.top as f32, page.top());
        assert_eq!(glass_scroll.bottom(), page.bottom());
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

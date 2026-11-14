//! Адаптивная раскладка: размеры, которые зависят от ширины окна.
//! Всё в «точках» egui (логических пикселях), поэтому масштаб Windows (125%, 150%) учитывается сам.
//! Правила собраны здесь, а не размазаны по экранам: так их проще менять и проверять тестами.

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

    pub fn is_compact(self) -> bool {
        self == Self::Compact
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
/// не должны разъезжаться по всему окну). Боковые колонки делят остаток поровну, поэтому
/// центр всегда точно посередине окна, а громкость доходит до правого края.
pub fn bar_columns(total: f32, gap: f32) -> BarColumns {
    let center = (total * 0.5).clamp(360.0, 720.0);
    let side = ((total - center - gap * 2.0) / 2.0).max(0.0);
    BarColumns { side, center }
}

/// Сторона обложки на экране «Сейчас играет»: растёт вместе с окном, но в разумных пределах.
pub fn cover_side(avail_w: f32, avail_h: f32) -> f32 {
    (avail_h * 0.5).min(avail_w * 0.5).clamp(160.0, 440.0)
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
    fn cover_scales_but_stays_in_range() {
        assert_eq!(cover_side(300.0, 300.0), 160.0);
        assert_eq!(cover_side(1600.0, 800.0), 400.0);
        assert_eq!(cover_side(3000.0, 3000.0), 440.0);
    }
}

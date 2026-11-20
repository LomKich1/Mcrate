//! Раскладка страницы плейлиста: боковая карточка слева и основной блок справа.
//! Общая для «Ваших загрузок» и подэкранов коллекции. На узком окне блоки идут друг под другом.

use egui::{Align, Layout, Ui, vec2};

use crate::ui::layout::centered_column;

/// Максимальная ширина страницы, px.
const PAGE_MAX_WIDTH: f32 = 1360.0;
/// С этой ширины боковая карточка встаёт слева от основного блока; уже этого блоки складываются вертикально.
const SPLIT_MIN_WIDTH: f32 = 920.0;
/// Ширина боковой колонки в широкой раскладке, px.
const SIDE_WIDTH: f32 = 292.0;
/// Зазор между колонками в широкой раскладке, px.
const COLUMN_GAP: f32 = 28.0;
/// Сторона обложки/иконки в боковой карточке в широкой раскладке, px.
pub(in crate::ui) const SIDE_COVER_SIZE: f32 = 252.0;

/// Состояние раскладки, которое видит содержимое страницы.
pub(in crate::ui) struct SplitPage {
    /// `true`: колонки рядом, `false`: блоки друг под другом.
    pub wide: bool,
    available_height: f32,
    main_align: Align,
    narrow_gap: f32,
}

impl SplitPage {
    /// Боковая карточка: слева (колонка `SIDE_WIDTH`, выравнивание по центру) или сверху.
    pub(crate) fn side(&self, ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
        if self.wide {
            ui.allocate_ui_with_layout(vec2(SIDE_WIDTH, self.available_height), Layout::top_down(Align::Center), add);
        } else {
            add(ui);
        }
    }

    /// Основной блок: справа от карточки (на всю оставшуюся ширину) или под ней через `narrow_gap`.
    pub(crate) fn main(&self, ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
        if self.wide {
            ui.add_space(COLUMN_GAP);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), self.available_height), Layout::top_down(self.main_align), add);
        } else {
            ui.add_space(self.narrow_gap);
            add(ui);
        }
    }
}

/// Страница в две колонки. `main_align`: выравнивание основного блока в широкой раскладке,
/// `narrow_gap`: отступ между блоками в узкой. Внутри вызывают `page.side(..)`, затем `page.main(..)`.
pub(in crate::ui) fn split_page(ui: &mut Ui, main_align: Align, narrow_gap: f32, body: impl FnOnce(&mut Ui, &SplitPage)) {
    centered_column(ui, PAGE_MAX_WIDTH, |ui| {
        let wide = ui.available_width() >= SPLIT_MIN_WIDTH;
        let page = SplitPage { wide, available_height: ui.available_height(), main_align, narrow_gap };
        if wide {
            ui.horizontal_top(|ui| body(ui, &page));
        } else {
            body(ui, &page);
        }
    });
}

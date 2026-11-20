//! Шапки экранов: «назад + заголовок» и «заголовок + поиск + кнопки».

use crate::icons::Icon;
use crate::theme::{TextRole, Theme};
use crate::ui::widgets::{ButtonKind, glass_icon_button, search_bar};
use egui::{Align, Layout, Rect, Response, RichText, Sense, Ui, UiBuilder, pos2, vec2};

/// Шапка подэкрана: круглая кнопка «назад» и заголовок. Возвращает `true`, если нажали «назад».
pub(in crate::ui) fn back_header(ui: &mut Ui, theme: &Theme, title: &str) -> bool {
    ui.horizontal(|ui| {
        let clicked = glass_icon_button(ui, theme, Icon::ChevronBack, 40.0, ButtonKind::Plain, false, "Назад").clicked();
        ui.heading(title);
        clicked
    })
    .inner
}

/// Крупный заголовок экрана с приглушённой подписью (как «Моя волна» на главной).
pub(in crate::ui) fn screen_title(ui: &mut Ui, theme: &Theme, title: &str, subtitle: &str) {
    let p = &theme.palette;
    ui.add(egui::Label::new(RichText::new(title).size(theme.text_size(TextRole::DisplaySmall)).strong()).truncate());
    ui.add(egui::Label::new(RichText::new(subtitle).size(theme.text_size(TextRole::LabelLarge)).color(p.on_surface_variant)).truncate());
}

/// Что показывает шапка `title_search_row`.
#[derive(Clone, Copy)]
pub(in crate::ui) struct TitleRow<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    /// Рисовать перед заголовком кнопку «назад».
    pub back: bool,
    /// Подсказка в пустом поле поиска.
    pub hint: &'a str,
    /// Ширина блока кнопок справа; 0 = кнопок нет.
    pub trailing_w: f32,
    /// Поле поиска по центру строки (а не прижато к заголовку).
    pub centered_search: bool,
}

/// Общая шапка экранов: слева заголовок с подписью (при `back` перед ним кнопка «назад»),
/// правее поиск, а справа (если `trailing_w` > 0) блок кнопок, который рисует `trailing`.
/// Всё стоит в одной строке фиксированной высоты и центрируется по ней: так «назад», заголовок,
/// поиск и кнопки лежат на одной линии. Возвращает ответ поля поиска и признак нажатия «назад».
pub(in crate::ui) fn title_search_row(
    ui: &mut Ui,
    theme: &Theme,
    spec: &TitleRow,
    value: &mut String,
    trailing: &mut dyn FnMut(&mut Ui),
) -> (Response, bool) {
    let TitleRow { title, subtitle, back, hint, trailing_w, centered_search } = *spec;
    const ROW_H: f32 = 60.0;
    const BACK_W: f32 = 48.0;
    const GAP: f32 = 16.0;
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let right_edge = if trailing_w > 0.0 { row.right() - trailing_w - 12.0 } else { row.right() };
    let centered = Layout::left_to_right(Align::Center);
    let mut back_clicked = false;

    let mut x = row.left();
    if back {
        let r = Rect::from_min_max(pos2(x, row.top()), pos2(x + BACK_W, row.bottom()));
        back_clicked = ui
            .scope_builder(UiBuilder::new().max_rect(r).layout(centered), |ui| {
                glass_icon_button(ui, theme, Icon::ChevronBack, 40.0, ButtonKind::Plain, false, "Назад").clicked()
            })
            .inner;
        x += BACK_W;
    }
    let title_w = ((right_edge - x) * 0.45).clamp(80.0, 360.0);
    let title_rect = Rect::from_min_size(pos2(x, row.top() + 2.0), vec2(title_w, ROW_H - 2.0));
    ui.scope_builder(UiBuilder::new().max_rect(title_rect).layout(Layout::top_down(Align::Min)), |ui| {
        screen_title(ui, theme, title, subtitle);
    });
    x += title_w + GAP;

    let search_rect = if centered_search {
        // Поле по центру строки (а не прижато к заголовку): симметрично вокруг середины, но не наезжает на заголовок.
        let half = (row.center().x - x).min(right_edge - row.center().x).clamp(100.0, 360.0);
        Rect::from_min_max(pos2(row.center().x - half, row.top()), pos2(row.center().x + half, row.bottom()))
    } else {
        Rect::from_min_max(pos2(x.min(right_edge), row.top()), pos2(right_edge, row.bottom()))
    };
    let search = ui.scope_builder(UiBuilder::new().max_rect(search_rect).layout(centered), |ui| search_bar(ui, theme, value, hint)).inner;

    if trailing_w > 0.0 {
        let r = Rect::from_min_max(pos2(row.right() - trailing_w, row.top()), row.max);
        ui.scope_builder(UiBuilder::new().max_rect(r).layout(Layout::right_to_left(Align::Center)), |ui| trailing(ui));
    }
    (search, back_clicked)
}

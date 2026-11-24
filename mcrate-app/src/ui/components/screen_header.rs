//! Шапки экранов: «назад + заголовок» и «заголовок + поиск + кнопки».

use crate::icons::Icon;
use crate::theme::{TextRole, Theme};
use crate::ui::widgets::{IconButton, glass_icon_button, search_bar};
use egui::{Align, Layout, Rect, Response, RichText, Sense, Ui, UiBuilder, pos2, vec2};

/// Шапка подэкрана: круглая кнопка «назад» и заголовок. Возвращает `true`, если нажали «назад».
pub(in crate::ui) fn back_header(ui: &mut Ui, theme: &Theme, title: &str) -> bool {
    ui.horizontal(|ui| {
        let clicked = glass_icon_button(ui, theme, IconButton::new(Icon::ChevronBack, 40.0, "Назад")).clicked();
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

/// Высота строки шапки, px.
const ROW_H: f32 = 60.0;
/// Ширина места под кнопку «назад», px.
const BACK_W: f32 = 48.0;
/// Зазор между заголовком и полем поиска, px.
const GAP: f32 = 16.0;

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
    /// Насколько показан заголовок: 1 = как обычно, 0 = его нет (поиск встаёт сразу за «назад»),
    /// между ними заголовок выезжает слева и проявляется, а поиск плавно уступает ему место.
    pub title_shown: f32,
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
    let TitleRow { title, subtitle, back, hint, trailing_w, centered_search, title_shown } = *spec;
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let right_edge = if trailing_w > 0.0 { row.right() - trailing_w - 12.0 } else { row.right() };
    let centered = Layout::left_to_right(Align::Center);
    let mut back_clicked = false;

    let mut x = row.left();
    if back {
        back_clicked = back_button(ui, theme, row);
        x += BACK_W;
    }
    let title_w = ((right_edge - x) * 0.45).clamp(80.0, 360.0);
    let shown = title_shown.clamp(0.0, 1.0);
    if shown > 0.0 {
        let title_rect = Rect::from_min_size(pos2(x, row.top() + 2.0), vec2(title_w, ROW_H - 2.0));
        reveal_title(ui, theme, title_rect, (title, subtitle), shown);
    }
    x += (title_w + GAP) * shown;

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

/// Круглая кнопка «назад» в левом краю строки `row`. Возвращает `true`, если нажали.
fn back_button(ui: &mut Ui, theme: &Theme, row: Rect) -> bool {
    let r = Rect::from_min_max(pos2(row.left(), row.top()), pos2(row.left() + BACK_W, row.bottom()));
    ui.scope_builder(UiBuilder::new().max_rect(r).layout(Layout::left_to_right(Align::Center)), |ui| {
        glass_icon_button(ui, theme, IconButton::new(Icon::ChevronBack, 40.0, "Назад")).clicked()
    })
    .inner
}

/// Заголовок с подписью в `title_rect`. `shown` (0..1) < 1: заголовок выезжает и проявляется,
/// его обрезает по ширине, которую уже уступило соседнее поле.
fn reveal_title(ui: &mut Ui, theme: &Theme, title_rect: Rect, (title, subtitle): (&str, &str), shown: f32) {
    ui.scope_builder(UiBuilder::new().max_rect(title_rect).layout(Layout::top_down(Align::Min)), |ui| {
        if shown < 1.0 {
            let revealed = Rect::from_min_size(title_rect.min, vec2(title_rect.width() * shown, ROW_H));
            ui.set_clip_rect(ui.clip_rect().intersect(revealed));
            ui.set_opacity(shown);
        }
        screen_title(ui, theme, title, subtitle);
    });
}

/// Шапка без поиска: кнопка «назад» и заголовок с подписью, который проявляется так же, как в
/// `title_search_row` (`title_shown`: 0 = названия нет, 1 = показано целиком). Возвращает `true`, если нажали «назад».
pub(in crate::ui) fn back_title_row(ui: &mut Ui, theme: &Theme, (title, subtitle): (&str, &str), title_shown: f32) -> bool {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let back_clicked = back_button(ui, theme, row);
    let shown = title_shown.clamp(0.0, 1.0);
    if shown > 0.0 {
        let x = row.left() + BACK_W;
        let title_w = (row.right() - x).clamp(80.0, 360.0);
        let title_rect = Rect::from_min_size(pos2(x, row.top() + 2.0), vec2(title_w, ROW_H - 2.0));
        reveal_title(ui, theme, title_rect, (title, subtitle), shown);
    }
    back_clicked
}

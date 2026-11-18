//! Общие виджеты новых экранов настроек (только Material 3; Liquid Glass оставляет прежний экран).
//!
//! Что здесь есть:
//! - `group_label` + `settings_group`: подпись раздела и тональный блок со строками без разделителей
//!   (иконка в плитке, название, справа значение/шеврон/переключатель);
//! - `m3_switch`: переключатель Material 3 (дорожка 52x32, бегунок растёт и получает галочку);
//! - `settings_tiles`: плитки 2x2 с крупным бледным значком и градиентом;
//! - `profile_card`: карточка профиля.
//! Всё рисуется самим кодом по ролям палитры, поэтому подстраивается под любую из 12 тем.

use egui::epaint::Mesh;
use egui::{Align2, Color32, CursorIcon, FontFamily, FontId, Frame, Id, Margin, Painter, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons as mi;

use super::widgets::{glide_begin, glide_end_flat, squish};
use crate::theme::{ShapeRole, TextRole, Theme, with_alpha};

/// Рисует глиф Material Symbols по центру точки. Поправка вверх на 5% размера нужна потому, что
/// строка шрифта иконок центрируется по метрикам шрифта, а не по самому рисунку (так же в `icons::paint`).
pub fn paint_symbol(painter: &Painter, icon: MaterialIcon, center: Pos2, size: f32, color: Color32) {
    painter.text(
        center - vec2(0.0, 0.05 * size),
        Align2::CENTER_CENTER,
        icon.codepoint,
        FontId::new(size, icon.font_family()),
        color,
    );
}

/// Подпись раздела над блоком строк: «ОФОРМЛЕНИЕ», «ПОДКЛЮЧЕНИЯ»... Мелкие заглавные с разрядкой.
pub fn group_label(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add_space(22.0);
    ui.horizontal(|ui| {
        ui.add_space(18.0);
        ui.label(
            RichText::new(text.to_uppercase())
                .size(12.0)
                .strong()
                // Разрядка между буквами (в egui её задаёт RichText, в пикселях).
                .extra_letter_spacing(1.6)
                .color(with_alpha(theme.palette.on_surface_variant, 210)),
        );
    });
    ui.add_space(6.0);
}

/// Один чип выбора («Низкое», «3s»...). `locked` = недоступен (замок, нажатие игнорируется).
pub struct Chip<'a> {
    pub label: &'a str,
    pub locked: bool,
}

impl<'a> Chip<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, locked: false }
    }

    pub fn locked(label: &'a str) -> Self {
        Self { label, locked: true }
    }
}

/// Выбор одного варианта из нескольких: ряд чипов под названием строки.
pub struct Choice<'a> {
    pub chips: Vec<Chip<'a>>,
    /// Номер выбранного чипа; по нажатию меняется прямо здесь.
    pub selected: &'a mut usize,
    /// `true`: чипы одинаковой ширины на всю строку (сегменты), `false`: по ширине текста с переносом.
    pub stretch: bool,
}

/// Что стоит справа в строке.
pub enum Trailing<'a> {
    /// Только шеврон «>» (переход на подэкран).
    Chevron,
    /// Текущее значение и шеврон («Русский >»).
    Value(String),
    /// Только значение справа, без шеврона (строка-сведение).
    Label(String),
    /// Переключатель. Нажатие на всю строку тоже переключает.
    Switch(&'a mut bool),
    /// Ряд чипов под названием (строка сама не нажимается, нажимаются чипы).
    Choice(Choice<'a>),
}

/// Описание одной строки блока.
pub struct Row<'a> {
    pub icon: Option<MaterialIcon>,
    /// Пустое название = строка без заголовка (например, только сегменты).
    pub title: &'a str,
    /// Пояснение серым под названием (переносится по ширине).
    pub subtitle: Option<&'a str>,
    pub trailing: Trailing<'a>,
}

impl<'a> Row<'a> {
    pub fn new(icon: MaterialIcon, title: &'a str, trailing: Trailing<'a>) -> Self {
        Self { icon: Some(icon), title, subtitle: None, trailing }
    }

    /// Строка без иконки слева.
    pub fn plain(title: &'a str, trailing: Trailing<'a>) -> Self {
        Self { icon: None, title, subtitle: None, trailing }
    }

    pub fn subtitle(mut self, text: &'a str) -> Self {
        self.subtitle = Some(text);
        self
    }
}

/// Минимальная высота строки блока.
const ROW_H: f32 = 60.0;
/// Сторона плитки с иконкой слева в строке.
const ROW_ICON_TILE: f32 = 38.0;
const CHIP_H: f32 = 40.0;
const CHIP_GAP: f32 = 8.0;

/// Тональный блок со строками. Возвращает номер строки, на которую нажали (переключатели и чипы
/// сами меняют своё значение и сюда не попадают). `id` отличает блоки друг от друга
/// (подсветка при наведении скользит между строками ВНУТРИ блока, как в списках треков).
pub fn settings_group(ui: &mut Ui, theme: &Theme, id: &str, rows: Vec<Row<'_>>) -> Option<usize> {
    let p = &theme.palette;
    let mut clicked = None;
    let radius = theme.shape_radius(ShapeRole::ExtraLarge);

    ui.scope(|ui| {
        // Строки идут встык, без разделителей: их отделяет только скользящая подсветка.
        ui.spacing_mut().item_spacing.y = 0.0;
        Frame::new()
            .fill(p.surface_container_high)
            .corner_radius(radius)
            // Отступ 8 px нужен, чтобы подсветка строки (со своим скруглением) не торчала за углы блока.
            .inner_margin(Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // Слой под подсветку резервируем до строк: текст рисуется поверх неё.
                let glide_slot = glide_begin(ui);
                let mut hovered = None;
                for (i, row) in rows.into_iter().enumerate() {
                    let (resp, activated, interactive) = row_ui(ui, theme, id, i, row);
                    if interactive && resp.hovered() {
                        hovered = Some((i as u64, resp.rect));
                    }
                    if activated {
                        clicked = Some(i);
                    }
                }
                // Слой состояния M3: 8% цвета контента под курсором (alpha 20 из 255).
                glide_end_flat(ui, theme, glide_slot, ui.scope_id().with(("settings_glide", id)), hovered, 20.0, (p.on_surface, 20));
            });
    });
    clicked
}

/// Одиночный тональный блок под произвольное содержимое (например, ряд цветов акцента).
pub fn settings_card<R>(ui: &mut Ui, theme: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(theme.palette.surface_container_high)
        .corner_radius(theme.shape_radius(ShapeRole::ExtraLarge))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Серая поясняющая строка под блоком.
pub fn hint_text(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(18.0);
        ui.add(egui::Label::new(RichText::new(text).size(theme.text_size(TextRole::BodySmall)).color(with_alpha(theme.palette.on_surface_variant, 200))).wrap());
    });
}

/// Одна строка блока. Возвращает отклик, признак «нажали на действие» (для переключателя и чипов это
/// `false`: они уже изменили своё значение внутри) и признак «строка нажимаемая» (для подсветки).
fn row_ui(ui: &mut Ui, theme: &Theme, group_id: &str, index: usize, mut row: Row<'_>) -> (Response, bool, bool) {
    let p = &theme.palette;
    let total_w = ui.available_width();
    let row_id = ui.scope_id().with(("settings_row", group_id, index));
    let text_off = if row.icon.is_some() { 10.0 + ROW_ICON_TILE + 16.0 } else { 14.0 };
    let reserve = match row.trailing {
        Trailing::Switch(_) => 14.0 + 52.0 + 16.0,
        Trailing::Chevron | Trailing::Value(_) => 52.0,
        Trailing::Label(_) => 150.0,
        _ => 14.0,
    };
    let title_font = FontId::proportional(theme.text_size(TextRole::BodyLarge));
    let sub_font = FontId::proportional(theme.text_size(TextRole::BodySmall));
    let text_width = (total_w - text_off - reserve).max(40.0);

    // Подписи раскладываем заранее: от их высоты зависит высота строки.
    // На узкой панели длинные названия переносятся, а не наезжают на переключатель или значение.
    let title_galley = (!row.title.is_empty()).then(|| ui.painter().layout(row.title.to_string(), title_font, p.on_surface, text_width));
    let sub_galley = row.subtitle.map(|t| ui.painter().layout(t.to_string(), sub_font, p.on_surface_variant, text_width));
    let title_h = title_galley.as_ref().map_or(0.0, |g| g.size().y);
    let sub_h = sub_galley.as_ref().map_or(0.0, |g| g.size().y);

    // Для рядов чипов считаем расположение каждого чипа заранее (x, y, ширина) относительно начала ряда.
    let mut chip_slots: Vec<(f32, f32, f32)> = Vec::new();
    let mut chips_h = 0.0;
    if let Trailing::Choice(choice) = &row.trailing {
        let avail = (total_w - text_off - 14.0).max(60.0);
        // На широком окне чипы не растягиваются на сотни пикселей каждый:
        // оставляем компактную группу, а при узком окне используем всю доступную ширину.
        let chip_width = if total_w >= 900.0 { avail.min(560.0) } else { avail };
        let n = choice.chips.len().max(1) as f32;
        let (mut x, mut y) = (0.0_f32, 0.0_f32);
        for chip in &choice.chips {
            let w = if choice.stretch {
                (chip_width - CHIP_GAP * (n - 1.0)) / n
            } else {
                let tw = ui.painter().layout_no_wrap(chip.label.to_string(), FontId::proportional(theme.text_size(TextRole::LabelLarge)), p.on_surface).size().x;
                tw + 36.0 + if chip.locked { 22.0 } else { 0.0 }
            };
            if x > 0.0 && x + w > avail + 0.5 {
                x = 0.0;
                y += CHIP_H + CHIP_GAP;
            }
            chip_slots.push((x, y, w));
            x += w + CHIP_GAP;
        }
        chips_h = y + CHIP_H;
    }
    let is_choice = matches!(row.trailing, Trailing::Choice(_));

    // Высота строки.
    let chips_top_rel = if title_galley.is_some() { 29.0 + title_h * 0.5 + 10.0 } else { 14.0 };
    let row_h = if is_choice {
        (chips_top_rel + chips_h + 16.0).max(ROW_H)
    } else {
        let block = title_h + if sub_h > 0.0 { 3.0 + sub_h } else { 0.0 };
        (block + 28.0).max(ROW_H)
    };

    let sense = if is_choice { Sense::hover() } else { Sense::click() };
    let (rect, resp) = ui.allocate_exact_size(vec2(total_w, row_h), sense);
    // Продавливание при нажатии, те же параметры, что у строки трека.
    let shown = if is_choice { rect } else { squish(ui, resp.id, &resp, rect, 0.012, 0.06) };
    let painter = ui.painter();
    let cy = shown.center().y;
    let text_x = shown.left() + text_off;

    // Плитка с иконкой слева (у рядов чипов она на уровне названия).
    if let Some(icon) = row.icon {
        let icon_cy = if is_choice { shown.top() + 29.0 } else { cy };
        let tile = Rect::from_center_size(pos2(shown.left() + 10.0 + ROW_ICON_TILE * 0.5, icon_cy), vec2(ROW_ICON_TILE, ROW_ICON_TILE));
        painter.rect_filled(tile, theme.shape_radius(ShapeRole::Medium), p.surface_container_highest);
        paint_symbol(painter, icon, tile.center(), 20.0, p.on_surface);
    }

    // Название и пояснение.
    if is_choice {
        if let Some(g) = title_galley {
            painter.galley(pos2(text_x, shown.top() + 29.0 - title_h * 0.5), g, p.on_surface);
        }
    } else {
        let block = title_h + if sub_h > 0.0 { 3.0 + sub_h } else { 0.0 };
        let top = cy - block * 0.5;
        if let Some(g) = title_galley {
            painter.galley(pos2(text_x, top), g, p.on_surface);
        }
        if let Some(g) = sub_galley {
            painter.galley(pos2(text_x, top + title_h + 3.0), g, p.on_surface_variant);
        }
    }

    // Правая часть.
    let right = shown.right() - 14.0;
    let mut activated = resp.clicked();
    match &mut row.trailing {
        Trailing::Chevron => {
            paint_symbol(painter, mi::ICON_CHEVRON_RIGHT, pos2(right - 10.0, cy), 24.0, p.on_surface_variant);
        }
        Trailing::Value(value) => {
            paint_symbol(painter, mi::ICON_CHEVRON_RIGHT, pos2(right - 10.0, cy), 24.0, p.on_surface_variant);
            painter.text(
                pos2(right - 26.0, cy),
                Align2::RIGHT_CENTER,
                value.as_str(),
                FontId::proportional(theme.text_size(TextRole::BodyMedium)),
                p.on_surface_variant,
            );
        }
        Trailing::Label(value) => {
            painter.text(pos2(right - 4.0, cy), Align2::RIGHT_CENTER, value.as_str(), FontId::proportional(theme.text_size(TextRole::BodyMedium)), p.on_surface_variant);
        }
        Trailing::Switch(on) => {
            if activated {
                **on = !**on;
                activated = false; // переключатель сам обработал нажатие
            }
            paint_switch(ui, theme, row_id, pos2(right - 26.0, cy), **on);
            let (label, on_now) = (row.title.to_string(), **on);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on_now, label.clone()));
        }
        Trailing::Choice(choice) => {
            let origin = pos2(text_x, shown.top() + chips_top_rel);
            for (i, (chip, &(dx, dy, w))) in choice.chips.iter().zip(chip_slots.iter()).enumerate() {
                let chip_rect = Rect::from_min_size(origin + vec2(dx, dy), vec2(w, CHIP_H));
                let sense = if chip.locked { Sense::hover() } else { Sense::click() };
                let chip_resp = ui.interact(chip_rect, row_id.with(("chip", i)), sense);
                let is_selected = *choice.selected == i;
                paint_chip(ui, theme, chip_rect, &chip_resp, chip, is_selected);
                let (label, locked) = (chip.label.to_string(), chip.locked);
                chip_resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, !locked, is_selected, label.clone()));
                let cursor = if chip.locked { CursorIcon::NotAllowed } else { CursorIcon::PointingHand };
                if chip_resp.on_hover_cursor(cursor).clicked() && !chip.locked {
                    *choice.selected = i;
                }
            }
        }
    }

    if is_choice {
        (resp, false, false)
    } else {
        (resp.on_hover_cursor(CursorIcon::PointingHand), activated, true)
    }
}

/// Рисует чип выбора: выбранный залит цветом primary, остальные тональные; недоступный с замком.
fn paint_chip(ui: &Ui, theme: &Theme, rect: Rect, resp: &Response, chip: &Chip<'_>, selected: bool) {
    let p = &theme.palette;
    let painter = ui.painter();
    let radius = rect.height() * 0.5;
    let (fill, fg) = if selected { (p.primary, p.on_primary) } else { (p.surface_container_highest, p.on_surface_variant) };
    painter.rect_filled(rect, radius, fill);
    let fg = if chip.locked { with_alpha(fg, 130) } else { fg };
    if !chip.locked {
        // Слой состояния: кружок от курсора (ripple.rs).
        super::ripple::state_layer(ui, theme, painter, resp, rect, radius, if selected { p.on_primary } else { p.on_surface }, 20, 30);
    }
    let font = FontId::proportional(theme.text_size(TextRole::LabelLarge));
    if chip.locked {
        let tw = painter.layout_no_wrap(chip.label.to_string(), font.clone(), fg).size().x;
        let start = rect.center().x - (tw + 22.0) * 0.5;
        paint_symbol(painter, mi::ICON_LOCK, pos2(start + 8.0, rect.center().y), 16.0, fg);
        painter.text(pos2(start + 22.0, rect.center().y), Align2::LEFT_CENTER, chip.label, font, fg);
    } else {
        painter.text(rect.center(), Align2::CENTER_CENTER, chip.label, font, fg);
    }
}

/// Ряд цветных кружков для выбора акцента. Возвращает номер кружка, на который нажали.
/// `selected` = номер выбранного (если ни один не подходит, `None`). Выбранный получает кольцо и галочку.
pub fn accent_picker(ui: &mut Ui, theme: &Theme, id: &str, swatches: &[Color32], selected: Option<usize>) -> Option<usize> {
    const D: f32 = 48.0;
    const GAP: f32 = 18.0;
    let p = &theme.palette;
    let avail = ui.available_width();
    let per_row = (((avail + GAP) / (D + GAP)).floor() as usize).max(1);
    let rows = swatches.len().div_ceil(per_row);
    let (area, _) = ui.allocate_exact_size(vec2(avail, rows as f32 * D + rows.saturating_sub(1) as f32 * GAP + 8.0), Sense::hover());

    let mut clicked = None;
    for (i, &color) in swatches.iter().enumerate() {
        let (col, row) = (i % per_row, i / per_row);
        let center = pos2(area.left() + 4.0 + D * 0.5 + col as f32 * (D + GAP), area.top() + 4.0 + D * 0.5 + row as f32 * (D + GAP));
        let rect = Rect::from_center_size(center, vec2(D + 8.0, D + 8.0));
        let resp = ui.interact(rect, ui.scope_id().with(("accent", id, i)), Sense::click());
        let is_selected = selected == Some(i);
        let t = ui.ctx().animate_bool_with_time(resp.id.with("sel"), is_selected, if theme.settings.reduce_motion { 0.0 } else { 0.14 });
        let painter = ui.painter();
        let grow = if resp.hovered() { 1.0 } else { 0.0 };
        painter.circle_filled(center, D * 0.5 - 2.0 * t + grow, color);
        if t > 0.01 {
            painter.circle_stroke(center, D * 0.5 + 3.0, Stroke::new(3.0 * t, with_alpha(p.on_surface, (t * 255.0) as u8)));
            // Галочка тёмная на светлом кружке и светлая на тёмном.
            let lum = 0.299 * color.r() as f32 + 0.587 * color.g() as f32 + 0.114 * color.b() as f32;
            let tick = if lum > 150.0 { Color32::from_gray(20) } else { Color32::WHITE };
            paint_symbol(painter, mi::ICON_CHECK, center, 22.0, with_alpha(tick, (t * 255.0) as u8));
        }
        let label = format!("Акцент {}", i + 1);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, is_selected, label.clone()));
        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// Рисует переключатель Material 3 (52x32) с плавным переходом. `center` это центр дорожки.
/// Выкл: контурная дорожка и маленький бегунок; вкл: дорожка цвета primary и крупный бегунок с галочкой.
fn paint_switch(ui: &Ui, theme: &Theme, id: Id, center: Pos2, on: bool) {
    let p = &theme.palette;
    let time = if theme.settings.reduce_motion { 0.0 } else { 0.16 };
    let t = ui.ctx().animate_bool_with_time(id.with("switch"), on, time);
    let track = Rect::from_center_size(center, vec2(52.0, 32.0));
    let painter = ui.painter();

    painter.rect_filled(track, 16.0, p.surface_container_highest.lerp_to_gamma(p.primary, t));
    // Контур виден только у выключенного (по мере включения гаснет).
    let outline_alpha = ((1.0 - t) * 255.0).round() as u8;
    if outline_alpha > 0 {
        painter.rect_stroke(track, 16.0, Stroke::new(2.0, with_alpha(p.outline, outline_alpha)), StrokeKind::Inside);
    }

    let radius = 8.0 + 4.0 * t;
    let x = track.left() + 16.0 + (track.width() - 32.0) * t;
    let thumb = pos2(x, center.y);
    painter.circle_filled(thumb, radius, p.outline.lerp_to_gamma(p.on_primary, t));
    if t > 0.02 {
        paint_symbol(painter, mi::ICON_CHECK, thumb, 16.0, with_alpha(p.primary, (t * 255.0).round() as u8));
    }
}

/// Что рисовать крупным бледным значком на плитке.
pub enum Glyph {
    Symbol(MaterialIcon),
    /// Крупная надпись (например код языка «RU»).
    Text(&'static str),
}

/// Одна плитка 2x2.
pub struct Tile {
    pub icon: MaterialIcon,
    pub title: &'static str,
    pub subtitle: Option<String>,
    pub glyph: Glyph,
}

const TILE_H: f32 = 116.0;
const TILE_GAP: f32 = 12.0;

/// Адаптивная сетка плиток. Возвращает номер плитки, на которую нажали.
pub fn settings_tiles(ui: &mut Ui, theme: &Theme, tiles: &[Tile]) -> Option<usize> {
    let total_w = ui.available_width();
    // На узком окне оставляем одну колонку, затем постепенно добавляем ещё колонки,
    // чтобы плитки не становились чрезмерно растянутыми.
    let columns = if total_w < 620.0 { 1 } else if total_w < 1000.0 { 2 } else if total_w < 1400.0 { 3 } else { 4 };
    let rows = tiles.len().div_ceil(columns);
    let tile_w = (total_w - TILE_GAP * (columns as f32 - 1.0)) / columns as f32;
    let total_h = rows as f32 * TILE_H + rows.saturating_sub(1) as f32 * TILE_GAP;
    let (area, _) = ui.allocate_exact_size(vec2(total_w, total_h), Sense::hover());

    let mut clicked = None;
    for (i, tile) in tiles.iter().enumerate() {
        let (col, row) = (i % columns, i / columns);
        let rect = Rect::from_min_size(
            pos2(area.left() + col as f32 * (tile_w + TILE_GAP), area.top() + row as f32 * (TILE_H + TILE_GAP)),
            vec2(tile_w, TILE_H),
        );
        let resp = ui.interact(rect, ui.scope_id().with(("settings_tile", i)), Sense::click());
        paint_tile(ui, theme, rect, &resp, tile);
        let label = tile.title;
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

fn paint_tile(ui: &Ui, theme: &Theme, base: Rect, resp: &Response, tile: &Tile) {
    let p = &theme.palette;
    let rect = squish(ui, resp.id, resp, base, 0.02, 0.04);
    let radius = theme.shape_radius(ShapeRole::ExtraLarge) as f32;
    let painter = ui.painter();

    painter.rect_filled(rect, radius, p.surface_container_high);
    // Градиент: слева чистая поверхность, к правому краю подмешивается primary_container.
    paint_gradient_x(painter, rect.shrink(0.5), radius - 0.5, |t| {
        let k = ((t - 0.30) / 0.70).clamp(0.0, 1.0);
        let k = k * k * (3.0 - 2.0 * k); // smoothstep: плавный вход и выход
        with_alpha(p.primary_container, (k * 190.0).round() as u8)
    });

    // Крупный бледный значок справа.
    let glyph_center = pos2(rect.right() - 64.0, rect.center().y);
    let glyph_color = with_alpha(p.on_primary_container, 64);
    match &tile.glyph {
        Glyph::Symbol(icon) => paint_symbol(painter, *icon, glyph_center, 92.0, glyph_color),
        Glyph::Text(text) => {
            painter.text(glyph_center, Align2::CENTER_CENTER, *text, FontId::new(68.0, FontFamily::Proportional), glyph_color);
        }
    }

    // Маленькая иконка слева сверху.
    let icon_tile = Rect::from_min_size(rect.min + vec2(16.0, 16.0), vec2(40.0, 40.0));
    painter.rect_filled(icon_tile, theme.shape_radius(ShapeRole::Medium), p.surface_container_highest);
    paint_symbol(painter, tile.icon, icon_tile.center(), 22.0, p.on_surface);

    // Подписи слева снизу.
    let x = rect.left() + 18.0;
    match &tile.subtitle {
        Some(subtitle) => {
            painter.text(pos2(x, rect.bottom() - 36.0), Align2::LEFT_CENTER, tile.title, FontId::proportional(theme.text_size(TextRole::BodyLarge)), p.on_surface);
            painter.text(pos2(x, rect.bottom() - 17.0), Align2::LEFT_CENTER, subtitle, FontId::proportional(theme.text_size(TextRole::BodySmall)), p.on_surface_variant);
        }
        None => {
            painter.text(pos2(x, rect.bottom() - 24.0), Align2::LEFT_CENTER, tile.title, FontId::proportional(theme.text_size(TextRole::BodyLarge)), p.on_surface);
        }
    }

    // Контур и слой состояния (наведение 8%, нажатие 12%).
    painter.rect_stroke(rect, radius, Stroke::new(1.0, with_alpha(p.outline_variant, 110)), StrokeKind::Inside);
    super::ripple::state_layer(ui, theme, painter, resp, rect, radius, p.on_surface, 20, 30);
}

/// Горизонтальный градиент внутри скруглённого прямоугольника. `color_at(t)` даёт цвет в точке
/// `t` (0 = левый край, 1 = правый). egui умеет заливать скруглённые прямоугольники только одним
/// цветом, поэтому собираем сетку из узких вертикальных полос: верх и низ каждой полосы
/// повторяют скругление углов. Полосы гуще у краёв, где кривая круче.
fn paint_gradient_x(painter: &Painter, rect: Rect, radius: f32, color_at: impl Fn(f32) -> Color32) {
    const STEPS: usize = 48;
    let radius = radius.clamp(0.0, rect.height().min(rect.width()) * 0.5);
    // Насколько нижняя/верхняя граница отступает внутрь на расстоянии `x` от левого края.
    let inset = |x: f32| -> f32 {
        let d = x.min(rect.width() - x);
        if d >= radius {
            0.0
        } else {
            let dy = radius - d;
            radius - (radius * radius - dy * dy).max(0.0).sqrt()
        }
    };
    let mut mesh = Mesh::default();
    for i in 0..=STEPS {
        // Косинусное распределение: узкие полосы у обоих краёв, широкие посередине.
        let u = 0.5 - 0.5 * (std::f32::consts::PI * i as f32 / STEPS as f32).cos();
        let x = rect.width() * u;
        let color = color_at(u);
        let inset = inset(x);
        mesh.colored_vertex(pos2(rect.left() + x, rect.top() + inset), color);
        mesh.colored_vertex(pos2(rect.left() + x, rect.bottom() - inset), color);
        if i > 0 {
            let base = (i as u32 - 1) * 2;
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 1, base + 3, base + 2);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// Карточка профиля: кружок с инициалом, имя, подпись и шеврон.
pub fn profile_card(ui: &mut Ui, theme: &Theme, name: &str, subtitle: &str) -> Response {
    let p = &theme.palette;
    let (base, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 88.0), Sense::click());
    let rect = squish(ui, resp.id, &resp, base, 0.012, 0.05);
    let radius = theme.shape_radius(ShapeRole::ExtraLarge) as f32;
    let painter = ui.painter();

    painter.rect_filled(rect, radius, p.surface_container_high);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, with_alpha(p.outline_variant, 110)), StrokeKind::Inside);

    // Аватар и подписи подстраиваются под ширину; длинные имена не заходят на стрелку.
    let compact = rect.width() < 560.0;
    let avatar_radius = if compact { 24.0 } else { 28.0 };
    let avatar_left = if compact { 16.0 } else { 20.0 };
    let avatar = pos2(rect.left() + avatar_left + avatar_radius, rect.center().y);
    painter.circle_filled(avatar, avatar_radius, p.primary_container);
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    painter.text(avatar, Align2::CENTER_CENTER, initial, FontId::proportional(if compact { 21.0 } else { 24.0 }), p.on_primary_container);

    let text_gap = if compact { 12.0 } else { 16.0 };
    let x = avatar.x + avatar_radius + text_gap;
    let text_clip = Rect::from_min_max(pos2(x, rect.top()), pos2((rect.right() - 56.0).max(x), rect.bottom()));
    let text_painter = painter.with_clip_rect(text_clip);
    text_painter.text(pos2(x, rect.center().y - 11.0), Align2::LEFT_CENTER, name, FontId::proportional(theme.text_size(TextRole::TitleMedium) + 1.0), p.on_surface);
    text_painter.text(pos2(x, rect.center().y + 12.0), Align2::LEFT_CENTER, subtitle, FontId::proportional(theme.text_size(TextRole::BodySmall) + 0.5), p.on_surface_variant);
    paint_symbol(painter, mi::ICON_CHEVRON_RIGHT, pos2(rect.right() - 26.0, rect.center().y), 24.0, p.on_surface_variant);

    super::ripple::state_layer(ui, theme, painter, &resp, rect, radius, p.on_surface, 20, 30);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

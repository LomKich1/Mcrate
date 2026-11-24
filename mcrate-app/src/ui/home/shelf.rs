//! Полка: заголовок, две стрелки и ряд карточек с горизонтальной прокруткой.

use crate::theme::{Theme, with_alpha};
use crate::ui::layout::{Bleed, bleed_clip};
use crate::ui::ripple;
use crate::ui::widgets::{glide_begin, glide_end, squish};
use egui::{
    Align, Color32, CursorIcon, Id, Layout, Rect, Response, RichText, ScrollArea, Sense, Shape, Stroke, Ui, UiBuilder, Vec2, pos2, vec2,
};

const SHELF_GAP: f32 = 12.0; // зазор между карточками на полке
const SHELF_STEP: f32 = 420.0; // на сколько пикселей прокручивает стрелка
const SHELF_HALO: f32 = 8.0; // запас подсветки вокруг выбранной карточки (виден как ореол)
/// Ключ в памяти egui: карточка пишет сюда свой id и прямоугольник, если курсор на ней.
pub(super) const SHELF_HOVER_KEY: &str = "mcrate_shelf_hover";
/// Насколько стрелка «вдавливается» при нажатии (доля диаметра): круг и значок сжимаются вокруг центра.
const ARROW_PRESS: f32 = 0.16;

/// Прокрутка и анимация полки между кадрами (хранится в памяти egui).
#[derive(Clone, Default)]
struct ShelfState {
    offset: f32,
    max: f32,
    /// Идущая анимация от стрелки: (откуда, куда, время старта).
    anim: Option<(f32, f32, f64)>,
}

/// Описание полки: что написать в шапке.
#[derive(Clone, Copy)]
pub(super) struct ShelfSpec<'a> {
    /// Ключ полки: из него строятся id прокрутки и подсветки, поэтому он уникален на экране.
    pub id: &'a str,
    pub title: &'a str,
    pub subtitle: &'a str,
}

/// Полка: заголовок с подписью, справа две круглые стрелки, под ними ряд карточек, который
/// прокручивается стрелками (плавно), колесом при зажатом Shift или перетаскиванием.
pub(super) fn shelf(ui: &mut Ui, theme: &Theme, bleed: Bleed, spec: &ShelfSpec, add_cards: impl FnOnce(&mut Ui)) {
    let id = spec.id;
    let sid = ui.scope_id().with(("shelf", id));
    let mut st: ShelfState = ui.ctx().data_mut(|d| d.get_temp(sid)).unwrap_or_default();
    let now = ui.input(|i| i.time);

    shelf_header(ui, theme, spec, &mut st, now);
    let forced = shelf_forced_offset(ui, &mut st, now);

    let mut area = ScrollArea::horizontal()
        .id_salt(id)
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if let Some(x) = forced {
        area = area.horizontal_scroll_offset(x);
    }
    let out = area.show(ui, |ui| {
        // Карточки при прокрутке уходят до самых краёв панели, а не обрезаются в отступе от них.
        bleed_clip(ui, bleed.sides());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SHELF_GAP;
            let glass = theme.glass_active();
            // Подсветка одна на всю полку и скользит между карточками. Только Liquid Glass: стеклянная
            // подложка, слот резервируем ДО карточек, чтобы она лежала под ними. В Material 3 карточки залиты
            // сплошным цветом, подложки под ними нет: состояние наведения там рисует сама карточка.
            let under_slot = if glass { Some(glide_begin(ui)) } else { None };
            // Сбрасываем прошлый кадр записью None (remove_temp требует Default, а Rect его не имеет).
            ui.ctx().data_mut(|d| d.insert_temp(Id::unique(SHELF_HOVER_KEY), None::<(u64, Rect)>));
            add_cards(ui);
            let hovered = ui.ctx().data_mut(|d| d.get_temp::<Option<(u64, Rect)>>(Id::unique(SHELF_HOVER_KEY))).flatten();
            if let Some(slot) = under_slot {
                let salt = ui.scope_id().with(("glide_shelf", id));
                let target = hovered.map(|(key, rect)| (key, rect.expand(SHELF_HALO)));
                glide_end(ui, theme, slot, salt, target, 22.0);
            }
        });
    });
    st.offset = out.state.clamped_offset().x;
    st.max = (out.content_size.x - out.inner_rect.width()).max(0.0);
    ui.ctx().data_mut(|d| d.insert_temp(sid, st));
}

/// Шапка полки: текст слева, стрелки справа. Нажатая стрелка запускает анимацию прокрутки (`st.anim`).
fn shelf_header(ui: &mut Ui, theme: &Theme, spec: &ShelfSpec, st: &mut ShelfState, now: f64) {
    let ShelfSpec { title, subtitle, .. } = *spec;
    let p = &theme.palette;
    let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
    let text_rect = Rect::from_min_max(head.min, pos2((head.right() - 100.0).max(head.left() + 80.0), head.bottom()));
    ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
        ui.add(egui::Label::new(RichText::new(title).size(22.0).strong()).truncate());
        ui.add(egui::Label::new(RichText::new(subtitle).size(13.0).color(p.on_surface_variant)).truncate());
    });
    let arrows = Rect::from_min_max(pos2(head.right() - 92.0, head.top()), pos2(head.right(), head.bottom() - 8.0));
    let mut step = 0.0_f32;
    ui.scope_builder(UiBuilder::new().max_rect(arrows).layout(Layout::right_to_left(Align::Center)), |ui| {
        // right_to_left: первой добавленная стрелка окажется правее.
        if arrow_button(ui, theme, true, st.offset < st.max - 1.0).clicked() {
            step = 1.0;
        }
        if arrow_button(ui, theme, false, st.offset > 1.0).clicked() {
            step = -1.0;
        }
    });
    if step != 0.0 {
        let to = (st.offset + step * SHELF_STEP).clamp(0.0, st.max.max(0.0));
        st.anim = Some((st.offset, to, now));
    }
}

/// Пока идёт анимация стрелки, сами задаём смещение (с замедлением к концу); иначе прокруткой управляет пользователь.
fn shelf_forced_offset(ui: &Ui, st: &mut ShelfState, now: f64) -> Option<f32> {
    let mut forced = None;
    if let Some((from, to, t0)) = st.anim {
        let k = ((now - t0) / 0.28).clamp(0.0, 1.0) as f32;
        forced = Some(from + (to - from) * (1.0 - (1.0 - k).powi(3)));
        if k >= 1.0 {
            st.anim = None;
        } else {
            ui.ctx().request_repaint();
        }
    }
    forced
}

/// Круглая кнопка-стрелка полки. Если прокручивать некуда, она бледная и не нажимается.
/// При нажатии кнопка плавно «вдавливается»: уменьшается вокруг центра (`squish`, пружина с перелётом),
/// в M3 к тому же появляется внутренняя тень и слой состояния 12%. Раньше `squish` считался, но радиус круга
/// был константой 20 px и ничего не менялось: теперь все размеры берутся от сжатого прямоугольника.
fn arrow_button(ui: &mut Ui, theme: &Theme, right: bool, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), if enabled { Sense::click() } else { Sense::hover() });
    let rect = squish(ui, resp.id, &resp, rect, ARROW_PRESS, ARROW_PRESS);
    let scale = rect.width() / 40.0; // 1.0 в покое, около 0.84 при полном нажатии
    let depth = ((1.0 - scale) / ARROW_PRESS).clamp(0.0, 1.0); // 0..1: насколько кнопка вдавлена
    let pressed = enabled && resp.is_pointer_button_down_on();
    let center = rect.center();
    let painter = ui.painter();
    let fade = if enabled { 255 } else { 72 };
    if theme.glass_active() {
        // В Liquid Glass стрелки это лёгкие toolbar-контролы: без постоянной M3-заливки.
        if enabled && (resp.hovered() || pressed) {
            let alpha = if pressed { 58 } else { 38 };
            painter.circle_filled(center, 19.0 * scale, with_alpha(theme.palette.on_surface, alpha));
            painter.circle_stroke(center, 18.5 * scale, Stroke::new(0.8, with_alpha(theme.palette.on_surface, 66)));
        }
    } else {
        let radius = 20.0 * scale;
        painter.circle_filled(center, radius, with_alpha(theme.tonal_fill(), fade));
        if enabled {
            // Слой состояния M3: наведение 8%, нажатие 12% цвета контента.
            // Кружок от курсора (ripple.rs); форма обрезки: круг кнопки.
            let circle = Rect::from_center_size(center, Vec2::splat(radius * 2.0));
            ripple::state_layer(ui, theme, painter, &resp, circle, radius);
            // Внутренняя тень: кнопка выглядит продавленной внутрь.
            if depth > 0.01 {
                painter.circle_stroke(center, radius - 1.0, Stroke::new(2.0, Color32::from_black_alpha((44.0 * depth) as u8)));
            }
        }
    }
    let dx = if right { 1.0 } else { -1.0 };
    let pts = vec![center + vec2(-3.0 * dx, -6.0) * scale, center + vec2(3.0 * dx, 0.0) * scale, center + vec2(-3.0 * dx, 6.0) * scale];
    let arrow_color = if theme.glass_active() { theme.palette.on_surface } else { theme.on_tonal() };
    painter.add(Shape::line(pts, Stroke::new(if theme.glass_active() { 1.8 } else { 2.0 }, with_alpha(arrow_color, fade))));
    if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

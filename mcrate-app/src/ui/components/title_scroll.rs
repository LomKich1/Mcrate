//! Страница с общей прокруткой и названием экрана в шапке, которое проявляется по мере прокрутки.
//!
//! Нужна экранам с боковой карточкой (`split_page`): на узком окне карточка стоит над содержимым и едет вместе
//! с ним, а когда её название уезжает за верх страницы, оно появляется в шапке. На широком окне карточка
//! с названием всегда на виду, и названия в шапке нет. Шапка рисуется раньше прокрутки, поэтому смещение
//! запоминается в памяти egui под ключом экрана и читается шапкой с прошлого кадра.

use egui::{Context, Id, ScrollArea, Ui};

use super::split_is_wide;
use crate::ui::layout::{Bleed, bleed_clip, pane_scroll, scroll_end_padding};

/// С какого смещения прокрутки (px) название начинает появляться в шапке: к этому моменту название карточки
/// коллекции уже уехало вверх. Полностью название видно через `TITLE_REVEAL_RANGE` px после этого.
const TITLE_REVEAL_START: f32 = 16.0;
/// Сколько пикселей прокрутки название проявляется от 0 до 1.
const TITLE_REVEAL_RANGE: f32 = 40.0;

/// Узкая страница: одна область прокрутки на всё содержимое `add`. `key`: уникальный ключ экрана (он же
/// `id_salt` прокрутки); по нему шапка читает смещение через `narrow_title_shown`.
pub(in crate::ui) fn title_scroll_page(ui: &mut Ui, bleed: Bleed, bottom_inset: f32, key: &'static str, add: impl FnOnce(&mut Ui)) {
    let scroll = pane_scroll(ui, bleed, ScrollArea::vertical().id_salt(key).auto_shrink([false, false]), |area, ui| {
        area.show(ui, |ui| {
            bleed_clip(ui, bleed);
            add(ui);
            scroll_end_padding(ui, bottom_inset);
        })
    });
    remember_scroll_y(ui.ctx(), key, scroll.state.clamped_offset().y);
}

/// Насколько показывать название в шапке (0..1) у экрана с общей прокруткой `key`: на широком окне 0
/// (название есть на карточке), на узком по смещению прокрутки.
pub(in crate::ui) fn narrow_title_shown(ui: &Ui, key: &str) -> f32 {
    if split_is_wide(ui.available_width()) {
        return 0.0;
    }
    title_reveal(scroll_y(ui.ctx(), key))
}

/// Под каким ключом хранится смещение прокрутки страницы `key`.
fn scroll_y_id(key: &str) -> Id {
    Id::unique(("page_scroll_y", key))
}

fn scroll_y(ctx: &Context, key: &str) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(scroll_y_id(key))).unwrap_or(0.0)
}

/// Запоминает смещение; если оно изменилось, просит ещё кадр, чтобы шапка догнала прокрутку, когда та остановилась.
fn remember_scroll_y(ctx: &Context, key: &str, y: f32) {
    if (scroll_y(ctx, key) - y).abs() > 0.01 {
        ctx.data_mut(|d| d.insert_temp(scroll_y_id(key), y));
        ctx.request_repaint();
    }
}

/// Доля появления названия в шапке (0..1) по смещению прокрутки страницы.
fn title_reveal(scroll_y: f32) -> f32 {
    ((scroll_y - TITLE_REVEAL_START) / TITLE_REVEAL_RANGE).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_appears_only_after_the_card_title_scrolled_away() {
        assert_eq!(title_reveal(0.0), 0.0);
        assert_eq!(title_reveal(TITLE_REVEAL_START), 0.0);
        assert!(title_reveal(TITLE_REVEAL_START + TITLE_REVEAL_RANGE * 0.5) > 0.4);
        assert_eq!(title_reveal(TITLE_REVEAL_START + TITLE_REVEAL_RANGE), 1.0);
        assert_eq!(title_reveal(5000.0), 1.0);
        // Монотонно: чем ниже прокрутили, тем виднее название.
        let mut prev = 0.0;
        for y in 0..200 {
            let v = title_reveal(y as f32);
            assert!(v >= prev);
            prev = v;
        }
    }

    #[test]
    fn scroll_offsets_are_remembered_per_screen() {
        let ctx = Context::default();
        remember_scroll_y(&ctx, "a", 120.0);
        assert_eq!(scroll_y(&ctx, "a"), 120.0);
        assert_eq!(scroll_y(&ctx, "b"), 0.0);
    }
}

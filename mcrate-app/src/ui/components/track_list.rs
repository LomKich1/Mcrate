//! Список строк треков с общей скользящей подсветкой.
//!
//! Цикл «слот подсветки → строки → подсветка под курсором» одинаков на всех экранах со списками треков
//! (главная, «Ваши загрузки», история, SoundCloud). Здесь он собран в одном месте, чтобы порядок слоёв
//! и id подсветки нигде не разошлись.

use crate::theme::Theme;
use crate::ui::widgets::{glide_begin, glide_end, track_row};
use egui::{Id, Ui};
use mcrate_core::Track;

/// Скругление скользящей подсветки под строкой трека.
const ROW_GLIDE_RADIUS: f32 = 18.0;

/// Рисует строки `rows` одним списком и возвращает индекс строки, по которой кликнули.
///
/// * `rows` — пары «номер строки, трек»; номер выводится в первой колонке и возвращается при клике;
/// * `is_current` — как отличить играющий трек (где-то хватает `id`, где-то нужен ещё провайдер);
/// * `glide_salt` — ключ подсветки списка, уникальный на экране: он смешивается с `scope_id` текущего `ui`.
pub(crate) fn track_list<'a>(
    ui: &mut Ui,
    theme: &Theme,
    rows: impl IntoIterator<Item = (usize, &'a Track)>,
    is_current: impl Fn(&Track) -> bool,
    playing: bool,
    glide_salt: &str,
) -> Option<usize> {
    let time = ui.input(|i| i.time);
    let glide_slot = glide_begin(ui);
    let mut hovered = None;
    let mut clicked = None;
    for (index, track) in rows {
        let current = is_current(track);
        let resp = track_row(ui, theme, index, track, current, current && playing, time);
        if resp.hovered() {
            hovered = Some((Id::unique(&track.id).value(), resp.rect));
        }
        if resp.clicked() {
            clicked = Some(index);
        }
    }
    glide_end(ui, theme, glide_slot, ui.scope_id().with(glide_salt), hovered, ROW_GLIDE_RADIUS);
    clicked
}

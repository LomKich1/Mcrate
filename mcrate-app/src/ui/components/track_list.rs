//! Список строк треков.
//!
//! Цикл «строки → клик» одинаков на всех экранах со списками треков (главная, «Ваши загрузки», история,
//! SoundCloud). Здесь он собран в одном месте. Подсветка строки под курсором (плашка, затемнение обложки,
//! кнопка воспроизведения) рисуется самой строкой (`widgets::track_row`) и ни от списка, ни от соседних строк не зависит.

use crate::theme::Theme;
use crate::ui::widgets::{RowPlayback, SegmentPos, track_row};
use egui::Ui;
use mcrate_core::{ProviderKind, Track};

/// Играет ли сейчас именно этот трек: сравниваются `id` и провайдер (у разных сервисов `id` могут совпасть).
/// `current` это пара `(id, провайдер)` играющего трека, её берут один раз до цикла по строкам.
pub(crate) fn same_track_as(current: Option<&(String, ProviderKind)>, track: &Track) -> bool {
    current.is_some_and(|(id, kind)| *id == track.id && *kind == track.provider)
}

/// Рисует строки `rows` одним списком и возвращает индекс строки, по которой кликнули.
///
/// * `rows` — пары «номер строки, трек»; номер выводится в первой колонке и возвращается при клике;
/// * `total` — сколько всего строк в списке (рисоваться может только часть): по нему первая и последняя плашки
///   получают крупные углы, см. `widgets::segment`;
/// * `is_current` — как отличить играющий трек (где-то хватает `id`, где-то нужен ещё провайдер).
pub(crate) fn track_list<'a>(
    ui: &mut Ui,
    theme: &Theme,
    rows: impl IntoIterator<Item = (usize, &'a Track)>,
    total: usize,
    is_current: impl Fn(&Track) -> bool,
    playing: bool,
) -> Option<usize> {
    let time = ui.input(|i| i.time);
    let mut clicked = None;
    for (index, track) in rows {
        let current = is_current(track);
        let resp = track_row(ui, theme, index, track, RowPlayback { is_current: current, is_playing: current && playing, time }, SegmentPos { index, count: total });
        if resp.clicked() {
            clicked = Some(index);
        }
    }
    clicked
}

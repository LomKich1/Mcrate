//! Готовые блоки экранов, собранные из виджетов `widgets/`: шапки и списки-переходов.
//! Один и тот же по смыслу блок строится здесь один раз, а не копируется по экранам.

mod confirm_dialog;
mod empty_state;
mod nav_list;
mod screen_header;
mod split_page;
mod text_dialog;
mod title_scroll;
mod track_list;

pub(super) use confirm_dialog::{ConfirmDialog, DialogChoice, confirm_dialog, modal_card};
pub(super) use empty_state::{EmptyText, empty_state, empty_state_top_space, empty_state_with};
pub(super) use nav_list::nav_list;
pub(super) use screen_header::{TitleRow, back_header, back_title_row, screen_title, title_search_row};
pub(super) use split_page::{SIDE_COVER_SIZE, SplitPage, split_is_wide, split_page};
pub(super) use text_dialog::{TextDialog, text_dialog};
pub(super) use title_scroll::{narrow_title_shown, title_scroll_page};
pub(super) use track_list::{same_track_as, track_list};

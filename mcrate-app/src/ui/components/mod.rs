//! Готовые блоки экранов, собранные из виджетов `widgets/`: шапки и списки-переходов.
//! Один и тот же по смыслу блок строится здесь один раз, а не копируется по экранам.

mod empty_state;
mod nav_list;
mod screen_header;
mod split_page;
mod track_list;

pub(super) use empty_state::{empty_state, empty_state_top_space, empty_state_with};
pub(super) use nav_list::nav_list;
pub(super) use screen_header::{TitleRow, back_header, screen_title, title_search_row};
pub(super) use split_page::{SIDE_COVER_SIZE, split_page};
pub(super) use track_list::track_list;

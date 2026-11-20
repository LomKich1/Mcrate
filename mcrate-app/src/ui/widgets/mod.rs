//! Общие виджеты. Рисуются вручную (painter), цвета берут из темы,
//! поэтому выглядят правильно и в Material 3, и в Liquid Glass.
//!
//! Файлы по смыслу: кнопки, поиск, навигация, ползунок, подсветка, обложки, обратная связь, строка трека.

mod buttons;
mod cover;
mod feedback;
mod glide;
mod nav;
mod search;
mod slider;
mod text;
mod track_row;

pub(crate) use buttons::{ButtonKind, glass_icon_button, icon_button, pill_button};
pub(crate) use cover::{cover_tile, loading_outline};
pub(crate) use feedback::{error_banner, loading_indicator, status_snackbar};
pub(super) use glide::squish;
pub(crate) use glide::{glide_begin, glide_end};
pub(super) use nav::NAV_PRESS;
pub(crate) use nav::{NavItem, nav_item, section_label};
pub(crate) use search::search_bar;
pub(crate) use slider::slider_bar;
pub(crate) use text::elided;
pub(crate) use track_row::track_row;

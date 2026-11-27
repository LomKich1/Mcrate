//! Общие виджеты. Рисуются вручную (painter), цвета берут из темы,
//! поэтому выглядят правильно и в Material 3, и в Liquid Glass.
//!
//! Файлы по смыслу: кнопки, поиск, навигация, ползунок, подсветка, обложки, обратная связь, строка трека.

mod buttons;
mod cover;
mod cover_images;
mod download_icon;
mod feedback;
mod glide;
mod loading_indicator;
mod nav;
mod search;
mod segment;
mod slider;
mod text;
mod track_row;

pub(crate) use buttons::{ButtonKind, IconButton, glass_icon_button, icon_button, like_button, pill_button, place_at};
pub(crate) use cover::{collage_sample, cover_tile, loading_outline, playlist_cover, track_cover};
pub(crate) use cover_images::install as install_cover_hub;
pub(crate) use feedback::{error_banner, loading_indicator, status_snackbar};
pub(super) use glide::squish;
pub(crate) use glide::{glide_begin, glide_end};
pub(super) use nav::NAV_PRESS;
pub(crate) use nav::{NavItem, nav_item, section_label};
pub(crate) use search::search_bar;
pub(crate) use segment::{SEGMENT_GAP, SegmentPos, apply_list_gap, ripple_corner, segment_radius, segment_shape};
pub(crate) use slider::slider_bar;
pub(crate) use text::elided;
pub(crate) use track_row::{COVER_SCRIM_ALPHA, RowPlayback, row_hover, track_row, track_row_with_provider_badge};

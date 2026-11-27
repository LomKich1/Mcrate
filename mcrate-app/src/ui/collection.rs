//! Раздел «Библиотека»: главный экран-список и его подэкраны.
//!
//! Главный экран (`View::Collection`): строки-переходы «Понравившиеся», «Плейлисты», «Скачанные»,
//! «Ваши загрузки» и внизу блок «Недавно слушали» (последние `RECENT_COUNT` треков + «Смотреть все»).
//! «Ваши загрузки» и «Скачанные» — два варианта общей страницы коллекции (`ui/library.rs`):
//! первый показывает локальные файлы, второй — постоянные онлайн-загрузки с меткой сервиса на обложке.
//! «Понравившиеся», «Плейлисты» и один плейлист (`View::CollectionPlaylist`) лежат здесь;
//! история хранится на диске (`Controller::history`).

use egui::{RichText, ScrollArea, Sense, Ui, Vec2};
use mcrate_core::Track;

use egui_material_icons::icons as mi;

use super::components::{back_header, empty_state, empty_state_top_space, nav_list, same_track_as, track_list};
use super::layout::{
    SETTINGS_CONTENT_MAX_WIDTH, TRACK_LIST_MAX_WIDTH, centered_column, pane_scroll, rows_in_view, scroll_end_padding,
};
use super::playlists::{PlaylistDialog, playlist_row, tracks_count_text};
use super::settings::controls::{Row, Trailing, settings_group};
use super::widgets::{ButtonKind, SEGMENT_GAP, SegmentPos, apply_list_gap, collage_sample, pill_button, playlist_cover};
use crate::app::{McrateApp, View};
use crate::icons::Icon;
use crate::theme::Theme;

/// Сколько последних треков показываем внизу главного экрана раздела.
const RECENT_COUNT: usize = 10;
/// Сторона обложки плейлиста на его экране и скругление её внешних углов.
const PLAYLIST_COVER_SIZE: f32 = 96.0;
const PLAYLIST_COVER_RADIUS: f32 = 14.0;

/// Кнопки экрана плейлиста.
#[derive(Clone, Copy)]
enum PlaylistAction {
    Play,
    Rename,
    Delete,
}

impl McrateApp {
    /// Главный экран раздела «Библиотека».
    pub(crate) fn collection_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, f32::INFINITY, |ui| {
            centered_column(ui, SETTINGS_CONTENT_MAX_WIDTH, |ui| {
                ui.heading("Библиотека");
                ui.add_space(8.0);
            });
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    self.collection_menu(ui, theme);
                    self.collection_recent(ui, theme);
                    scroll_end_padding(ui, self.bottom_inset);
                })
            });
        });
    }

    /// Меню подразделов: «Понравившиеся», «Плейлисты», «Скачанные», «Ваши загрузки».
    fn collection_menu(&mut self, ui: &mut Ui, theme: &Theme) {
        let uploads = format!("Своих файлов: {}", self.ctl.library.len());
        let liked = tracks_count_text(self.ctl.likes.items().len());
        let playlists = format!("Плейлистов: {}", self.ctl.playlists.list().len());
        let downloaded = tracks_count_text(self.ctl.downloaded.len());
        let views = [View::CollectionLiked, View::CollectionPlaylists, View::CollectionDownloaded, View::Library];
        // Меню шириной 700 и с тем же видом пунктов, что на экране настроек.
        centered_column(ui, SETTINGS_CONTENT_MAX_WIDTH, |ui| {
            if theme.glass_active() {
                let rows = [
                    (views[0], "Понравившиеся", liked),
                    (views[1], "Плейлисты", playlists),
                    (views[2], "Скачанные", downloaded),
                    (views[3], "Ваши загрузки", uploads),
                ];
                if let Some(view) = nav_list(ui, theme, &rows, SETTINGS_CONTENT_MAX_WIDTH, true) {
                    self.view = view;
                }
            } else {
                let rows = vec![
                    Row::new(mi::ICON_FAVORITE, "Понравившиеся", Trailing::Value(liked)),
                    Row::new(mi::ICON_QUEUE_MUSIC, "Плейлисты", Trailing::Value(playlists)),
                    Row::new(mi::ICON_DOWNLOAD, "Скачанные", Trailing::Value(downloaded)),
                    Row::new(mi::ICON_FOLDER, "Ваши загрузки", Trailing::Value(uploads)),
                ];
                if let Some(i) = settings_group(ui, theme, "collection", rows) {
                    self.view = views[i];
                }
            }
        });
    }

    /// Блок «Недавно слушали»: последние `RECENT_COUNT` треков и «Смотреть все».
    fn collection_recent(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        centered_column(ui, SETTINGS_CONTENT_MAX_WIDTH, |ui| {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Недавно слушали").size(18.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !self.ctl.history.items().is_empty() && pill_button(ui, theme, "Смотреть все", None, ButtonKind::Tonal).clicked()
                    {
                        self.view = View::CollectionHistory;
                    }
                });
            });
            ui.add_space(6.0);
            if self.ctl.history.items().is_empty() {
                ui.label(RichText::new("Здесь появятся треки, которые вы включали в этом сеансе.").color(p.on_surface_variant));
            } else {
                let history = self.ctl.history.items();
                let recent = &history[..history.len().min(RECENT_COUNT)];
                if let Some(i) = self.history_rows(ui, theme, recent) {
                    // Очередь = эти же три трека (как в «Моей музыке»: очередь это показанный список).
                    self.ctl.play_list(recent.to_vec(), i);
                }
            }
        });
    }

    /// Полная история прослушивания (за сеанс).
    pub(crate) fn collection_history_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, f32::INFINITY, |ui| {
            let p = &theme.palette;
            if back_header(ui, theme, "История") {
                self.view = View::Collection;
            }
            ui.add_space(4.0);
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    if self.ctl.history.items().is_empty() {
                        ui.label(RichText::new("Пока пусто: включите любой трек.").color(p.on_surface_variant));
                    } else {
                        if let Some(i) = self.history_rows_in_view(ui, theme) {
                            // Копия очереди делается только по клику, не каждый кадр.
                            let items = self.ctl.history.items().to_vec();
                            self.ctl.play_list(items, i);
                        }
                    }
                    scroll_end_padding(ui, self.bottom_inset);
                })
            });
        });
    }

    /// «Плейлисты»: кнопка «Новый плейлист» и список плейлистов; клик открывает плейлист.
    pub(crate) fn collection_playlists_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let mut back = false;
        let mut create = false;
        let mut open = None;
        centered_column(ui, f32::INFINITY, |ui| {
            back = back_header(ui, theme, "Плейлисты");
            ui.add_space(4.0);
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    centered_column(ui, SETTINGS_CONTENT_MAX_WIDTH, |ui| {
                        create = pill_button(ui, theme, "Новый плейлист", Some(Icon::Plus), ButtonKind::Filled).clicked();
                        ui.add_space(10.0);
                        let playlists = self.ctl.playlists.list();
                        if playlists.is_empty() {
                            let top = empty_state_top_space(ui.available_height(), self.bottom_inset);
                            empty_state(
                                ui,
                                theme,
                                top,
                                Icon::Queue,
                                "Плейлистов пока нет",
                                "Создайте первый и собирайте в него любимые треки.",
                            );
                        }
                        // Material 3: сегментированный список (зазор и крупные углы у краёв), Liquid Glass: строки встык.
                        ui.spacing_mut().item_spacing.y = if theme.glass_active() { 0.0 } else { SEGMENT_GAP };
                        let count = playlists.len();
                        for (index, playlist) in playlists.iter().enumerate() {
                            if playlist_row(ui, theme, playlist, None, Some(SegmentPos { index, count })) {
                                open = Some(playlist.id);
                            }
                        }
                    });
                    scroll_end_padding(ui, self.bottom_inset);
                })
            });
        });
        if back {
            self.view = View::Collection;
        }
        if create {
            self.playlist_dialog = Some(PlaylistDialog::Create { text: String::new(), then_add: None });
        }
        if let Some(id) = open {
            self.open_playlist = Some(id);
            self.view = View::CollectionPlaylist;
        }
    }

    /// Один плейлист: «Слушать», «Переименовать», «Удалить» и список треков. Трек убирают через меню «три точки».
    pub(crate) fn collection_playlist_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let Some(playlist) = self.open_playlist.and_then(|id| self.ctl.playlists.get(id)) else {
            self.open_playlist = None;
            self.view = View::CollectionPlaylists;
            return;
        };
        let (id, name, count) = (playlist.id, playlist.name.clone(), playlist.tracks.len());
        // В обложку идут несколько треков с разными обложками: копируются они, а не весь список (в нём могут быть тысячи).
        let cover_tracks: Vec<Track> = collage_sample(&playlist.tracks);
        let mut back = false;
        let mut action = None;
        let mut clicked = None;
        centered_column(ui, f32::INFINITY, |ui| {
            let p = &theme.palette;
            back = back_header(ui, theme, &name);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let (cover, _) = ui.allocate_exact_size(Vec2::splat(PLAYLIST_COVER_SIZE), Sense::hover());
                playlist_cover(ui, theme, cover, (&name, &cover_tracks), PLAYLIST_COVER_RADIUS);
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(tracks_count_text(count)).color(p.on_surface_variant));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(count > 0, |ui| {
                            if pill_button(ui, theme, "Слушать", Some(Icon::Play), ButtonKind::Filled).clicked() {
                                action = Some(PlaylistAction::Play);
                            }
                        });
                        if pill_button(ui, theme, "Переименовать", None, ButtonKind::Tonal).clicked() {
                            action = Some(PlaylistAction::Rename);
                        }
                        if pill_button(ui, theme, "Удалить", None, ButtonKind::Tonal).clicked() {
                            action = Some(PlaylistAction::Delete);
                        }
                    });
                });
            });
            ui.add_space(6.0);
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    match self.ctl.playlists.get(id) {
                        Some(pl) if !pl.tracks.is_empty() => clicked = self.track_rows_in_view(ui, theme, &pl.tracks),
                        _ => {
                            let top = empty_state_top_space(ui.available_height(), self.bottom_inset);
                            empty_state(
                                ui,
                                theme,
                                top,
                                Icon::Queue,
                                "В плейлисте пока пусто",
                                "Добавьте треки через меню «три точки» у любого трека.",
                            );
                        }
                    }
                    scroll_end_padding(ui, self.bottom_inset);
                })
            });
        });
        if back {
            self.view = View::CollectionPlaylists;
        }
        match action {
            Some(PlaylistAction::Play) => clicked = clicked.or(Some(0)),
            Some(PlaylistAction::Rename) => self.playlist_dialog = Some(PlaylistDialog::Rename { id, text: name }),
            Some(PlaylistAction::Delete) => self.playlist_dialog = Some(PlaylistDialog::Delete(id)),
            None => {}
        }
        if let Some(i) = clicked {
            // Копия очереди делается только по клику, не каждый кадр.
            let tracks = self.ctl.playlists.get(id).map(|p| p.tracks.clone()).unwrap_or_default();
            self.ctl.play_list(tracks, i);
        }
    }

    /// Вся история в общей прокрутке: рисуются только видимые строки (`layout::rows_in_view`), а не все 700.
    /// Возвращает номер строки, по которой кликнули.
    fn history_rows_in_view(&self, ui: &mut Ui, theme: &Theme) -> Option<usize> {
        self.track_rows_in_view(ui, theme, self.ctl.history.items())
    }

    /// Любой список треков в общей прокрутке: рисуются только видимые строки. Возвращает номер строки, по которой кликнули.
    fn track_rows_in_view(&self, ui: &mut Ui, theme: &Theme, items: &[Track]) -> Option<usize> {
        centered_column(ui, TRACK_LIST_MAX_WIDTH, |ui| {
            apply_list_gap(ui, theme);
            let current = self.ctl.now.as_ref().map(|t| (t.id.clone(), t.provider));
            let playing = self.ctl.is_playing();
            let is_current = |track: &Track| same_track_as(current.as_ref(), track);
            rows_in_view(ui, items.len(), theme.list_item_height(2), |ui, range| {
                track_list(ui, theme, range.map(|i| (i, &items[i])), items.len(), is_current, playing)
            })
        })
    }

    /// Список треков из истории; возвращает номер строки, по которой кликнули.
    fn history_rows(&self, ui: &mut Ui, theme: &Theme, items: &[Track]) -> Option<usize> {
        centered_column(ui, TRACK_LIST_MAX_WIDTH, |ui| {
            apply_list_gap(ui, theme);
            let current = self.ctl.now.as_ref().map(|t| (t.id.clone(), t.provider));
            let playing = self.ctl.is_playing();
            let is_current = |track: &Track| same_track_as(current.as_ref(), track);
            track_list(ui, theme, items.iter().enumerate(), items.len(), is_current, playing)
        })
    }
}


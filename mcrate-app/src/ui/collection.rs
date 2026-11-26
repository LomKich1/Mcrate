//! Раздел «Библиотека»: главный экран-список и его подэкраны.
//!
//! Главный экран (`View::Collection`): строки-переходы «Понравившиеся», «Плейлисты», «Скачанные»,
//! «Ваши загрузки» и внизу блок «Недавно слушали» (последние 3 трека + «Смотреть все»).
//! «Ваши загрузки» это экран своих файлов (`View::Library`, ui/library.rs). «Понравившиеся», «Плейлисты» и один
//! плейлист (`View::CollectionPlaylist`) настоящие экраны; окна плейлистов лежат в `ui/playlists.rs`.
//! «Скачанные» пока заглушка: у скачанных нет списка (кэш движка хранит только последние 20 треков и списком
//! не отдаётся). История хранится на диске (`Controller::history`).

use egui::{Align, RichText, ScrollArea, Sense, Ui, Vec2};
use mcrate_core::Track;

use egui_material_icons::icons as mi;

use super::components::{
    SIDE_COVER_SIZE, SplitPage, back_header, back_title_row, empty_state, empty_state_top_space, narrow_title_shown, nav_list,
    same_track_as, split_page, title_scroll_page, track_list,
};
use super::layout::{
    SETTINGS_CONTENT_MAX_WIDTH, TRACK_LIST_MAX_WIDTH, centered_column, pane_scroll, rows_in_view, scroll_end_padding,
};
use super::playlists::{PlaylistDialog, playlist_row, tracks_count_text};
use super::settings::controls::{Row, Trailing, settings_group};
use super::widgets::{ButtonKind, COLLAGE_MAX_TILES, SEGMENT_GAP, SegmentPos, apply_list_gap, pill_button, playlist_cover};
use crate::app::{McrateApp, View};
use crate::icons::{self, Icon};
use crate::theme::Theme;

/// Сколько последних треков показываем внизу главного экрана раздела.
const RECENT_COUNT: usize = 10;
/// Подпись под названием заглушки (в шапке и на карточке).
const STUB_SUBTITLE: &str = "Коллекция Mcrate";

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

/// Содержимое заглушки подэкрана коллекции.
struct Stub {
    /// Ключ общей прокрутки узкой страницы (уникален для экрана: по нему шапка читает смещение).
    key: &'static str,
    title: &'static str,
    icon: Icon,
    headline: &'static str,
    text: &'static str,
}

const DOWNLOADED_STUB: Stub = Stub {
    key: "collection_downloaded_scroll",
    title: "Скачанные",
    icon: Icon::Folder,
    headline: "Списка скачанных пока нет",
    text: "Движок временно кэширует последние 20 онлайн-треков, но списком их не отдаёт. Постоянные загрузки появятся позже.",
};

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
        let views = [View::CollectionLiked, View::CollectionPlaylists, View::CollectionDownloaded, View::Library];
        // Меню шириной 700 и с тем же видом пунктов, что на экране настроек.
        centered_column(ui, SETTINGS_CONTENT_MAX_WIDTH, |ui| {
            if theme.glass_active() {
                let rows = [
                    (views[0], "Понравившиеся", liked),
                    (views[1], "Плейлисты", playlists),
                    (views[2], "Скачанные", "Скоро".to_string()),
                    (views[3], "Ваши загрузки", uploads),
                ];
                if let Some(view) = nav_list(ui, theme, &rows, SETTINGS_CONTENT_MAX_WIDTH, true) {
                    self.view = view;
                }
            } else {
                let rows = vec![
                    Row::new(mi::ICON_FAVORITE, "Понравившиеся", Trailing::Value(liked)),
                    Row::new(mi::ICON_QUEUE_MUSIC, "Плейлисты", Trailing::Value(playlists)),
                    Row::new(mi::ICON_DOWNLOAD, "Скачанные", Trailing::Value("Скоро".into())),
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

    /// «Понравившиеся»: треки с сердечком, свежие первыми. Клик играет список с этого трека.
    pub(crate) fn collection_liked_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let mut back = false;
        let mut clicked = None;
        centered_column(ui, f32::INFINITY, |ui| {
            back = back_header(ui, theme, "Понравившиеся");
            ui.add_space(4.0);
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    let items = self.ctl.likes.items();
                    if items.is_empty() {
                        let top = empty_state_top_space(ui.available_height(), self.bottom_inset);
                        empty_state(
                            ui,
                            theme,
                            top,
                            Icon::HeartOutline,
                            "Лайков пока нет",
                            "Нажмите на сердечко у трека, и он появится здесь.",
                        );
                    } else {
                        clicked = self.track_rows_in_view(ui, theme, items);
                    }
                    scroll_end_padding(ui, self.bottom_inset);
                })
            });
        });
        if back {
            self.view = View::Collection;
        }
        if let Some(i) = clicked {
            // Копия очереди делается только по клику, не каждый кадр.
            let items = self.ctl.likes.items().to_vec();
            self.ctl.play_list(items, i);
        }
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
        // В обложку идут только первые треки: копируются они, а не весь список (в нём могут быть тысячи).
        let cover_tracks: Vec<Track> = playlist.tracks.iter().take(COLLAGE_MAX_TILES).cloned().collect();
        let mut back = false;
        let mut action = None;
        let mut clicked = None;
        centered_column(ui, f32::INFINITY, |ui| {
            let p = &theme.palette;
            back = back_header(ui, theme, &name);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let (cover, _) = ui.allocate_exact_size(Vec2::splat(PLAYLIST_COVER_SIZE), Sense::hover());
                let first = &cover_tracks[..cover_tracks.len().min(COLLAGE_MAX_TILES)];
                playlist_cover(ui, theme, cover, (&name, first), PLAYLIST_COVER_RADIUS);
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

    pub(crate) fn collection_downloaded_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.collection_stub(ui, theme, &DOWNLOADED_STUB);
    }

    /// Пустой экран коллекции в той же двухколоночной компоновке, что и страницы плейлистов.
    /// Liquid Glass сохраняет прежнюю компактную заглушку под шапкой «Библиотека». Material 3 использует тот же
    /// левый блок и правую область, что и «Ваши загрузки»: на узком окне блок и сообщение едут одной прокруткой,
    /// а название экрана появляется в шапке, когда уезжает карточка (на широком окне названия в шапке нет).
    fn collection_stub(&mut self, ui: &mut Ui, theme: &Theme, stub: &Stub) {
        centered_column(ui, f32::INFINITY, |ui| {
            if theme.glass_active() {
                if back_header(ui, theme, "Библиотека") {
                    self.view = View::Collection;
                }
                ui.add_space(14.0);
                let top = empty_state_top_space(ui.available_height(), self.bottom_inset);
                empty_state(ui, theme, top, stub.icon, stub.headline, stub.text);
                return;
            }
            let title_shown = narrow_title_shown(ui, stub.key);
            if back_title_row(ui, theme, (stub.title, STUB_SUBTITLE), title_shown) {
                self.view = View::Collection;
            }
            ui.add_space(14.0);

            let bleed = self.bleed.below_header();
            let bottom_inset = self.bottom_inset;
            split_page(ui, Align::Center, 24.0, |ui, page| {
                if page.wide {
                    stub_wide(ui, theme, page, stub);
                } else {
                    title_scroll_page(ui, bleed, bottom_inset, stub.key, |ui| stub_narrow(ui, theme, page, stub));
                }
            });
        });
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

/// Заглушка в широкой раскладке: слева крупная карточка с названием, справа сообщение.
fn stub_wide(ui: &mut Ui, theme: &Theme, page: &SplitPage, stub: &Stub) {
    let p = &theme.palette;
    page.side(ui, |ui| {
        ui.add_space(12.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(SIDE_COVER_SIZE), Sense::hover());
        ui.painter().rect_filled(rect, theme.shape_radius(crate::theme::ShapeRole::Large), p.surface_container_high);
        icons::paint(theme, ui.painter(), stub.icon, rect.center(), 104.0, p.on_surface_variant);
        ui.add_space(18.0);
        ui.label(RichText::new(stub.title).size(theme.text_size(crate::theme::TextRole::HeadlineSmall)).strong());
        ui.add_space(6.0);
        ui.label(RichText::new(STUB_SUBTITLE).size(13.0).color(p.on_surface_variant));
        ui.add_space(8.0);
        ui.label(RichText::new("Пока нет сохранённых элементов").size(13.0).color(p.on_surface_variant));
    });
    page.main(ui, |ui| {
        ui.add_space((ui.available_height() * 0.24).max(20.0));
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
        icons::paint(theme, ui.painter(), stub.icon, rect.center(), 42.0, p.on_surface_variant);
        ui.add_space(12.0);
        ui.label(RichText::new(stub.headline).size(20.0).strong());
        ui.add_space(5.0);
        ui.add(egui::Label::new(RichText::new(stub.text).size(14.0).color(p.on_surface_variant)).wrap());
    });
}

/// Заглушка в узкой раскладке: компактная карточка сверху, сообщение под ней.
fn stub_narrow(ui: &mut Ui, theme: &Theme, page: &SplitPage, stub: &Stub) {
    let p = &theme.palette;
    page.side(ui, |ui| {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(88.0), Sense::hover());
            ui.painter().rect_filled(rect, theme.shape_radius(crate::theme::ShapeRole::Medium), p.surface_container_high);
            icons::paint(theme, ui.painter(), stub.icon, rect.center(), 42.0, p.on_surface_variant);
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(stub.title).size(theme.text_size(crate::theme::TextRole::TitleLarge)).strong());
                ui.label(RichText::new(STUB_SUBTITLE).size(13.0).color(p.on_surface_variant));
            });
        });
    });
    page.main(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(stub.headline).size(20.0).strong());
            ui.add(egui::Label::new(RichText::new(stub.text).size(14.0).color(p.on_surface_variant)).wrap());
        });
    });
}

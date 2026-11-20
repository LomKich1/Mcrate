//! Раздел «Библиотека»: главный экран-список и его подэкраны.
//!
//! Главный экран (`View::Collection`): строки-переходы «Понравившиеся», «Плейлисты», «Скачанные»,
//! «Ваши загрузки» и внизу блок «Недавно слушали» (последние 3 трека + «Смотреть все»).
//! «Ваши загрузки» это экран своих файлов (`View::Library`, ui/library.rs). Остальные подэкраны
//! пока заглушки: для лайков и плейлистов в ядре нет хранения, а у скачанных нет списка (кэш движка
//! хранит только последние 20 треков и списком не отдаётся). История живёт в `Controller::history`
//! только в рамках сеанса (на диск не пишется: `Track` не умеет сохраняться).

use egui::{Align, RichText, ScrollArea, Sense, Ui, Vec2};
use mcrate_core::Track;

use egui_material_icons::icons as mi;

use super::components::{SIDE_COVER_SIZE, back_header, empty_state, empty_state_top_space, nav_list, split_page, track_list};
use super::layout::{SETTINGS_CONTENT_MAX_WIDTH, TRACK_LIST_MAX_WIDTH, bleed_clip, centered_column, pane_scroll, scroll_end_padding};
use super::settings::controls::{Row, Trailing, settings_group};
use super::widgets::{ButtonKind, pill_button};
use crate::app::{McrateApp, View};
use crate::icons::{self, Icon};
use crate::theme::Theme;

/// Сколько последних треков показываем внизу главного экрана раздела.
const RECENT_COUNT: usize = 3;

impl McrateApp {
    /// Главный экран раздела «Библиотека».
    pub(crate) fn collection_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, f32::INFINITY, |ui| {
            let p = &theme.palette;
            centered_column(ui, SETTINGS_CONTENT_MAX_WIDTH, |ui| {
                ui.heading("Библиотека");
                ui.add_space(8.0);
            });
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    bleed_clip(ui, self.bleed.below_header());

                    let uploads = format!("Своих файлов: {}", self.ctl.library.len());
                    let views = [View::CollectionLiked, View::CollectionPlaylists, View::CollectionDownloaded, View::Library];
                    // Меню шириной 700 и с тем же видом пунктов, что на экране настроек.
                    centered_column(ui, SETTINGS_CONTENT_MAX_WIDTH, |ui| {
                        if theme.glass_active() {
                            let rows = [
                                (views[0], "Понравившиеся", "Скоро".to_string()),
                                (views[1], "Плейлисты", "Скоро".to_string()),
                                (views[2], "Скачанные", "Скоро".to_string()),
                                (views[3], "Ваши загрузки", uploads),
                            ];
                            if let Some(view) = nav_list(ui, theme, &rows, SETTINGS_CONTENT_MAX_WIDTH, true) {
                                self.view = view;
                            }
                        } else {
                            let rows = vec![
                                Row::new(mi::ICON_FAVORITE, "Понравившиеся", Trailing::Value("Скоро".into())),
                                Row::new(mi::ICON_QUEUE_MUSIC, "Плейлисты", Trailing::Value("Скоро".into())),
                                Row::new(mi::ICON_DOWNLOAD, "Скачанные", Trailing::Value("Скоро".into())),
                                Row::new(mi::ICON_FOLDER, "Ваши загрузки", Trailing::Value(uploads)),
                            ];
                            if let Some(i) = settings_group(ui, theme, "collection", rows) {
                                self.view = views[i];
                            }
                        }
                    });

                    // ---------- недавно слушали ----------
                    centered_column(ui, SETTINGS_CONTENT_MAX_WIDTH, |ui| {
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Недавно слушали").size(18.0).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if !self.ctl.history.is_empty() && pill_button(ui, theme, "Смотреть все", None, ButtonKind::Tonal).clicked()
                                {
                                    self.view = View::CollectionHistory;
                                }
                            });
                        });
                        ui.add_space(6.0);
                        if self.ctl.history.is_empty() {
                            ui.label(RichText::new("Здесь появятся треки, которые вы включали в этом сеансе.").color(p.on_surface_variant));
                        } else {
                            let recent = &self.ctl.history[..self.ctl.history.len().min(RECENT_COUNT)];
                            if let Some(i) = self.history_rows(ui, theme, recent, "glide_recent") {
                                // Очередь = эти же три трека (как в «Моей музыке»: очередь это показанный список).
                                self.ctl.play_list(recent.to_vec(), i);
                            }
                        }
                    });
                    scroll_end_padding(ui, self.bottom_inset);
                })
            });
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
                    bleed_clip(ui, self.bleed.below_header());
                    if self.ctl.history.is_empty() {
                        ui.label(RichText::new("Пока пусто: включите любой трек.").color(p.on_surface_variant));
                    } else {
                        let items = self.ctl.history.clone();
                        if let Some(i) = self.history_rows(ui, theme, &items, "glide_history") {
                            self.ctl.play_list(items, i);
                        }
                    }
                    scroll_end_padding(ui, self.bottom_inset);
                })
            });
        });
    }

    pub(crate) fn collection_liked_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.collection_stub(
            ui,
            theme,
            "Понравившиеся",
            Icon::Note,
            "Лайки пока не хранятся",
            "Кнопка лайка и список появятся, когда в ядре будет хранение лайков.",
        );
    }

    /// Требование к будущему экрану плейлиста: двухколоночная компоновка как на референсе.
    /// Слева — ограниченная панель обложки, описания, владельца и кнопок управления;
    /// справа — отдельный список треков с мини-обложкой, названием/исполнителем,
    /// самостоятельными колонками лайка и длительности. Список не растягивать на весь экран.
    pub(crate) fn collection_playlists_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.collection_stub(ui, theme, "Плейлисты", Icon::Queue, "Плейлистов пока нет", "Создание и хранение плейлистов ещё не сделано.");
    }

    pub(crate) fn collection_downloaded_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.collection_stub(
            ui,
            theme,
            "Скачанные",
            Icon::Folder,
            "Списка скачанных пока нет",
            "Движок временно кэширует последние 20 онлайн-треков, но списком их не отдаёт. Постоянные загрузки появятся позже.",
        );
    }

    /// Пустой экран коллекции в той же двухколоночной компоновке, что и страницы плейлистов.
    fn collection_stub(&mut self, ui: &mut Ui, theme: &Theme, title: &str, icon: Icon, headline: &str, text: &str) {
        let p = &theme.palette;
        centered_column(ui, f32::INFINITY, |ui| {
            if back_header(ui, theme, "Библиотека") {
                self.view = View::Collection;
            }
            ui.add_space(14.0);

            // Liquid Glass сохраняет прежнюю компактную заглушку. Material 3 использует
            // тот же левый блок коллекции и правую область, что и «Ваши загрузки».
            if theme.glass_active() {
                empty_state(ui, theme, empty_state_top_space(ui.available_height(), self.bottom_inset), icon, headline, text);
                return;
            }

            split_page(ui, Align::Center, 24.0, |ui, page| {
                if page.wide {
                    page.side(ui, |ui| {
                        ui.add_space(12.0);
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(SIDE_COVER_SIZE), Sense::hover());
                        ui.painter().rect_filled(rect, theme.shape_radius(crate::theme::ShapeRole::Large), p.surface_container_high);
                        icons::paint(theme, ui.painter(), icon, rect.center(), 104.0, p.on_surface_variant);
                        ui.add_space(18.0);
                        ui.label(RichText::new(title).size(theme.text_size(crate::theme::TextRole::HeadlineSmall)).strong());
                        ui.add_space(6.0);
                        ui.label(RichText::new("Коллекция Mcrate").size(13.0).color(p.on_surface_variant));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Пока нет сохранённых элементов").size(13.0).color(p.on_surface_variant));
                    });
                    page.main(ui, |ui| {
                        ui.add_space((ui.available_height() * 0.24).max(20.0));
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                        icons::paint(theme, ui.painter(), icon, rect.center(), 42.0, p.on_surface_variant);
                        ui.add_space(12.0);
                        ui.label(RichText::new(headline).size(20.0).strong());
                        ui.add_space(5.0);
                        ui.add(egui::Label::new(RichText::new(text).size(14.0).color(p.on_surface_variant)).wrap());
                    });
                } else {
                    page.side(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(88.0), Sense::hover());
                            ui.painter().rect_filled(rect, theme.shape_radius(crate::theme::ShapeRole::Medium), p.surface_container_high);
                            icons::paint(theme, ui.painter(), icon, rect.center(), 42.0, p.on_surface_variant);
                            ui.add_space(14.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(title).size(theme.text_size(crate::theme::TextRole::TitleLarge)).strong());
                                ui.label(RichText::new("Коллекция Mcrate").size(13.0).color(p.on_surface_variant));
                            });
                        });
                    });
                    page.main(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(headline).size(20.0).strong());
                            ui.add(egui::Label::new(RichText::new(text).size(14.0).color(p.on_surface_variant)).wrap());
                        });
                    });
                }
            });
        });
    }

    /// Список треков из истории; возвращает номер строки, по которой кликнули.
    fn history_rows(&self, ui: &mut Ui, theme: &Theme, items: &[Track], glide_id: &str) -> Option<usize> {
        centered_column(ui, TRACK_LIST_MAX_WIDTH, |ui| {
            let current = self.ctl.now.as_ref().map(|t| (t.id.clone(), t.provider));
            let playing = self.ctl.is_playing();
            let is_current = |track: &Track| current.as_ref().is_some_and(|(id, kind)| *id == track.id && *kind == track.provider);
            track_list(ui, theme, items.iter().enumerate(), is_current, playing, glide_id)
        })
    }
}

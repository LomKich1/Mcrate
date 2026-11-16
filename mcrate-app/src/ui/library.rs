//! Экран «Ваши загрузки» (раньше «Моя музыка»): свои файлы (LocalProvider). Подэкран раздела «Библиотека».

use egui::{Align, Layout, RichText, Sense, Ui, Vec2};

use super::layout::{bleed_clip, centered_column};
use super::settings_view::settings_header;
use super::widgets::{ButtonKind, error_banner, glide_begin, glide_end, pill_button, search_bar, status_snackbar, track_row};
use crate::app::{McrateApp, View};
use crate::icons::{self, Icon};
use crate::theme::{Theme, with_alpha};

/// Расширения для диалога выбора файлов. Список такой же, как в LocalProvider.
const AUDIO_EXT: [&str; 6] = ["mp3", "flac", "ogg", "wav", "m4a", "aac"];

impl McrateApp {
    pub fn library_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, 1160.0, |ui| {
            let p = &theme.palette;

            ui.horizontal(|ui| {
                if settings_header(ui, theme, "Ваши загрузки") {
                    self.view = View::Collection;
                }
                // right_to_left: первый добавленный элемент окажется правее остальных.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if pill_button(ui, theme, "Папка", Some(Icon::Folder), ButtonKind::Tonal).clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.ctl.add_paths(&[dir]);
                        }
                    }
                    if pill_button(ui, theme, "Файлы", Some(Icon::Plus), ButtonKind::Filled).clicked() {
                        if let Some(files) = rfd::FileDialog::new().add_filter("Аудио", &AUDIO_EXT).pick_files() {
                            self.ctl.add_paths(&files);
                        }
                    }
                });
            });
            ui.add_space(10.0);

            search_bar(ui, theme, &mut self.search, "Поиск по названию и исполнителю");

            if let Some(status) = self.ctl.status.clone() {
                ui.add_space(6.0);
                if theme.glass_active() {
                    ui.label(RichText::new(status).size(13.0).color(p.on_surface_variant));
                } else if status_snackbar(ui, theme, &status) {
                    self.ctl.status = None;
                }
            }
            ui.add_space(10.0);

            if self.ctl.library.is_empty() {
                self.empty_library(ui, theme);
                return;
            }
            if self.filtered.is_empty() {
                ui.label(RichText::new("Ничего не найдено").color(p.on_surface_variant));
                return;
            }

            let count = self.filtered.len();
            let current_id = self.ctl.now.as_ref().map(|t| t.id.clone());
            let playing = self.ctl.is_playing();
            let time = ui.input(|i| i.time);
            let bleed = self.bleed.below_header();
            let mut clicked: Option<usize> = None;
            // show_rows использует фиксированную высоту строки. Резервируем безопасную зону
            // целыми невидимыми строками, чтобы список можно было прокрутить над Glass-плеером.
            let row_height = theme.list_item_height(2);
            let spacer_rows = if self.bottom_inset > 0.0 {
                ((self.bottom_inset + 16.0) / row_height).ceil() as usize
            } else {
                0
            };

            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_height, count + spacer_rows, |ui, range| {
                    bleed_clip(ui, bleed);
                    let glide_slot = glide_begin(ui);
                    let mut hovered = None;
                    for i in range {
                        if i >= count {
                            ui.allocate_exact_size(Vec2::new(1.0, row_height), Sense::hover());
                            continue;
                        }
                        let track = &self.filtered[i];
                        let is_current = current_id.as_deref() == Some(track.id.as_str());
                        let resp = track_row(ui, theme, i, track, is_current, is_current && playing, time);
                        if resp.hovered() {
                            hovered = Some((egui::Id::unique(&track.id).value(), resp.rect));
                        }
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                    }
                    glide_end(ui, theme, glide_slot, ui.scope_id().with("glide_library"), hovered, 18.0);
                });
            });

            if let Some(i) = clicked {
                // Очередь = текущий отфильтрованный список, старт с выбранного трека.
                let items = self.filtered.clone();
                self.ctl.play_list(items, i);
            }
        });
    }

    fn empty_library(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        ui.vertical_centered(|ui| {
            ui.add_space(((ui.available_height() - self.bottom_inset).max(1.0) * 0.22).max(16.0));
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(88.0), Sense::hover());
            if theme.glass_active() {
                ui.painter().circle_stroke(rect.center(), 42.0, egui::Stroke::new(0.9, with_alpha(p.on_surface, 46)));
                icons::paint(theme, ui.painter(), Icon::Note, rect.center(), 40.0, p.on_surface);
            } else {
                ui.painter().circle_filled(rect.center(), 44.0, theme.tonal_fill());
                icons::paint(theme, ui.painter(), Icon::Note, rect.center(), 40.0, theme.on_tonal());
            }
            ui.add_space(14.0);
            ui.label(RichText::new("Здесь пока пусто").size(20.0).strong());
            ui.label(
                RichText::new("Перетащите аудиофайлы или папку в окно\nили нажмите «Файлы» / «Папка»")
                    .size(14.0)
                    .color(p.on_surface_variant),
            );
            if let Some(err) = &self.ctl.audio_error {
                ui.add_space(10.0);
                error_banner(ui, theme, &format!("Звук недоступен: {err}"));
            }
        });
    }
}

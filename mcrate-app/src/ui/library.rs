//! Экран «Ваши загрузки» (раньше «Моя музыка»): свои файлы (LocalProvider). Подэкран раздела «Библиотека».

use egui::{Align, Layout, RichText, Sense, Ui, Vec2, vec2};

use super::home::{title_search_row, tracks_word};
use super::layout::{centered_column, TRACK_LIST_MAX_WIDTH};
use super::widgets::{ButtonKind, cover_tile, error_banner, glide_begin, glide_end, pill_button, status_snackbar, track_row};
use crate::app::{McrateApp, View};
use crate::icons::{self, Icon};
use crate::theme::{Theme, with_alpha};

/// Расширения для диалога выбора файлов. Список такой же, как в LocalProvider.
const AUDIO_EXT: [&str; 6] = ["mp3", "flac", "ogg", "wav", "m4a", "aac"];

impl McrateApp {
    pub fn library_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, f32::INFINITY, |ui| {
            let p = &theme.palette;

            // Шапка как на главной: «назад», заголовок с подписью, поиск и кнопки «Папка»/«Файлы» на одной линии.
            let ctl = &mut self.ctl;
            let (_, back) = title_search_row(
                ui, theme, "Ваши загрузки", "Локальная коллекция", true, &mut self.search,
                "Поиск по названию и исполнителю", 250.0,
                &mut |ui| {
                    // right_to_left: первой добавленная кнопка окажется правее.
                    if pill_button(ui, theme, "Файлы", Some(Icon::Plus), ButtonKind::Filled).clicked() {
                        if let Some(files) = rfd::FileDialog::new().add_filter("Аудио", &AUDIO_EXT).pick_files() {
                            ctl.add_paths(&files);
                        }
                    }
                    if pill_button(ui, theme, "Папка", Some(Icon::Folder), ButtonKind::Tonal).clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            ctl.add_paths(&[dir]);
                        }
                    }
                },
                false,
            );
            if back {
                self.view = View::Collection;
            }
            ui.add_space(10.0);

            if let Some(status) = self.ctl.status.clone() {
                if theme.glass_active() {
                    ui.label(RichText::new(status).size(13.0).color(p.on_surface_variant));
                } else if status_snackbar(ui, theme, &status) {
                    self.ctl.status = None;
                }
                ui.add_space(8.0);
            }

            if self.ctl.library.is_empty() {
                self.empty_library(ui, theme);
                return;
            }

            let count = self.filtered.len();
            let total = self.ctl.library.len();
            let current_id = self.ctl.now.as_ref().map(|t| t.id.clone());
            let playing = self.ctl.is_playing();
            let time = ui.input(|i| i.time);

            // Компоновка как у страницы плейлиста: фиксированная карточка коллекции слева,
            // самостоятельный прокручиваемый список справа. На узком окне блок схлопывается.
            centered_column(ui, 1360.0, |ui| {
                let split = ui.available_width() >= 920.0;
                if split {
                    let available_h = ui.available_height();
                    let left_w = 292.0;
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            vec2(left_w, available_h),
                            Layout::top_down(Align::Center),
                            |ui| self.uploads_playlist_panel(ui, theme, total, count, 252.0, true),
                        );
                        ui.add_space(28.0);
                        ui.allocate_ui_with_layout(
                            vec2(ui.available_width(), available_h),
                            Layout::top_down(Align::Min),
                            |ui| self.uploads_track_list(ui, theme, count, &current_id, playing, time),
                        );
                    });
                } else {
                    self.uploads_playlist_panel(ui, theme, total, count, 96.0, false);
                    ui.add_space(18.0);
                    self.uploads_track_list(ui, theme, count, &current_id, playing, time);
                }
            });
        });
    }

    /// Левая информация о локальной коллекции (обложка, описание и действия).
    fn uploads_playlist_panel(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        total: usize,
        filtered: usize,
        cover_size: f32,
        vertical: bool,
    ) {
        let p = &theme.palette;
        if vertical {
            ui.add_space(12.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(cover_size), Sense::hover());
            cover_tile(ui, theme, rect, "mcrate-local-uploads", theme.shape_radius(crate::theme::ShapeRole::Large) as f32);
            ui.add_space(18.0);
            ui.label(RichText::new("Ваши загрузки").size(theme.text_size(crate::theme::TextRole::HeadlineSmall)).strong());
            ui.add_space(5.0);
            ui.label(RichText::new("Локальная коллекция").size(13.0).color(p.on_surface_variant));
            ui.add_space(8.0);
            let meta = format!("{total} {} · файлы с устройства", tracks_word(total));
            ui.label(RichText::new(meta).size(13.0).color(p.on_surface_variant));
            ui.add_space(12.0);
            ui.add(egui::Label::new(
                RichText::new("Музыка, добавленная из файлов и папок на этом устройстве.")
                    .size(13.0)
                    .color(p.on_surface_variant),
            ).wrap());
            ui.add_space(18.0);
        } else {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(cover_size), Sense::hover());
                cover_tile(ui, theme, rect, "mcrate-local-uploads", theme.shape_radius(crate::theme::ShapeRole::Medium) as f32);
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Ваши загрузки").size(theme.text_size(crate::theme::TextRole::TitleLarge)).strong());
                    ui.add_space(3.0);
                    ui.label(RichText::new("Локальная коллекция").size(13.0).color(p.on_surface_variant));
                    let meta = format!("{total} {} · файлы с устройства", tracks_word(total));
                    ui.label(RichText::new(meta).size(13.0).color(p.on_surface_variant));
                });
            });
            ui.add_space(10.0);
        }

        if filtered > 0 {
            let items = self.filtered.clone();
            ui.horizontal(|ui| {
                if pill_button(ui, theme, "Перемешать", Some(Icon::Shuffle), ButtonKind::Tonal).clicked() {
                    self.ctl.toggle_shuffle();
                    self.ctl.play_list(items.clone(), 0);
                }
                if pill_button(ui, theme, "Слушать", Some(Icon::Play), ButtonKind::Filled).clicked() {
                    self.ctl.play_list(items, 0);
                }
            });
        }
    }

    /// Самостоятельный список с собственной прокруткой; ширина строк ограничена колонкой.
    fn uploads_track_list(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        count: usize,
        current_id: &Option<String>,
        playing: bool,
        time: f64,
    ) {
        let p = &theme.palette;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Треки").size(theme.text_size(crate::theme::TextRole::TitleLarge)).strong());
            ui.add_space(8.0);
            ui.label(RichText::new(count.to_string()).size(13.0).color(p.on_surface_variant));
        });
        ui.add_space(8.0);

        if count == 0 {
            ui.label(RichText::new("Ничего не найдено").color(p.on_surface_variant));
            return;
        }

        let mut clicked: Option<usize> = None;
        let row_height = theme.list_item_height(2);
        egui::ScrollArea::vertical()
            .id_salt("uploads_playlist_tracks")
            .auto_shrink([false, false])
            .show_rows(ui, row_height, count, |ui, range| {
                centered_column(ui, TRACK_LIST_MAX_WIDTH, |ui| {
                    let glide_slot = glide_begin(ui);
                    let mut hovered = None;
                    for i in range {
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
                    glide_end(ui, theme, glide_slot, ui.scope_id().with("glide_uploads_playlist"), hovered, 18.0);
                });
            });

        if let Some(i) = clicked {
            let items = self.filtered.clone();
            self.ctl.play_list(items, i);
        }
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

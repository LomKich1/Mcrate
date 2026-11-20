//! Экран «Ваши загрузки» (раньше «Моя музыка»): свои файлы (LocalProvider). Подэкран раздела «Библиотека».

use egui::{Align, RichText, Sense, Ui, Vec2};

use super::components::{SIDE_COVER_SIZE, TitleRow, empty_state_top_space, empty_state_with, split_page, title_search_row, track_list};
use super::format::tracks_word;
use super::layout::{TRACK_LIST_MAX_WIDTH, centered_column};
use super::widgets::{ButtonKind, cover_tile, error_banner, pill_button, status_snackbar};
use crate::app::{McrateApp, View};
use crate::icons::Icon;
use crate::theme::Theme;
use mcrate_core::Track;

/// Расширения для диалога выбора файлов. Список такой же, как в LocalProvider.
const AUDIO_EXT: [&str; 6] = ["mp3", "flac", "ogg", "wav", "m4a", "aac"];

impl McrateApp {
    pub(crate) fn library_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, f32::INFINITY, |ui| {
            let p = &theme.palette;

            // Шапка как на главной: «назад», заголовок с подписью, поиск и кнопки «Папка»/«Файлы» на одной линии.
            let ctl = &mut self.ctl;
            let header = TitleRow {
                title: "Ваши загрузки",
                subtitle: "Локальная коллекция",
                back: true,
                hint: "Поиск по названию и исполнителю",
                trailing_w: 250.0,
                centered_search: false,
            };
            let (_, back) = title_search_row(ui, theme, &header, &mut self.search, &mut |ui| {
                // right_to_left: первой добавленная кнопка окажется правее.
                if pill_button(ui, theme, "Файлы", Some(Icon::Plus), ButtonKind::Filled).clicked()
                    && let Some(files) = rfd::FileDialog::new().add_filter("Аудио", &AUDIO_EXT).pick_files()
                {
                    ctl.add_paths(&files);
                }
                if pill_button(ui, theme, "Папка", Some(Icon::Folder), ButtonKind::Tonal).clicked()
                    && let Some(dir) = rfd::FileDialog::new().pick_folder()
                {
                    ctl.add_paths(&[dir]);
                }
            });
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

            // Компоновка как у страницы плейлиста: фиксированная карточка коллекции слева,
            // самостоятельный прокручиваемый список справа. На узком окне блок схлопывается.
            split_page(ui, Align::Min, 18.0, |ui, page| {
                let wide = page.wide;
                page.side(ui, |ui| {
                    let cover = if wide { SIDE_COVER_SIZE } else { 96.0 };
                    self.uploads_playlist_panel(ui, theme, total, count, cover, wide);
                });
                page.main(ui, |ui| self.uploads_track_list(ui, theme, count, &current_id, playing));
            });
        });
    }

    /// Левая информация о локальной коллекции (обложка, описание и действия).
    fn uploads_playlist_panel(&mut self, ui: &mut Ui, theme: &Theme, total: usize, filtered: usize, cover_size: f32, vertical: bool) {
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
            ui.add(
                egui::Label::new(
                    RichText::new("Музыка, добавленная из файлов и папок на этом устройстве.").size(13.0).color(p.on_surface_variant),
                )
                .wrap(),
            );
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
            let (shuffle, play) = ui
                .horizontal(|ui| {
                    let shuffle = pill_button(ui, theme, "Перемешать", Some(Icon::Shuffle), ButtonKind::Tonal).clicked();
                    let play = pill_button(ui, theme, "Слушать", Some(Icon::Play), ButtonKind::Filled).clicked();
                    (shuffle, play)
                })
                .inner;
            // Список копируем только по клику, а не на каждом кадре.
            if shuffle {
                self.ctl.toggle_shuffle();
                self.ctl.play_list(self.filtered.clone(), 0);
            }
            if play {
                self.ctl.play_list(self.filtered.clone(), 0);
            }
        }
    }

    /// Самостоятельный список с собственной прокруткой; ширина строк ограничена колонкой.
    fn uploads_track_list(&mut self, ui: &mut Ui, theme: &Theme, count: usize, current_id: &Option<String>, playing: bool) {
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
        egui::ScrollArea::vertical().id_salt("uploads_playlist_tracks").auto_shrink([false, false]).show_rows(
            ui,
            row_height,
            count,
            |ui, range| {
                centered_column(ui, TRACK_LIST_MAX_WIDTH, |ui| {
                    let rows = range.map(|i| (i, &self.filtered[i]));
                    let is_current = |track: &Track| current_id.as_deref() == Some(track.id.as_str());
                    if let Some(i) = track_list(ui, theme, rows, is_current, playing, "glide_uploads_playlist") {
                        clicked = Some(i);
                    }
                });
            },
        );

        if let Some(i) = clicked {
            let items = self.filtered.clone();
            self.ctl.play_list(items, i);
        }
    }

    fn empty_library(&mut self, ui: &mut Ui, theme: &Theme) {
        let top = empty_state_top_space(ui.available_height(), self.bottom_inset);
        let audio_error = self.ctl.audio_error.as_deref();
        empty_state_with(
            ui,
            theme,
            top,
            Icon::Note,
            "Здесь пока пусто",
            "Перетащите аудиофайлы или папку в окно\nили нажмите «Файлы» / «Папка»",
            |ui| {
                if let Some(err) = audio_error {
                    ui.add_space(10.0);
                    error_banner(ui, theme, &format!("Звук недоступен: {err}"));
                }
            },
        );
    }
}

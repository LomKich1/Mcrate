//! Экран «Моя музыка»: свои файлы (LocalProvider).

use egui::{Align, Layout, RichText, Sense, Ui, Vec2};

use super::widgets::{ButtonKind, pill_button, track_row};
use crate::app::McrateApp;
use crate::icons::{self, Icon};
use crate::theme::Theme;

/// Расширения для диалога выбора файлов. Список такой же, как в LocalProvider.
const AUDIO_EXT: [&str; 6] = ["mp3", "flac", "ogg", "wav", "m4a", "aac"];

impl McrateApp {
    pub fn library_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;

        ui.horizontal(|ui| {
            ui.heading("Моя музыка");
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
        ui.add_space(6.0);

        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Поиск по названию и исполнителю")
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(16, 10)),
        );

        if let Some(status) = &self.ctl.status {
            ui.add_space(2.0);
            ui.label(RichText::new(status).size(13.0).color(p.on_surface_variant));
        }
        ui.add_space(8.0);

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
        let mut clicked: Option<usize> = None;

        // scope: меняем интервал между строками только для списка.
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            // show_rows рисует только видимые строки (быстро даже на тысячах треков);
            // для этого высота строки должна быть фиксированной.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 56.0, count, |ui, range| {
                for i in range {
                    let track = &self.filtered[i];
                    let is_current = current_id.as_deref() == Some(track.id.as_str());
                    if track_row(ui, theme, i, track, is_current, is_current && playing, time).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });

        if let Some(i) = clicked {
            // Очередь = текущий отфильтрованный список, старт с выбранного трека.
            let items = self.filtered.clone();
            self.ctl.play_list(items, i);
        }
    }

    fn empty_library(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.22).max(16.0));
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(88.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 44.0, theme.tonal_fill());
            icons::paint(ui.painter(), Icon::Note, rect.center(), 40.0, theme.on_tonal());
            ui.add_space(14.0);
            ui.label(RichText::new("Здесь пока пусто").size(20.0).strong());
            ui.label(
                RichText::new("Перетащите аудиофайлы или папку в окно\nили нажмите «Файлы» / «Папка»")
                    .size(14.0)
                    .color(p.on_surface_variant),
            );
            if let Some(err) = &self.ctl.audio_error {
                ui.add_space(10.0);
                ui.label(RichText::new(format!("Звук недоступен: {err}")).size(13.0).color(p.error));
            }
        });
    }
}

//! Экран «Сейчас играет».
//! Список «дальше в очереди» пока показать нельзя: у PlayQueue нет публичного доступа
//! к элементам. Если он понадобится, надо согласовать с LomKich метод вроде `items()`.

use egui::{RichText, Sense, Ui, Vec2};

use super::layout::cover_side;
use super::widgets::{artists_text, cover_tile};
use crate::app::McrateApp;
use crate::theme::Theme;

impl McrateApp {
    pub fn now_playing_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        ui.heading("Сейчас играет");

        let Some(track) = self.ctl.now.clone() else {
            ui.add_space(12.0);
            ui.label(RichText::new("Выберите трек в библиотеке").color(p.on_surface_variant));
            return;
        };

        ui.vertical_centered(|ui| {
            ui.add_space(12.0);
            let side = cover_side(ui.available_width(), ui.available_height());
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
            cover_tile(ui, theme, rect, &track.id, 36.0);
            ui.add_space(18.0);
            ui.label(RichText::new(&track.title).size(26.0).strong());
            ui.label(RichText::new(artists_text(&track)).size(16.0).color(p.on_surface_variant));
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("Свои файлы · в очереди: {}", self.ctl.queue.len()))
                    .size(13.0)
                    .color(p.on_surface_variant),
            );
        });
    }
}

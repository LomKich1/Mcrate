//! Левая навигация.

use egui::{RichText, Sense, Ui, Vec2};

use super::widgets::{cover_tile, nav_item, section_label};
use crate::app::{McrateApp, View};
use crate::icons::Icon;
use crate::theme::Theme;

impl McrateApp {
    pub fn rail_view(&mut self, ui: &mut Ui, theme: &Theme, compact: bool) {
        ui.add_space(4.0);
        if compact {
            // Узкая панель: только значок приложения по центру.
            ui.vertical_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                cover_tile(ui, theme, rect, "mcrate", 13.0);
            });
        } else {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                cover_tile(ui, theme, rect, "mcrate", 12.0);
                ui.label(RichText::new("Mcrate").size(22.0).strong());
            });
        }
        ui.add_space(14.0);

        for (view, icon, label) in [
            (View::Library, Icon::Note, "Моя музыка"),
            (View::NowPlaying, Icon::Queue, "Сейчас играет"),
            (View::Settings, Icon::Settings, "Оформление"),
        ] {
            if nav_item(ui, theme, icon, label, self.view == view, true, None, compact).clicked() {
                self.view = view;
            }
        }

        ui.add_space(10.0);
        if compact {
            ui.add_space(6.0); // вместо подписи «Сервисы» просто отступ
        } else {
            section_label(ui, theme, "Сервисы");
        }
        // Провайдеры появятся по шагам плана (CLAUDE.md, п. 3). Пока это заглушки.
        for name in ["Yandex Music", "VK Music", "SoundCloud", "Spotify"] {
            nav_item(ui, theme, Icon::Plus, name, false, false, Some("скоро"), compact);
        }
    }
}

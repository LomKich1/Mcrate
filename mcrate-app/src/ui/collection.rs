//! Раздел «Библиотека»: главный экран-список и его подэкраны.
//!
//! Главный экран (`View::Collection`): строки-переходы «Понравившиеся», «Плейлисты», «Скачанные»,
//! «Ваши загрузки» и внизу блок «Недавно слушали» (последние 3 трека + «Смотреть все»).
//! «Ваши загрузки» это экран своих файлов (`View::Library`, ui/library.rs). Остальные подэкраны
//! пока заглушки: для лайков и плейлистов в ядре нет хранения, а у скачанных нет списка (кэш движка
//! хранит только последние 20 треков и списком не отдаётся). История живёт в `Controller::history`
//! только в рамках сеанса (на диск не пишется: `Track` не умеет сохраняться).

use egui::{RichText, ScrollArea, Sense, Ui, Vec2};
use mcrate_core::Track;

use super::layout::{bleed_clip, centered_column, scroll_end_padding};
use super::settings_view::{settings_header, settings_nav_row};
use super::widgets::{ButtonKind, glide_begin, glide_end, pill_button, track_row};
use crate::app::{McrateApp, View};
use crate::icons::{self, Icon};
use crate::theme::{Theme, with_alpha};

/// Сколько последних треков показываем внизу главного экрана раздела.
const RECENT_COUNT: usize = 3;

impl McrateApp {
    /// Главный экран раздела «Библиотека».
    pub fn collection_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, 760.0, |ui| {
            let p = &theme.palette;
            ui.heading("Библиотека");
            ui.add_space(8.0);

            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                bleed_clip(ui, self.bleed.below_header());

                let uploads = format!("Своих файлов: {}", self.ctl.library.len());
                let rows = [
                    (View::CollectionLiked, "Понравившиеся", "Скоро".to_string()),
                    (View::CollectionPlaylists, "Плейлисты", "Скоро".to_string()),
                    (View::CollectionDownloaded, "Скачанные", "Скоро".to_string()),
                    (View::Library, "Ваши загрузки", uploads),
                ];
                for (view, title, subtitle) in rows {
                    if settings_nav_row(ui, theme, title, &subtitle).clicked() {
                        self.view = view;
                    }
                    ui.add_space(8.0);
                }

                // ---------- недавно слушали ----------
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Недавно слушали").size(18.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.ctl.history.is_empty()
                            && pill_button(ui, theme, "Смотреть все", None, ButtonKind::Tonal).clicked()
                        {
                            self.view = View::CollectionHistory;
                        }
                    });
                });
                ui.add_space(6.0);
                if self.ctl.history.is_empty() {
                    ui.label(
                        RichText::new("Здесь появятся треки, которые вы включали в этом сеансе.")
                            .color(p.on_surface_variant),
                    );
                } else {
                    let recent: Vec<Track> = self.ctl.history.iter().take(RECENT_COUNT).cloned().collect();
                    if let Some(i) = self.history_rows(ui, theme, &recent, "glide_recent") {
                        // Очередь = эти же три трека (как в «Моей музыке»: очередь это показанный список).
                        self.ctl.play_list(recent, i);
                    }
                }
                scroll_end_padding(ui, self.bottom_inset);
            });
        });
    }

    /// Полная история прослушивания (за сеанс).
    pub fn collection_history_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, 900.0, |ui| {
            let p = &theme.palette;
            if settings_header(ui, theme, "История") {
                self.view = View::Collection;
            }
            ui.add_space(4.0);

            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
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
            });
        });
    }

    pub fn collection_liked_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.collection_stub(
            ui,
            theme,
            "Понравившиеся",
            Icon::Note,
            "Лайки пока не хранятся",
            "Кнопка лайка и список появятся, когда в ядре будет хранение лайков.",
        );
    }

    pub fn collection_playlists_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.collection_stub(
            ui,
            theme,
            "Плейлисты",
            Icon::Queue,
            "Плейлистов пока нет",
            "Создание и хранение плейлистов ещё не сделано.",
        );
    }

    pub fn collection_downloaded_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.collection_stub(
            ui,
            theme,
            "Скачанные",
            Icon::Folder,
            "Списка скачанных пока нет",
            "Движок временно кэширует последние 20 онлайн-треков, но списком их не отдаёт. Постоянные загрузки появятся позже.",
        );
    }

    /// Подэкран-заглушка: заголовок с кнопкой «назад» и пустое состояние по центру.
    fn collection_stub(&mut self, ui: &mut Ui, theme: &Theme, title: &str, icon: Icon, headline: &str, text: &str) {
        let p = &theme.palette;
        centered_column(ui, 760.0, |ui| {
            if settings_header(ui, theme, title) {
                self.view = View::Collection;
            }
            ui.vertical_centered(|ui| {
                ui.add_space(((ui.available_height() - self.bottom_inset).max(1.0) * 0.22).max(16.0));
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(88.0), Sense::hover());
                if theme.glass_active() {
                    ui.painter().circle_stroke(rect.center(), 42.0, egui::Stroke::new(0.9, with_alpha(p.on_surface, 46)));
                    icons::paint(ui.painter(), icon, rect.center(), 40.0, p.on_surface);
                } else {
                    ui.painter().circle_filled(rect.center(), 44.0, theme.tonal_fill());
                    icons::paint(ui.painter(), icon, rect.center(), 40.0, theme.on_tonal());
                }
                ui.add_space(14.0);
                ui.label(RichText::new(headline).size(20.0).strong());
                ui.label(RichText::new(text).size(14.0).color(p.on_surface_variant));
            });
        });
    }

    /// Список треков из истории; возвращает номер строки, по которой кликнули.
    fn history_rows(&self, ui: &mut Ui, theme: &Theme, items: &[Track], glide_id: &str) -> Option<usize> {
        let current = self.ctl.now.as_ref().map(|t| (t.id.clone(), t.provider));
        let playing = self.ctl.is_playing();
        let time = ui.input(|i| i.time);
        let glide_slot = glide_begin(ui);
        let mut hovered = None;
        let mut clicked = None;
        for (i, track) in items.iter().enumerate() {
            let is_current = current.as_ref().is_some_and(|(id, kind)| *id == track.id && *kind == track.provider);
            let resp = track_row(ui, theme, i, track, is_current, is_current && playing, time);
            if resp.hovered() {
                hovered = Some((egui::Id::unique(&track.id).value(), resp.rect));
            }
            if resp.clicked() {
                clicked = Some(i);
            }
        }
        glide_end(ui, theme, glide_slot, ui.scope_id().with(glide_id), hovered, 18.0);
        clicked
    }
}

//! Горизонтальные полки главной: «Быстрый выбор» (Liquid Glass) и «Ваши исполнители».

use super::media_card::{MediaCard, media_card};
use super::shelf::{ShelfSpec, shelf};
use crate::app::McrateApp;
use crate::theme::Theme;
use crate::ui::format::{artists_text, tracks_word};
use egui::Ui;
use mcrate_core::Track;
use std::collections::HashMap;

const QUICK_W: f32 = 160.0; // карточка «Быстрого выбора» (M3 card: обложка 144 + подпись)
const QUICK_H: f32 = 214.0;
const ARTIST_W: f32 = 144.0; // исполнитель: круглый аватар 128 + подпись по центру
const ARTIST_H: f32 = 190.0;
const GLASS_QUICK_COUNT: usize = 12; // прежнее количество карточек Liquid Glass
const ARTISTS_COUNT: usize = 12; // сколько исполнителей на полке

impl McrateApp {
    /// Полка «Быстрый выбор»: последние добавленные треки (LocalProvider хранит их в порядке добавления).
    pub(super) fn home_quick(&mut self, ui: &mut Ui, theme: &Theme) {
        // Liquid Glass сохраняет исходные 12 карточек.
        let recents: Vec<(usize, &Track)> = self.ctl.library.iter().enumerate().rev().take(GLASS_QUICK_COUNT).collect();
        let current_id = self.ctl.now.as_ref().map(|t| t.id.as_str());
        let playing = self.ctl.is_playing();
        let time = ui.input(|i| i.time);
        let mut play_index: Option<usize> = None;

        let spec = ShelfSpec {
            id: "quick", title: "Быстрый выбор", subtitle: "Недавно добавленные: запустите музыку с любого трека"
        };
        shelf(ui, theme, self.bleed, &spec, |ui| {
            for (idx, track) in &recents {
                let is_current = current_id == Some(track.id.as_str());
                let artists = artists_text(track);
                let card = MediaCard { seed: &track.id, title: &track.title, subtitle: &artists, round: false, w: QUICK_W, h: QUICK_H };
                if media_card(ui, theme, &card, is_current, playing, time).clicked() {
                    play_index = Some(*idx);
                }
            }
        });
        if let Some(i) = play_index {
            self.play_library_from(i);
        }
    }

    /// Полка «Ваши исполнители»: из локальной библиотеки. Клик включает все треки исполнителя.
    pub(super) fn home_artists(&mut self, ui: &mut Ui, theme: &Theme) {
        let artists = self.top_artists(ui);
        if artists.is_empty() {
            return;
        }
        let time = ui.input(|i| i.time);
        let mut play_artist: Option<String> = None;

        let spec = ShelfSpec {
            id: "artists", title: "Ваши исполнители", subtitle: "Из вашей библиотеки, больше всего треков сверху"
        };
        shelf(ui, theme, self.bleed, &spec, |ui| {
            for (name, count) in &artists {
                let sub = format!("{count} {}", tracks_word(*count));
                let card = MediaCard { seed: name, title: name, subtitle: &sub, round: true, w: ARTIST_W, h: ARTIST_H };
                if media_card(ui, theme, &card, false, false, time).clicked() {
                    play_artist = Some(name.clone());
                }
            }
        });
        if let Some(name) = play_artist {
            let items: Vec<Track> = self.ctl.library.iter().filter(|t| t.artists.contains(&name)).cloned().collect();
            if !items.is_empty() {
                self.ctl.play_list(items, 0);
            }
        }
    }

    /// Исполнители по числу треков. Пересчитываем только когда меняется размер библиотеки.
    fn top_artists(&self, ui: &Ui) -> Vec<(String, usize)> {
        let id = ui.scope_id().with("artists_cache");
        let len = self.ctl.library.len();
        if let Some((cached_len, items)) = ui.ctx().data_mut(|d| d.get_temp::<(usize, Vec<(String, usize)>)>(id))
            && cached_len == len
        {
            return items;
        }
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for track in &self.ctl.library {
            for artist in &track.artists {
                *counts.entry(artist.as_str()).or_default() += 1;
            }
        }
        let mut items: Vec<(String, usize)> = counts.into_iter().map(|(a, n)| (a.to_string(), n)).collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        items.truncate(ARTISTS_COUNT);
        ui.ctx().data_mut(|d| d.insert_temp(id, (len, items.clone())));
        items
    }
}

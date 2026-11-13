//! Нижняя панель плеера: трек слева, управление и перемотка по центру, громкость справа.

use egui::{Align, Layout, RichText, Sense, Ui, Vec2, vec2};
use mcrate_audio::RepeatMode;

use super::widgets::{ButtonKind, artists_text, cover_tile, fmt_time, icon_button, slider_bar};
use crate::app::McrateApp;
use crate::icons::Icon;
use crate::theme::Theme;

impl McrateApp {
    pub fn player_bar(&mut self, ui: &mut Ui, theme: &Theme) {
        let total = ui.available_width();
        let h = 68.0;
        let gap = ui.spacing().item_spacing.x;
        let left_w = (total * 0.27).max(190.0);
        let right_w = (total * 0.20).max(150.0);
        let center_w = (total - left_w - right_w - gap * 2.0).max(240.0);

        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(vec2(left_w, h), Layout::left_to_right(Align::Center), |ui| {
                self.pb_track_info(ui, theme, left_w);
            });
            ui.allocate_ui_with_layout(vec2(center_w, h), Layout::top_down(Align::Center), |ui| {
                self.pb_controls(ui, theme, center_w);
            });
            ui.allocate_ui_with_layout(vec2(right_w, h), Layout::right_to_left(Align::Center), |ui| {
                self.pb_volume(ui, theme);
            });
        });
    }

    fn pb_track_info(&mut self, ui: &mut Ui, theme: &Theme, width: f32) {
        let p = &theme.palette;
        match self.ctl.now.clone() {
            Some(t) => {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
                cover_tile(ui, theme, rect, &t.id, 14.0);
                ui.vertical(|ui| {
                    ui.set_max_width((width - 52.0 - 14.0).max(60.0));
                    // truncate = длинный текст обрезается многоточием, а не ломает раскладку
                    ui.add(egui::Label::new(RichText::new(&t.title).size(15.0).strong()).truncate());
                    ui.add(egui::Label::new(RichText::new(artists_text(&t)).size(12.5).color(p.on_surface_variant)).truncate());
                });
            }
            None => {
                ui.label(RichText::new("Ничего не играет").color(p.on_surface_variant));
            }
        }
    }

    fn pb_controls(&mut self, ui: &mut Ui, theme: &Theme, width: f32) {
        // Ширина ряда кнопок известна заранее, по ней центрируем: 36+40+52+40+36 + 4 зазора.
        let controls_w = 36.0 + 40.0 + 52.0 + 40.0 + 36.0 + 4.0 * ui.spacing().item_spacing.x;
        ui.horizontal(|ui| {
            ui.add_space(((width - controls_w) / 2.0).max(0.0));

            let shuffle = self.ctl.queue.shuffle();
            if icon_button(ui, theme, Icon::Shuffle, 36.0, ButtonKind::Plain, shuffle, "Перемешать").clicked() {
                self.ctl.toggle_shuffle();
            }
            if icon_button(ui, theme, Icon::Prev, 40.0, ButtonKind::Plain, false, "Предыдущий").clicked() {
                self.ctl.prev();
            }
            let (play_icon, tip) = if self.ctl.is_playing() { (Icon::Pause, "Пауза") } else { (Icon::Play, "Играть") };
            if icon_button(ui, theme, play_icon, 52.0, ButtonKind::Filled, false, tip).clicked() {
                self.ctl.toggle_pause();
            }
            if icon_button(ui, theme, Icon::Next, 40.0, ButtonKind::Plain, false, "Следующий").clicked() {
                self.ctl.next();
            }
            let (rep_icon, rep_active, rep_tip) = match self.ctl.repeat_mode() {
                RepeatMode::Off => (Icon::Repeat, false, "Повтор: выкл"),
                RepeatMode::All => (Icon::Repeat, true, "Повтор: все"),
                RepeatMode::One => (Icon::RepeatOne, true, "Повтор: один трек"),
            };
            if icon_button(ui, theme, rep_icon, 36.0, ButtonKind::Plain, rep_active, rep_tip).clicked() {
                self.ctl.cycle_repeat();
            }
        });

        // Строка перемотки: время, полоса, длительность.
        let p = &theme.palette;
        let dur = self.ctl.now.as_ref().map(|t| t.duration_secs as f32).unwrap_or(0.0);
        let pos = self.ctl.position().as_secs_f32().min(dur.max(0.0));
        let frac = self.seek_drag.unwrap_or(if dur > 0.0 { pos / dur } else { 0.0 });
        let shown = if self.seek_drag.is_some() { frac * dur } else { pos };

        ui.horizontal(|ui| {
            let time_w = 44.0;
            let bar_w = (width - time_w * 2.0 - ui.spacing().item_spacing.x * 2.0).max(60.0);
            ui.add_sized(
                [time_w, 20.0],
                egui::Label::new(RichText::new(fmt_time(shown as u64)).size(12.0).color(p.on_surface_variant)),
            );
            let out = slider_bar(ui, theme, bar_w, frac, dur > 0.0);
            if out.response.dragged() {
                if let Some(v) = out.value {
                    self.seek_drag = Some(v);
                }
            }
            // Перематываем один раз при отпускании (или при клике), а не на каждом кадре.
            if out.response.drag_stopped() || out.response.clicked() {
                let v = out.value.or(self.seek_drag).unwrap_or(frac);
                self.ctl.seek_to(v * dur);
                self.seek_drag = None;
            }
            ui.add_sized(
                [time_w, 20.0],
                egui::Label::new(RichText::new(fmt_time(dur as u64)).size(12.0).color(p.on_surface_variant)),
            );
        });
    }

    fn pb_volume(&mut self, ui: &mut Ui, theme: &Theme) {
        // Layout здесь right_to_left: что добавлено первым, то правее.
        let out = slider_bar(ui, theme, 110.0, self.ctl.volume, true);
        if let Some(v) = out.value {
            self.ctl.set_volume(v);
        }
        let muted = self.ctl.volume <= 0.001;
        let icon = if muted { Icon::VolumeMute } else { Icon::Volume };
        if icon_button(ui, theme, icon, 36.0, ButtonKind::Plain, false, "Громкость").clicked() {
            if muted {
                self.ctl.set_volume(self.volume_before_mute.max(0.2));
            } else {
                self.volume_before_mute = self.ctl.volume;
                self.ctl.set_volume(0.0);
            }
        }
    }
}

//! Левая навигация.

use egui::{Rect, RichText, Sense, Shape, Ui, Vec2, pos2};

use super::widgets::{cover_tile, nav_item, section_label, spring_highlight, spring_highlight_resting};
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

        // Фон выбранного пункта: одна «пилюля», которая скользит между пунктами (как в демо). Место под
        // неё резервируем сейчас (Noop), чтобы она легла ПОД пункты, а нарисуем после, когда известны их позиции.
        // Подсветки от курсора нет: тёмная заливка появляется только пока кнопка мыши зажата (см. nav_item).
        let pill_slot = ui.painter().add(Shape::Noop);
        let mut selected: Option<(usize, Rect)> = None;
        let mut idx = 0usize;

        for (view, icon, label) in [
            (View::Home, Icon::Home, "Главная"),
            (View::Library, Icon::Note, "Моя музыка"),
            (View::NowPlaying, Icon::Queue, "Сейчас играет"),
        ] {
            let is_sel = self.view == view;
            let resp = nav_item(ui, theme, icon, label, is_sel, true, None, compact);
            if is_sel {
                selected = Some((idx, resp.rect));
            }
            if resp.clicked() {
                self.view = view;
            }
            idx += 1;
        }

        ui.add_space(10.0);
        if compact {
            ui.add_space(6.0); // вместо подписи «Сервисы» просто отступ
        } else {
            section_label(ui, theme, "Сервисы");
        }
        // Провайдеры появятся по шагам плана (CLAUDE.md, п. 3). Пока это заглушки.
        // SoundCloud уже открывается (экран-заглушка, см. ui/soundcloud_view.rs), остальные ждут своей очереди.
        let is_sel = self.view == View::SoundCloud;
        let resp = nav_item(ui, theme, Icon::Cloud, "SoundCloud", is_sel, true, Some("бета"), compact);
        if is_sel {
            selected = Some((idx, resp.rect));
        }
        if resp.clicked() {
            self.view = View::SoundCloud;
        }
        idx += 1;
        for name in ["Yandex Music", "VK Music", "Spotify"] {
            nav_item(ui, theme, Icon::Plus, name, false, false, Some("скоро"), compact);
        }

        // «Настройки» стоят под сервисами и подсвечены на любом из своих подэкранов.
        ui.add_space(10.0);
        let is_sel = self.view.is_settings();
        let resp = nav_item(ui, theme, Icon::Settings, "Настройки", is_sel, true, None, compact);
        if is_sel {
            selected = Some((idx, resp.rect));
        }
        if resp.clicked() {
            self.view = View::Settings; // из подэкрана вернёт на список разделов
        }

        // Пилюля: до первого пика та же скорость, что раньше (0.23 с), дальше ещё несколько
        // затухающих колебаний вокруг пункта (spring_highlight).
        if let Some((key, target)) = selected {
            let now = ui.input(|i| i.time);
            let id = ui.scope_id().with("rail_pill");
            let mut a: PillAnim = ui.ctx().data_mut(|d| d.get_temp(id)).unwrap_or(PillAnim { key, from: target.top(), t0: now - 10.0, last_y: target.top() });
            if a.key != key {
                a.key = key;
                a.from = a.last_y; // если нажали на другой пункт на полпути, едем от текущего места
                a.t0 = now;
            }
            let elapsed = (now - a.t0) as f32;
            if !spring_highlight_resting(elapsed) {
                ui.ctx().request_repaint();
            }
            let y = a.from + (target.top() - a.from) * spring_highlight(elapsed);
            a.last_y = y;
            ui.ctx().data_mut(|d| d.insert_temp(id, a));
            let rect = Rect::from_min_size(pos2(target.left(), y), target.size());
            ui.painter().set(pill_slot, Shape::rect_filled(rect, 24.0, theme.selected_fill()));
        }
    }
}

/// Состояние скольжения пилюли выбора.
#[derive(Clone, Copy)]
struct PillAnim {
    key: usize,
    from: f32,
    t0: f64,
    last_y: f32,
}

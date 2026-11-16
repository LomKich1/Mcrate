//! Левая навигация.

use egui::{Rect, RichText, Sense, Shape, Ui, Vec2, pos2};

use super::widgets::{cover_tile, nav_item, section_label, spring_step};
use crate::app::{McrateApp, View};
use crate::icons::Icon;
use crate::theme::Theme;

impl McrateApp {
    pub fn rail_view(&mut self, ui: &mut Ui, theme: &Theme, compact: bool) {
        // В низком окне прячем пока неработающие сервисы-заглушки, чтобы основные разделы
        // и закреплённые настройки помещались без вертикального обрезания.
        let short_height = ui.available_height() < 500.0;
        if short_height {
            ui.spacing_mut().item_spacing.y = 4.0;
        }
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
        if !short_height {
            for name in ["Yandex Music", "VK Music", "Spotify"] {
                nav_item(ui, theme, Icon::Plus, name, false, false, Some("скоро"), compact);
            }
        }

        // «Настройки» закреплены у нижнего края рейки и не смещаются,
        // когда список сервисов становится длиннее.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(8.0);
            let is_sel = self.view.is_settings();
            let resp = nav_item(ui, theme, Icon::Settings, "Настройки", is_sel, true, None, compact);
            if is_sel {
                selected = Some((idx, resp.rect));
            }
            if resp.clicked() {
                self.view = View::Settings; // из подэкрана вернёт на список разделов
            }
        });

        // Физическая пружина совпадает с демо: stiffness=500, damping=34, mass=1.
        // При быстром переключении переносим скорость, поэтому индикатор продолжает
        // движение из текущего состояния и естественно разворачивается к новой цели.
        if let Some((key, target)) = selected {
            let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.064);
            let id = ui.scope_id().with("rail_pill");
            let mut a: PillAnim = ui.ctx().data_mut(|d| d.get_temp(id)).unwrap_or(PillAnim {
                key,
                y: target.top(),
                velocity: 0.0,
                initialized: false,
            });

            if !a.initialized {
                a.key = key;
                a.y = target.top();
                a.velocity = 0.0;
                a.initialized = true;
            } else if a.key != key {
                a.key = key;
            }

            let moving = spring_step(&mut a.y, &mut a.velocity, target.top(), dt);
            if moving {
                ui.ctx().request_repaint();
            } else {
                a.y = target.top();
                a.velocity = 0.0;
            }

            let compact_m3 = compact && !theme.glass_active();
            let indicator_size = if compact_m3 { Vec2::new(56.0, 32.0) } else { target.size() };
            let indicator_left = target.center().x - indicator_size.x * 0.5;
            let indicator_top = if compact_m3 { a.y + (target.height() - indicator_size.y) * 0.5 } else { a.y };
            let rect = Rect::from_min_size(pos2(indicator_left, indicator_top), indicator_size);
            let indicator_radius = if compact_m3 { 16.0 } else { 24.0 };
            ui.painter().set(pill_slot, Shape::rect_filled(rect, indicator_radius, theme.selected_fill()));
            if theme.glass_active() {
                // Тонкая внутренняя кромка даёт выбранному пункту лёгкое стеклянное свечение,
                // не создавая ещё одну стеклянную поверхность поверх рейки.
                ui.painter().rect_stroke(
                    rect,
                    24.0,
                    egui::Stroke::new(1.0, crate::theme::with_alpha(theme.palette.on_surface, if theme.palette.dark { 24 } else { 56 })),
                    egui::StrokeKind::Inside,
                );
            }
            ui.ctx().data_mut(|d| d.insert_temp(id, a));
        }
    }
}

/// Состояние пружинной анимации пилюли выбора.
#[derive(Clone, Copy)]
struct PillAnim {
    key: usize,
    y: f32,
    velocity: f32,
    initialized: bool,
}

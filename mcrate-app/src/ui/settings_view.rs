//! Экран «Оформление»: выбор дизайна, цветов и подробные настройки графики.

use egui::{Color32, CursorIcon, FontId, Rect, RichText, Sense, Ui, pos2, vec2};

use super::widgets::{ButtonKind, pill_button};
use crate::app::McrateApp;
use crate::platform;
use crate::settings::{ACCENT_PRESETS, Design, GraphicsSettings, WindowBackdrop};
use crate::theme::{Theme, with_alpha};

impl McrateApp {
    pub fn settings_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        ui.heading("Оформление и графика");
        ui.add_space(4.0);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // ---------- дизайн ----------
            group_title(ui, "Дизайн");
            let w = ((ui.available_width() - 14.0) / 2.0).min(340.0);
            ui.horizontal(|ui| {
                for (design, name) in [(Design::Material3, "Material 3"), (Design::LiquidGlass, "Liquid Glass")] {
                    if design_card(ui, theme, design, name, self.settings.design == design, w).clicked() {
                        self.settings.design = design;
                    }
                }
            });

            // ---------- цвет ----------
            group_title(ui, "Цвет");
            ui.horizontal(|ui| {
                let dark = self.settings.dark;
                let kind = |on: bool| if on { ButtonKind::Filled } else { ButtonKind::Tonal };
                if pill_button(ui, theme, "Светлая", None, kind(!dark)).clicked() {
                    self.settings.dark = false;
                }
                if pill_button(ui, theme, "Тёмная", None, kind(dark)).clicked() {
                    self.settings.dark = true;
                }
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for seed in ACCENT_PRESETS {
                    let selected = self.settings.accent == seed;
                    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
                    let color = Color32::from_rgb(seed[0], seed[1], seed[2]);
                    ui.painter().circle_filled(rect.center(), 14.0, color);
                    if selected {
                        ui.painter().circle_stroke(rect.center(), 17.0, egui::Stroke::new(2.5, p.on_surface));
                    }
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        self.settings.accent = seed;
                    }
                }
                ui.label("Свой:");
                ui.color_edit_button_srgb(&mut self.settings.accent);
            });
            ui.label(
                RichText::new("Из одного цвета строится вся палитра Material 3 (спецификация 2025).")
                    .size(12.5)
                    .color(p.on_surface_variant),
            );

            // ---------- блюр ----------
            group_title(ui, "Блюр внутри приложения");
            ui.checkbox(&mut self.settings.blur_enabled, "Включить блюр и стекло");
            let glass_design = self.settings.design == Design::LiquidGlass;
            let sliders_on = self.settings.blur_enabled && glass_design;
            ui.add_enabled_ui(sliders_on, |ui| {
                ui.add(egui::Slider::new(&mut self.settings.blur_radius, 2.0..=48.0).text("Радиус блюра"));
                ui.add(egui::Slider::new(&mut self.settings.refraction, 0.0..=14.0).text("Преломление"));
                ui.add(egui::Slider::new(&mut self.settings.glass_opacity, 0.0..=1.0).text("Плотность стекла"));
            });
            let hint = if !glass_design {
                "Блюр и стекло работают только в дизайне Liquid Glass. Material 3 использует плотные поверхности."
            } else if !self.settings.blur_enabled {
                "Блюр выключен: все панели непрозрачные (подходит для слабых видеокарт)."
            } else {
                "Стекло преломляет и размывает фоновые обои. Содержимое соседних панелей оно не размывает: это ограничение библиотеки egui_glass."
            };
            ui.label(RichText::new(hint).size(12.5).color(p.on_surface_variant));

            // ---------- окно ----------
            group_title(ui, "Материал окна (Windows 11)");
            ui.add_enabled_ui(platform::BACKDROP_SUPPORTED, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.settings.window_backdrop, WindowBackdrop::Off, "Выкл");
                    ui.selectable_value(&mut self.settings.window_backdrop, WindowBackdrop::Mica, "Mica");
                    ui.selectable_value(&mut self.settings.window_backdrop, WindowBackdrop::Acrylic, "Acrylic");
                });
            });
            let note = if platform::BACKDROP_SUPPORTED {
                "Нужна Windows 11 сборки 22621+. Материал виден там, где панели не закрашены: в Material 3 это фон под навигацией."
            } else {
                "Недоступно на этой ОС."
            };
            ui.label(RichText::new(note).size(12.5).color(p.on_surface_variant));
            if let Some(err) = &self.backdrop_error {
                ui.label(RichText::new(err).size(12.5).color(p.error));
            }

            ui.add_space(16.0);
            if pill_button(ui, theme, "Сбросить настройки", None, ButtonKind::Tonal).clicked() {
                self.settings = GraphicsSettings::default();
            }
            ui.add_space(8.0);
        });
    }
}

fn group_title(ui: &mut Ui, text: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(text).size(18.0).strong());
    ui.add_space(4.0);
}

/// Карточка выбора дизайна с мини-превью.
fn design_card(ui: &mut Ui, theme: &Theme, design: Design, name: &str, selected: bool, width: f32) -> egui::Response {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 150.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 20.0, p.surface_container_high);
    if selected {
        painter.rect_stroke(rect, 20.0, egui::Stroke::new(2.5, p.primary), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        painter.rect_filled(rect, 20.0, theme.row_hover_fill());
    }

    let pv = Rect::from_min_max(rect.min + vec2(12.0, 12.0), pos2(rect.right() - 12.0, rect.bottom() - 42.0));
    let r = 12.0;
    match design {
        Design::Material3 => {
            painter.rect_filled(pv, r, p.surface);
            painter.rect_filled(Rect::from_min_size(pv.min + vec2(8.0, 10.0), vec2(pv.width() * 0.22, 14.0)), 7.0, p.secondary_container);
            let pane = Rect::from_min_max(pos2(pv.left() + pv.width() * 0.28, pv.top() + 8.0), pos2(pv.right() - 8.0, pv.bottom() - 26.0));
            painter.rect_filled(pane, 10.0, p.surface_container_low);
            let bar = Rect::from_min_max(pos2(pv.left() + 10.0, pv.bottom() - 20.0), pos2(pv.right() - 10.0, pv.bottom() - 6.0));
            painter.rect_filled(bar, 8.0, p.surface_container_highest);
            painter.circle_filled(pos2(bar.center().x, bar.center().y), 5.0, p.primary);
        }
        Design::LiquidGlass => {
            // Цветные пятна «обоев» и полупрозрачные панели поверх.
            let clip = painter.with_clip_rect(pv);
            painter.rect_filled(pv, r, p.surface_container_lowest);
            clip.circle_filled(pv.left_top() + vec2(30.0, 20.0), 40.0, with_alpha(p.primary, 210));
            clip.circle_filled(pv.right_top() + vec2(-20.0, 30.0), 34.0, with_alpha(p.tertiary_container, 255));
            clip.circle_filled(pv.center_bottom() + vec2(10.0, 4.0), 40.0, with_alpha(p.secondary_container, 255));
            let glass = |rr: Rect, rad: f32| {
                painter.rect_filled(rr, rad, Color32::from_white_alpha(if p.dark { 34 } else { 120 }));
                painter.rect_stroke(rr, rad, egui::Stroke::new(1.0, Color32::from_white_alpha(110)), egui::StrokeKind::Inside);
            };
            glass(Rect::from_min_size(pv.min + vec2(8.0, 8.0), vec2(pv.width() * 0.24, pv.height() - 38.0)), 9.0);
            glass(Rect::from_min_max(pos2(pv.left() + pv.width() * 0.32, pv.top() + 8.0), pos2(pv.right() - 8.0, pv.bottom() - 32.0)), 9.0);
            glass(Rect::from_min_max(pos2(pv.left() + 10.0, pv.bottom() - 24.0), pos2(pv.right() - 10.0, pv.bottom() - 8.0)), 8.0);
        }
    }

    painter.text(
        pos2(rect.left() + 16.0, rect.bottom() - 20.0),
        egui::Align2::LEFT_CENTER,
        name,
        FontId::proportional(15.0),
        p.on_surface,
    );
    if selected {
        painter.circle_filled(pos2(rect.right() - 22.0, rect.bottom() - 20.0), 6.0, p.primary);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

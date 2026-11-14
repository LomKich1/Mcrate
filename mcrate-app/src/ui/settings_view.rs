//! Экран «Оформление»: выбор дизайна, цветов и подробные настройки графики.

use egui::{Color32, CursorIcon, FontId, Rect, RichText, Sense, Ui, pos2, vec2};

use super::widgets::{ButtonKind, pill_button};
use crate::app::McrateApp;
use crate::platform;
use crate::settings::{Design, GlassQuality, GraphicsSettings, ThemePreset, WindowBackdrop};
use crate::theme::Palette;
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
            group_title(ui, "Цветовая тема");
            // Карточки тем в несколько рядов: сколько влезает по ширине.
            let card_w = 150.0;
            let gap = ui.spacing().item_spacing.x;
            let per_row = (((ui.available_width() + gap) / (card_w + gap)).floor() as usize).max(1);
            for row in ThemePreset::ALL.chunks(per_row) {
                ui.horizontal(|ui| {
                    for &preset in row {
                        if theme_card(ui, theme, preset, &self.settings, self.settings.theme == preset, card_w).clicked() {
                            self.settings.theme = preset;
                        }
                    }
                });
            }
            if self.settings.theme == ThemePreset::Custom {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let dark = self.settings.dark;
                    let kind = |on: bool| if on { ButtonKind::Filled } else { ButtonKind::Tonal };
                    if pill_button(ui, theme, "Светлая", None, kind(!dark)).clicked() {
                        self.settings.dark = false;
                    }
                    if pill_button(ui, theme, "Тёмная", None, kind(dark)).clicked() {
                        self.settings.dark = true;
                    }
                    ui.label("Цвет:");
                    ui.color_edit_button_srgb(&mut self.settings.accent);
                });
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new("Готовые темы совпадают с приложением расписания. «Свой цвет» строит палитру из одного цвета.")
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
                ui.horizontal(|ui| {
                    ui.label("Качество живого фона:");
                    for q in GlassQuality::ALL {
                        ui.selectable_value(&mut self.settings.glass_quality, q, q.label());
                    }
                });
            });
            let hint = if !glass_design {
                "Блюр и стекло работают только в дизайне Liquid Glass. Material 3 использует плотные поверхности."
            } else if !self.settings.blur_enabled {
                "Блюр выключен: все панели непрозрачные (подходит для слабых видеокарт)."
            } else {
                "Рейка и панель плеера плавают над содержимым и преломляют всё, что под ними, включая текст. Фон рисуется ещё раз за кадр: на слабой видеокарте снизьте качество или выключите блюр."
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
            // Диагностика: без DX12 (DirectComposition) окно не прозрачное, и материал не виден.
            let (diag, ok) = if self.gpu_backend == "Dx12" {
                ("Графический API: DX12 (прозрачность окна доступна).".to_string(), true)
            } else {
                (
                    format!("Графический API: {}. Для Mica/Acrylic нужен DX12 (см. MCRATE_NO_DCOMP в CLAUDE.md).", self.gpu_backend),
                    !platform::BACKDROP_SUPPORTED,
                )
            };
            ui.label(RichText::new(diag).size(12.5).color(if ok { p.on_surface_variant } else { p.error }));

            ui.add_space(16.0);
            if pill_button(ui, theme, "Сбросить настройки", None, ButtonKind::Tonal).clicked() {
                self.settings = GraphicsSettings::default();
            }
            ui.add_space(8.0 + self.bottom_inset); // подушка под плавающую панель плеера (Liquid Glass)
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
            // Пятна «обоев»: (положение в долях превью, радиус, цвет).
            let blobs = [
                (pos2(0.10, 0.18), 40.0, p.primary),
                (pos2(0.90, 0.28), 34.0, p.tertiary_container),
                (pos2(0.55, 1.00), 40.0, p.secondary_container),
            ];
            // Рисует пятна; scale > 1 даёт «линзу»: те же пятна, но крупнее и мягче
            // (так выглядит преломление и блюр, когда стекло лежит поверх картинки).
            let draw_blobs = |pt: &egui::Painter, focus: egui::Pos2, scale: f32, soft: bool| {
                for (f, radius, color) in blobs {
                    let c = pv.min + vec2(pv.width() * f.x, pv.height() * f.y);
                    let c = focus + (c - focus) * scale;
                    if soft {
                        // несколько слоёв с убывающей прозрачностью = размытый край
                        for (k, a) in [(1.35_f32, 40_u8), (1.15, 70), (0.95, 110)] {
                            pt.circle_filled(c, radius * scale * k, with_alpha(color, a));
                        }
                    } else {
                        pt.circle_filled(c, radius, color);
                    }
                }
            };
            let clip = painter.with_clip_rect(pv);
            painter.rect_filled(pv, r, p.surface_container_lowest);
            draw_blobs(&clip, pv.center(), 1.0, false);

            let glass = |rr: Rect, rad: f32| {
                // Внутри панели: фон закрашен заново, поверх увеличенные мягкие пятна («линза»).
                let lens = painter.with_clip_rect(rr.intersect(pv));
                painter.rect_filled(rr, rad, p.surface_container_lowest);
                draw_blobs(&lens, rr.center(), 1.3, true);
                // Плёнка стекла и контур: светлее сверху-слева, темнее снизу-справа.
                painter.rect_filled(rr, rad, Color32::from_white_alpha(if p.dark { 26 } else { 90 }));
                painter.rect_stroke(rr, rad, egui::Stroke::new(1.0, Color32::from_white_alpha(70)), egui::StrokeKind::Inside);
                let spec = Rect::from_min_size(rr.min + vec2(rad * 0.6, 2.0), vec2((rr.width() - rad * 1.2).max(4.0), 2.5));
                painter.rect_filled(spec, 1.25, Color32::from_white_alpha(120)); // блик по верхней кромке
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

/// Карточка цветовой темы: мини-окно в цветах этой темы + название.
/// Цвета берём из палитры самой темы, а не текущей, чтобы все карточки выглядели по-разному.
fn theme_card(ui: &mut Ui, theme: &Theme, preset: ThemePreset, settings: &GraphicsSettings, selected: bool, width: f32) -> egui::Response {
    let cur = &theme.palette;
    let pv = Palette::for_preset(preset, settings.accent, settings.dark);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 92.0), Sense::click());
    let painter = ui.painter();

    painter.rect_filled(rect, 16.0, cur.surface_container_high);
    if resp.hovered() && !selected {
        painter.rect_filled(rect, 16.0, theme.row_hover_fill());
    }
    if selected {
        painter.rect_stroke(rect, 16.0, egui::Stroke::new(2.5, cur.primary), egui::StrokeKind::Inside);
    }

    // Мини-окно: фон, боковая панель, основная область, кружок акцента.
    let win = Rect::from_min_max(rect.min + vec2(8.0, 8.0), pos2(rect.right() - 8.0, rect.bottom() - 28.0));
    painter.rect_filled(win, 10.0, pv.surface);
    let side = Rect::from_min_max(win.min + vec2(5.0, 5.0), pos2(win.left() + win.width() * 0.26, win.bottom() - 5.0));
    painter.rect_filled(side, 6.0, pv.surface_container_high);
    painter.rect_filled(Rect::from_min_size(side.min + vec2(4.0, 5.0), vec2(side.width() - 8.0, 7.0)), 3.5, pv.secondary_container);
    let main = Rect::from_min_max(pos2(side.right() + 5.0, win.top() + 5.0), pos2(win.right() - 5.0, win.bottom() - 5.0));
    painter.rect_filled(main, 6.0, pv.surface_container_low);
    painter.circle_filled(main.left_top() + vec2(10.0, 10.0), 5.0, pv.primary);
    painter.rect_filled(Rect::from_min_size(main.left_top() + vec2(20.0, 7.0), vec2(main.width() * 0.4, 6.0)), 3.0, pv.on_surface_variant);
    painter.rect_filled(Rect::from_min_size(main.left_bottom() + vec2(6.0, -13.0), vec2(main.width() - 12.0, 7.0)), 3.5, pv.primary_container);

    painter.text(
        pos2(rect.left() + 12.0, rect.bottom() - 14.0),
        egui::Align2::LEFT_CENTER,
        preset.label(),
        FontId::proportional(13.5),
        cur.on_surface,
    );
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

//! Экраны настроек: главный список разделов и подэкраны «Оформление», «Окно», «О приложении».

use egui::collapsing_header::CollapsingState;
use egui::{Color32, CornerRadius, CursorIcon, FontId, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::layout::{bleed_clip, centered_column, scroll_end_padding};
use super::widgets::{ButtonKind, error_banner, glass_icon_button, glide_begin, glide_end, pill_button, squish};
use crate::app::{McrateApp, View};
use crate::icons::{self, Icon};
use crate::platform;
use crate::settings::{Design, GlassQuality, GraphicsSettings, ThemePreset, WindowBackdrop};
use crate::theme::Palette;
use crate::theme::{ShapeRole, TextRole, Theme, with_alpha};

impl McrateApp {
    /// Главный экран настроек: список разделов. Каждый раздел это отдельный экран (`View::Settings*`),
    /// переход между ними анимируется так же, как между вкладками меню (ui/transition.rs).
    pub fn settings_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, 760.0, |ui| {
            ui.heading("Настройки");
            ui.add_space(8.0);

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                bleed_clip(ui, self.bleed.below_header());
                let design = match self.settings.design {
                    Design::Material3 => "Material 3",
                    Design::LiquidGlass => "Liquid Glass",
                };
                let backdrop = match self.settings.window_backdrop {
                    WindowBackdrop::Off => "выкл",
                    WindowBackdrop::Mica => "Mica",
                    WindowBackdrop::Acrylic => "Acrylic",
                };
                let rows = [
                    (View::SettingsAppearance, "Оформление", format!("{design}, тема «{}»", self.settings.theme.label())),
                    (View::SettingsWindow, "Окно", format!("Материал окна: {backdrop}")),
                    (View::SettingsAbout, "О приложении", format!("Версия {}", env!("CARGO_PKG_VERSION"))),
                ];
                if let Some(view) = nav_list(ui, theme, &rows, 700.0, true) {
                    self.view = view;
                }

                ui.add_space(12.0);
                let reset_kind = if theme.glass_active() { ButtonKind::Tonal } else { ButtonKind::Outlined };
                if pill_button(ui, theme, "Сбросить все настройки", None, reset_kind).clicked() {
                    self.settings = GraphicsSettings::default();
                }
                scroll_end_padding(ui, self.bottom_inset);
            });
        });
    }

    /// Подэкран «Оформление»: дизайн, параметры стекла, цветовая тема.
    pub fn settings_appearance_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        centered_column(ui, 760.0, |ui| {
            if settings_header(ui, theme, "Оформление") {
                self.view = View::Settings;
            }
            ui.add_space(4.0);

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                bleed_clip(ui, self.bleed.below_header());
                group_title(ui, theme, "Дизайн");
                let w = ((ui.available_width() - 14.0) / 2.0).min(340.0);
                ui.horizontal(|ui| {
                    for (design, name) in [(Design::Material3, "Material 3"), (Design::LiquidGlass, "Liquid Glass")] {
                        if design_card(ui, theme, design, name, self.settings.design == design, w).clicked() {
                            self.settings.design = design;
                        }
                    }
                });

                ui.add_space(10.0);
                if self.settings.design == Design::LiquidGlass {
                    glass_options(ui, theme, &mut self.settings);
                } else {
                    ui.label(
                        RichText::new("Блюр и стекло включаются вместе с дизайном Liquid Glass. Material 3 использует плотные поверхности.")
                            .size(12.5)
                            .color(p.on_surface_variant),
                    );
                }

                group_title(ui, theme, "Цветовая тема");
                theme_menu(ui, theme, &mut self.settings);
                if self.settings.theme == ThemePreset::Custom {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let dark = self.settings.dark;
                        let kind = |on: bool| {
                            if on {
                                ButtonKind::Filled
                            } else if theme.glass_active() {
                                ButtonKind::Tonal
                            } else {
                                ButtonKind::Outlined
                            }
                        };
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

                group_title(ui, theme, "Движение и анимация");
                ui.checkbox(&mut self.settings.reduce_motion, "Уменьшить анимацию");
                ui.label(
                    RichText::new("Отключает непрерывное движение волны и визуализатора, оставляя спокойное статичное состояние.")
                        .size(12.0)
                        .color(p.on_surface_variant),
                );

                group_title(ui, theme, "Предпросмотр палитры");
                let preview = Palette::from_settings(&self.settings, self.settings.design);
                palette_preview(ui, theme, &preview);
                scroll_end_padding(ui, self.bottom_inset);
            });
        });
    }

    /// Подэкран «Окно»: материал окна Windows 11 и диагностика графического API.
    pub fn settings_window_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        centered_column(ui, 760.0, |ui| {
            if settings_header(ui, theme, "Окно") {
                self.view = View::Settings;
            }
            ui.add_space(4.0);

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                bleed_clip(ui, self.bleed.below_header());
                group_title(ui, theme, "Материал окна (Windows 11)");
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
                    error_banner(ui, theme, err);
                }
                let (diag, ok) = if self.gpu_backend == "Dx12" {
                    ("Графический API: DX12 (прозрачность окна доступна).".to_string(), true)
                } else {
                    (
                        format!("Графический API: {}. Для Mica/Acrylic нужен DX12 (см. MCRATE_NO_DCOMP в CLAUDE.md).", self.gpu_backend),
                        !platform::BACKDROP_SUPPORTED,
                    )
                };
                if ok {
                    ui.label(RichText::new(diag).size(12.5).color(p.on_surface_variant));
                } else {
                    error_banner(ui, theme, &diag);
                }
                scroll_end_padding(ui, self.bottom_inset);
            });
        });
    }

    /// Подэкран «О приложении»: версия и сведения об окружении.
    pub fn settings_about_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        centered_column(ui, 760.0, |ui| {
            if settings_header(ui, theme, "О приложении") {
                self.view = View::Settings;
            }
            ui.add_space(4.0);

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                bleed_clip(ui, self.bleed.below_header());
                group_title(ui, theme, "Mcrate");
                ui.label(RichText::new("Музыкальный плеер на Rust и egui.").color(p.on_surface_variant));
                ui.add_space(12.0);
                egui::Grid::new("about_grid").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
                    for (key, value) in [
                        ("Версия", env!("CARGO_PKG_VERSION").to_string()),
                        ("Графический API", self.gpu_backend.clone()),
                        ("Система", std::env::consts::OS.to_string()),
                    ] {
                        ui.label(RichText::new(key).color(p.on_surface_variant));
                        ui.label(value);
                        ui.end_row();
                    }
                });
                scroll_end_padding(ui, self.bottom_inset);
            });
        });
    }

}

/// Компактная M3-проба основных семантических ролей выбранной палитры.
fn palette_preview(ui: &mut Ui, theme: &Theme, palette: &Palette) {
    let samples = [
        ("Primary", palette.primary, palette.on_primary),
        ("Secondary", palette.secondary_container, palette.on_secondary_container),
        ("Tertiary", palette.tertiary_container, palette.on_tertiary_container),
        ("Surface", palette.surface_container_high, palette.on_surface),
        ("Error", palette.error_container, palette.on_error_container),
    ];
    ui.horizontal_wrapped(|ui| {
        for (label, fill, foreground) in samples {
            let (rect, _) = ui.allocate_exact_size(vec2(104.0, 48.0), Sense::hover());
            ui.painter().rect_filled(rect, theme.shape_radius(ShapeRole::Medium), fill);
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(12.0), foreground);
        }
    });
}

/// Шапка подэкрана настроек: круглая кнопка «назад» и заголовок. Возвращает `true`, если нажали «назад».
pub(super) fn settings_header(ui: &mut Ui, theme: &Theme, title: &str) -> bool {
    ui.horizontal(|ui| {
        let clicked = glass_icon_button(ui, theme, Icon::ChevronBack, 40.0, ButtonKind::Plain, false, "Назад").clicked();
        ui.heading(title);
        clicked
    })
    .inner
}

/// Список строк-переходов (главный экран настроек и экран «Библиотека»). Возвращает вид, на который нажали.
/// Material 3: все строки лежат в одном тональном блоке со скруглением MENU_CORNER, как список в меню тем
/// (`theme_menu`): без зазоров между строками, у строки при наведении своя скруглённая подсветка, шеврона нет.
/// Liquid Glass: отдельные строки, как раньше. `max_width` ограничивает ширину (700 в настройках).
pub(super) fn nav_list(ui: &mut Ui, theme: &Theme, rows: &[(View, &str, String)], max_width: f32, chevron: bool) -> Option<View> {
    let width = ui.available_width().min(max_width);
    let mut clicked = None;
    if theme.glass_active() {
        for (view, title, subtitle) in rows {
            if glass_nav_row(ui, theme, title, subtitle, width, chevron).clicked() {
                clicked = Some(*view);
            }
            ui.add_space(8.0);
        }
        return clicked;
    }
    ui.scope(|ui| {
        // Без вертикального зазора: строки идут встык внутри блока.
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::Frame::new()
            .fill(theme.tonal_fill())
            .corner_radius(CornerRadius::same(MENU_CORNER))
            // Без внутреннего отступа: строки и подсветка занимают блок целиком, без зазоров по краям.
            .inner_margin(Margin::ZERO)
            .show(ui, |ui| {
                ui.set_width(width);
                // Слой под подсветку резервируем до строк, чтобы текст рисовался поверх неё.
                let glide_slot = glide_begin(ui);
                let mut hovered = None;
                for (i, (view, title, subtitle)) in rows.iter().enumerate() {
                    let resp = m3_nav_row(ui, theme, title, subtitle);
                    if resp.hovered() {
                        hovered = Some((i as u64, resp.rect));
                    }
                    if resp.clicked() {
                        clicked = Some(*view);
                    }
                }
                // Та же скользящая подсветка, что в списке треков (ui/widgets.rs). Радиус равен скруглению
                // блока: у первой и последней строки внешние углы подсветки совпадают с углами блока.
                glide_end(ui, theme, glide_slot, ui.scope_id().with("glide_nav_list"), hovered, MENU_CORNER as f32);
            });
    });
    clicked
}

/// Строка внутри тонального блока Material 3: название и подпись. Подсветку рисует `glide_end` в `nav_list`.
fn m3_nav_row(ui: &mut Ui, theme: &Theme, title: &str, subtitle: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), NAV_ROW_H_M3), Sense::click());
    // Продавливание при нажатии, те же параметры, что у строки трека (`track_row`).
    let rect = squish(ui, resp.id, &resp, rect, 0.015, 0.07);
    let painter = ui.painter();
    let fg = theme.on_tonal();
    // 20 px от края, как у текста кнопки выбора темы.
    let x = rect.left() + 20.0;
    painter.text(pos2(x, rect.center().y - 10.0), egui::Align2::LEFT_CENTER, title, FontId::proportional(15.0), fg);
    painter.text(
        pos2(x, rect.center().y + 12.0),
        egui::Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(theme.text_size(TextRole::BodySmall)),
        with_alpha(fg, 190),
    );
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Отдельная строка Liquid Glass (прежний вид): название, подпись, при желании шеврон «>».
fn glass_nav_row(ui: &mut Ui, theme: &Theme, title: &str, subtitle: &str, width: f32, chevron: bool) -> egui::Response {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 64.0), Sense::click());
    let painter = ui.painter();
    if resp.hovered() || resp.is_pointer_button_down_on() || resp.has_focus() {
        let style = crate::theme::glass::control_style(theme, false, 14.0);
        egui_glass::paint_glass(ui, rect, &style);
        painter.rect_stroke(rect, 14.0, Stroke::new(0.8, with_alpha(p.on_surface, 42)), egui::StrokeKind::Inside);
    }
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(0.7, with_alpha(p.on_surface, 22)));
    painter.text(pos2(rect.left() + 18.0, rect.center().y - 10.0), egui::Align2::LEFT_CENTER, title, FontId::proportional(16.0), p.on_surface);
    painter.text(pos2(rect.left() + 18.0, rect.center().y + 12.0), egui::Align2::LEFT_CENTER, subtitle, FontId::proportional(12.5), p.on_surface_variant);
    if chevron {
        icons::paint(theme, painter, Icon::ChevronForward, pos2(rect.right() - 26.0, rect.center().y), 20.0, p.on_surface_variant);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Раскрывающийся блок «Параметры стекла»: строка-заголовок и под ней настройки.
/// Раскрытие двигает нижние элементы (высота блока плавно растёт), а не рисуется поверх них.
fn glass_options(ui: &mut Ui, theme: &Theme, s: &mut GraphicsSettings) {
    let p = &theme.palette;
    let id = ui.make_persistent_id("glass_options");
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let openness = state.openness(ui.ctx());
    if expander_header(ui, theme, "Параметры стекла", "Блюр, преломление, плотность, качество", openness).clicked() {
        state.toggle(ui);
    }
    state.show_body_unindented(ui, |ui| {
        ui.add_space(6.0);
        let options_frame = if theme.glass_active() {
            egui::Frame::new()
                .fill(with_alpha(p.surface_container_low, if p.dark { 82 } else { 96 }))
                .stroke(Stroke::new(0.8, with_alpha(p.on_surface, 28)))
                .corner_radius(16)
                .inner_margin(egui::Margin::same(14))
        } else {
            egui::Frame::new()
                .fill(p.surface_container_low)
                .corner_radius(16)
                .inner_margin(egui::Margin::same(14))
        };
        options_frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Slider::new(&mut s.blur_radius, 2.0..=48.0).text("Радиус блюра"));
                ui.add(egui::Slider::new(&mut s.refraction, 0.0..=14.0).text("Преломление"));
                ui.add(egui::Slider::new(&mut s.glass_opacity, 0.0..=1.0).text("Плотность стекла"));
                ui.horizontal(|ui| {
                    ui.label("Качество живого фона:");
                    for q in GlassQuality::ALL {
                        ui.selectable_value(&mut s.glass_quality, q, q.label());
                    }
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Стекло используется в навигации и панели плеера. Списки и текст остаются на плотной поверхности для читаемости; плеер преломляет содержимое под собой. \
                         Фон рисуется ещё раз за кадр: на слабой видеокарте снизьте качество или выберите Material 3.",
                    )
                    .size(12.5)
                    .color(p.on_surface_variant),
                );
            });
    });
}

/// Строка-заголовок раскрывающегося блока: название, подпись и шеврон, который
/// поворачивается вместе с раскрытием (`openness`: 0 = закрыто, 1 = открыто).
fn expander_header(ui: &mut Ui, theme: &Theme, title: &str, subtitle: &str, openness: f32) -> egui::Response {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(700.0), 60.0), Sense::click());
    let painter = ui.painter();
    if theme.glass_active() {
        if resp.hovered() || resp.is_pointer_button_down_on() || openness > 0.01 || resp.has_focus() {
            let style = crate::theme::glass::control_style(theme, false, 14.0);
            egui_glass::paint_glass(ui, rect, &style);
            painter.rect_stroke(rect, 14.0, Stroke::new(0.8, with_alpha(p.on_surface, 38)), egui::StrokeKind::Inside);
        }
        painter.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(0.7, with_alpha(p.on_surface, 22)));
    } else {
        painter.rect_filled(rect, 16.0, p.surface_container_high);
        if resp.hovered() {
            painter.rect_filled(rect, 16.0, theme.row_hover_fill());
        }
    }
    painter.text(
        pos2(rect.left() + 16.0, rect.center().y - 9.0),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(15.0),
        p.on_surface,
    );
    painter.text(
        pos2(rect.left() + 16.0, rect.center().y + 11.0),
        egui::Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(12.5),
        p.on_surface_variant,
    );
    // Шеврон «>» поворачиваем на 90° по часовой, получается «v».
    let (sin, cos) = (openness * std::f32::consts::FRAC_PI_2).sin_cos();
    let rot = |v: Vec2| vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
    let c = pos2(rect.right() - 24.0, rect.center().y);
    let pts: Vec<egui::Pos2> = [vec2(-3.0, -6.0), vec2(3.0, 0.0), vec2(-3.0, 6.0)].iter().map(|&v| c + rot(v)).collect();
    painter.add(egui::Shape::line(pts, Stroke::new(2.0, p.on_surface_variant)));
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

const MENU_CORNER: u8 = 24; // скругление кнопки и списка, как MENU_CORNER в sheduleappreload
const MENU_ROW_H: f32 = 48.0;
const MENU_BUTTON_H: f32 = 48.0;
const MENU_MAX_H: f32 = 288.0; // выше этого список прокручивается
const MENU_MAX_W: f32 = 460.0;
const NAV_ROW_H_M3: f32 = 64.0; // высота строки-перехода (Библиотека, Настройки) в Material 3

/// Выбор цветовой темы: кнопка на всю ширину блока с названием текущей темы и «приклеенным»
/// к ней списком (по образцу ThemeMenuButton из sheduleappreload/SettingsScreen.kt).
/// Отличие от образца: там список это Popup (новый слой поверх экрана), а здесь он раскрывается
/// прямо в потоке экрана: высота блока плавно растёт, и всё, что ниже, сдвигается вниз.
/// Пока список открыт, нижние углы кнопки плавно становятся прямыми, а стрелка поворачивается.
fn theme_menu(ui: &mut Ui, theme: &Theme, s: &mut GraphicsSettings) {
    // Внутри scope убираем вертикальный зазор, чтобы список прилегал к кнопке без щели.
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let id = ui.make_persistent_id("theme_menu");
        let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, false);
        let openness = state.openness(ui.ctx());
        let width = ui.available_width().min(MENU_MAX_W);

        if theme_menu_button(ui, theme, s.theme.label(), openness, width).clicked() {
            state.toggle(ui);
        }

        let mut picked = None;
        state.show_body_unindented(ui, |ui| {
            // Прямые верхние углы (стык с кнопкой) и скруглённые нижние: вместе одна форма.
            // Отступ 8 px слева, справа и снизу нужен, чтобы подсветка строки не торчала за скруглением.
            let menu_frame = if theme.glass_active() {
                egui::Frame::new()
                    .fill(with_alpha(theme.palette.surface_container_high, if theme.palette.dark { 104 } else { 122 }))
                    .stroke(Stroke::new(0.8, with_alpha(theme.palette.on_surface, 42)))
                    .corner_radius(CornerRadius { nw: 0, ne: 0, sw: MENU_CORNER, se: MENU_CORNER })
                    .inner_margin(Margin { left: 8, right: 8, top: 0, bottom: 8 })
            } else {
                egui::Frame::new()
                    .fill(theme.tonal_fill())
                    .corner_radius(CornerRadius { nw: 0, ne: 0, sw: MENU_CORNER, se: MENU_CORNER })
                    .inner_margin(Margin { left: 8, right: 8, top: 0, bottom: 8 })
            };
            menu_frame.show(ui, |ui| {
                    ui.set_width(width - 16.0);
                    egui::ScrollArea::vertical()
                        .max_height(MENU_MAX_H)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for &preset in ThemePreset::ALL.iter() {
                                if theme_menu_row(ui, theme, s, preset, s.theme == preset) {
                                    picked = Some(preset);
                                }
                            }
                        });
                });
        });
        // Выбор применяем после показа списка (внутри замыкания `state` занят) и сворачиваем список.
        if let Some(preset) = picked {
            s.theme = preset;
            state.set_open(false);
            state.store(ui.ctx());
        }
    });
}

/// Кнопка меню тем: название слева, стрелка справа (▼ закрыто, ▲ открыто).
fn theme_menu_button(ui: &mut Ui, theme: &Theme, label: &str, openness: f32, width: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, MENU_BUTTON_H), Sense::click());
    let painter = ui.painter();
    let bottom = (MENU_CORNER as f32 * (1.0 - openness)).round() as u8;
    let radius = CornerRadius { nw: MENU_CORNER, ne: MENU_CORNER, sw: bottom, se: bottom };
    let fg = if theme.glass_active() { theme.palette.on_surface } else { theme.on_tonal() };
    if theme.glass_active() {
        painter.rect_filled(rect, radius, with_alpha(theme.palette.surface_container_high, if resp.hovered() { 118 } else { 86 }));
        painter.rect_stroke(rect, radius, Stroke::new(0.8, with_alpha(theme.palette.on_surface, if openness > 0.01 { 66 } else { 38 })), egui::StrokeKind::Inside);
    } else {
        painter.rect_filled(rect, radius, theme.tonal_fill());
        if resp.hovered() {
            painter.rect_filled(rect, radius, theme.row_hover_fill());
        }
    }
    painter.text(pos2(rect.left() + 20.0, rect.center().y), egui::Align2::LEFT_CENTER, label, FontId::proportional(15.0), fg);

    // Плавный разворот оставляем геометрическим — иконка остаётся чёткой на каждом кадре.
    let (sin, cos) = (openness * std::f32::consts::PI).sin_cos();
    let rot = |v: Vec2| vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
    let c = pos2(rect.right() - 26.0, rect.center().y);
    let pts: Vec<egui::Pos2> = [vec2(-5.0, -2.5), vec2(5.0, -2.5), vec2(0.0, 3.0)].iter().map(|&v| c + rot(v)).collect();
    painter.add(egui::Shape::convex_polygon(pts, fg, Stroke::NONE));
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Строка списка: цветная точка темы, название, галочка у выбранной. Возвращает `true` по клику.
fn theme_menu_row(ui: &mut Ui, theme: &Theme, s: &GraphicsSettings, preset: ThemePreset, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), MENU_ROW_H), Sense::click());
    let painter = ui.painter();
    let row_radius = if theme.glass_active() { 12.0 } else { theme.shape_radius(ShapeRole::Small) as f32 };
    let fg = if theme.glass_active() { theme.palette.on_surface } else { theme.on_tonal() };
    if theme.glass_active() {
        if selected {
            painter.rect_filled(rect, row_radius, with_alpha(theme.palette.primary, 34));
        } else if resp.hovered() {
            painter.rect_filled(rect, row_radius, with_alpha(theme.palette.on_surface, 18));
        }
    } else if selected {
        painter.rect_filled(rect, row_radius, with_alpha(fg, 26));
    } else if resp.hovered() {
        painter.rect_filled(rect, row_radius, theme.row_hover_fill());
    }
    // Точка показывает главный цвет самой темы (а не текущей), так темы легко различать.
    let swatch_settings = GraphicsSettings { theme: preset, ..s.clone() };
    let swatch = Palette::from_settings(&swatch_settings, theme.design()).primary;
    let dot = pos2(rect.left() + 20.0, rect.center().y);
    painter.circle_filled(dot, 8.0, swatch);
    painter.circle_stroke(dot, 8.0, Stroke::new(1.0, with_alpha(fg, 70)));
    let label_size = if theme.glass_active() { 15.0 } else { theme.text_size(TextRole::LabelLarge) };
    painter.text(pos2(rect.left() + 40.0, rect.center().y), egui::Align2::LEFT_CENTER, preset.label(), FontId::proportional(label_size), fg);
    if selected {
        icons::paint(theme, painter, Icon::Checkmark, pos2(rect.right() - 20.0, rect.center().y), 18.0, fg);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn group_title(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add_space(14.0);
    let size = if theme.glass_active() { 18.0 } else { theme.text_size(TextRole::TitleLarge) };
    ui.label(RichText::new(text).size(size).strong());
    ui.add_space(4.0);
}

/// Карточка выбора дизайна с мини-превью.
fn design_card(ui: &mut Ui, theme: &Theme, design: Design, name: &str, selected: bool, width: f32) -> egui::Response {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 150.0), Sense::click());
    let painter = ui.painter();
    let shape = if theme.glass_active() { 18.0 } else { theme.shape_radius(ShapeRole::Large) as f32 };
    if theme.glass_active() {
        if resp.hovered() {
            painter.rect_filled(rect, shape, with_alpha(p.on_surface, 14));
        }
        painter.rect_stroke(
            rect,
            shape,
            Stroke::new(if selected { 1.4 } else { 0.8 }, if selected { with_alpha(p.primary, 220) } else { with_alpha(p.on_surface, 36) }),
            egui::StrokeKind::Inside,
        );
    } else {
        painter.rect_filled(rect, shape, p.surface_container_high);
        if selected {
            painter.rect_stroke(rect, shape, egui::Stroke::new(2.5, p.primary), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            painter.rect_filled(rect, shape, theme.row_hover_fill());
        }
    }

    let pv = Rect::from_min_max(rect.min + vec2(12.0, 12.0), pos2(rect.right() - 12.0, rect.bottom() - 42.0));
    let r = 12.0;
    match design {
        Design::Material3 => {
            let extra_small = theme.shape_radius(ShapeRole::ExtraSmall) as f32;
            let small = theme.shape_radius(ShapeRole::Small) as f32;
            let medium = theme.shape_radius(ShapeRole::Medium) as f32;
            painter.rect_filled(pv, medium, p.surface);
            painter.rect_filled(Rect::from_min_size(pv.min + vec2(8.0, 10.0), vec2(pv.width() * 0.22, 14.0)), extra_small, p.secondary_container);
            let pane = Rect::from_min_max(pos2(pv.left() + pv.width() * 0.28, pv.top() + 8.0), pos2(pv.right() - 8.0, pv.bottom() - 26.0));
            painter.rect_filled(pane, small, p.surface_container_low);
            let bar = Rect::from_min_max(pos2(pv.left() + 10.0, pv.bottom() - 20.0), pos2(pv.right() - 10.0, pv.bottom() - 6.0));
            painter.rect_filled(bar, extra_small, p.surface_container_highest);
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

//! Левая навигация.

use egui::{Align, Layout, Rect, Response, RichText, Sense, Shape, Ui, Vec2, layers::ShapeIdx, pos2, vec2};

use super::anim::spring_step;
use super::widgets::{ButtonKind, NAV_PRESS, NavItem, cover_tile, icon_button, nav_item, section_label, squish};
use crate::app::{McrateApp, View};
use crate::icons::Icon;
use crate::theme::Theme;

impl McrateApp {
    pub(crate) fn rail_view(&mut self, ui: &mut Ui, theme: &Theme, compact: bool) {
        // В низком окне прячем пока неработающие сервисы-заглушки, чтобы основные разделы
        // и закреплённые настройки помещались без вертикального обрезания.
        let short_height = ui.available_height() < 500.0;
        if short_height {
            ui.spacing_mut().item_spacing.y = 4.0;
        }
        ui.add_space(4.0);
        // «Сейчас играет» не отдельный раздел: пока он открыт, подсвечен пункт, с которого его открыли.
        let nav_view = if self.view == View::NowPlaying { self.view_before_np } else { self.view };
        let settings_active = nav_view.is_settings();
        if rail_header(ui, theme, compact, settings_active) {
            self.view = View::Settings; // из подэкрана вернёт на список разделов
        }
        ui.add_space(14.0);

        // Фон выбранного пункта: одна «пилюля», которая скользит между пунктами (как в демо). Место под
        // неё резервируем сейчас (Noop), чтобы она легла ПОД пункты, а нарисуем после, когда известны их позиции.
        // Подсветки от курсора нет: тёмная заливка появляется только пока кнопка мыши зажата (см. nav_item).
        let pill_slot = ui.painter().add(Shape::Noop);
        // Храним весь ответ пункта (а не только прямоугольник): нужны id и нажатие для сжатия пилюли.
        let mut selected: Option<(usize, Response)> = None;
        let mut idx = 0usize;

        for (view, icon, label) in [(View::Home, Icon::Home, "Главная"), (View::Collection, Icon::Library, "Библиотека")] {
            // «Библиотека» подсвечена и на её подэкранах (в том числе «Ваши загрузки»).
            let is_sel = if view == View::Collection { nav_view.is_collection() } else { nav_view == view };
            let resp = nav_item(ui, theme, &NavItem { icon, label, selected: is_sel, enabled: true, trailing: None }, compact);
            if is_sel {
                selected = Some((idx, resp.clone()));
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
        // SoundCloud уже открывается (экран-заглушка, см. ui/soundcloud.rs), остальные ждут своей очереди.
        let is_sel = nav_view == View::SoundCloud;
        let soundcloud = NavItem { icon: Icon::Cloud, label: "SoundCloud", selected: is_sel, enabled: true, trailing: Some("бета") };
        let resp = nav_item(ui, theme, &soundcloud, compact);
        if is_sel {
            selected = Some((idx, resp.clone()));
        }
        if resp.clicked() {
            self.view = View::SoundCloud;
        }
        if !short_height {
            for name in ["Yandex Music", "VK Music", "Spotify"] {
                let stub = NavItem { icon: Icon::Plus, label: name, selected: false, enabled: false, trailing: Some("скоро") };
                nav_item(ui, theme, &stub, compact);
            }
        }

        paint_selection_pill(ui, theme, compact, pill_slot, selected);
    }
}

/// Верх рейки: кнопка настроек (шестерёнка) стоит рядом со значком приложения. Возвращает true, если её нажали.
fn rail_header(ui: &mut Ui, theme: &Theme, compact: bool, settings_active: bool) -> bool {
    let mut open_settings = false;
    if compact {
        // Узкая панель: шестерёнка, под ней значок приложения по центру.
        ui.vertical_centered(|ui| {
            open_settings = icon_button(ui, theme, Icon::Settings, 40.0, ButtonKind::Plain, settings_active, "Настройки").clicked();
            ui.add_space(4.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
            cover_tile(ui, theme, rect, "mcrate", 13.0);
        });
    } else {
        let row_w = ui.available_width();
        ui.allocate_ui_with_layout(vec2(row_w, 44.0), Layout::left_to_right(Align::Center), |ui| {
            ui.add_space(2.0);
            open_settings = icon_button(ui, theme, Icon::Settings, 40.0, ButtonKind::Plain, settings_active, "Настройки").clicked();
            ui.add_space(2.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
            cover_tile(ui, theme, rect, "mcrate", 12.0);
            ui.label(RichText::new("Mcrate").size(22.0).strong());
        });
    }
    open_settings
}

/// Рисует пилюлю выбранного пункта в слоте `pill_slot` (он зарезервирован под пунктами) и двигает её пружиной.
/// Пока выбранного пункта нет (открыты настройки), состояние пилюли сбрасывается.
fn paint_selection_pill(ui: &mut Ui, theme: &Theme, compact: bool, pill_slot: ShapeIdx, selected: Option<(usize, Response)>) {
    // Физическая пружина совпадает с демо: stiffness=500, damping=34, mass=1.
    // При быстром переключении переносим скорость, поэтому индикатор продолжает
    // движение из текущего состояния и естественно разворачивается к новой цели.
    let id = ui.scope_id().with("rail_pill");
    if let Some((key, sel)) = selected {
        let target = sel.rect;
        let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.064);
        let mut a: PillAnim =
            ui.ctx().data_mut(|d| d.get_temp(id)).unwrap_or(PillAnim { key, y: target.top(), velocity: 0.0, initialized: false });

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

        let glass = theme.glass_active();
        let compact_m3 = compact && !glass;
        let indicator_size = if compact_m3 { Vec2::new(56.0, 32.0) } else { target.size() };
        let indicator_left = target.center().x - indicator_size.x * 0.5;
        let indicator_top = if compact_m3 { a.y + (target.height() - indicator_size.y) * 0.5 } else { a.y };
        let rect = Rect::from_min_size(pos2(indicator_left, indicator_top), indicator_size);
        let indicator_radius = if compact_m3 { 16.0 } else { 24.0 };

        if glass {
            // Liquid Glass: пилюля — настоящее стекло (эффекты мини-плеера, без заливки).
            // Кладём в слот ПОД пункты, чтобы иконка и текст оставались сверху.
            // Сжатие: при нажатии на выбранный пункт пилюля уменьшается тем же приёмом, что и кнопки.
            let shrink = squish(ui, sel.id, &sel, target, NAV_PRESS, NAV_PRESS);
            let kx = shrink.width() / target.width();
            let ky = shrink.height() / target.height();
            let pressed_rect = Rect::from_center_size(rect.center(), vec2(rect.width() * kx, rect.height() * ky));
            let style = crate::theme::glass::selection_style(theme, indicator_radius);
            ui.painter().set(pill_slot, egui_glass::glass_shape(ui, pressed_rect, &style));
        } else {
            ui.painter().set(pill_slot, Shape::rect_filled(rect, indicator_radius, theme.selected_fill()));
        }
        ui.ctx().data_mut(|d| d.insert_temp(id, a));
    } else {
        // Выбранного пункта нет (открыты настройки): забываем положение пилюли. Иначе при возврате
        // на другой раздел она поехала бы от того пункта, что был выбран до настроек.
        ui.ctx().data_mut(|d| d.remove::<PillAnim>(id));
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

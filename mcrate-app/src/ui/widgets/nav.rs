//! Пункт левой навигации и заголовок группы.

use super::glide::squish;
use crate::icons::{self, Icon};
use crate::settings::Design;
use crate::theme::{TextRole, Theme, with_alpha};
use crate::ui::ripple;
use egui::{Align2, Color32, CursorIcon, FontId, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

/// Интерполяция sRGBA для плавного изменения цвета текста навигации.
fn mix_color(from: Color32, to: Color32, t: f32) -> Color32 {
    let a = from.to_array();
    let b = to.to_array();
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
        (a[3] as f32 + (b[3] as f32 - a[3] as f32) * t).round() as u8,
    )
}

/// Доля вдавливания пункта навигации при нажатии. Общая для пункта и пилюли выбора (rail.rs), чтобы они сжимались синхронно.
pub(in crate::ui) const NAV_PRESS: f32 = 0.06;

/// Описание пункта навигации.
#[derive(Clone, Copy)]
pub(crate) struct NavItem<'a> {
    pub icon: Icon,
    pub label: &'a str,
    pub selected: bool,
    /// Недоступный пункт бледный и не нажимается.
    pub enabled: bool,
    /// Мелкая подпись у правого края («бета», «скоро»).
    pub trailing: Option<&'a str>,
}

/// Пункт навигации (ширина = вся доступная, высота 48, форма «таблетка»).
/// `compact`: только иконка по центру (узкая панель), подпись показывается во всплывающей подсказке.
pub(crate) fn nav_item(ui: &mut Ui, theme: &Theme, item: &NavItem, compact: bool) -> Response {
    let NavItem { icon, label, selected, enabled, trailing } = *item;
    let p = &theme.palette;
    let size = vec2(ui.available_width(), 48.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let id = resp.id;
    // Вдавливание строго равномерное (одинаково по ширине и высоте): разные коэффициенты давали ощущение наклона.
    let rect = if enabled { squish(ui, id, &resp, rect, NAV_PRESS, NAV_PRESS) } else { rect };
    let painter = ui.painter();

    // Выбранную капсулу рисует rail.rs. Для остальных пунктов Liquid Glass добавляет
    // лишь лёгкий hover-glow, чтобы не наслаивать вторую стеклянную поверхность.
    if theme.glass_active() && enabled && !selected {
        let t = ui.ctx().animate_bool_with_time(id.with("glass_hover"), resp.hovered() || resp.is_pointer_button_down_on(), 0.16);
        if t > 0.01 {
            painter.rect_filled(
                rect.shrink(1.0),
                22.0,
                with_alpha(p.on_surface, (t * if resp.is_pointer_button_down_on() { 32.0 } else { 20.0 }) as u8),
            );
        }
    } else if enabled && !selected {
        // У выбранного пункта слой не рисуем: он уже подсвечен пилюлей (rail.rs).
        // M3: кружок от курсора при наведении, при нажатии разрастается до краёв пункта.
        ripple::state_layer(ui, theme, painter, &resp, rect, 24.0);
    }
    if resp.has_focus() {
        painter.rect_stroke(
            rect.shrink(2.0),
            22.0,
            Stroke::new(if theme.glass_active() { 1.5 } else { 2.0 }, p.primary),
            StrokeKind::Inside,
        );
    }

    let fg = if !enabled {
        with_alpha(p.on_surface_variant, 120)
    } else {
        // Как в CSS-примере: активный и наведённый пункты плавно переходят
        // от приглушённого текста к основному цвету за 0.3 секунды.
        let selected_fg = match theme.design() {
            Design::Material3 => theme.on_tonal(),
            Design::LiquidGlass => p.on_surface,
        };
        let emphasized = selected || resp.hovered();
        let emphasized_fg = if selected { selected_fg } else { p.on_surface };
        let t = ui.ctx().animate_bool_with_time(id.with("emphasis"), emphasized, 0.3);
        mix_color(p.on_surface_variant, emphasized_fg, t)
    };

    // Фон выбранного пункта рисует rail.rs: пилюля, которая плавно скользит между пунктами.
    let glyph_scale = if enabled { (rect.width() / size.x).clamp(0.8, 1.0) } else { 1.0 };
    if compact {
        icons::paint(theme, painter, icon, rect.center(), 24.0 * glyph_scale, fg);
    } else {
        let icon_center = rect.center() + vec2((28.0 - size.x * 0.5) * glyph_scale, 0.0);
        icons::paint(theme, painter, icon, icon_center, 24.0 * glyph_scale, fg);
        let label_size = if theme.glass_active() { 15.0 } else { theme.text_size(TextRole::LabelLarge) };
        painter.text(
            pos2(rect.left() + 52.0 * glyph_scale, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(label_size * glyph_scale),
            fg,
        );
        if let Some(t) = trailing {
            painter.text(
                pos2(rect.right() - 14.0 * glyph_scale, rect.center().y),
                Align2::RIGHT_CENTER,
                t,
                FontId::proportional(11.0 * glyph_scale),
                fg,
            );
        }
    }
    let resp = if compact { resp.on_hover_text(label) } else { resp };
    let resp = if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp };
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, selected, label));
    resp
}

/// Маленький заголовок группы (например «Сервисы»).
pub(crate) fn section_label(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(egui::RichText::new(text).size(12.5).color(theme.palette.on_surface_variant));
    });
}

//! Общие виджеты. Рисуются вручную (painter), цвета берут из темы,
//! поэтому выглядят правильно и в Material 3, и в Liquid Glass.

use egui::{Align2, CursorIcon, FontId, Rect, Response, Sense, Ui, Vec2, pos2, vec2};
use mcrate_core::Track;

use crate::icons::{self, Icon};
use crate::settings::Design;
use crate::theme::{Theme, with_alpha};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Plain,  // только иконка
    Tonal,  // приглушённая заливка
    Filled, // главный акцент (кнопка «играть»)
}

/// «Слой состояния» из M3: при наведении поверх кладётся 8% цвета контента, при нажатии 12%.
fn state_alpha(r: &Response) -> u8 {
    if r.is_pointer_button_down_on() {
        30
    } else if r.hovered() {
        20
    } else {
        0
    }
}

/// Круглая кнопка с иконкой.
pub fn icon_button(ui: &mut Ui, theme: &Theme, icon: Icon, diameter: f32, kind: ButtonKind, active: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    let p = &theme.palette;
    let (bg, fg) = match kind {
        ButtonKind::Filled => (Some(p.primary), p.on_primary),
        ButtonKind::Tonal => (Some(theme.tonal_fill()), theme.on_tonal()),
        ButtonKind::Plain => (None, if active { p.primary } else { p.on_surface }),
    };
    let painter = ui.painter();
    let r = diameter / 2.0;
    if let Some(bg) = bg {
        painter.circle_filled(rect.center(), r, bg);
    }
    let a = state_alpha(&resp);
    if a > 0 {
        painter.circle_filled(rect.center(), r, with_alpha(fg, a));
    }
    icons::paint(painter, icon, rect.center(), diameter * 0.5, fg);
    resp.on_hover_text(tip).on_hover_cursor(CursorIcon::PointingHand)
}

/// Кнопка-«таблетка» с текстом и необязательной иконкой.
pub fn pill_button(ui: &mut Ui, theme: &Theme, label: &str, icon: Option<Icon>, kind: ButtonKind) -> Response {
    let p = &theme.palette;
    let (bg, fg) = match kind {
        ButtonKind::Filled => (p.primary, p.on_primary),
        _ => (theme.tonal_fill(), theme.on_tonal()),
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(14.0), fg);
    let icon_w = if icon.is_some() { 20.0 + 8.0 } else { 0.0 };
    let size = vec2(galley.size().x + icon_w + 36.0, 40.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 20.0, bg);
    let a = state_alpha(&resp);
    if a > 0 {
        painter.rect_filled(rect, 20.0, with_alpha(fg, a));
    }
    let mut x = rect.left() + 18.0;
    if let Some(icon) = icon {
        icons::paint(painter, icon, pos2(x + 10.0, rect.center().y), 18.0, fg);
        x += icon_w;
    }
    painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Пункт навигации (ширина = вся доступная, высота 48, форма «таблетка»).
/// `compact`: только иконка по центру (узкая панель), подпись показывается во всплывающей подсказке.
pub fn nav_item(ui: &mut Ui, theme: &Theme, icon: Icon, label: &str, selected: bool, enabled: bool, trailing: Option<&str>, compact: bool) -> Response {
    let p = &theme.palette;
    let size = vec2(ui.available_width(), 48.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter();

    let fg = if !enabled {
        with_alpha(p.on_surface_variant, 120)
    } else if selected {
        match theme.design() {
            Design::Material3 => theme.on_tonal(),
            Design::LiquidGlass => p.on_surface,
        }
    } else {
        p.on_surface_variant
    };

    if selected {
        painter.rect_filled(rect, 24.0, theme.selected_fill());
    } else if enabled && resp.hovered() {
        painter.rect_filled(rect, 24.0, theme.row_hover_fill());
    }
    if compact {
        icons::paint(painter, icon, rect.center(), 22.0, fg);
    } else {
        icons::paint(painter, icon, pos2(rect.left() + 28.0, rect.center().y), 22.0, fg);
        painter.text(pos2(rect.left() + 52.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(15.0), fg);
        if let Some(t) = trailing {
            painter.text(pos2(rect.right() - 14.0, rect.center().y), Align2::RIGHT_CENTER, t, FontId::proportional(11.0), fg);
        }
    }
    let resp = if compact { resp.on_hover_text(label) } else { resp };
    if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

/// Результат ползунка: `value` = Some(доля 0..1), пока пользователь тянет или кликает.
pub struct BarOutput {
    pub response: Response,
    pub value: Option<f32>,
}

/// Ползунок для перемотки и громкости. В M3 ручка в виде вертикальной «таблетки» (Expressive),
/// в Liquid Glass круглая.
pub fn slider_bar(ui: &mut Ui, theme: &Theme, width: f32, frac: f32, enabled: bool) -> BarOutput {
    let p = &theme.palette;
    let sense = if enabled { Sense::click_and_drag() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 24.0), sense);
    let pad = 10.0;
    let (left, right) = (rect.left() + pad, rect.right() - pad);
    let cy = rect.center().y;
    let active = resp.hovered() || resp.dragged();
    let th = if active { 8.0 } else { 6.0 };
    let frac = frac.clamp(0.0, 1.0);
    let x = left + (right - left) * frac;

    let painter = ui.painter();
    let track_bg = with_alpha(p.on_surface, if p.dark { 50 } else { 40 });
    let fg = if enabled { p.primary } else { with_alpha(p.on_surface_variant, 120) };

    match theme.design() {
        Design::Material3 => {
            // Активная часть, зазор, ручка-«таблетка», неактивная часть.
            let gap = 6.0;
            let a_end = (x - gap).max(left);
            if a_end > left {
                painter.rect_filled(Rect::from_min_max(pos2(left, cy - th / 2.0), pos2(a_end, cy + th / 2.0)), th / 2.0, fg);
            }
            let i_start = (x + gap).min(right);
            if right > i_start {
                painter.rect_filled(Rect::from_min_max(pos2(i_start, cy - th / 2.0), pos2(right, cy + th / 2.0)), th / 2.0, track_bg);
            }
            let hw = if resp.dragged() { 3.0 } else { 4.0 };
            painter.rect_filled(Rect::from_center_size(pos2(x, cy), vec2(hw, 22.0)), hw / 2.0, fg);
        }
        Design::LiquidGlass => {
            painter.rect_filled(Rect::from_min_max(pos2(left, cy - th / 2.0), pos2(right, cy + th / 2.0)), th / 2.0, track_bg);
            if x > left {
                painter.rect_filled(Rect::from_min_max(pos2(left, cy - th / 2.0), pos2(x, cy + th / 2.0)), th / 2.0, fg);
            }
            let r = if active { 9.0 } else { 7.5 };
            painter.circle_filled(pos2(x, cy + 1.0), r, egui::Color32::from_black_alpha(50));
            painter.circle_filled(pos2(x, cy), r, egui::Color32::WHITE);
        }
    }

    let value = if resp.dragged() || resp.clicked() || resp.drag_stopped() {
        resp.interact_pointer_pos().map(|pos| ((pos.x - left) / (right - left)).clamp(0.0, 1.0))
    } else {
        None
    };
    BarOutput { response: resp, value }
}

/// Цветная плитка-заглушка вместо обложки (обложек в ядре пока нет).
pub fn cover_tile(ui: &mut Ui, theme: &Theme, rect: Rect, seed: &str, radius: f32) {
    let p = &theme.palette;
    let h = seed.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let (bg, fg) = match h % 3 {
        0 => (p.primary_container, p.on_primary_container),
        1 => (p.secondary_container, p.on_secondary_container),
        _ => (p.tertiary_container, p.on_surface),
    };
    let painter = ui.painter();
    painter.rect_filled(rect, radius, bg);
    icons::paint(painter, Icon::Note, rect.center(), rect.height() * 0.5, with_alpha(fg, 200));
}

pub fn fmt_time(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub fn artists_text(track: &Track) -> String {
    if track.artists.is_empty() {
        "Неизвестный исполнитель".to_string()
    } else {
        track.artists.join(", ")
    }
}

/// Строка списка треков. Высота фиксированная (56), чтобы работала виртуализация списка.
pub fn track_row(ui: &mut Ui, theme: &Theme, index: usize, track: &Track, is_current: bool, is_playing: bool, time: f64) -> Response {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
    let painter = ui.painter();

    if is_current {
        painter.rect_filled(rect, 18.0, with_alpha(p.primary, if p.dark { 34 } else { 28 }));
    } else if resp.hovered() {
        painter.rect_filled(rect, 18.0, theme.row_hover_fill());
    }

    let cover = Rect::from_min_size(pos2(rect.left() + 10.0, rect.center().y - 20.0), Vec2::splat(40.0));
    cover_tile(ui, theme, cover, &track.id, 12.0);
    let painter = ui.painter();
    if is_current {
        painter.rect_filled(cover, 12.0, egui::Color32::from_black_alpha(110));
        icons::paint_equalizer(painter, cover.center(), 18.0, egui::Color32::WHITE, time, is_playing);
    }

    let text_left = cover.right() + 14.0;
    let dur_x = rect.right() - 16.0;
    let clip = Rect::from_min_max(pos2(text_left, rect.top()), pos2(dur_x - 56.0, rect.bottom()));
    let clipped = painter.with_clip_rect(clip);
    let title_color = if is_current { p.primary } else { p.on_surface };
    clipped.text(pos2(text_left, rect.center().y - 9.0), Align2::LEFT_CENTER, &track.title, FontId::proportional(15.0), title_color);
    clipped.text(pos2(text_left, rect.center().y + 10.0), Align2::LEFT_CENTER, artists_text(track), FontId::proportional(12.5), p.on_surface_variant);
    painter.text(pos2(dur_x, rect.center().y), Align2::RIGHT_CENTER, fmt_time(track.duration_secs as u64), FontId::proportional(13.0), p.on_surface_variant);

    let _ = index;
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Маленький заголовок группы (например «Сервисы»).
pub fn section_label(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(egui::RichText::new(text).size(12.5).color(theme.palette.on_surface_variant));
    });
}

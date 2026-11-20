//! Строка списка треков.

use super::cover::cover_tile;
use super::glide::squish;
use crate::icons;
use crate::theme::{ShapeRole, TextRole, Theme, with_alpha};
use crate::ui::format::{artists_text, fmt_time};
use egui::{Align2, CursorIcon, FontId, Rect, Response, Sense, Ui, Vec2, pos2, vec2};
use mcrate_core::Track;

/// Строка списка треков. В Material 3 двухстрочный элемент занимает 72 px;
/// Liquid Glass сохраняет исходные 56 px для совместимости с прежней геометрией.
/// Индекс выводится отдельной колонкой: он нужен для порядка в библиотеке и списка «Мои треки».
pub(crate) fn track_row(
    ui: &mut Ui,
    theme: &Theme,
    index: usize,
    track: &Track,
    is_current: bool,
    is_playing: bool,
    time: f64,
) -> Response {
    let p = &theme.palette;
    let row_height = theme.list_item_height(2);
    let row_width = ui.available_width();
    // На узкой панели убираем номер и уменьшаем обложку, чтобы название и длительность
    // не конкурировали за оставшееся место. На обычной ширине сохраняется исходная геометрия.
    let compact = !theme.glass_active() && row_width < 520.0;
    let show_index = theme.glass_active() || row_width >= 460.0;
    let cover_size = if theme.glass_active() || compact { 40.0 } else { 48.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(row_width, row_height), Sense::click());
    // Подсветку и pressed-state в Material 3 уже рисует общий glide/ripple списка.
    // Дополнительное сжатие здесь уменьшало только фон текущего трека, тогда как
    // общий слой оставался исходного размера — получались две вложенные карточки.
    // В Liquid Glass собственного M3 ripple нет, поэтому там сохраняем squish.
    if !ui.is_rect_visible(rect) {
        return resp; // не в кадре: ничего не считаем и не рисуем
    }
    let id = ui.scope_id().with(("track_row", &track.id));
    // Продавливание: содержимое строки чуть сжимается при нажатии. В Material 3 фон текущего трека
    // и слой подсветки остаются исходного размера (`row_rect`), сжимается только содержимое.
    let row_rect = rect;
    let rect = squish(ui, id, &resp, rect, 0.015, 0.07);
    let painter = ui.painter();

    if theme.glass_active() && resp.is_pointer_button_down_on() && !is_current {
        let radius = theme.shape_radius(ShapeRole::Large) as f32;
        painter.rect_filled(rect, radius, with_alpha(p.on_surface, 20));
    }
    if is_current {
        if theme.glass_active() {
            // Тот же цвет, что у выбранного пункта меню (`selected_fill`), плюс акцентная полоска слева.
            painter.rect_filled(rect, theme.shape_radius(ShapeRole::Large), theme.selected_fill());
            painter.rect_filled(
                Rect::from_min_size(pos2(rect.left() + 4.0, rect.top() + 12.0), vec2(2.5, 32.0)),
                1.25,
                with_alpha(p.primary, 210),
            );
        } else {
            painter.rect_filled(row_rect, theme.shape_radius(ShapeRole::Large), p.secondary_container);
        }
    }

    let cover_left = if show_index { 34.0 } else { 16.0 };
    if show_index {
        painter.text(
            pos2(rect.left() + 26.0, rect.center().y),
            Align2::RIGHT_CENTER,
            format!("{}", index + 1),
            FontId::proportional(12.5),
            p.on_surface_variant,
        );
    }
    let cover = Rect::from_min_size(pos2(rect.left() + cover_left, rect.center().y - cover_size * 0.5), Vec2::splat(cover_size));
    let cover_radius = if theme.glass_active() { 12.0 } else { theme.shape_radius(ShapeRole::Medium) as f32 };
    cover_tile(ui, theme, cover, &track.id, cover_radius);
    let painter = ui.painter();
    if is_current {
        painter.rect_filled(cover, 12.0, egui::Color32::from_black_alpha(110));
        icons::paint_equalizer(painter, cover.center(), 18.0, egui::Color32::WHITE, time, is_playing);
    }

    let text_left = cover.right() + if compact { 10.0 } else { 14.0 };
    let dur_x = rect.right() - if compact { 12.0 } else { 16.0 };
    let duration_gap = if compact { 44.0 } else { 56.0 };
    let clip = Rect::from_min_max(pos2(text_left, rect.top()), pos2((dur_x - duration_gap).max(text_left), rect.bottom()));
    let clipped = painter.with_clip_rect(clip);
    let title_color = if is_current { if theme.glass_active() { p.primary } else { p.on_secondary_container } } else { p.on_surface };
    let title_offset = if theme.glass_active() { 9.0 } else { 11.0 };
    let caption_offset = if theme.glass_active() { 10.0 } else { 12.0 };
    let supporting_role = if theme.glass_active() { TextRole::BodySmall } else { TextRole::BodyMedium };
    clipped.text(
        pos2(text_left, rect.center().y - title_offset),
        Align2::LEFT_CENTER,
        &track.title,
        FontId::proportional(theme.text_size(TextRole::TitleMedium)),
        title_color,
    );
    clipped.text(
        pos2(text_left, rect.center().y + caption_offset),
        Align2::LEFT_CENTER,
        artists_text(track),
        FontId::proportional(theme.text_size(supporting_role)),
        p.on_surface_variant,
    );
    let time_size = if theme.glass_active() { 12.5 } else { theme.text_size(TextRole::LabelSmall) };
    painter.text(
        pos2(dur_x, rect.center().y),
        Align2::RIGHT_CENTER,
        fmt_time(track.duration_secs as u64),
        FontId::proportional(time_size),
        p.on_surface_variant,
    );

    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let accessible_label = format!("{} — {}", track.title, artists_text(track));
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, is_current, accessible_label.clone()));
    resp
}

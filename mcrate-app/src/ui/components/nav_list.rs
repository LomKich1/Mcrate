//! Список строк-переходов: главный экран настроек и экран «Библиотека».

use crate::app::View;
use crate::icons::{self, Icon};
use crate::theme::{TextRole, Theme, with_alpha};
use crate::ui::metrics::{MENU_CORNER, NAV_ROW_H};
use crate::ui::widgets::{glide_begin, glide_end, squish};
use egui::{CornerRadius, CursorIcon, FontId, Margin, Sense, Ui, pos2, vec2};

/// Список строк-переходов (главный экран настроек и экран «Библиотека»). Возвращает вид, на который нажали.
/// Material 3: все строки лежат в одном тональном блоке со скруглением MENU_CORNER, как список в меню тем
/// (`theme_menu`): без зазоров между строками, у строки при наведении своя скруглённая подсветка, шеврона нет.
/// Liquid Glass: отдельные строки, как раньше. `max_width` ограничивает ширину (700 в настройках).
pub(in crate::ui) fn nav_list(ui: &mut Ui, theme: &Theme, rows: &[(View, &str, String)], max_width: f32, chevron: bool) -> Option<View> {
    let width = ui.available_width().min(max_width);
    let mut clicked = None;
    if theme.glass_active() {
        // Одинаковый плотный список для обеих тем: без разделителей и промежутков.
        // Подсветка glide скользит между строками так же, как в списках треков.
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let glide_slot = glide_begin(ui);
            let mut hovered = None;
            for (i, (view, title, subtitle)) in rows.iter().enumerate() {
                let resp = glass_nav_row(ui, theme, title, subtitle, width, chevron);
                if resp.hovered() {
                    hovered = Some((i as u64, resp.rect));
                }
                if resp.clicked() {
                    clicked = Some(*view);
                }
            }
            glide_end(ui, theme, glide_slot, ui.scope_id().with("glide_glass_nav_list"), hovered, 14.0);
        });
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
                // Та же скользящая подсветка, что в списке треков (ui/widgets/glide.rs). Радиус равен скруглению
                // блока: у первой и последней строки внешние углы подсветки совпадают с углами блока.
                glide_end(ui, theme, glide_slot, ui.scope_id().with("glide_nav_list"), hovered, MENU_CORNER as f32);
            });
    });
    clicked
}

/// Строка внутри тонального блока Material 3: название и подпись. Подсветку рисует `glide_end` в `nav_list`.
fn m3_nav_row(ui: &mut Ui, theme: &Theme, title: &str, subtitle: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), NAV_ROW_H), Sense::click());
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
    let (base_rect, resp) = ui.allocate_exact_size(vec2(width, NAV_ROW_H), Sense::click());
    let rect = squish(ui, resp.id, &resp, base_rect, 0.015, 0.07);
    let painter = ui.painter();
    // Стеклянная поверхность выбранной строки рисуется единым анимированным слоем
    // в nav_list/glide_end, чтобы она плавно перемещалась между пунктами.
    painter.text(
        pos2(rect.left() + 18.0, rect.center().y - 10.0),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(16.0),
        p.on_surface,
    );
    painter.text(
        pos2(rect.left() + 18.0, rect.center().y + 12.0),
        egui::Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(12.5),
        p.on_surface_variant,
    );
    if chevron {
        icons::paint(theme, painter, Icon::ChevronForward, pos2(rect.right() - 26.0, rect.center().y), 20.0, p.on_surface_variant);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

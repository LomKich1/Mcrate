//! Список строк-переходов: главный экран настроек и экран «Библиотека».

use crate::app::View;
use crate::icons::{self, Icon};
use crate::theme::{TextRole, Theme, with_alpha};
use crate::ui::metrics::{MENU_CORNER, NAV_ROW_H};
use crate::ui::ripple::{LayerArea, press_sparkles};
use crate::ui::widgets::{glide_begin, squish};
use egui::{CornerRadius, CursorIcon, FontId, Margin, Sense, Ui, pos2, vec2};

/// Скругление волны нажатия на строке Liquid Glass.
const GLASS_ROW_CORNER: f32 = 14.0;

/// Список строк-переходов (главный экран настроек и экран «Библиотека»). Возвращает вид, на который нажали.
/// Material 3: все строки лежат в одном тональном блоке со скруглением MENU_CORNER, как список в меню тем
/// (`theme_menu`): без зазоров между строк, шеврона нет. Подсветки при наведении нет: при нажатии от места нажатия
/// расходятся тёмные крапинки (`ripple::press_sparkles`).
/// Liquid Glass: отдельные строки, как раньше. `max_width` ограничивает ширину (700 в настройках).
pub(in crate::ui) fn nav_list(ui: &mut Ui, theme: &Theme, rows: &[(View, &str, String)], max_width: f32, chevron: bool) -> Option<View> {
    let width = ui.available_width().min(max_width);
    let mut clicked = None;
    if theme.glass_active() {
        // Плотный список: без разделителей и промежутков. Подсветки при наведении нет, при нажатии расходится волна.
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (view, title, subtitle) in rows {
                let wave_slot = glide_begin(ui);
                let resp = glass_nav_row(ui, theme, title, subtitle, width, chevron);
                press_sparkles(ui, theme, wave_slot, &resp, LayerArea { rect: resp.rect, corner: GLASS_ROW_CORNER });
                if resp.clicked() {
                    clicked = Some(*view);
                }
            }
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
                for (view, title, subtitle) in rows {
                    // Слой под волну резервируем до строки, чтобы текст рисовался поверх неё. Радиус волны равен
                    // скруглению блока: у первой и последней строки внешние углы волны совпадают с углами блока.
                    let wave_slot = glide_begin(ui);
                    let resp = m3_nav_row(ui, theme, title, subtitle);
                    press_sparkles(ui, theme, wave_slot, &resp, LayerArea { rect: resp.rect, corner: MENU_CORNER as f32 });
                    if resp.clicked() {
                        clicked = Some(*view);
                    }
                }
            });
    });
    clicked
}

/// Строка внутри тонального блока Material 3: название и подпись. Волну нажатия рисует `nav_list`.
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

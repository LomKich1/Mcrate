//! Карточка медиа на полках: обложка-заглушка и две строки подписи.

use super::shelf::SHELF_HOVER_KEY;
use crate::icons::{self, Icon};
use crate::theme::{Theme, with_alpha};
use crate::ui::widgets::{cover_tile, elided, squish};
use egui::{Color32, CursorIcon, FontId, Id, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};

/// Что показать на карточке полки.
pub(super) struct MediaCard<'a> {
    /// От чего зависит цвет плитки-заглушки (пока нет обложек).
    pub(super) seed: &'a str,
    pub(super) title: &'a str,
    pub(super) subtitle: &'a str,
    /// Круглая картинка (исполнитель) вместо скруглённого квадрата (трек).
    pub(super) round: bool,
    pub(super) w: f32,
    pub(super) h: f32,
}

/// Медиакарточка. В M3 — filled card; в Liquid Glass — обложка и подписи напрямую
/// на контентной поверхности без отдельного M3-контейнера.
///
/// Длиннее 60 строк намеренно: линейная раскладка одной карточки (фон, обложка, слои наведения, подписи), каждый шаг
/// опирается на геометрию предыдущего, а общих частей, которые стоило бы вынести, нет.
pub(super) fn media_card(ui: &mut Ui, theme: &Theme, card: &MediaCard, is_current: bool, is_playing: bool, time: f64) -> Response {
    let p = &theme.palette;
    let glass = theme.glass_active();
    let (rect, resp) = ui.allocate_exact_size(vec2(card.w, card.h), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp; // карточка за краем полки или экрана: не рисуем
    }
    // Продавливание при нажатии, как у строк треков (`squish` в widgets/glide.rs): карточка плавно уменьшается
    // вокруг центра. Доля по высоте подобрана так, чтобы абсолютное сжатие по ширине и высоте было
    // одинаковым: содержимое (обложка и подписи) считается от верхнего края и остаётся внутри карточки.
    // Клик по-прежнему ловится по исходному прямоугольнику (`resp`), поэтому края не «дрожат».
    const PRESS: f32 = 0.05;
    let rect = squish(ui, resp.id, &resp, rect, PRESS, PRESS * card.w / card.h);
    // Подсветку под карточкой рисует полка (скользит между карточками). Карточка лишь сообщает,
    // где сейчас курсор; сжатие при нажатии уходит и в подсветку (берём уже сжатый прямоугольник).
    if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.ctx().data_mut(|d| d.insert_temp(Id::unique(SHELF_HOVER_KEY), Some((resp.id.value(), rect))));
    }
    let radius = if glass { 14.0 } else { 16.0 };
    let (on_container, on_variant) = if is_current && !glass {
        (p.on_secondary_container, with_alpha(p.on_secondary_container, 200))
    } else {
        (p.on_surface, p.on_surface_variant)
    };
    // Отпускаем неизменяемое заимствование `ui` до `cover_tile()`, которому
    // требуется `&mut Ui`. После отрисовки обложки painter запрашивается повторно.
    {
        let painter = ui.painter();
        if glass {
            // Apple Music-подобная медиаполка: обложка и подписи лежат непосредственно
            // на контентной поверхности, без отдельного M3 filled-card контейнера.
            // Подсветку при наведении даёт полка (стекло-выделение под карточкой).
        } else {
            let container = if is_current { p.secondary_container } else { p.surface_container_highest };
            painter.rect_filled(rect, radius, container);
        }
    }

    // Обложка: у трека скруглённый квадрат, у исполнителя круг.
    let pad = if glass { 4.0 } else { 8.0 };
    let side = rect.width() - pad * 2.0; // от сжатого прямоугольника, а не от card.w
    let cover = Rect::from_min_size(rect.min + vec2(pad, pad), Vec2::splat(side));
    let cover_radius = if card.round {
        side / 2.0
    } else if glass {
        10.0
    } else {
        12.0
    };
    cover_tile(ui, theme, cover, card.seed, cover_radius);
    let painter = ui.painter();
    if glass {
        painter.rect_stroke(cover, cover_radius, Stroke::new(0.8, with_alpha(p.on_surface, 36)), egui::StrokeKind::Inside);
    }

    if is_current {
        painter.rect_filled(cover, cover_radius, Color32::from_black_alpha(if glass { 84 } else { 100 }));
        icons::paint_equalizer(painter, cover.center(), 32.0, Color32::WHITE, time, is_playing);
        if glass {
            painter.rect_stroke(cover, cover_radius, Stroke::new(1.4, with_alpha(p.primary, 210)), egui::StrokeKind::Inside);
        }
    } else if resp.hovered() {
        // Для Liquid Glass — компактная круглая prominent-кнопка поверх обложки;
        // в M3 оставляем квадратную FAB.
        if glass {
            let fab = Rect::from_min_size(pos2(cover.right() - 42.0, cover.bottom() - 42.0), Vec2::splat(34.0));
            painter.circle_filled(fab.center() + vec2(0.0, 1.0), 17.0, Color32::from_black_alpha(46));
            painter.circle_filled(fab.center(), 17.0, with_alpha(p.surface_container_high, 218));
            painter.circle_stroke(fab.center(), 16.5, Stroke::new(0.9, with_alpha(p.on_surface, 70)));
            icons::paint(theme, painter, Icon::Play, fab.center(), 21.0, p.on_surface);
        } else {
            let fab = Rect::from_min_size(pos2(cover.right() - 8.0 - 40.0, cover.bottom() - 8.0 - 40.0), Vec2::splat(40.0));
            painter.rect_filled(fab, 12.0, p.primary_container);
            icons::paint(theme, painter, Icon::Play, fab.center(), 26.0, p.on_primary_container);
        }
    }

    let text_w = rect.width() - pad * 2.0 - 4.0;
    let ty = cover.bottom() + if glass { 8.0 } else { 10.0 };
    let title = elided(painter, card.title, FontId::proportional(14.5), on_container, text_w);
    let sub = elided(painter, card.subtitle, FontId::proportional(12.0), on_variant, text_w);
    let tx = rect.left() + pad + 2.0;
    painter.galley(pos2(tx, ty), title.clone(), on_container);
    painter.galley(pos2(tx, ty + title.size().y + 2.0), sub, on_variant);

    // В M3 отдельного слоя наведения у карточки нет: меняются только курсор и кнопка воспроизведения поверх обложки.
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

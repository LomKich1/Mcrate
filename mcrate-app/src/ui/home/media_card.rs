//! Карточка медиа на полках: обложка-заглушка и две строки подписи.

use super::shelf::SHELF_HOVER_KEY;
use crate::icons::{self, Icon};
use crate::theme::{Theme, with_alpha};
use crate::ui::ripple;
use crate::ui::widgets::{COVER_SCRIM_ALPHA, cover_tile, elided, row_hover, squish, track_cover};
use egui::{Color32, CursorIcon, FontId, Id, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};
use mcrate_core::Track;

/// Размер кнопки воспроизведения на обложке карточки, доля стороны обложки (у строки трека 0,5: там обложка мелкая).
const PLAY_RATIO: f32 = 0.34;

/// Что показать на карточке полки.
pub(super) struct MediaCard<'a> {
    /// От чего зависит цвет плитки-заглушки (когда обложки нет или она ещё грузится).
    pub(super) seed: &'a str,
    /// Трек, чья обложка на карточке (у исполнителя его первый трек: фото исполнителей в данных нет); `None`: только заглушка.
    pub(super) track: Option<&'a Track>,
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
    match card.track {
        Some(track) => track_cover(ui, theme, cover, track, cover_radius),
        None => cover_tile(ui, theme, cover, card.seed, cover_radius),
    }
    let painter = ui.painter();
    if glass {
        painter.rect_stroke(cover, cover_radius, Stroke::new(0.8, with_alpha(p.on_surface, 36)), egui::StrokeKind::Inside);
    }

    // Как у строк треков: при наведении обложка плавно темнеет, по центру плавно появляется кнопка воспроизведения,
    // когда курсор уходит, всё так же гаснет. У играющего карточка затемнена всегда, эквалайзер сменяется кнопкой.
    let hover = row_hover(ui, theme, resp.id, &resp);
    let scrim = if is_current { 1.0 } else { hover };
    if scrim > 0.0 {
        let shade = Color32::from_black_alpha((COVER_SCRIM_ALPHA as f32 * scrim).round() as u8);
        if card.round {
            painter.circle_filled(cover.center(), side / 2.0, shade); // rect_filled с радиусом в полстороны даёт полосу
        } else {
            painter.rect_filled(cover, cover_radius, shade);
        }
    }
    if is_current {
        let bars = with_alpha(Color32::WHITE, (255.0 * (1.0 - hover)).round() as u8);
        icons::paint_equalizer(painter, cover.center(), 32.0, bars, time, is_playing);
        if glass {
            painter.rect_stroke(cover, cover_radius, Stroke::new(1.4, with_alpha(p.primary, 210)), egui::StrokeKind::Inside);
        }
    }
    if hover > 0.0 {
        let icon = with_alpha(Color32::WHITE, (255.0 * hover).round() as u8);
        icons::paint(theme, painter, Icon::Play, cover.center(), cover.height() * PLAY_RATIO, icon);
    }

    let text_w = rect.width() - pad * 2.0 - 4.0;
    let ty = cover.bottom() + if glass { 8.0 } else { 10.0 };
    let title = elided(painter, card.title, FontId::proportional(14.5), on_container, text_w);
    let sub = elided(painter, card.subtitle, FontId::proportional(12.0), on_variant, text_w);
    let tx = rect.left() + pad + 2.0;
    painter.galley(pos2(tx, ty), title.clone(), on_container);
    painter.galley(pos2(tx, ty + title.size().y + 2.0), sub, on_variant);

    // Ripple поверх обложки: она занимает почти всю карточку, и под ней крапинок не было бы видно.
    ripple::state_layer(ui, theme, ui.painter(), &resp, rect, radius);

    // В M3 отдельного слоя наведения у карточки нет: меняются только курсор и кнопка воспроизведения поверх обложки.
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

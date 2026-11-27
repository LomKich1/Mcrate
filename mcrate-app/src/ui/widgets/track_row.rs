//! Строка списка треков.
//!
//! Подсветка строки под курсором неподвижна и у каждой строки своя: обложка затемняется, поверх неё показывается
//! кнопка воспроизведения; когда курсор уходит, всё так же плавно гаснет. При скачивании на обложке появляется
//! отдельный анимированный значок. Плашки под строкой нет ни в Material 3, ни в
//! Liquid Glass (по просьбе владельца): при наведении выделяется только обложка.
//! Те же слои включаются при нажатии (касание на Android: наведения там нет) и при фокусе с клавиатуры.

use super::buttons::{IconButton, icon_button, like_button};
use super::cover::cover_tile;
use super::download_icon::paint_download_overlay;
use super::glide::squish;
use super::segment::{SEGMENT_INNER_RADIUS, SegmentPos, ripple_corner, segment_radius, segment_shape, track_plate};
use crate::icons::{self, Icon};
use crate::theme::{ShapeRole, TextRole, Theme, with_alpha};
use crate::ui::format::{artists_text, fmt_time};
use crate::ui::ripple::{self, LayerArea};
use crate::controller::DownloadVisual;
use crate::ui::thanos;
use crate::ui::track_menu::{download_visual, is_liked, request_like, request_menu};
use egui::{Align2, Color32, CursorIcon, FontId, Id, Pos2, Rect, Response, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use mcrate_core::Track;

/// За сколько секунд подсветка строки появляется и гаснет.
const HOVER_FADE_SECS: f32 = 0.16;
/// Плотность чёрной подложки на обложке (0..255): под значком эквалайзера играющего трека и под кнопкой воспроизведения.
pub(crate) const COVER_SCRIM_ALPHA: u8 = 150;
/// Размер кнопки воспроизведения на обложке, доля стороны обложки.
const COVER_PLAY_RATIO: f32 = 0.5;
/// Диаметр кнопки «три точки» (меню действий трека) и её отступ от правого края строки.
const MORE_SIZE: f32 = 36.0;
const MORE_INSET_RIGHT: f32 = 6.0;
/// Диаметр сердечка «Нравится» слева от кнопки «три точки».
const LIKE_SIZE: f32 = 36.0;
/// Уже этого ширина строки сердечка нет: название осталось бы без места (кнопка «Нравится» есть и в меню трёх точек).
const LIKE_MIN_ROW_WIDTH: f32 = 360.0;
/// Сколько места справа в строке занимают кнопки (без сердечка) вместе с зазором до длительности.
const BUTTONS_SLOT: f32 = MORE_SIZE + MORE_INSET_RIGHT + 4.0;

/// Что нужно для отрисовки строки, кроме геометрии.
#[derive(Clone, Copy)]
struct RowState {
    is_current: bool,
    is_playing: bool,
    /// Насколько строка «включена» (0..1), см. `row_hover`.
    hover: f32,
    /// Время кадра для анимации эквалайзера.
    time: f64,
    /// Скачивание этого трека; значок рисуется поверх его обложки.
    download: Option<DownloadVisual>,
}

/// Что сейчас играет относительно строки: от этого зависят подсветка, эквалайзер и кнопка на обложке.
#[derive(Clone, Copy)]
pub(crate) struct RowPlayback {
    /// Строка это текущий трек.
    pub is_current: bool,
    /// Текущий трек сейчас звучит (не пауза).
    pub is_playing: bool,
    /// Время кадра для анимации эквалайзера.
    pub time: f64,
}

/// Строка списка треков. В Material 3 двухстрочный элемент занимает 72 px;
/// Liquid Glass сохраняет исходные 56 px для совместимости с прежней геометрией.
/// Индекс выводится отдельной колонкой: он нужен для порядка в библиотеке и списка «Мои треки».
pub(crate) fn track_row(ui: &mut Ui, theme: &Theme, index: usize, track: &Track, playback: RowPlayback, pos: SegmentPos) -> Response {
    track_row_with_provider_badge(ui, theme, index, track, playback, pos, false)
}

/// Строка для «Скачанных»: на обложке дополнительно показывает короткую метку источника.
pub(crate) fn track_row_with_provider_badge(
    ui: &mut Ui,
    theme: &Theme,
    index: usize,
    track: &Track,
    playback: RowPlayback,
    pos: SegmentPos,
    show_provider_badge: bool,
) -> Response {
    let RowPlayback { is_current, is_playing, time } = playback;
    let row_height = theme.list_item_height(2);
    let row_width = ui.available_width();
    // На узкой панели убираем номер и уменьшаем обложку, чтобы название и длительность
    // не конкурировали за оставшееся место. На обычной ширине сохраняется исходная геометрия.
    let compact = !theme.glass_active() && row_width < 520.0;
    let show_index = theme.glass_active() || row_width >= 460.0;
    let cover_size = if theme.glass_active() || compact { 40.0 } else { 48.0 };
    let row_id = ui.scope_id().with(("thanos_track_row", index, track.provider, &track.id));
    let now = ui.input(|input| input.time);
    if let Some(progress) = thanos::row_progress(ui.ctx(), row_id, now) {
        let spacing = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = spacing * (1.0 - progress);
        let (_, response) = ui.allocate_exact_size(vec2(row_width, row_height * (1.0 - progress)), Sense::hover());
        ui.spacing_mut().item_spacing.y = spacing;
        return response;
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(row_width, row_height), Sense::click());
    // ПКМ по любой точке строки открывает то же меню, что и кнопка «три точки».
    if resp.secondary_clicked() {
        request_menu(ui.ctx(), track, rect, row_id);
    }
    // Подсветку и pressed-state в Material 3 уже рисует общий glide/ripple списка.
    // В Liquid Glass собственного M3 ripple нет, поэтому там сохраняем squish.
    if !ui.is_rect_visible(rect) {
        return resp; // не в кадре: ничего не считаем и не рисуем
    }
    // Плашка строки (только Material 3, сегментированный список): слот резервируем первым, чтобы плашка лежала под
    // волной нажатия и содержимым, и рисуем по исходному прямоугольнику (сжимается только содержимое).
    let segment = (!theme.glass_active()).then(|| {
        let mut radius = segment_radius(theme, pos);
        if let Some((reveal_top, reveal_bottom, progress)) =
            thanos::segment_corner_progress(ui.ctx(), row_id, pos.index, now)
        {
            let inner = f32::from(SEGMENT_INNER_RADIUS);
            let outer = f32::from(theme.shape_radius(ShapeRole::ExtraLarge));
            let animated = (inner + (outer - inner) * progress).round().clamp(0.0, 255.0) as u8;
            if reveal_top {
                radius.nw = animated;
                radius.ne = animated;
            }
            if reveal_bottom {
                radius.sw = animated;
                radius.se = animated;
            }
        }
        radius
    });
    if let Some(radius) = segment {
        ui.painter().add(segment_shape(track_plate(rect), radius, theme.palette.surface_container_high));
    }
    let id = ui.scope_id().with(("track_row", &track.id));
    let hover = row_hover(ui, theme, id.with(index), &resp);
    // Продавливание: содержимое строки чуть сжимается при нажатии (слой подсветки M3 остаётся исходного размера).
    let rect = squish(ui, id, &resp, rect, 0.015, 0.07);
    let ripple_slot = ripple::reserve_slot(ui);

    let cover_left = if show_index { 34.0 } else { 16.0 };
    if show_index {
        paint_row_index(ui, theme, rect, index);
    }
    let cover = Rect::from_min_size(pos2(rect.left() + cover_left, rect.center().y - cover_size * 0.5), Vec2::splat(cover_size));
    let download = download_visual(ui.ctx(), track);
    paint_row_cover(ui, theme, cover, track, show_provider_badge, &RowState { is_current, is_playing, hover, time, download });
    let show_like = row_width >= LIKE_MIN_ROW_WIDTH;
    let buttons_slot = BUTTONS_SLOT + if show_like { LIKE_SIZE } else { 0.0 };
    paint_row_text(ui, theme, rect, cover.right(), track, (compact, is_current, buttons_slot));
    paint_row_buttons(ui, theme, rect, track, show_like, row_id);

    let corner = segment.map_or_else(|| theme.shape_radius(ShapeRole::Large) as f32, ripple_corner);
    ripple::press_sparkles(ui, theme, ripple_slot, &resp, LayerArea { rect, corner });
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let accessible_label = format!("{} — {}", track.title, artists_text(track));
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, is_current, accessible_label.clone()));
    thanos::record_row(ui.ctx(), row_id, rect.intersect(ui.clip_rect()), pos.index, pos.count);
    resp
}

/// Номер строки в левой колонке.
fn paint_row_index(ui: &Ui, theme: &Theme, rect: Rect, index: usize) {
    ui.painter().text(
        pos2(rect.left() + 26.0, rect.center().y),
        Align2::RIGHT_CENTER,
        format!("{}", index + 1),
        FontId::proportional(12.5),
        theme.palette.on_surface_variant,
    );
}

/// Обложка и слои поверх неё: у текущего трека затемнение с эквалайзером (при наведении он сменяется кнопкой
/// воспроизведения), у остальных затемнение и кнопка только при наведении.
fn paint_row_cover(ui: &mut Ui, theme: &Theme, cover: Rect, track: &Track, show_provider_badge: bool, state: &RowState) {
    let RowState { is_current, is_playing, hover, time, download } = *state;
    let cover_radius = if theme.glass_active() { 12.0 } else { theme.shape_radius(ShapeRole::Medium) as f32 };
    cover_tile(ui, theme, cover, &track.id, cover_radius);
    if let Some(download) = download {
        paint_download_overlay(ui, theme, cover, cover_radius, download);
        return;
    }
    let painter = ui.painter();
    if is_current {
        painter.rect_filled(cover, 12.0, Color32::from_black_alpha(COVER_SCRIM_ALPHA));
        // При наведении эквалайзер плавно сменяется кнопкой воспроизведения (клик по строке запускает трек заново).
        let bars = with_alpha(Color32::WHITE, (255.0 * (1.0 - hover)).round() as u8);
        icons::paint_equalizer(painter, cover.center(), 18.0, bars, time, is_playing);
        paint_play_icon(ui, theme, cover, hover);
    } else {
        paint_cover_play(ui, theme, cover, cover_radius, hover);
    }
    if show_provider_badge {
        paint_provider_badge(ui, cover, track);
    }
}

/// Короткий код сервиса, откуда сохранён трек. Остальные коды добавляются вместе с провайдерами:
/// Yandex -> "YM", VK -> "VK", Spotify -> "SP".
fn provider_badge(provider: mcrate_core::ProviderKind) -> Option<&'static str> {
    match provider {
        mcrate_core::ProviderKind::SoundCloud => Some("SC"),
        mcrate_core::ProviderKind::Yandex => None, // TODO: Some("YM") после подключения Yandex Music.
        mcrate_core::ProviderKind::Vk => None, // TODO: Some("VK") после подключения VK Music.
        mcrate_core::ProviderKind::Spotify => None, // TODO: Some("SP") после подключения Spotify.
        mcrate_core::ProviderKind::Local => None,
    }
}

/// Тёмная плашка с кодом сервиса в правом нижнем углу обложки (как на макете скачанного трека).
fn paint_provider_badge(ui: &Ui, cover: Rect, track: &Track) {
    let Some(label) = provider_badge(track.provider) else { return };
    let width = (cover.width() * 0.42).clamp(14.0, 21.0);
    let height = (cover.height() * 0.26).clamp(10.0, 13.0);
    let badge = Rect::from_min_size(pos2(cover.right() - width, cover.bottom() - height), vec2(width, height));
    ui.painter().rect_filled(badge, 2.0, Color32::from_black_alpha(205));
    ui.painter().text(
        badge.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(height * 0.78),
        Color32::WHITE,
    );
}

/// Название, исполнитель и длительность. `cover_right`: правый край обложки, от него идёт текст.
/// `compact`: узкая панель (меньше отступы); `is_current`: название цветом акцента;
/// `buttons_slot`: сколько места справа занято кнопками (длительность стоит левее них).
fn paint_row_text(
    ui: &Ui,
    theme: &Theme,
    rect: Rect,
    cover_right: f32,
    track: &Track,
    (compact, is_current, buttons_slot): (bool, bool, f32),
) {
    let p = &theme.palette;
    let painter = ui.painter();
    let text_left = cover_right + if compact { 10.0 } else { 14.0 };
    let dur_x = rect.right() - buttons_slot - if compact { 6.0 } else { 10.0 };
    let duration_gap = if compact { 44.0 } else { 56.0 };
    let clip = Rect::from_min_max(pos2(text_left, rect.top()), pos2((dur_x - duration_gap).max(text_left), rect.bottom()));
    let clipped = painter.with_clip_rect(clip);
    let title_color = if is_current { p.primary } else { p.on_surface };
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
}

/// Кнопки у правого края строки: «три точки» (меню трека) и левее неё сердечко «Нравится» (`show_like`).
/// Нажатия только просят приложение что-то сделать (`track_menu::request_menu`, `request_like`),
/// само действие выполняется там.
fn paint_row_buttons(ui: &mut Ui, theme: &Theme, rect: Rect, track: &Track, show_like: bool, row_id: Id) {
    let more_center = pos2(rect.right() - MORE_INSET_RIGHT - MORE_SIZE / 2.0, rect.center().y);
    let more = place_in_row(ui, more_center, MORE_SIZE, |ui| icon_button(ui, theme, IconButton::new(Icon::MoreVertical, MORE_SIZE, "Ещё")));
    if more.clicked() {
        request_menu(ui.ctx(), track, more.rect, row_id);
    }
    if show_like {
        let like_center = pos2(more_center.x - MORE_SIZE / 2.0 - LIKE_SIZE / 2.0, rect.center().y);
        let liked = is_liked(ui.ctx(), track);
        if place_in_row(ui, like_center, LIKE_SIZE, |ui| like_button(ui, theme, liked, LIKE_SIZE)).clicked() {
            request_like(ui.ctx(), track, row_id);
        }
    }
}

/// Кнопка внутри уже выделенной строки: как `buttons::place_at`, но курсор списка не сдвигается. Иначе видимая строка
/// (с кнопками) и невидимая (без них) занимали бы разную высоту, и список дёргался бы при прокрутке.
fn place_in_row<R>(ui: &mut Ui, center: Pos2, size: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let rect = Rect::from_center_size(center, Vec2::splat(size));
    add(&mut ui.new_child(UiBuilder::new().max_rect(rect)))
}

/// Общая и для карточек полок (`home/media_card.rs`): у них та же подсветка обложки при наведении.
/// Насколько строка «включена» (0..1): курсор над ней, нажатие или фокус с клавиатуры. Значение плавно растёт и спадает.
/// `id` включает номер строки: один трек может стоять в списке дважды (история), а состояние у каждой строки своё.
pub(crate) fn row_hover(ui: &Ui, theme: &Theme, id: Id, resp: &Response) -> f32 {
    let active = resp.hovered() || resp.is_pointer_button_down_on() || resp.has_focus();
    let secs = if theme.settings.reduce_motion { 0.0 } else { HOVER_FADE_SECS };
    ui.ctx().animate_bool_with_time(id.with("hover"), active, secs)
}

/// Затемнение обложки и кнопка воспроизведения поверх неё для обычной строки; появляются и гаснут плавно (`hover`: 0..1).
fn paint_cover_play(ui: &Ui, theme: &Theme, cover: Rect, radius: f32, hover: f32) {
    if hover <= 0.0 {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(cover, radius, Color32::from_black_alpha((COVER_SCRIM_ALPHA as f32 * hover).round() as u8));
    paint_play_icon(ui, theme, cover, hover);
}

/// Белая кнопка воспроизведения по центру обложки; `hover`: 0..1, прозрачность.
fn paint_play_icon(ui: &Ui, theme: &Theme, cover: Rect, hover: f32) {
    let icon = with_alpha(Color32::WHITE, (255.0 * hover).round() as u8);
    icons::paint(theme, ui.painter(), Icon::Play, cover.center(), cover.height() * COVER_PLAY_RATIO, icon);
}

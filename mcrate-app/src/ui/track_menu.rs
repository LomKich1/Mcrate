//! Действия над треком из строки списка: меню «три точки» («Добавить в очередь», «Добавить в плейлист», внутри
//! плейлиста ещё «Убрать из плейлиста») и сердечко «Нравится» рядом.
//!
//! Строка (`widgets::track_row`) ничего не знает о приложении: нажатия она только записывает во временную память
//! egui (`request_menu`, `request_like`), а лайкнутые треки читает оттуда же (`is_liked`). Выполняет всё приложение:
//! `McrateApp::sync_row_state` (лайки, раз за кадр перед экранами) и `McrateApp::show_track_menu` (меню). Так одни
//! и те же кнопки работают на всех экранах со списками, и экранам не нужно ничего передавать.

use crate::app::{McrateApp, View};
use crate::controller::LikeKey;
use crate::icons::{self, Icon};
use crate::theme::{ShapeRole, TextRole, Theme, with_alpha};
use crate::ui::playlists::PlaylistDialog;
use egui::{Align2, Area, Color32, Context, FontId, Frame, Id, Key, Margin, Order, Pos2, Rect, Sense, Shadow, Stroke, Ui, vec2};
use mcrate_core::Track;
use std::collections::HashSet;
use std::sync::Arc;

/// Ширина меню. Material 3 допускает 112-280 px; 260 вмещает самую длинную подпись одной строкой.
const MENU_WIDTH: f32 = 260.0;
/// Высота пункта меню (Material 3: 48 px).
const ITEM_HEIGHT: f32 = 48.0;
/// Вертикальный отступ внутри карточки меню (Material 3: 8 px сверху и снизу).
const MENU_PADDING: i8 = 8;
/// Отступ значка пункта от левого края.
const ITEM_ICON_INSET: f32 = 16.0;
/// Размер значка в пункте меню.
const ITEM_ICON_SIZE: f32 = 20.0;
/// Расстояние от левого края значка до начала подписи.
const ITEM_LABEL_INSET: f32 = ITEM_ICON_INSET + ITEM_ICON_SIZE + 12.0;
/// Зазор между кнопкой «три точки» и меню.
const ANCHOR_GAP: f32 = 4.0;
/// Прозрачность слоя наведения и нажатия на пункте (Material 3: 8 % и 12 % от 255).
const HOVER_ALPHA: u8 = 20;
const PRESS_ALPHA: u8 = 31;
/// Заливка карточки меню в Liquid Glass: чуть просвечивает.
const GLASS_FILL_ALPHA: u8 = 235;
/// Тень карточки меню Material 3 (уровень 2): смещение вниз, размытие и плотность.
const SHADOW_OFFSET_Y: i8 = 2;
const SHADOW_BLUR: u8 = 10;
const SHADOW_ALPHA: u8 = 60;

/// Открытое меню: для какого трека и рядом с какой кнопкой.
pub(crate) struct OpenTrackMenu {
    track: Track,
    anchor: Rect,
}

/// Что выбрали в меню.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MenuAction {
    AddToQueue,
    /// Открыть окно выбора плейлиста для этого трека.
    AddToPlaylist,
    /// Убрать трек из открытого плейлиста с этим номером.
    RemoveFromPlaylist(u64),
}

/// Во временной памяти egui лежит запрос на открытие меню, пока приложение его не заберёт.
fn request_id() -> Id {
    Id::unique("track_menu_request")
}

/// Где во временной памяти лежит множество лайкнутых треков для строк.
fn liked_id() -> Id {
    Id::unique("row_liked_keys")
}

/// Где лежит запрос «поставить/снять лайк», пока приложение его не заберёт.
fn like_request_id() -> Id {
    Id::unique("row_like_request")
}

/// Стоит ли лайк на треке (по множеству, которое приложение выложило в этом кадре).
pub(crate) fn is_liked(ctx: &Context, track: &Track) -> bool {
    ctx.data(|d| d.get_temp::<Arc<HashSet<LikeKey>>>(liked_id())).is_some_and(|keys| keys.contains(&(track.provider, track.id.clone())))
}

/// Строка просит поставить или снять лайк с `track`.
pub(crate) fn request_like(ctx: &Context, track: &Track) {
    ctx.data_mut(|d| d.insert_temp(like_request_id(), track.clone()));
}

/// Строка просит открыть меню для `track` рядом с кнопкой `anchor`.
pub(crate) fn request_menu(ctx: &Context, track: &Track, anchor: Rect) {
    ctx.data_mut(|d| d.insert_temp(request_id(), (track.clone(), anchor)));
}

impl McrateApp {
    /// Выполняет запрошенный лайк и выкладывает актуальное множество лайков для строк. Раз за кадр, до отрисовки экранов.
    pub(crate) fn sync_row_state(&mut self, ctx: &Context) {
        let request = ctx.data_mut(|d| {
            let request = d.get_temp::<Track>(like_request_id());
            d.remove::<Track>(like_request_id());
            request
        });
        if let Some(track) = request {
            self.ctl.toggle_like(&track);
        }
        ctx.data_mut(|d| d.insert_temp(liked_id(), self.ctl.liked_keys()));
    }

    /// Открывает запрошенное меню, рисует его поверх интерфейса и выполняет выбранное действие.
    /// Вызывается один раз за кадр после отрисовки экранов.
    pub(crate) fn show_track_menu(&mut self, ctx: &Context, theme: &Theme) {
        let request = ctx.data_mut(|d| {
            let request = d.get_temp::<(Track, Rect)>(request_id());
            d.remove::<(Track, Rect)>(request_id());
            request
        });
        if let Some((track, anchor)) = request {
            self.track_menu = Some(OpenTrackMenu { track, anchor });
        }
        let Some(open) = &self.track_menu else { return };
        // «Убрать из плейлиста» нужен только на экране самого плейлиста.
        let in_playlist = if self.view == View::CollectionPlaylist { self.open_playlist } else { None };
        let items = menu_items(in_playlist);
        let pos = menu_position(ctx.content_rect(), open.anchor, items.len());

        let area = Area::new(Id::unique("track_menu")).order(Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            let frame = if theme.glass_active() { glass_frame(theme) } else { m3_frame(theme) };
            frame
                .show(ui, |ui| {
                    ui.set_width(MENU_WIDTH);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    items.iter().find_map(|&(action, icon, label)| menu_item(ui, theme, icon, label).then_some(action))
                })
                .inner
        });

        let outside_press =
            ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p)));
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let chosen = area.inner;
        if let Some(action) = chosen
            && let Some(open) = &self.track_menu
        {
            let track = open.track.clone();
            match action {
                MenuAction::AddToQueue => self.ctl.add_to_queue(track),
                MenuAction::AddToPlaylist => self.playlist_dialog = Some(PlaylistDialog::Pick(track)),
                MenuAction::RemoveFromPlaylist(id) => {
                    self.ctl.playlists.remove_track(id, track.provider, &track.id);
                }
            }
        }
        if chosen.is_some() || outside_press || escape {
            self.track_menu = None;
        }
    }
}

/// Пункты меню в порядке показа: действие, значок, подпись. `in_playlist`: номер открытого плейлиста, если трек
/// показан на его экране (тогда есть пункт «Убрать из плейлиста»). Лайк сюда не входит: у него своя кнопка в строке.
fn menu_items(in_playlist: Option<u64>) -> Vec<(MenuAction, Icon, &'static str)> {
    let mut items =
        vec![(MenuAction::AddToQueue, Icon::Queue, "Добавить в очередь"), (MenuAction::AddToPlaylist, Icon::Plus, "Добавить в плейлист")];
    if let Some(id) = in_playlist {
        items.push((MenuAction::RemoveFromPlaylist(id), Icon::Close, "Убрать из плейлиста"));
    }
    items
}

/// Где поставить меню: правым краем к правому краю кнопки, под ней; не хватает места снизу, то над ней;
/// по горизонтали не выходит за окно.
fn menu_position(window: Rect, anchor: Rect, item_count: usize) -> Pos2 {
    let height = item_count as f32 * ITEM_HEIGHT + 2.0 * f32::from(MENU_PADDING);
    let below = anchor.bottom() + ANCHOR_GAP;
    let y = if below + height <= window.bottom() { below } else { (anchor.top() - ANCHOR_GAP - height).max(window.top()) };
    let x = (anchor.right() - MENU_WIDTH).clamp(window.left(), (window.right() - MENU_WIDTH).max(window.left()));
    Pos2::new(x, y)
}

/// Карточка меню Material 3: тональная поверхность, небольшое скругление и тень.
fn m3_frame(theme: &Theme) -> Frame {
    Frame::new()
        .fill(theme.palette.surface_container)
        .corner_radius(theme.shape_radius(ShapeRole::Medium))
        .inner_margin(Margin::symmetric(0, MENU_PADDING))
        .shadow(Shadow { offset: [0, SHADOW_OFFSET_Y], blur: SHADOW_BLUR, spread: 0, color: Color32::from_black_alpha(SHADOW_ALPHA) })
}

/// Карточка меню Liquid Glass: полупрозрачная заливка и тонкая светлая кромка.
fn glass_frame(theme: &Theme) -> Frame {
    let p = &theme.palette;
    Frame::new()
        .fill(with_alpha(p.surface_container, GLASS_FILL_ALPHA))
        .stroke(Stroke::new(1.0, with_alpha(p.on_surface, if p.dark { 28 } else { 22 })))
        .corner_radius(theme.shape_radius(ShapeRole::Medium))
        .inner_margin(Margin::symmetric(0, MENU_PADDING))
}

/// Один пункт: слой наведения/нажатия, значок и подпись. Возвращает true, если по нему кликнули.
fn menu_item(ui: &mut Ui, theme: &Theme, icon: Icon, label: &str) -> bool {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(MENU_WIDTH, ITEM_HEIGHT), Sense::click());
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 0.0, with_alpha(p.on_surface, PRESS_ALPHA));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, with_alpha(p.on_surface, HOVER_ALPHA));
    }
    let icon_center = Pos2::new(rect.left() + ITEM_ICON_INSET + ITEM_ICON_SIZE / 2.0, rect.center().y);
    icons::paint(theme, ui.painter(), icon, icon_center, ITEM_ICON_SIZE, p.on_surface_variant);
    ui.painter().text(
        Pos2::new(rect.left() + ITEM_LABEL_INSET, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme.text_size(TextRole::BodyLarge)),
        p.on_surface,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1000.0, 700.0));

    fn anchor(x: f32, y: f32) -> Rect {
        Rect::from_min_size(Pos2::new(x, y), vec2(36.0, 36.0))
    }

    #[test]
    fn menu_opens_below_button_right_aligned() {
        let a = anchor(900.0, 100.0);
        let pos = menu_position(WINDOW, a, 2);
        assert_eq!(pos.y, a.bottom() + ANCHOR_GAP);
        assert_eq!(pos.x + MENU_WIDTH, a.right());
    }

    #[test]
    fn menu_flips_above_when_no_room_below() {
        let a = anchor(900.0, 660.0);
        let pos = menu_position(WINDOW, a, 2);
        let height = 2.0 * ITEM_HEIGHT + 2.0 * f32::from(MENU_PADDING);
        assert_eq!(pos.y, a.top() - ANCHOR_GAP - height);
        assert!(pos.y + height <= a.top());
    }

    #[test]
    fn menu_stays_inside_window_horizontally() {
        let left = menu_position(WINDOW, anchor(10.0, 100.0), 2);
        assert_eq!(left.x, 0.0, "слишком близко к левому краю: прижимается к нему");
        let right = menu_position(WINDOW, anchor(990.0, 100.0), 2);
        assert!(right.x + MENU_WIDTH <= WINDOW.right());
    }

    #[test]
    fn menu_has_no_like_item_and_remove_only_inside_playlist() {
        let outside: Vec<MenuAction> = menu_items(None).iter().map(|i| i.0).collect();
        assert_eq!(outside, [MenuAction::AddToQueue, MenuAction::AddToPlaylist], "лайк вынесен в кнопку строки");
        let inside: Vec<MenuAction> = menu_items(Some(7)).iter().map(|i| i.0).collect();
        assert_eq!(inside, [MenuAction::AddToQueue, MenuAction::AddToPlaylist, MenuAction::RemoveFromPlaylist(7)]);
    }
}

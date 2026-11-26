//! Действия над треком из строки списка: очередь, плейлист, скачивание, удаление из коллекций и лайк.
//!
//! Строка (`widgets::track_row`) ничего не знает о приложении: нажатия она только записывает во временную память
//! egui (`request_menu`, `request_like`), а лайкнутые треки читает оттуда же (`is_liked`). Запрос лайка хранит ID строки,
//! чтобы на экране понравившихся можно было анимировать удаление именно нажатой строки. Выполняет всё приложение:
//! `McrateApp::sync_row_state` (лайки и состояние скачивания, раз за кадр перед экранами) и
//! `McrateApp::show_track_menu` (меню). Так одни и те же кнопки работают на всех экранах со списками.

use crate::app::{McrateApp, View};
use crate::controller::{DownloadVisual, LikeKey};
use crate::icons::{self, Icon};
use crate::theme::{ShapeRole, TextRole, Theme, with_alpha};
use crate::ui::widgets::status_snackbar;
use crate::ui::playlists::PlaylistDialog;
use crate::ui::ripple;
use crate::ui::thanos::{self, RemovalKind};
use egui::{Align2, Area, Color32, Context, FontId, Frame, Id, Key, Margin, Order, Pos2, Rect, Sense, Shadow, Stroke, Ui, vec2};
use mcrate_core::{ProviderKind, Track};
use std::collections::{HashMap, HashSet};
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
/// Зазор между областью строки (или кнопкой «три точки») и меню.
const ANCHOR_GAP: f32 = 4.0;
/// Прозрачность слоя наведения на пункте (Material 3: 8 % от 255); нажатие рисует `ripple`.
const HOVER_ALPHA: u8 = 20;
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
    row_id: Id,
}

/// Что выбрали в меню.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MenuAction {
    AddToQueue,
    /// Открыть окно выбора плейлиста для этого трека.
    AddToPlaylist,
    /// Сохранить онлайн-трек в постоянную папку загрузок.
    Download,
    /// Убрать трек из понравившихся.
    RemoveFromLiked,
    /// Удалить аудиофайл из постоянной папки загрузок.
    DeleteDownload,
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

/// Где во временной памяти egui лежат состояния анимации скачивания по ключам треков.
fn download_visuals_id() -> Id {
    Id::unique("row_download_visuals")
}

/// Где лежит запрос «поставить/снять лайк», пока приложение его не заберёт.
fn like_request_id() -> Id {
    Id::unique("row_like_request")
}

/// Стоит ли лайк на треке (по множеству, которое приложение выложило в этом кадре).
pub(crate) fn is_liked(ctx: &Context, track: &Track) -> bool {
    ctx.data(|d| d.get_temp::<Arc<HashSet<LikeKey>>>(liked_id())).is_some_and(|keys| keys.contains(&(track.provider, track.id.clone())))
}

/// Снимок состояния скачивания конкретного трека для его строки.
pub(crate) fn download_visual(ctx: &Context, track: &Track) -> Option<DownloadVisual> {
    let key = (track.provider, track.id.clone());
    ctx.data(|d| d.get_temp::<Arc<HashMap<LikeKey, DownloadVisual>>>(download_visuals_id()))?.get(&key).copied()
}

/// Строка просит поставить или снять лайк с `track`; `row_id` нужен для анимации удаления из понравившихся.
pub(crate) fn request_like(ctx: &Context, track: &Track, row_id: Id) {
    ctx.data_mut(|d| d.insert_temp(like_request_id(), (track.clone(), row_id)));
}

/// Строка просит открыть меню для `track` рядом с кнопкой `anchor`.
pub(crate) fn request_menu(ctx: &Context, track: &Track, anchor: Rect, row_id: Id) {
    ctx.data_mut(|d| d.insert_temp(request_id(), (track.clone(), anchor, row_id)));
}

impl McrateApp {
    /// Выполняет запрошенный лайк и выкладывает актуальное множество лайков для строк. Раз за кадр, до отрисовки экранов.
    pub(crate) fn sync_row_state(&mut self, ctx: &Context) {
        let request = ctx.data_mut(|d| {
            let request = d.get_temp::<(Track, Id)>(like_request_id());
            d.remove::<(Track, Id)>(like_request_id());
            request
        });
        if let Some((track, row_id)) = request {
            // На экране понравившихся снятие лайка одновременно удаляет строку.
            // Запускаем тот же Thanos-эффект, что и в контекстном меню, и снимаем лайк
            // только после завершения анимации.
            let remove_from_liked = self.view == View::CollectionLiked && is_liked(ctx, &track);
            if remove_from_liked {
                // Одновременно поддерживаем только один распад: иначе виртуализированный
                // список изменит геометрию под уже захваченным снимком.
                if !thanos::has_active_removal(ctx)
                    && !thanos::start(
                        ctx,
                        row_id,
                        track.clone(),
                        RemovalKind::Liked,
                        self.frame_time,
                        self.theme.settings.reduce_motion,
                    )
                {
                    self.ctl.remove_like(&track);
                }
            } else {
                self.ctl.toggle_like(&track);
            }
        }
        ctx.data_mut(|d| {
            d.insert_temp(liked_id(), self.ctl.liked_keys());
            d.insert_temp(download_visuals_id(), self.ctl.download_visuals());
        });
    }

    /// Открывает запрошенное меню, рисует его поверх интерфейса и выполняет выбранное действие.
    /// Вызывается один раз за кадр после отрисовки экранов.
    pub(crate) fn show_track_menu(&mut self, ctx: &Context, theme: &Theme) {
        let request = ctx.data_mut(|d| {
            let request = d.get_temp::<(Track, Rect, Id)>(request_id());
            d.remove::<(Track, Rect, Id)>(request_id());
            request
        });
        if let Some((track, anchor, row_id)) = request {
            self.track_menu = Some(OpenTrackMenu { track, anchor, row_id });
        }
        let Some(open) = &self.track_menu else { return };
        // Пока распадается одна строка, не запускаем второе удаление: это сохраняет стабильную геометрию виртуального списка.
        let removal = if thanos::has_active_removal(ctx) {
            None
        } else {
            match self.view {
                View::CollectionLiked => Some(RemovalKind::Liked),
                View::CollectionDownloaded => Some(RemovalKind::Downloaded),
                View::CollectionPlaylist => self.open_playlist.map(RemovalKind::Playlist),
                _ => None,
            }
        };
        let downloadable = open.track.provider != ProviderKind::Local && self.view != View::CollectionDownloaded;
        let items = menu_items(downloadable, removal);
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
            let row_id = open.row_id;
            match action {
                MenuAction::AddToQueue => self.ctl.add_to_queue(track),
                MenuAction::AddToPlaylist => self.playlist_dialog = Some(PlaylistDialog::Pick(track)),
                MenuAction::Download => self.ctl.download_track(track),
                MenuAction::RemoveFromLiked => self.begin_track_removal(ctx, row_id, track, RemovalKind::Liked, theme),
                MenuAction::DeleteDownload => self.begin_track_removal(ctx, row_id, track, RemovalKind::Downloaded, theme),
                MenuAction::RemoveFromPlaylist(id) => self.begin_track_removal(ctx, row_id, track, RemovalKind::Playlist(id), theme),
            }
        }
        if chosen.is_some() || outside_press || escape {
            self.track_menu = None;
        }
    }

    /// Показывает ошибки фоновой загрузки; успешное сохранение отмечается анимацией на обложке трека.
    pub(crate) fn show_download_status(&mut self, ctx: &Context, theme: &Theme) {
        let Some(message) = self.ctl.download_status.clone() else { return };
        let dismissed = Area::new(Id::unique("download_status"))
            .order(Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -104.0))
            .show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 32.0).min(560.0));
                status_snackbar(ui, theme, &message)
            })
            .inner;
        if dismissed {
            self.ctl.download_status = None;
        }
    }

    /// Запускает общий распад; если снимок строки недоступен или включено уменьшение движения — удаляет сразу.
    fn begin_track_removal(&mut self, ctx: &Context, row_id: Id, track: Track, kind: RemovalKind, theme: &Theme) {
        if !thanos::start(ctx, row_id, track.clone(), kind, self.frame_time, theme.settings.reduce_motion) {
            self.finish_track_removal(track, kind);
        }
    }

    /// Применяет завершённое удаление к нужному хранилищу; анимация не управляет бизнес-логикой.
    fn finish_track_removal(&mut self, track: Track, kind: RemovalKind) {
        match kind {
            RemovalKind::Liked => {
                self.ctl.remove_like(&track);
            }
            RemovalKind::Downloaded => {
                if let Err(error) = self.ctl.remove_downloaded_track(&track) {
                    self.ctl.download_status = Some(format!("Не удалось удалить скачанный трек: {error}"));
                }
            }
            RemovalKind::Playlist(id) => {
                self.ctl.playlists.remove_track(id, track.provider, &track.id);
            }
        }
    }

    /// Забирает действия, у которых распад завершился, и передаёт их контроллеру.
    pub(crate) fn finish_thanos_removals(&mut self, ctx: &Context, now: f64) {
        for finished in thanos::take_finished(ctx, now) {
            self.finish_track_removal(finished.track, finished.kind);
        }
    }
}

/// Пункты меню в порядке показа. Удаление зависит от коллекции, в которой показан трек.
fn menu_items(downloadable: bool, removal: Option<RemovalKind>) -> Vec<(MenuAction, Icon, &'static str)> {
    let mut items = vec![
        (MenuAction::AddToQueue, Icon::Queue, "Добавить в очередь"),
        (MenuAction::AddToPlaylist, Icon::Plus, "Добавить в плейлист"),
    ];
    if downloadable {
        items.push((MenuAction::Download, Icon::Download, "Скачать трек"));
    }
    match removal {
        Some(RemovalKind::Liked) => items.push((MenuAction::RemoveFromLiked, Icon::Trash, "Убрать из понравившихся")),
        Some(RemovalKind::Downloaded) => items.push((MenuAction::DeleteDownload, Icon::Trash, "Удалить скачанный трек")),
        Some(RemovalKind::Playlist(id)) => items.push((MenuAction::RemoveFromPlaylist(id), Icon::Trash, "Убрать из плейлиста")),
        None => {}
    }
    items
}

/// Где поставить меню: правым краем к правому краю области строки/кнопки, под ней; если снизу не хватает места — над ней;
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
    if resp.hovered() && !resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 0.0, with_alpha(p.on_surface, HOVER_ALPHA));
    }
    ripple::state_layer(ui, theme, ui.painter(), &resp, rect, 0.0);
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
    fn menu_removal_matches_the_current_collection() {
        let local: Vec<MenuAction> = menu_items(false, None).iter().map(|item| item.0).collect();
        assert_eq!(local, [MenuAction::AddToQueue, MenuAction::AddToPlaylist], "локальный трек не скачивается повторно");
        let online: Vec<MenuAction> = menu_items(true, None).iter().map(|item| item.0).collect();
        assert_eq!(online, [MenuAction::AddToQueue, MenuAction::AddToPlaylist, MenuAction::Download]);
        let liked: Vec<MenuAction> = menu_items(true, Some(RemovalKind::Liked)).iter().map(|item| item.0).collect();
        assert_eq!(liked, [MenuAction::AddToQueue, MenuAction::AddToPlaylist, MenuAction::Download, MenuAction::RemoveFromLiked]);
        let downloaded: Vec<MenuAction> = menu_items(false, Some(RemovalKind::Downloaded)).iter().map(|item| item.0).collect();
        assert_eq!(downloaded, [MenuAction::AddToQueue, MenuAction::AddToPlaylist, MenuAction::DeleteDownload]);
        let playlist: Vec<MenuAction> = menu_items(true, Some(RemovalKind::Playlist(7))).iter().map(|item| item.0).collect();
        assert_eq!(playlist, [MenuAction::AddToQueue, MenuAction::AddToPlaylist, MenuAction::Download, MenuAction::RemoveFromPlaylist(7)]);
    }
}

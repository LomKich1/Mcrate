//! Окна плейлистов: выбор плейлиста для трека («Добавить в плейлист»), создание, переименование, удаление.
//! Окно живёт в `McrateApp::playlist_dialog`; рисует и выполняет его `show_playlist_dialog` (раз за кадр).
//! Экраны плейлистов лежат в `ui/collection.rs`.

use super::components::{ConfirmDialog, DialogChoice, TextDialog, confirm_dialog, modal_card, text_dialog};
use super::format::tracks_word;
use super::ripple;
use super::widgets::{ButtonKind, SegmentPos, collage_sample, pill_button, playlist_cover, ripple_corner, segment_radius};
use crate::app::{McrateApp, View};
use crate::icons::Icon;
use crate::theme::{TextRole, Theme, with_alpha};
use egui::{Align, Align2, Context, FontId, Layout, Pos2, Rect, RichText, ScrollArea, Sense, Ui, Vec2, vec2};
use mcrate_core::{Playlist, Track};

/// Ширина окна выбора плейлиста: окно заметное, но не на весь экран (Material 3 допускает до 560 px).
const PICKER_WIDTH: f32 = 440.0;
/// Высота списка плейлистов в окне. Фиксированная, чтобы окно не прыгало, пока плейлистов 1, 3 или 30.
const PICKER_LIST_HEIGHT: f32 = 320.0;
/// Высота строки плейлиста в окне выбора.
const PICKER_ROW_HEIGHT: f32 = 60.0;
/// Отступ значка и текста от краёв строки.
const PICKER_ROW_INSET: f32 = 12.0;
/// На сколько больше `PICKER_ROW_INSET` отступ в сегментированной строке (углы крупнее).
const SEGMENT_EXTRA_INSET: f32 = 4.0;
/// Сторона обложки плейлиста в строке и скругление её внешних углов.
const ROW_COVER_SIZE: f32 = 44.0;
const ROW_COVER_RADIUS: f32 = 8.0;
/// Прозрачность слоя наведения на строке (8 % от 255); нажатие рисует `ripple`.
const HOVER_ALPHA: u8 = 20;

/// Какое окно плейлистов открыто.
pub(crate) enum PlaylistDialog {
    /// Выбор плейлиста, в который добавить этот трек.
    Pick(Track),
    /// Название нового плейлиста; если задан `then_add`, трек сразу добавляется в созданный плейлист.
    Create { text: String, then_add: Option<Track> },
    /// Новое название плейлиста с номером `id`.
    Rename { id: u64, text: String },
    /// Подтверждение удаления плейлиста с номером `id`.
    Delete(u64),
}

/// Что выбрали в окне выбора плейлиста.
enum PickChoice {
    Add(u64),
    New,
    Close,
}

impl McrateApp {
    /// Рисует открытое окно плейлистов и выполняет выбор. Вызывается раз за кадр после экранов.
    pub(crate) fn show_playlist_dialog(&mut self, ctx: &Context, theme: &Theme) {
        let Some(dialog) = self.playlist_dialog.take() else { return };
        self.playlist_dialog = match dialog {
            PlaylistDialog::Pick(track) => self.pick_dialog(ctx, theme, track),
            PlaylistDialog::Create { text, then_add } => self.create_dialog(ctx, theme, text, then_add),
            PlaylistDialog::Rename { id, text } => self.rename_dialog(ctx, theme, id, text),
            PlaylistDialog::Delete(id) => self.delete_dialog(ctx, theme, id),
        };
    }

    /// Окно выбора плейлиста. Возвращает, что показывать дальше (`None` = закрыто).
    fn pick_dialog(&mut self, ctx: &Context, theme: &Theme, track: Track) -> Option<PlaylistDialog> {
        let playlists = self.ctl.playlists.list();
        let out = modal_card(ctx, theme, ("playlist_pick", PICKER_WIDTH), |ui| pick_body(ui, theme, playlists, &track));
        match out.inner {
            Some(PickChoice::Add(id)) => {
                self.ctl.playlists.add_track(id, &track);
                None
            }
            Some(PickChoice::New) => Some(PlaylistDialog::Create { text: String::new(), then_add: Some(track) }),
            Some(PickChoice::Close) => None,
            None if out.dismissed => None,
            None => Some(PlaylistDialog::Pick(track)),
        }
    }

    fn create_dialog(&mut self, ctx: &Context, theme: &Theme, mut text: String, then_add: Option<Track>) -> Option<PlaylistDialog> {
        let spec = TextDialog {
            key: "playlist_create", title: "Новый плейлист", hint: "Название плейлиста", confirm_label: "Создать"
        };
        match text_dialog(ctx, theme, &spec, &mut text) {
            Some(DialogChoice::Confirm) => {
                if let (Some(id), Some(track)) = (self.ctl.playlists.create(&text), &then_add) {
                    self.ctl.playlists.add_track(id, track);
                }
                None
            }
            Some(DialogChoice::Cancel) => None,
            None => Some(PlaylistDialog::Create { text, then_add }),
        }
    }

    fn rename_dialog(&mut self, ctx: &Context, theme: &Theme, id: u64, mut text: String) -> Option<PlaylistDialog> {
        let spec =
            TextDialog {
                key: "playlist_rename", title: "Переименовать плейлист", hint: "Название плейлиста", confirm_label: "Сохранить"
            };
        match text_dialog(ctx, theme, &spec, &mut text) {
            Some(DialogChoice::Confirm) => {
                self.ctl.playlists.rename(id, &text);
                None
            }
            Some(DialogChoice::Cancel) => None,
            None => Some(PlaylistDialog::Rename { id, text }),
        }
    }

    fn delete_dialog(&mut self, ctx: &Context, theme: &Theme, id: u64) -> Option<PlaylistDialog> {
        let name = self.ctl.playlists.get(id).map(|p| p.name.clone())?;
        let text = format!("Плейлист «{name}» будет удалён. Сами треки останутся в библиотеке.");
        let spec = ConfirmDialog {
            key: "playlist_delete", title: "Удалить плейлист?", text: &text, confirm_label: "Удалить"
        };
        match confirm_dialog(ctx, theme, &spec) {
            Some(DialogChoice::Confirm) => {
                self.ctl.playlists.delete(id);
                if self.open_playlist == Some(id) {
                    self.open_playlist = None;
                    self.view = View::CollectionPlaylists;
                }
                None
            }
            Some(DialogChoice::Cancel) => None,
            None => Some(PlaylistDialog::Delete(id)),
        }
    }
}

/// Содержимое окна выбора: заголовок, трек, список плейлистов и кнопки. Кнопка «Новый плейлист» есть всегда.
fn pick_body(ui: &mut Ui, theme: &Theme, playlists: &[Playlist], track: &Track) -> Option<PickChoice> {
    let p = &theme.palette;
    ui.label(RichText::new("Добавить в плейлист").size(theme.text_size(TextRole::HeadlineSmall)).color(p.on_surface));
    ui.add_space(4.0);
    ui.add(
        egui::Label::new(RichText::new(&track.title).size(theme.text_size(TextRole::BodyMedium)).color(p.on_surface_variant)).truncate(),
    );
    ui.add_space(12.0);

    let mut chosen = None;
    ScrollArea::vertical().min_scrolled_height(PICKER_LIST_HEIGHT).max_height(PICKER_LIST_HEIGHT).auto_shrink([false, false]).show(
        ui,
        |ui| {
            if playlists.is_empty() {
                ui.add_space(PICKER_LIST_HEIGHT * 0.3);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Плейлистов пока нет").size(theme.text_size(TextRole::TitleMedium)).color(p.on_surface));
                    ui.label(
                        RichText::new("Создайте первый, и трек сразу попадёт в него.")
                            .size(theme.text_size(TextRole::BodyMedium))
                            .color(p.on_surface_variant),
                    );
                });
                return;
            }
            ui.spacing_mut().item_spacing.y = 0.0;
            for playlist in playlists {
                let already = playlist.tracks.iter().any(|t| t.provider == track.provider && t.id == track.id);
                if playlist_row(ui, theme, playlist, already.then_some("Уже добавлен"), None) {
                    chosen = Some(PickChoice::Add(playlist.id));
                }
            }
        },
    );
    ui.add_space(16.0);
    let footer = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        // В right_to_left первой рисуется самая правая кнопка.
        if pill_button(ui, theme, "Закрыть", None, ButtonKind::Plain).clicked() {
            return Some(PickChoice::Close);
        }
        if pill_button(ui, theme, "Новый плейлист", Some(Icon::Plus), ButtonKind::Tonal).clicked() {
            return Some(PickChoice::New);
        }
        None
    });
    chosen.or(footer.inner)
}

/// Строка плейлиста (в окне выбора и в списке плейлистов): значок, название и «N треков». Если задана `note`, строка
/// неактивна и справа написана эта заметка («Уже добавлен»). `segment`: место строки в сегментированном списке
/// (Material 3: плашка с зазором и крупными углами у краёв группы); `None`: плоская строка, как в окне выбора.
/// Возвращает true, если по активной строке кликнули.
pub(crate) fn playlist_row(ui: &mut Ui, theme: &Theme, playlist: &Playlist, note: Option<&str>, segment: Option<SegmentPos>) -> bool {
    let already = note.is_some();
    let p = &theme.palette;
    let sense = if already { Sense::hover() } else { Sense::click() };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), PICKER_ROW_HEIGHT), sense);
    let segment = segment.filter(|_| !theme.glass_active());
    let radius = match segment {
        Some(pos) => segment_radius(theme, pos),
        None => egui::CornerRadius::same(theme.shape_radius(crate::theme::ShapeRole::Medium)),
    };
    if segment.is_some() {
        ui.painter().rect_filled(rect, radius, p.surface_container_high);
    }
    if !already {
        if resp.hovered() && !resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(rect, radius, with_alpha(p.on_surface, HOVER_ALPHA));
        }
        ripple::state_layer(ui, theme, ui.painter(), &resp, rect, ripple_corner(radius));
    }
    // У сегмента крупные углы: содержимое отодвигаем, чтобы обложка не упиралась в скругление.
    let inset = PICKER_ROW_INSET + if segment.is_some() { SEGMENT_EXTRA_INSET } else { 0.0 };
    let faded = |c: egui::Color32| if already { with_alpha(c, 110) } else { c };
    let cover = Rect::from_center_size(
        Pos2::new(rect.left() + inset + ROW_COVER_SIZE / 2.0, rect.center().y),
        Vec2::splat(ROW_COVER_SIZE),
    );
    playlist_cover(ui, theme, cover, (&playlist.name, &collage_sample(&playlist.tracks)), ROW_COVER_RADIUS);
    let text_left = cover.right() + 14.0;
    let clipped = ui
        .painter()
        .with_clip_rect(egui::Rect::from_min_max(Pos2::new(text_left, rect.top()), Pos2::new(rect.right() - 90.0, rect.bottom())));
    clipped.text(
        Pos2::new(text_left, rect.center().y - 10.0),
        Align2::LEFT_CENTER,
        &playlist.name,
        FontId::proportional(theme.text_size(TextRole::TitleMedium)),
        faded(p.on_surface),
    );
    clipped.text(
        Pos2::new(text_left, rect.center().y + 12.0),
        Align2::LEFT_CENTER,
        tracks_count_text(playlist.tracks.len()),
        FontId::proportional(theme.text_size(TextRole::BodyMedium)),
        faded(p.on_surface_variant),
    );
    if let Some(note) = note {
        ui.painter().text(
            Pos2::new(rect.right() - inset, rect.center().y),
            Align2::RIGHT_CENTER,
            note,
            FontId::proportional(theme.text_size(TextRole::LabelMedium)),
            p.on_surface_variant,
        );
    }
    !already && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// «1 трек», «3 трека», «12 треков».
pub(crate) fn tracks_count_text(count: usize) -> String {
    format!("{count} {}", tracks_word(count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_count_text_uses_russian_plural() {
        assert_eq!(tracks_count_text(0), "0 треков");
        assert_eq!(tracks_count_text(1), "1 трек");
        assert_eq!(tracks_count_text(3), "3 трека");
        assert_eq!(tracks_count_text(12), "12 треков");
    }
}

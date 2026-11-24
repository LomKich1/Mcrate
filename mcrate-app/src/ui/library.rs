//! Экран «Ваши загрузки» (раньше «Моя музыка»): свои файлы (LocalProvider). Подэкран раздела «Библиотека».

use egui::{Align, RichText, Sense, Ui, Vec2};

use super::components::{
    EmptyText, SIDE_COVER_SIZE, SplitPage, TitleRow, empty_state_top_space, empty_state_with, narrow_title_shown, split_page,
    title_scroll_page, title_search_row, track_list,
};
use super::format::tracks_word;
use super::layout::{TRACK_LIST_MAX_WIDTH, centered_column, rows_in_view};
use super::widgets::{ButtonKind, cover_tile, error_banner, pill_button, status_snackbar};
use crate::app::{McrateApp, View};
use crate::icons::Icon;
use crate::theme::Theme;
use mcrate_core::Track;

/// Расширения для диалога выбора файлов. Список такой же, как в LocalProvider.
const AUDIO_EXT: [&str; 6] = ["mp3", "flac", "ogg", "wav", "m4a", "aac"];
/// Ключ общей прокрутки узкой страницы «Ваших загрузок» (по нему шапка читает смещение).
const UPLOADS_SCROLL_KEY: &str = "uploads_page_scroll";

/// Что показывает левая панель «Ваших загрузок» (`uploads_playlist_panel`).
#[derive(Clone, Copy)]
struct PanelInfo {
    /// Всего треков в коллекции.
    total: usize,
    /// Сколько осталось после поиска: от этого зависят кнопки «Перемешать» и «Слушать».
    filtered: usize,
    /// Сторона обложки в px.
    cover_size: f32,
    /// Вертикальная раскладка (широкое окно) или обложка слева от текста (узкое).
    vertical: bool,
}

impl McrateApp {
    pub(crate) fn library_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, f32::INFINITY, |ui| {
            let p = &theme.palette;

            self.uploads_header(ui, theme);
            ui.add_space(10.0);

            if let Some(status) = self.ctl.status.clone() {
                if theme.glass_active() {
                    ui.label(RichText::new(status).size(13.0).color(p.on_surface_variant));
                } else if status_snackbar(ui, theme, &status) {
                    self.ctl.status = None;
                }
                ui.add_space(8.0);
            }

            if self.ctl.library.is_empty() {
                self.empty_library(ui, theme);
                return;
            }

            let count = self.filtered.len();
            let total = self.ctl.library.len();
            let current_id = self.ctl.now.as_ref().map(|t| t.id.clone());
            let playing = self.ctl.is_playing();

            // Компоновка как у страницы плейлиста: фиксированная карточка коллекции слева,
            // самостоятельный прокручиваемый список справа. На узком окне карточка стоит над списком
            // и прокручивается вместе с ним (одна область прокрутки на обоих).
            split_page(ui, Align::Min, 18.0, |ui, page| {
                if page.wide {
                    page.side(ui, |ui| {
                        self.uploads_playlist_panel(
                            ui,
                            theme,
                            PanelInfo { total, filtered: count, cover_size: SIDE_COVER_SIZE, vertical: true },
                        )
                    });
                    page.main(ui, |ui| self.uploads_track_list(ui, theme, count, &current_id, playing));
                } else {
                    self.uploads_narrow_page(ui, theme, page, (total, count), &current_id, playing);
                }
            });
        });
    }

    /// Шапка как на главной: «назад», заголовок с подписью, поиск и кнопки «Папка»/«Файлы» на одной линии.
    /// Название в шапке нужно только тогда, когда карточки коллекции с названием не видно: на узком окне
    /// оно появляется по мере того, как карточка уезжает вверх; на широком карточка всегда на виду.
    fn uploads_header(&mut self, ui: &mut Ui, theme: &Theme) {
        let title_shown = self.header_title_shown(ui);
        let ctl = &mut self.ctl;
        let header = TitleRow {
            title: "Ваши загрузки",
            subtitle: "Локальная коллекция",
            back: true,
            hint: "Поиск по названию и исполнителю",
            trailing_w: 250.0,
            centered_search: false,
            title_shown,
        };
        let (_, back) = title_search_row(ui, theme, &header, &mut self.search, &mut |ui| {
            // right_to_left: первой добавленная кнопка окажется правее.
            if pill_button(ui, theme, "Файлы", Some(Icon::Plus), ButtonKind::Filled).clicked()
                && let Some(files) = rfd::FileDialog::new().add_filter("Аудио", &AUDIO_EXT).pick_files()
            {
                ctl.add_paths(&files);
            }
            if pill_button(ui, theme, "Папка", Some(Icon::Folder), ButtonKind::Tonal).clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                ctl.add_paths(&[dir]);
            }
        });
        if back {
            self.view = View::Collection;
        }
    }

    /// Насколько показывать название в шапке (0..1): см. `TitleRow::title_shown`.
    fn header_title_shown(&self, ui: &Ui) -> f32 {
        if self.ctl.library.is_empty() {
            return 1.0; // карточки нет, название экрана остаётся в шапке
        }
        narrow_title_shown(ui, UPLOADS_SCROLL_KEY)
    }

    /// Левая информация о локальной коллекции (обложка, описание и действия).
    fn uploads_playlist_panel(&mut self, ui: &mut Ui, theme: &Theme, panel: PanelInfo) {
        let PanelInfo { total, filtered, cover_size, vertical } = panel;
        let p = &theme.palette;
        if vertical {
            ui.add_space(12.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(cover_size), Sense::hover());
            cover_tile(ui, theme, rect, "mcrate-local-uploads", theme.shape_radius(crate::theme::ShapeRole::Large) as f32);
            ui.add_space(18.0);
            ui.label(RichText::new("Ваши загрузки").size(theme.text_size(crate::theme::TextRole::HeadlineSmall)).strong());
            ui.add_space(5.0);
            ui.label(RichText::new("Локальная коллекция").size(13.0).color(p.on_surface_variant));
            ui.add_space(8.0);
            let meta = format!("{total} {} · файлы с устройства", tracks_word(total));
            ui.label(RichText::new(meta).size(13.0).color(p.on_surface_variant));
            ui.add_space(12.0);
            ui.add(
                egui::Label::new(
                    RichText::new("Музыка, добавленная из файлов и папок на этом устройстве.").size(13.0).color(p.on_surface_variant),
                )
                .wrap(),
            );
            ui.add_space(18.0);
        } else {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(cover_size), Sense::hover());
                cover_tile(ui, theme, rect, "mcrate-local-uploads", theme.shape_radius(crate::theme::ShapeRole::Medium) as f32);
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Ваши загрузки").size(theme.text_size(crate::theme::TextRole::TitleLarge)).strong());
                    ui.add_space(3.0);
                    ui.label(RichText::new("Локальная коллекция").size(13.0).color(p.on_surface_variant));
                    let meta = format!("{total} {} · файлы с устройства", tracks_word(total));
                    ui.label(RichText::new(meta).size(13.0).color(p.on_surface_variant));
                });
            });
            ui.add_space(10.0);
        }

        if filtered > 0 {
            let (shuffle, play) = ui
                .horizontal(|ui| {
                    let shuffle = pill_button(ui, theme, "Перемешать", Some(Icon::Shuffle), ButtonKind::Tonal).clicked();
                    let play = pill_button(ui, theme, "Слушать", Some(Icon::Play), ButtonKind::Filled).clicked();
                    (shuffle, play)
                })
                .inner;
            // Список копируем только по клику, а не на каждом кадре.
            if shuffle {
                self.ctl.toggle_shuffle();
                self.ctl.play_list(self.filtered.clone(), 0);
            }
            if play {
                self.ctl.play_list(self.filtered.clone(), 0);
            }
        }
    }

    /// Самостоятельный список с собственной прокруткой; ширина строк ограничена колонкой.
    fn uploads_track_list(&mut self, ui: &mut Ui, theme: &Theme, count: usize, current_id: &Option<String>, playing: bool) {
        let p = &theme.palette;
        uploads_list_header(ui, theme, count);

        if count == 0 {
            ui.label(RichText::new("Ничего не найдено").color(p.on_surface_variant));
            return;
        }

        let mut clicked: Option<usize> = None;
        let row_height = theme.list_item_height(2);
        egui::ScrollArea::vertical().id_salt("uploads_playlist_tracks").auto_shrink([false, false]).show_rows(
            ui,
            row_height,
            count,
            |ui, range| {
                centered_column(ui, TRACK_LIST_MAX_WIDTH, |ui| {
                    let rows = range.map(|i| (i, &self.filtered[i]));
                    let is_current = |track: &Track| current_id.as_deref() == Some(track.id.as_str());
                    if let Some(i) = track_list(ui, theme, rows, is_current, playing) {
                        clicked = Some(i);
                    }
                });
            },
        );

        if let Some(i) = clicked {
            let items = self.filtered.clone();
            self.ctl.play_list(items, i);
        }
    }

    /// Узкое окно: карточка коллекции и список идут друг под другом и едут вместе, одной прокруткой.
    /// Строки списка рисуются только в видимой части (`uploads_rows_in_view`).
    fn uploads_narrow_page(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        page: &SplitPage,
        (total, count): (usize, usize),
        current_id: &Option<String>,
        playing: bool,
    ) {
        let bleed = self.bleed.below_header();
        let mut clicked: Option<usize> = None;
        title_scroll_page(ui, bleed, self.bottom_inset, UPLOADS_SCROLL_KEY, |ui| {
            page.side(ui, |ui| {
                self.uploads_playlist_panel(ui, theme, PanelInfo { total, filtered: count, cover_size: 96.0, vertical: false })
            });
            page.main(ui, |ui| {
                uploads_list_header(ui, theme, count);
                if count == 0 {
                    ui.label(RichText::new("Ничего не найдено").color(theme.palette.on_surface_variant));
                } else {
                    clicked = self.uploads_rows_in_view(ui, theme, current_id, playing);
                }
            });
        });
        if let Some(i) = clicked {
            let items = self.filtered.clone();
            self.ctl.play_list(items, i);
        }
    }

    /// Все строки отфильтрованного списка в общей прокрутке: рисуются только видимые (`layout::rows_in_view`).
    /// Возвращает номер строки, по которой кликнули.
    fn uploads_rows_in_view(&self, ui: &mut Ui, theme: &Theme, current_id: &Option<String>, playing: bool) -> Option<usize> {
        centered_column(ui, TRACK_LIST_MAX_WIDTH, |ui| {
            rows_in_view(ui, self.filtered.len(), theme.list_item_height(2), |ui, range| {
                let rows = range.map(|i| (i, &self.filtered[i]));
                let is_current = |track: &Track| current_id.as_deref() == Some(track.id.as_str());
                track_list(ui, theme, rows, is_current, playing)
            })
        })
    }

    fn empty_library(&mut self, ui: &mut Ui, theme: &Theme) {
        let top = empty_state_top_space(ui.available_height(), self.bottom_inset);
        let audio_error = self.ctl.audio_error.as_deref();
        empty_state_with(
            ui,
            theme,
            top,
            EmptyText {
                icon: Icon::Note,
                headline: "Здесь пока пусто",
                text: "Перетащите аудиофайлы или папку в окно\nили нажмите «Файлы» / «Папка»",
            },
            |ui| {
                if let Some(err) = audio_error {
                    ui.add_space(10.0);
                    error_banner(ui, theme, &format!("Звук недоступен: {err}"));
                }
            },
        );
    }
}

/// Заголовок списка: «Треки» и число найденных.
fn uploads_list_header(ui: &mut Ui, theme: &Theme, count: usize) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Треки").size(theme.text_size(crate::theme::TextRole::TitleLarge)).strong());
        ui.add_space(8.0);
        ui.label(RichText::new(count.to_string()).size(13.0).color(theme.palette.on_surface_variant));
    });
    ui.add_space(8.0);
}

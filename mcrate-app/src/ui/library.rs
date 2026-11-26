//! Экраны коллекций треков «Ваши загрузки» и «Скачанные».
//! Оба используют одну компоновку и общий список; отличаются источником данных и меткой сервиса на обложке.

use egui::{Align, Layout, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2};

use super::components::{
    EmptyText, SIDE_COVER_SIZE, SplitPage, TitleRow, downloaded_track_list, empty_state, empty_state_top_space, empty_state_with,
    narrow_title_shown, split_page, title_scroll_page, title_search_row, track_list,
};
use super::format::tracks_word;
use super::layout::{TRACK_LIST_MAX_WIDTH, centered_column, rows_in_view, scroll_end_padding};
use super::window_layout::EDGE;
use super::widgets::{ButtonKind, apply_list_gap, cover_tile, error_banner, pill_button, status_snackbar};
use crate::app::{McrateApp, View};
use crate::icons::Icon;
use crate::theme::Theme;
use mcrate_core::{ProviderKind, Track};

/// Расширения для диалога выбора файлов. Список такой же, как в LocalProvider.
const AUDIO_EXT: [&str; 6] = ["mp3", "flac", "ogg", "wav", "m4a", "aac"];
/// Ключ общей прокрутки узкой страницы «Ваши загрузки».
const UPLOADS_SCROLL_KEY: &str = "uploads_page_scroll";
/// Ключ общей прокрутки узкой страницы «Скачанные».
const DOWNLOADS_SCROLL_KEY: &str = "downloaded_page_scroll";

/// Источник треков, для которого строится общая страница коллекции.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TrackCollectionKind {
    Local,
    Downloaded,
}

impl TrackCollectionKind {
    fn title(self) -> &'static str {
        match self {
            Self::Local => "Ваши загрузки",
            Self::Downloaded => "Скачанные",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Self::Local => "Локальная коллекция",
            Self::Downloaded => "Сохранённые онлайн-треки",
        }
    }

    fn scroll_key(self) -> &'static str {
        match self {
            Self::Local => UPLOADS_SCROLL_KEY,
            Self::Downloaded => DOWNLOADS_SCROLL_KEY,
        }
    }

    fn has_source_badges(self) -> bool {
        self == Self::Downloaded
    }
}

/// Что показывает боковая карточка коллекции.
#[derive(Clone, Copy)]
struct PanelInfo {
    /// Всего треков в коллекции.
    total: usize,
    /// Сколько осталось после поиска: от этого зависят кнопки воспроизведения.
    filtered: usize,
    /// Сторона обложки в px.
    cover_size: f32,
    /// Вертикальная раскладка (широкое окно) или обложка слева от текста (узкое).
    vertical: bool,
    kind: TrackCollectionKind,
}

impl McrateApp {
    pub(crate) fn library_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.track_collection_view(ui, theme, TrackCollectionKind::Local);
    }

    pub(crate) fn collection_downloaded_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.track_collection_view(ui, theme, TrackCollectionKind::Downloaded);
    }

    /// Общий экран локальной и онлайн-коллекции: левая карточка, поиск и виртуализированный список.
    fn track_collection_view(&mut self, ui: &mut Ui, theme: &Theme, kind: TrackCollectionKind) {
        centered_column(ui, f32::INFINITY, |ui| {
            let p = &theme.palette;
            self.track_collection_header(ui, theme, kind);
            ui.add_space(10.0);

            if kind == TrackCollectionKind::Local
                && let Some(status) = self.ctl.status.clone()
            {
                if theme.glass_active() {
                    ui.label(RichText::new(status).size(13.0).color(p.on_surface_variant));
                } else if status_snackbar(ui, theme, &status) {
                    self.ctl.status = None;
                }
                ui.add_space(8.0);
            }

            let total = match kind {
                TrackCollectionKind::Local => self.ctl.library.len(),
                TrackCollectionKind::Downloaded => self.ctl.downloaded.len(),
            };
            if total == 0 {
                match kind {
                    TrackCollectionKind::Local => self.empty_library(ui, theme),
                    TrackCollectionKind::Downloaded => self.empty_downloads(ui, theme),
                }
                return;
            }

            let count = self.filtered.len();
            let current = self.ctl.now.as_ref().map(|track| (track.id.clone(), track.provider));
            let playing = self.ctl.is_playing();
            let mut clicked = None;
            split_page(ui, Align::Min, 18.0, |ui, page| {
                if page.wide {
                    page.side(ui, |ui| {
                        self.track_collection_panel(
                            ui,
                            theme,
                            PanelInfo { total, filtered: count, cover_size: SIDE_COVER_SIZE, vertical: true, kind },
                        )
                    });
                    page.main(ui, |ui| {
                        clicked = self.track_collection_list(ui, theme, count, current.as_ref(), playing, kind);
                    });
                } else {
                    clicked = self.track_collection_narrow_page(ui, theme, page, (total, count), current.as_ref(), playing, kind);
                }
            });
            if let Some(index) = clicked {
                self.ctl.play_list(self.filtered.clone(), index);
            }
        });
    }

    /// Шапка поиска; у локальной коллекции справа остаются кнопки выбора файлов и папки.
    fn track_collection_header(&mut self, ui: &mut Ui, theme: &Theme, kind: TrackCollectionKind) {
        let has_items = match kind {
            TrackCollectionKind::Local => !self.ctl.library.is_empty(),
            TrackCollectionKind::Downloaded => !self.ctl.downloaded.is_empty(),
        };
        let title_shown = if has_items { narrow_title_shown(ui, kind.scroll_key()) } else { 1.0 };
        let ctl = &mut self.ctl;
        let header = TitleRow {
            title: kind.title(),
            subtitle: kind.subtitle(),
            back: true,
            hint: "Поиск по названию и исполнителю",
            trailing_w: if kind == TrackCollectionKind::Local { 250.0 } else { 0.0 },
            centered_search: false,
            title_shown,
        };
        let (_, back) = title_search_row(ui, theme, &header, &mut self.search, &mut |ui| {
            if kind != TrackCollectionKind::Local {
                return;
            }
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

    /// Левая карточка коллекции: обложка, подписи, число треков и действия воспроизведения.
    fn track_collection_panel(&mut self, ui: &mut Ui, theme: &Theme, panel: PanelInfo) {
        let PanelInfo { total, filtered, cover_size, vertical, kind } = panel;
        let p = &theme.palette;
        let (cover_key, summary, description) = match kind {
            TrackCollectionKind::Local => (
                "mcrate-local-uploads",
                "файлы с устройства",
                "Музыка, добавленная из файлов и папок на этом устройстве.",
            ),
            TrackCollectionKind::Downloaded => (
                "mcrate-downloaded-tracks",
                "сохранено из сервисов",
                "Онлайн-треки, сохранённые на устройстве и доступные без повторной загрузки.",
            ),
        };
        if vertical {
            ui.add_space(12.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(cover_size), Sense::hover());
            cover_tile(ui, theme, rect, cover_key, theme.shape_radius(crate::theme::ShapeRole::Large) as f32);
            ui.add_space(18.0);
            ui.label(RichText::new(kind.title()).size(theme.text_size(crate::theme::TextRole::HeadlineSmall)).strong());
            ui.add_space(5.0);
            ui.label(RichText::new(kind.subtitle()).size(13.0).color(p.on_surface_variant));
            ui.add_space(8.0);
            let meta = format!("{total} {} · {summary}", tracks_word(total));
            ui.label(RichText::new(meta).size(13.0).color(p.on_surface_variant));
            ui.add_space(12.0);
            ui.add(egui::Label::new(RichText::new(description).size(13.0).color(p.on_surface_variant)).wrap());
            ui.add_space(18.0);
        } else {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(cover_size), Sense::hover());
                cover_tile(ui, theme, rect, cover_key, theme.shape_radius(crate::theme::ShapeRole::Medium) as f32);
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(kind.title()).size(theme.text_size(crate::theme::TextRole::TitleLarge)).strong());
                    ui.add_space(3.0);
                    ui.label(RichText::new(kind.subtitle()).size(13.0).color(p.on_surface_variant));
                    let meta = format!("{total} {} · {summary}", tracks_word(total));
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
            if shuffle {
                self.ctl.toggle_shuffle();
                self.ctl.play_list(self.filtered.clone(), 0);
            }
            if play {
                self.ctl.play_list(self.filtered.clone(), 0);
            }
        }
    }

    /// Отдельная прокрутка треков в правой колонке широкой страницы.
    fn track_collection_list(
        &self,
        ui: &mut Ui,
        theme: &Theme,
        count: usize,
        current: Option<&(String, ProviderKind)>,
        playing: bool,
        kind: TrackCollectionKind,
    ) -> Option<usize> {
        collection_list_header(ui, theme, count);
        if count == 0 {
            ui.label(RichText::new("Ничего не найдено").color(theme.palette.on_surface_variant));
            return None;
        }
        let end_inset = self.bottom_inset;
        // Список доходит до нижнего края панели под мини-плеером; нижний запас позволяет прокрутить последнюю строку выше него.
        let pane_bottom = ui.ctx().content_rect().bottom() - EDGE;
        let free = ui.available_rect_before_wrap();
        let area = Rect::from_min_max(free.min, pos2(free.max.x, pane_bottom.max(free.min.y)));
        let mut clicked = None;
        ui.scope_builder(UiBuilder::new().max_rect(area).layout(Layout::top_down(Align::Min)), |ui| {
            let scroll_id = match kind {
                TrackCollectionKind::Local => "uploads_playlist_tracks",
                TrackCollectionKind::Downloaded => "downloaded_tracks",
            };
            egui::ScrollArea::vertical().id_salt(scroll_id).auto_shrink([false, false]).wheel_scroll_multiplier(egui::vec2(1.0, 2.0)).show(ui, |ui| {
                clicked = self.track_collection_rows(ui, theme, &self.filtered, current, playing, kind);
                if !theme.glass_active() {
                    ui.add_space(10.0);
                }
                scroll_end_padding(ui, end_inset);
            });
        });
        clicked
    }

    /// Узкое окно: карточка и список прокручиваются вместе, заголовок появляется в верхней строке.
    fn track_collection_narrow_page(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        page: &SplitPage,
        (total, count): (usize, usize),
        current: Option<&(String, ProviderKind)>,
        playing: bool,
        kind: TrackCollectionKind,
    ) -> Option<usize> {
        let bleed = self.bleed.below_header();
        let mut clicked = None;
        title_scroll_page(ui, bleed, self.bottom_inset, kind.scroll_key(), |ui| {
            page.side(ui, |ui| {
                self.track_collection_panel(ui, theme, PanelInfo { total, filtered: count, cover_size: 96.0, vertical: false, kind })
            });
            page.main(ui, |ui| {
                collection_list_header(ui, theme, count);
                if count == 0 {
                    ui.label(RichText::new("Ничего не найдено").color(theme.palette.on_surface_variant));
                } else {
                    clicked = self.track_collection_rows(ui, theme, &self.filtered, current, playing, kind);
                }
            });
        });
        clicked
    }

    /// Видимые строки коллекции; `rows_in_view` резервирует высоту полного списка, не создавая невидимые виджеты.
    fn track_collection_rows(
        &self,
        ui: &mut Ui,
        theme: &Theme,
        items: &[Track],
        current: Option<&(String, ProviderKind)>,
        playing: bool,
        kind: TrackCollectionKind,
    ) -> Option<usize> {
        centered_column(ui, TRACK_LIST_MAX_WIDTH, |ui| {
            apply_list_gap(ui, theme);
            rows_in_view(ui, items.len(), theme.list_item_height(2), |ui, range| {
                let rows = range.map(|index| (index, &items[index]));
                let is_current = |track: &Track| current.is_some_and(|(id, provider)| track.id == *id && track.provider == *provider);
                if kind.has_source_badges() {
                    downloaded_track_list(ui, theme, rows, items.len(), is_current, playing)
                } else {
                    track_list(ui, theme, rows, items.len(), is_current, playing)
                }
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

    fn empty_downloads(&self, ui: &mut Ui, theme: &Theme) {
        let top = empty_state_top_space(ui.available_height(), self.bottom_inset);
        empty_state(
            ui,
            theme,
            top,
            Icon::Download,
            "Скачанных треков пока нет",
            "Сохраните онлайн-трек через меню «три точки» — он появится здесь.",
        );
    }
}

/// Заголовок списка: «Треки» и число найденных.
fn collection_list_header(ui: &mut Ui, theme: &Theme, count: usize) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Треки").size(theme.text_size(crate::theme::TextRole::TitleLarge)).strong());
        ui.add_space(8.0);
        ui.label(RichText::new(count.to_string()).size(13.0).color(theme.palette.on_surface_variant));
    });
    ui.add_space(8.0);
}

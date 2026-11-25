//! Экран «SoundCloud»: поиск по сервису и воспроизведение результатов (шаг 5c плана в CLAUDE.md).
//!
//! Поиск идёт в фоне (`Controller::sc_search`), окно не замирает: пока ответа нет, показываем
//! индикатор загрузки (M3 Expressive, `widgets::loading_indicator`). Играет всё через `PlaybackEngine`:
//! клик по строке ставит в очередь все найденные треки, начиная с выбранного. Онлайн-трек сначала
//! скачивается (первые секунды тишина, на медленном интернете дольше), а потом играет по мере
//! докачки; ход загрузки виден в мини-плеере.
//!
//! Вход в аккаунт (токен) появится позже; пока экран работает без него.

use egui::{Align, Key, Layout, RichText, ScrollArea, Ui};

use crate::app::McrateApp;
use crate::icons::Icon;
use crate::theme::Theme;
use crate::ui::components::{TitleRow, empty_state, same_track_as, title_search_row, track_list};
use crate::ui::format::tracks_word;
use crate::ui::layout::{TRACK_LIST_MAX_WIDTH, bleed_clip, centered_column, pane_scroll, scroll_end_padding};
use crate::ui::widgets::{apply_list_gap, error_banner, loading_indicator};
use mcrate_core::Track;

/// Размер индикатора загрузки по умолчанию в M3: 48 dp.
const LOADING_SIZE: f32 = 48.0;
/// Примерная высота пустого состояния (значок и две строки): по ней считаем отступ сверху.
const EMPTY_STATE_HEIGHT: f32 = 220.0;

impl McrateApp {
    /// Экран «SoundCloud»: шапка с поиском и прокручиваемая область (состояние, пустые экраны, результаты).
    pub(crate) fn soundcloud_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, f32::INFINITY, |ui| {
            // Шапка как на главной: заголовок с подписью слева, поиск справа.
            let header = TitleRow {
                title: "SoundCloud",
                subtitle: "Поиск музыки по сервису",
                back: false,
                hint: "Поиск по SoundCloud",
                trailing_w: 0.0,
                centered_search: true,
                title_shown: 1.0,
            };
            let (search, _) = title_search_row(ui, theme, &header, &mut self.sc_input, &mut |_| {});
            if search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !self.sc_input.trim().is_empty() {
                let query = self.sc_input.clone();
                self.ctl.sc_search(&query);
            }
            ui.add_space(18.0);
            // Высоту видимой области берём ДО ScrollArea: внутри неё доступная высота не ограничена,
            // а она нужна, чтобы поставить пустое состояние примерно по центру.
            let view_height = ui.available_height();
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    bleed_clip(ui, self.bleed.below_header());
                    centered_column(ui, TRACK_LIST_MAX_WIDTH, |ui| {
                        self.soundcloud_status(ui, theme, view_height);
                        self.soundcloud_empty_states(ui, theme, view_height);
                        self.soundcloud_results(ui, theme);
                        scroll_end_padding(ui, self.bottom_inset);
                    });
                })
            });
        });
    }

    /// Отступ сверху, чтобы пустое состояние встало примерно по центру видимой области.
    fn empty_state_top(&self, view_height: f32) -> f32 {
        ((view_height - self.bottom_inset - EMPTY_STATE_HEIGHT) * 0.3).max(16.0)
    }

    /// Состояние поиска: индикатор загрузки и баннеры ошибок поиска и воспроизведения.
    /// Загрузка трека показывается контуром вокруг обложки в мини-плеере (player_bar.rs), здесь текста нет.
    fn soundcloud_status(&self, ui: &mut Ui, theme: &Theme, view_height: f32) {
        // ---------- состояние: поиск, загрузка, ошибки ----------
        if self.ctl.sc.searching {
            // Индикатор M3 Expressive по центру области, без подписей.
            ui.vertical_centered(|ui| {
                ui.add_space(((view_height - self.bottom_inset - LOADING_SIZE) * 0.5).max(16.0));
                loading_indicator(ui, theme, LOADING_SIZE);
            });
        }
        if let Some(err) = self.ctl.sc.error.clone() {
            error_banner(ui, theme, &err);
            ui.add_space(8.0);
        }
        if let Some(err) = self.ctl.play_error.clone() {
            error_banner(ui, theme, &err);
            ui.add_space(8.0);
        }
    }

    /// Пустые состояния: «ничего не искали» и «ничего не нашлось».
    fn soundcloud_empty_states(&self, ui: &mut Ui, theme: &Theme, view_height: f32) {
        let count = self.ctl.sc.results.len();

        // ---------- пока ничего не искали ----------
        if !self.ctl.sc.searched && !self.ctl.sc.searching {
            empty_state(
                ui,
                theme,
                self.empty_state_top(view_height),
                Icon::Cloud,
                "Найдите музыку на SoundCloud",
                "Введите название трека или исполнителя и нажмите Enter",
            );
        }

        // ---------- ничего не нашлось ----------
        if self.ctl.sc.searched && count == 0 && !self.ctl.sc.searching && self.ctl.sc.error.is_none() {
            let text = format!("По запросу «{}» нет полных треков", self.ctl.sc.query);
            empty_state(ui, theme, self.empty_state_top(view_height), Icon::Search, "Ничего не найдено", &text);
        }
    }

    /// Результаты поиска: заголовок со счётчиком и список треков. Клик ставит в очередь все найденные.
    fn soundcloud_results(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        let count = self.ctl.sc.results.len();
        // ---------- результаты ----------
        if count > 0 {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Результаты по «{}»", self.ctl.sc.query)).size(18.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format!("{count} {}", tracks_word(count))).size(13.0).color(p.on_surface_variant));
                });
            });
            ui.add_space(6.0);

            let current = self.ctl.now.as_ref().map(|t| (t.id.clone(), t.provider));
            let playing = self.ctl.is_playing();
            let is_current = |track: &Track| same_track_as(current.as_ref(), track);
            let rows = self.ctl.sc.results.iter().enumerate();
            apply_list_gap(ui, theme);
            let clicked = track_list(ui, theme, rows, count, is_current, playing);

            if let Some(i) = clicked {
                // Очередь = все найденные треки, старт с выбранного (как в «Ваших загрузках»).
                let items = self.ctl.sc.results.clone();
                self.ctl.play_list(items, i);
            }
        }
    }
}

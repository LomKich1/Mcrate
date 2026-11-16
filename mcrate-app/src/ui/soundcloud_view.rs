//! Экран «SoundCloud»: поиск по сервису и воспроизведение результатов (шаг 5c плана в CLAUDE.md).
//!
//! Поиск идёт в фоне (`Controller::sc_search`), окно не замирает: пока ответа нет, показываем
//! строку со спиннером. Играет всё через `PlaybackEngine`: клик по строке ставит в очередь все
//! найденные треки, начиная с выбранного. Онлайн-трек сначала скачивается (первые секунды тишина,
//! на медленном интернете дольше), а потом играет по мере докачки.
//!
//! Вход в аккаунт (токен) появится позже; пока экран работает без него.

use egui::{Align, Key, Layout, RichText, ScrollArea, Sense, Spinner, Ui, Vec2};

use super::home::tracks_word;
use super::layout::{bleed_clip, centered_column, scroll_end_padding};
use super::widgets::{error_banner, glide_begin, glide_end, search_bar, track_row};
use crate::app::McrateApp;
use crate::icons::{self, Icon};
use crate::theme::{Theme, with_alpha};

impl McrateApp {
    pub fn soundcloud_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, 900.0, |ui| {
            let p = &theme.palette;
            ui.heading("SoundCloud");
            ui.add_space(8.0);
            // Высоту видимой области берём ДО ScrollArea: внутри неё доступная высота не ограничена,
            // а она нужна, чтобы поставить пустое состояние примерно по центру.
            let view_height = ui.available_height();

            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                bleed_clip(ui, self.bleed.below_header());

                // ---------- поиск ----------
                // Подсказка входит в id поля (см. `search_bar`), поэтому она отличается от подсказок других экранов.
                let search = search_bar(ui, theme, &mut self.sc_input, "Поиск по SoundCloud");
                if search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !self.sc_input.trim().is_empty() {
                    let query = self.sc_input.clone();
                    self.ctl.sc_search(&query);
                }
                ui.add_space(10.0);

                // ---------- состояние: поиск, загрузка, ошибки ----------
                if self.ctl.sc.searching {
                    let hint = format!("Ищем «{}»…", self.ctl.sc.query);
                    status_line(ui, theme, &hint, Some("Первый поиск после установки может занять до 30 секунд"), true);
                }
                if let Some(err) = self.ctl.sc.error.clone() {
                    error_banner(ui, theme, &err);
                    ui.add_space(8.0);
                }
                if let Some(err) = self.ctl.play_error.clone() {
                    error_banner(ui, theme, &err);
                    ui.add_space(8.0);
                }
                if self.ctl.is_loading() {
                    let title = self.ctl.now.as_ref().map(|t| t.title.clone()).unwrap_or_default();
                    status_line(ui, theme, &format!("Загружается «{title}»"), Some("Звук начнётся, как только скачается начало трека"), true);
                } else if let Some(percent) = self.ctl.download_percent() {
                    status_line(ui, theme, &format!("Трек докачивается: {percent}%"), None, false);
                }

                let count = self.ctl.sc.results.len();

                // ---------- пока ничего не искали ----------
                if !self.ctl.sc.searched && !self.ctl.sc.searching {
                    empty_state(
                        ui,
                        theme,
                        ((view_height - self.bottom_inset - 220.0) * 0.3).max(16.0),
                        Icon::Cloud,
                        "Найдите музыку на SoundCloud",
                        "Введите название трека или исполнителя и нажмите Enter",
                    );
                }

                // ---------- ничего не нашлось ----------
                if self.ctl.sc.searched && count == 0 && !self.ctl.sc.searching && self.ctl.sc.error.is_none() {
                    let text = format!("По запросу «{}» нет полных треков", self.ctl.sc.query);
                    empty_state(
                        ui,
                        theme,
                        ((view_height - self.bottom_inset - 220.0) * 0.3).max(16.0),
                        Icon::Search,
                        "Ничего не найдено",
                        &text,
                    );
                }

                // ---------- результаты ----------
                if count > 0 {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Результаты по «{}»", self.ctl.sc.query)).size(18.0).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{count} {}", tracks_word(count)))
                                    .size(13.0)
                                    .color(p.on_surface_variant),
                            );
                        });
                    });
                    ui.add_space(6.0);

                    let current = self.ctl.now.as_ref().map(|t| (t.id.clone(), t.provider));
                    let playing = self.ctl.is_playing();
                    let time = ui.input(|i| i.time);
                    let glide_slot = glide_begin(ui);
                    let mut hovered = None;
                    let mut clicked: Option<usize> = None;
                    for (i, track) in self.ctl.sc.results.iter().enumerate() {
                        let is_current = current.as_ref().is_some_and(|(id, kind)| *id == track.id && *kind == track.provider);
                        let resp = track_row(ui, theme, i, track, is_current, is_current && playing, time);
                        if resp.hovered() {
                            hovered = Some((egui::Id::unique(&track.id).value(), resp.rect));
                        }
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                    }
                    glide_end(ui, theme, glide_slot, ui.scope_id().with("glide_soundcloud"), hovered, 18.0);

                    if let Some(i) = clicked {
                        // Очередь = все найденные треки, старт с выбранного (как в «Ваших загрузках»).
                        let items = self.ctl.sc.results.clone();
                        self.ctl.play_list(items, i);
                    }
                }
                scroll_end_padding(ui, self.bottom_inset);
            });
        });
    }
}

/// Строка состояния: спиннер (если идёт работа), основной текст и необязательная приглушённая подпись.
fn status_line(ui: &mut Ui, theme: &Theme, text: &str, sub: Option<&str>, spinner: bool) {
    let p = &theme.palette;
    ui.horizontal(|ui| {
        if spinner {
            ui.add(Spinner::new().size(16.0).color(p.primary));
        }
        ui.vertical(|ui| {
            ui.label(RichText::new(text).size(14.0).color(p.on_surface));
            if let Some(sub) = sub {
                ui.label(RichText::new(sub).size(12.5).color(p.on_surface_variant));
            }
        });
    });
    ui.add_space(10.0);
}

/// Пустое состояние по центру: круглая иконка, заголовок и пояснение (как на экране «Ваши загрузки»).
fn empty_state(ui: &mut Ui, theme: &Theme, top_space: f32, icon: Icon, headline: &str, text: &str) {
    let p = &theme.palette;
    ui.vertical_centered(|ui| {
        ui.add_space(top_space);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(88.0), Sense::hover());
        if theme.glass_active() {
            ui.painter().circle_stroke(rect.center(), 42.0, egui::Stroke::new(0.9, with_alpha(p.on_surface, 46)));
            icons::paint(theme, ui.painter(), icon, rect.center(), 40.0, p.on_surface);
        } else {
            ui.painter().circle_filled(rect.center(), 44.0, theme.tonal_fill());
            icons::paint(theme, ui.painter(), icon, rect.center(), 40.0, theme.on_tonal());
        }
        ui.add_space(14.0);
        ui.label(RichText::new(headline).size(20.0).strong());
        ui.label(RichText::new(text).size(14.0).color(p.on_surface_variant));
    });
}

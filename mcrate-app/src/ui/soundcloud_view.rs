//! Экран «SoundCloud»: поиск по сервису и воспроизведение результатов (шаг 5c плана в CLAUDE.md).
//!
//! Поиск идёт в фоне (`Controller::sc_search`), окно не замирает: пока ответа нет, показываем
//! «Ищем…». Играет всё через `PlaybackEngine`: клик по строке ставит в очередь все найденные
//! треки, начиная с выбранного. Онлайн-трек сначала скачивается (первые секунды тишина,
//! на медленном интернете дольше), а потом играет по мере докачки.

use egui::{Frame, Key, Margin, RichText, ScrollArea, Sense, Ui, vec2};

use super::layout::{bleed_clip, centered_column, scroll_end_padding};
use super::widgets::{error_banner, glide_begin, glide_end, search_bar, track_row};
use crate::app::McrateApp;
use crate::theme::Theme;

/// Насколько функция готова.
#[derive(Clone, Copy)]
enum Ready {
    /// Работает и подключено к окну.
    Yes,
    /// Не поддерживается.
    No,
}

impl McrateApp {
    pub fn soundcloud_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, 900.0, |ui| {
            let p = &theme.palette;
            ui.heading("SoundCloud");
            ui.add_space(4.0);

            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                bleed_clip(ui, self.bleed.below_header());

                // ---------- поиск ----------
                // Подсказка входит в id поля (см. `search_bar`), поэтому она отличается от подсказок других экранов.
                let search = search_bar(ui, theme, &mut self.sc_input, "Поиск по SoundCloud");
                if search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    let query = self.sc_input.clone();
                    self.ctl.sc_search(&query);
                }
                ui.add_space(10.0);

                // ---------- состояние: поиск, загрузка, ошибки ----------
                if self.ctl.sc.searching {
                    ui.label(
                        RichText::new(format!(
                            "Ищем «{}»… первый поиск после установки может занять до полуминуты",
                            self.ctl.sc.query
                        ))
                        .size(13.0)
                        .color(p.on_surface_variant),
                    );
                    ui.add_space(8.0);
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
                    ui.label(
                        RichText::new(format!("Загружается «{title}»… звук начнётся, как только скачается начало трека"))
                            .size(13.0)
                            .color(p.primary),
                    );
                    ui.add_space(8.0);
                } else if let Some(percent) = self.ctl.download_percent() {
                    ui.label(
                        RichText::new(format!("Трек докачивается: {percent}%")).size(13.0).color(p.on_surface_variant),
                    );
                    ui.add_space(8.0);
                }

                // ---------- пока ничего не искали: что умеет сервис ----------
                if !self.ctl.sc.searched && !self.ctl.sc.searching {
                    let status_frame = if theme.glass_active() {
                        Frame::new()
                            .fill(crate::theme::with_alpha(p.surface_container_low, if p.dark { 82 } else { 96 }))
                            .stroke(egui::Stroke::new(0.8, crate::theme::with_alpha(p.on_surface, 28)))
                            .corner_radius(18)
                            .inner_margin(Margin::same(16))
                    } else {
                        Frame::new().fill(p.surface_container_low).corner_radius(20).inner_margin(Margin::same(16))
                    };
                    status_frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("Состояние сервиса").size(16.0).strong());
                        ui.add_space(8.0);
                        status_row(ui, theme, Ready::Yes, "Поиск треков", "Введите запрос и нажмите Enter. Доступ неофициальный, сервис может закрыть его в любой момент.");
                        status_row(ui, theme, Ready::Yes, "Воспроизведение", "Трек сначала скачивается (формат aac_160k, запасной aac_96k), звук идёт по мере докачки. Перемотка вперёд ограничена скачанным.");
                        status_row(ui, theme, Ready::Yes, "30-секундные превью", "Ядро отсеивает превью (policy=SNIP), в списке только полные треки.");
                        status_row(ui, theme, Ready::No, "Вход в аккаунт и лайки", "Не поддерживается: официальный API закрыт, работаем через публичный client_id.");
                    });
                    ui.add_space(16.0);
                }

                // ---------- результаты ----------
                let count = self.ctl.sc.results.len();
                if self.ctl.sc.searched && count == 0 && !self.ctl.sc.searching && self.ctl.sc.error.is_none() {
                    ui.label(RichText::new("Ничего не найдено").color(p.on_surface_variant));
                }
                if count > 0 {
                    ui.label(
                        RichText::new(format!("Результаты по «{}»: {count}", self.ctl.sc.query)).size(18.0).strong(),
                    );
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
                        // Очередь = все найденные треки, старт с выбранного (как в «Моей музыке»).
                        let items = self.ctl.sc.results.clone();
                        self.ctl.play_list(items, i);
                    }
                }
                scroll_end_padding(ui, self.bottom_inset);
            });
        });
    }
}

/// Строка статуса: цветная точка (работает, нет), название и пояснение.
fn status_row(ui: &mut Ui, theme: &Theme, ready: Ready, title: &str, text: &str) {
    let p = &theme.palette;
    let (dot, state) = match ready {
        Ready::Yes => (p.primary, "работает"),
        Ready::No => (p.error, "нет"),
    };
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 22.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 5.0, dot);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).size(15.0).strong());
                ui.label(RichText::new(state).size(12.0).color(dot));
            });
            ui.label(RichText::new(text).size(13.0).color(p.on_surface_variant));
        });
    });
    ui.add_space(8.0);
}

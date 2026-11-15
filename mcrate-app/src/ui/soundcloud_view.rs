//! Экран «SoundCloud» (пока заглушка).
//!
//! Ядро уже умеет искать на SoundCloud и выдавать ссылку на звук (`SoundCloudProvider`, пример
//! `engine_sc`), но окно к этому не подключено: `Controller` ищет только по локальным файлам и играет
//! в обход `PlaybackEngine` (шаг 5c плана в CLAUDE.md). Поэтому здесь показываем честный статус, что работает
//! и что нет, и держим готовым список результатов. Когда 5c будет сделан, достаточно:
//!   1. заменить `results` ниже на результаты поиска из `Controller`;
//!   2. включить поле поиска и по Enter отправлять запрос;
//!   3. по клику на строку вызывать `self.ctl.play_list(results, index)`.

use egui::{Color32, Frame, Margin, RichText, ScrollArea, Sense, Ui, vec2};
use mcrate_core::Track;

use super::widgets::track_row;
use crate::app::McrateApp;
use crate::theme::Theme;

/// Насколько функция готова. (Вариант `Yes` пока не используется: появится, когда окно подключат к ядру.)
#[allow(dead_code)]
#[derive(Clone, Copy)]
enum Ready {
    /// Работает в ядре и подключено к окну.
    Yes,
    /// Работает в ядре, но окно к этому ещё не подключено.
    CoreOnly,
    /// Не поддерживается.
    No,
}

impl McrateApp {
    pub fn soundcloud_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        ui.heading("SoundCloud");
        ui.add_space(4.0);

        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // ---------- поиск (пока выключен) ----------
            let mut query = String::new();
            ui.add_enabled(
                false,
                egui::TextEdit::singleline(&mut query)
                    .hint_text("Поиск по SoundCloud (появится после шага 5c)")
                    .desired_width(f32::INFINITY)
                    .margin(Margin::symmetric(16, 10)),
            );
            ui.add_space(12.0);

            // ---------- что готово ----------
            Frame::new()
                .fill(p.surface_container_low)
                .corner_radius(20)
                .inner_margin(Margin::same(16))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Состояние сервиса").size(16.0).strong());
                    ui.add_space(8.0);
                    status_row(ui, theme, Ready::CoreOnly, "Поиск треков", "Умеет ядро (SoundCloudProvider). В окне появится вместе с шагом 5c: сейчас поиск идёт только по локальным файлам.");
                    status_row(ui, theme, Ready::CoreOnly, "Воспроизведение", "Движок качает и играет треки (aac_160k, запасной aac_96k). Окно пока играет в обход движка, поэтому онлайн-треки не запускаются.");
                    status_row(ui, theme, Ready::CoreOnly, "30-секундные превью", "Ядро отсеивает превью (policy=SNIP), в списке будут только полные треки.");
                    status_row(ui, theme, Ready::No, "Вход в аккаунт и лайки", "Не поддерживается: официальный API закрыт, работаем через публичный client_id.");
                });
            ui.add_space(16.0);

            // ---------- результаты ----------
            ui.label(RichText::new("Результаты поиска").size(18.0).strong());
            ui.add_space(6.0);
            // Сюда придут найденные треки (шаг 5c). Пока список пуст.
            let results: &[Track] = &[];
            if results.is_empty() {
                ui.label(RichText::new("Здесь появятся результаты поиска по SoundCloud.").color(p.on_surface_variant));
            } else {
                let time = ui.input(|i| i.time);
                for (i, track) in results.iter().enumerate() {
                    // Клик по строке пока ничего не делает (см. пункт 3 в описании модуля).
                    let _ = track_row(ui, theme, i, track, false, false, time);
                }
            }
            ui.add_space(8.0 + self.bottom_inset); // подушка под плавающую панель плеера (Liquid Glass)
        });
    }
}

/// Строка статуса: цветная точка (готово, только в ядре, нет), название и пояснение.
fn status_row(ui: &mut Ui, theme: &Theme, ready: Ready, title: &str, text: &str) {
    let p = &theme.palette;
    let (dot, state) = match ready {
        Ready::Yes => (p.primary, "работает"),
        Ready::CoreOnly => (Color32::from_rgb(0xE0, 0xA8, 0x30), "в ядре"),
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

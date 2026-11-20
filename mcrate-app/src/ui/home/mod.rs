//! Главная. Сверху поиск и «Моя волна» с анимированной волной из точек.
//! Material 3: сверху горизонтальная полка исполнителей, ниже «Быстрый выбор» слева и «Мои треки» справа.
//! Liquid Glass сохраняет прежние горизонтальные полки со стрелками и список ниже.
//!
//! Блоков «Для вас», «Новые релизы» и настоящего чарта здесь нет: у ядра пока нет подборок, обложек и истории.
//! Когда они появятся, это ещё одна полка через `shelf`.
//! Волна (`wave_block.rs`, расчёт в `ui/wave.rs`) живёт всегда (в паузе медленно дрейфует), реагирует на воспроизведение,
//! смену трека, прогресс, курсор и клики. В настройках доступно уменьшение движения. Высота, яркость и скорость лент
//! следуют за настоящим спектром из аудиодвижка (`Controller::visualizer`, 128 полос); на паузе спектр игнорируется.

mod dashboard;
mod media_card;
mod shelf;
mod shelves;
mod wave_block;

use crate::app::{McrateApp, View};
use crate::icons::Icon;
use crate::theme::{TextRole, Theme};
use crate::ui::components::screen_title;
use crate::ui::layout::{pane_scroll, scroll_end_padding};
use crate::ui::widgets::{ButtonKind, pill_button, search_bar};
use egui::{Key, RichText, ScrollArea, Ui};

impl McrateApp {
    pub(crate) fn home_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let bleed = self.bleed;
        // Прокрутка растянута до краёв панели: полоса прокрутки лежит на самом краю, а карточки
        // и заголовки уходят под край, а не режутся в отступе от него (см. `pane_scroll`).
        pane_scroll(ui, bleed, ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
            area.show(ui, |ui| {
                // ---------- заголовок «Моя волна» и поиск в одной строке ----------
                let title_w = (ui.available_width() * 0.45).min(360.0);
                let search = ui
                    .horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_max_width(title_w);
                            wave_title(ui, theme);
                        });
                        ui.add_space(16.0);
                        search_bar(ui, theme, &mut self.search, "Поиск по названию и исполнителю")
                    })
                    .inner;
                // Enter в поле: результаты показывает экран «Ваши загрузки» (свои файлы).
                if search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !self.search.trim().is_empty() {
                    self.view = View::Library;
                }
                ui.add_space(18.0);

                self.home_wave(ui, theme);

                if self.ctl.library.is_empty() {
                    ui.add_space(24.0);
                    self.home_empty(ui, theme);
                } else {
                    ui.add_space(28.0);
                    if theme.glass_active() {
                        // Liquid Glass сохраняет прежние горизонтальные полки и количество карточек.
                        self.home_quick(ui, theme);
                        ui.add_space(28.0);
                        self.home_artists(ui, theme);
                        ui.add_space(28.0);
                        self.home_chart(ui, theme);
                    } else {
                        // Material 3: полка исполнителей стоит над двухколоночным блоком.
                        self.home_artists(ui, theme);
                        ui.add_space(28.0);
                        // Полноширинные карточки «Быстрый выбор» и «Мои треки».
                        self.home_m3_dashboard(ui, theme);
                    }
                }
                scroll_end_padding(ui, self.bottom_inset);
            })
        });
    }

    /// Пустая библиотека: подсказка вместо полок.
    fn home_empty(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        ui.label(RichText::new("Здесь пока пусто").size(theme.text_size(TextRole::TitleLarge)).strong());
        ui.add_space(4.0);
        ui.label(RichText::new("Перетащите файлы в окно или добавьте музыку в «Библиотека → Ваши загрузки».").color(p.on_surface_variant));
        ui.add_space(12.0);
        if pill_button(ui, theme, "Добавить музыку", Some(Icon::Plus), ButtonKind::Filled).clicked() {
            self.view = View::Library;
        }
    }
}

/// Заголовок «Моя волна» и подпись. Стоит в одной строке с поиском (см. `home_view`).
fn wave_title(ui: &mut Ui, theme: &Theme) {
    screen_title(ui, theme, "Моя волна", "Ваша музыка вперемешку");
}

//! Блоки главной со списками треков: «Быстрый выбор» и «Мои треки» (в Material 3 двумя колонками).

use crate::app::{McrateApp, View};
use crate::theme::{ShapeRole, TextRole, Theme};
use crate::ui::components::track_list;
use crate::ui::layout::{TRACK_LIST_MAX_WIDTH, centered_column};
use crate::ui::widgets::{ButtonKind, apply_list_gap, pill_button};
use egui::{Align, Layout, RichText, Ui};
use mcrate_core::Track;

const QUICK_COUNT: usize = 10; // сколько треков в карточке Material 3 «Быстрый выбор»
const CHART_COUNT: usize = 10; // строк в списке «Мои треки»

impl McrateApp {
    /// Главная Material 3: на широком окне «Быстрый выбор» слева, «Мои треки» справа.
    /// Когда места для двух читаемых колонок не хватает, блоки складываются вертикально
    /// и получают ширину видимой поверхности мини-плеера. Внутренней прокрутки нет.
    pub(super) fn home_m3_dashboard(&mut self, ui: &mut Ui, theme: &Theme) {
        const TWO_COLUMN_MIN_WIDTH: f32 = 920.0;
        const SINGLE_COLUMN_PLAYER_OVERHANG: i8 = 4;

        if ui.available_width() >= TWO_COLUMN_MIN_WIDTH {
            let previous_item_spacing = ui.spacing().item_spacing;
            ui.spacing_mut().item_spacing.x = 20.0;
            let mut quick_clicked = None;
            ui.columns(2, |columns| {
                quick_clicked = self.home_quick_m3_card(&mut columns[0], theme);
                self.home_chart(&mut columns[1], theme);
            });
            ui.spacing_mut().item_spacing = previous_item_spacing;
            if let Some(index) = quick_clicked {
                self.play_library_from(index);
            }
        } else {
            // Surface::Content имеет inset 20 px, а плавающий мини-плеер — 16 px.
            // Добавляем по 4 px с каждой стороны, чтобы внешние края совпадали точно.
            let mut quick_clicked = None;
            egui::Frame::new().outer_margin(egui::Margin::symmetric(-SINGLE_COLUMN_PLAYER_OVERHANG, 0)).show(ui, |ui| {
                quick_clicked = self.home_quick_m3_card(ui, theme);
            });
            if let Some(index) = quick_clicked {
                self.play_library_from(index);
            }
            ui.add_space(20.0);
            egui::Frame::new()
                .outer_margin(egui::Margin::symmetric(-SINGLE_COLUMN_PLAYER_OVERHANG, 0))
                .show(ui, |ui| self.home_chart(ui, theme));
        }
    }

    /// Карточка последних `QUICK_COUNT` треков для Material 3. Все строки видны сразу;
    /// прокруткой управляет только основная страница. Возвращает номер трека в библиотеке, по которому кликнули.
    fn home_quick_m3_card(&mut self, ui: &mut Ui, theme: &Theme) -> Option<usize> {
        let items: Vec<(usize, &Track)> = self.ctl.library.iter().enumerate().rev().take(QUICK_COUNT).collect();
        let current_id = self.ctl.now.as_ref().map(|track| track.id.as_str());
        let playing = self.ctl.is_playing();
        let p = &theme.palette;

        let clicked_row = m3_track_card(ui, theme, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Быстрый выбор").size(theme.text_size(TextRole::TitleLarge)).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(items.len().to_string()).size(theme.text_size(TextRole::LabelSmall)).color(p.on_surface_variant),
                    );
                });
            });
            ui.add_space(10.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            // Тот же список, что и в «Мои треки»: единый размер, цвет текущего трека и подсветка от курсора.
            // Номер в строке считается с нуля по порядку показа, а наружу уходит номер в библиотеке.
            let rows = items.iter().enumerate().map(|(row, (_, track))| (row, *track));
            apply_list_gap(ui, theme);
            track_list(ui, theme, rows, items.len(), |track| current_id == Some(track.id.as_str()), playing)
        });
        clicked_row.map(|row| items[row].0)
    }

    /// Список «Мои треки»: первые `CHART_COUNT` треков библиотеки.
    /// В Material 3 оформлен той же полной карточкой и теми же строками, что «Быстрый выбор».
    pub(super) fn home_chart(&mut self, ui: &mut Ui, theme: &Theme) {
        if theme.glass_active() {
            centered_column(ui, TRACK_LIST_MAX_WIDTH, |ui| self.home_chart_contents(ui, theme));
        } else {
            m3_track_card(ui, theme, |ui| self.home_chart_contents(ui, theme));
        }
    }

    fn home_chart_contents(&mut self, ui: &mut Ui, theme: &Theme) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Мои треки").size(theme.text_size(TextRole::TitleLarge)).strong());
            ui.add_space(8.0);
            if pill_button(ui, theme, "Показать всё", None, ButtonKind::Tonal).clicked() {
                self.view = View::Library;
            }
        });
        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.y = 4.0;

        apply_list_gap(ui, theme);
        let rows = self.ctl.library.iter().enumerate().take(CHART_COUNT);
        let total = self.ctl.library.len().min(CHART_COUNT);
        let current_id = self.ctl.now.as_ref().map(|t| t.id.as_str());
        let playing = self.ctl.is_playing();
        let play_index = track_list(ui, theme, rows, total, |track| current_id == Some(track.id.as_str()), playing);
        if let Some(i) = play_index {
            self.play_library_from(i);
        }
    }

    /// Включает всю библиотеку по порядку, начиная с трека `index`. Копия очереди делается только по клику.
    pub(super) fn play_library_from(&mut self, index: usize) {
        let items = self.ctl.library.clone();
        self.ctl.play_list(items, index);
    }
}

/// Карточка Material 3 для списка треков: крупное скругление и внутренний отступ 14 px.
fn m3_track_card<R>(ui: &mut Ui, theme: &Theme, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .corner_radius(theme.shape_radius(ShapeRole::ExtraLarge))
        .inner_margin(egui::Margin::same(14))
        .show(ui, add_contents)
        .inner
}

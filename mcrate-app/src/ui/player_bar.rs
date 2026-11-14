//! Нижняя панель плеера: трек слева, управление и перемотка по центру, громкость справа.

use egui::{Align, Align2, FontId, Layout, Pos2, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use mcrate_audio::RepeatMode;

use super::layout::bar_columns;
use super::widgets::{ButtonKind, artists_text, cover_tile, fmt_time, icon_button, slider_bar};
use crate::app::McrateApp;
use crate::icons::Icon;
use crate::theme::Theme;

/// Высота содержимого панели: ряд кнопок + зазор + строка перемотки.
const CONTROLS_H: f32 = 52.0; // высота главной кнопки «играть»
const SEEK_H: f32 = 24.0; // высота слайдера (как в slider_bar)
const ROW_GAP: f32 = 4.0;
const BAR_H: f32 = CONTROLS_H + ROW_GAP + SEEK_H;
/// Зазор между кнопками управления. Задан явно, чтобы положение каждой кнопки считалось точно.
const BTN_GAP: f32 = 10.0;

/// Поставить виджет ровно в центр `center`. Размер области = размер кнопки, поэтому
/// кнопки разного диаметра оказываются на одной горизонтальной оси.
fn place_at<R>(ui: &mut Ui, center: Pos2, size: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let rect = Rect::from_center_size(center, Vec2::splat(size));
    ui.scope_builder(UiBuilder::new().max_rect(rect), add).inner
}

impl McrateApp {
    pub fn player_bar(&mut self, ui: &mut Ui, theme: &Theme) {
        let total = ui.available_width();
        let gap = ui.spacing().item_spacing.x;
        // Боковые колонки равны, центр точно посередине (правила и тесты в ui/layout.rs).
        let cols = bar_columns(total, gap);
        let (left_w, center_w, right_w) = (cols.side, cols.center, cols.side);

        // Одна общая область на всю панель, дальше режем её на три колонки ОДНОЙ высоты.
        // Так колонки не могут «поплыть» друг относительно друга по вертикали.
        let (full, _) = ui.allocate_exact_size(vec2(total, BAR_H), Sense::hover());
        let left = Rect::from_min_size(full.min, vec2(left_w, BAR_H));
        let center = Rect::from_min_size(pos2(full.left() + left_w + gap, full.top()), vec2(center_w, BAR_H));
        let right = Rect::from_min_max(pos2(full.right() - right_w, full.top()), full.max);

        ui.scope_builder(UiBuilder::new().max_rect(left).layout(Layout::left_to_right(Align::Center)), |ui| {
            self.pb_track_info(ui, theme, left_w);
        });
        self.pb_center(ui, theme, center);
        ui.scope_builder(UiBuilder::new().max_rect(right).layout(Layout::right_to_left(Align::Center)), |ui| {
            self.pb_volume(ui, theme, right_w);
        });
    }

    fn pb_track_info(&mut self, ui: &mut Ui, theme: &Theme, width: f32) {
        let p = &theme.palette;
        match self.ctl.now.clone() {
            Some(t) => {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
                cover_tile(ui, theme, rect, &t.id, 14.0);
                ui.vertical(|ui| {
                    ui.set_max_width((width - 52.0 - 14.0).max(60.0));
                    // truncate = длинный текст обрезается многоточием, а не ломает раскладку
                    ui.add(egui::Label::new(RichText::new(&t.title).size(15.0).strong()).truncate());
                    ui.add(egui::Label::new(RichText::new(artists_text(&t)).size(12.5).color(p.on_surface_variant)).truncate());
                });
            }
            None => {
                ui.label(RichText::new("Ничего не играет").color(p.on_surface_variant));
            }
        }
    }

    /// Центральная колонка: ряд кнопок сверху, строка перемотки снизу.
    fn pb_center(&mut self, ui: &mut Ui, theme: &Theme, rect: Rect) {
        let p = &theme.palette;
        let cy_controls = rect.top() + CONTROLS_H / 2.0;

        // Диаметры кнопок слева направо: перемешать, назад, играть, вперёд, повтор.
        let sizes = [36.0_f32, 40.0, CONTROLS_H, 40.0, 36.0];
        let row_w: f32 = sizes.iter().sum::<f32>() + BTN_GAP * (sizes.len() as f32 - 1.0);
        let mut x = rect.center().x - row_w / 2.0;
        let mut centers = [Pos2::ZERO; 5];
        for (i, d) in sizes.iter().enumerate() {
            centers[i] = pos2(x + d / 2.0, cy_controls);
            x += d + BTN_GAP;
        }

        let shuffle = self.ctl.queue.shuffle();
        if place_at(ui, centers[0], sizes[0], |ui| {
            icon_button(ui, theme, Icon::Shuffle, sizes[0], ButtonKind::Plain, shuffle, "Перемешать")
        })
        .clicked()
        {
            self.ctl.toggle_shuffle();
        }
        if place_at(ui, centers[1], sizes[1], |ui| {
            icon_button(ui, theme, Icon::Prev, sizes[1], ButtonKind::Plain, false, "Предыдущий")
        })
        .clicked()
        {
            self.ctl.prev();
        }
        let (play_icon, tip) = if self.ctl.is_playing() { (Icon::Pause, "Пауза") } else { (Icon::Play, "Играть") };
        if place_at(ui, centers[2], sizes[2], |ui| icon_button(ui, theme, play_icon, sizes[2], ButtonKind::Filled, false, tip)).clicked() {
            self.ctl.toggle_pause();
        }
        if place_at(ui, centers[3], sizes[3], |ui| {
            icon_button(ui, theme, Icon::Next, sizes[3], ButtonKind::Plain, false, "Следующий")
        })
        .clicked()
        {
            self.ctl.next();
        }
        let (rep_icon, rep_active, rep_tip) = match self.ctl.repeat_mode() {
            RepeatMode::Off => (Icon::Repeat, false, "Повтор: выкл"),
            RepeatMode::All => (Icon::Repeat, true, "Повтор: все"),
            RepeatMode::One => (Icon::RepeatOne, true, "Повтор: один трек"),
        };
        if place_at(ui, centers[4], sizes[4], |ui| {
            icon_button(ui, theme, rep_icon, sizes[4], ButtonKind::Plain, rep_active, rep_tip)
        })
        .clicked()
        {
            self.ctl.cycle_repeat();
        }

        // Строка перемотки: время слева, полоса, длительность справа. Всё на одной линии cy.
        let dur = self.ctl.now.as_ref().map(|t| t.duration_secs as f32).unwrap_or(0.0);
        let pos = self.ctl.position().as_secs_f32().min(dur.max(0.0));
        let frac = self.seek_drag.unwrap_or(if dur > 0.0 { pos / dur } else { 0.0 });
        let shown = if self.seek_drag.is_some() { frac * dur } else { pos };

        let seek = Rect::from_min_size(pos2(rect.left(), rect.top() + CONTROLS_H + ROW_GAP), vec2(rect.width(), SEEK_H));
        let time_w = 44.0;
        let font = FontId::proportional(12.0);
        // Время прижато к полосе: левое вправо, правое влево, поэтому ничего не «гуляет» от ширины цифр.
        ui.painter().text(pos2(seek.left() + time_w, seek.center().y), Align2::RIGHT_CENTER, fmt_time(shown as u64), font.clone(), p.on_surface_variant);
        ui.painter().text(pos2(seek.right() - time_w, seek.center().y), Align2::LEFT_CENTER, fmt_time(dur as u64), font, p.on_surface_variant);

        let bar_rect = Rect::from_min_max(pos2(seek.left() + time_w, seek.top()), pos2(seek.right() - time_w, seek.bottom()));
        let out = ui.scope_builder(UiBuilder::new().max_rect(bar_rect), |ui| slider_bar(ui, theme, bar_rect.width(), frac, dur > 0.0)).inner;
        if out.response.dragged() {
            if let Some(v) = out.value {
                self.seek_drag = Some(v);
            }
        }
        // Перематываем один раз при отпускании (или при клике), а не на каждом кадре.
        if out.response.drag_stopped() || out.response.clicked() {
            let v = out.value.or(self.seek_drag).unwrap_or(frac);
            self.ctl.seek_to(v * dur);
            self.seek_drag = None;
        }
    }

    fn pb_volume(&mut self, ui: &mut Ui, theme: &Theme, width: f32) {
        // Layout здесь right_to_left: что добавлено первым, то правее.
        // Ползунок растёт с панелью, но остаётся в пределах 90..180 px.
        let slider_w = (width * 0.45).clamp(90.0, 180.0);
        let out = slider_bar(ui, theme, slider_w, self.ctl.volume, true);
        if let Some(v) = out.value {
            self.ctl.set_volume(v);
        }
        let muted = self.ctl.volume <= 0.001;
        let icon = if muted { Icon::VolumeMute } else { Icon::Volume };
        if icon_button(ui, theme, icon, 36.0, ButtonKind::Plain, false, "Громкость").clicked() {
            if muted {
                self.ctl.set_volume(self.volume_before_mute.max(0.2));
            } else {
                self.volume_before_mute = self.ctl.volume;
                self.ctl.set_volume(0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::GraphicsSettings;

    /// Кнопки разного диаметра должны стоять на одной горизонтальной оси,
    /// а весь ряд быть симметричным относительно центра колонки.
    #[test]
    fn controls_share_one_axis_and_are_centered() {
        let theme = Theme::new(&GraphicsSettings::default());
        let ctx = egui::Context::default();
        let column = Rect::from_min_size(pos2(300.0, 40.0), vec2(640.0, BAR_H));
        let sizes = [36.0_f32, 40.0, CONTROLS_H, 40.0, 36.0];
        let icons = [Icon::Shuffle, Icon::Prev, Icon::Play, Icon::Next, Icon::Repeat];
        let mut found: Vec<Rect> = Vec::new();

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let row_w: f32 = sizes.iter().sum::<f32>() + BTN_GAP * 4.0;
            let mut x = column.center().x - row_w / 2.0;
            let cy = column.top() + CONTROLS_H / 2.0;
            for (d, icon) in sizes.iter().zip(icons) {
                let r = place_at(ui, pos2(x + d / 2.0, cy), *d, |ui| icon_button(ui, &theme, icon, *d, ButtonKind::Plain, false, "t"));
                found.push(r.rect);
                x += d + BTN_GAP;
            }
        });
        output.textures_delta.clear(); // egui требует явно «погасить» результат прогона

        assert_eq!(found.len(), 5);
        let cy0 = found[0].center().y;
        for r in &found {
            assert!((r.center().y - cy0).abs() < 0.01, "центры по вертикали разошлись: {found:?}");
        }
        let left = found.first().unwrap().left();
        let right = found.last().unwrap().right();
        assert!(((left + right) / 2.0 - column.center().x).abs() < 0.01, "ряд смещён от центра колонки");
    }
}

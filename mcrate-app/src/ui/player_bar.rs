//! Нижняя панель плеера: трек слева, управление и перемотка по центру, громкость справа.

use egui::{Align, Align2, CursorIcon, FontId, Id, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use mcrate_audio::RepeatMode;

use super::format::{artists_text, fmt_time};
use super::layout::bar_columns;
use super::ripple;
use super::widgets::{ButtonKind, IconButton, cover_tile, elided, icon_button, like_button, loading_outline, place_at, slider_bar};
use crate::app::{McrateApp, View};
use crate::icons::Icon;
use crate::theme::{Theme, with_alpha};

/// Высота содержимого панели: ряд кнопок + зазор + строка перемотки.
const CONTROLS_H: f32 = 52.0; // высота главной кнопки «играть»
const SEEK_H: f32 = 24.0; // высота слайдера (как в slider_bar)
const ROW_GAP: f32 = 4.0;
const BAR_H: f32 = CONTROLS_H + ROW_GAP + SEEK_H;
/// Зазор между кнопками управления. Задан явно, чтобы положение каждой кнопки считалось точно.
const BTN_GAP: f32 = 10.0;
/// Диаметр сердечка «Нравится» у правого края колонки с треком.
const LIKE_SIZE: f32 = 36.0;
/// Сколько места слева от сердечка отдано зазору, чтобы текст трека не упирался в кнопку.
const LIKE_GAP: f32 = 8.0;

impl McrateApp {
    pub(crate) fn player_bar(&mut self, ui: &mut Ui, theme: &Theme) {
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
                let compact = width < 220.0;
                let cover_size = if compact { 40.0 } else { 52.0 };
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(cover_size), Sense::hover());
                let cover_radius = if compact { 10.0 } else { 14.0 };
                cover_tile(ui, theme, rect, &t.id, cover_radius);
                // Пока трек грузится или докачивается, по периметру обложки бежит контур (без чисел).
                if self.ctl.is_loading() || self.ctl.download_percent().is_some() {
                    let time = ui.input(|i| i.time);
                    loading_outline(ui, theme, rect, cover_radius, time);
                }
                // Текст по вертикали центрируется по обложке: считаем высоту блока по реальным строкам
                // и кладём его вровень с центром обложки (ui.vertical прижимал текст к верху колонки).
                // В узкой колонке (compact) сердечка нет: места мало, лайк доступен в «Сейчас играет».
                let like_slot = if compact { 0.0 } else { LIKE_SIZE + LIKE_GAP };
                let text_w = (width - cover_size - 14.0 - like_slot).max(60.0);
                let painter = ui.painter().clone();
                let title = elided(&painter, &t.title, FontId::proportional(if compact { 13.5 } else { 15.0 }), p.on_surface, text_w);
                let artists =
                    (!compact).then(|| elided(&painter, &artists_text(&t), FontId::proportional(12.5), p.on_surface_variant, text_w));
                let gap = 3.0;
                let block_h = title.size().y + artists.as_ref().map_or(0.0, |a| gap + a.size().y);
                let x = rect.right() + 14.0;
                let mut y = rect.center().y - block_h * 0.5;
                painter.galley(pos2(x, y), title.clone(), p.on_surface);
                y += title.size().y + gap;
                if let Some(a) = artists {
                    painter.galley(pos2(x, y), a, p.on_surface_variant);
                }
                if !compact {
                    let center = pos2(ui.max_rect().right() - LIKE_SIZE / 2.0, rect.center().y);
                    let liked = self.ctl.is_liked(&t);
                    if place_at(ui, center, LIKE_SIZE, |ui| like_button(ui, theme, liked, LIKE_SIZE)).clicked() {
                        self.ctl.toggle_like(&t);
                    }
                }
            }
            None => {
                ui.label(RichText::new("Ничего не играет").color(p.on_surface_variant));
            }
        }
    }

    /// Центральная колонка: ряд кнопок сверху, строка перемотки снизу.
    fn pb_center(&mut self, ui: &mut Ui, theme: &Theme, rect: Rect) {
        self.pb_controls(ui, theme, rect);
        self.pb_seek(ui, theme, rect);
    }

    /// Ряд кнопок: перемешать, назад, играть, вперёд, повтор.
    fn pb_controls(&mut self, ui: &mut Ui, theme: &Theme, rect: Rect) {
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
            icon_button(ui, theme, IconButton::new(Icon::Shuffle, sizes[0], "Перемешать").active(shuffle))
        })
        .clicked()
        {
            self.ctl.toggle_shuffle();
        }
        if place_at(ui, centers[1], sizes[1], |ui| icon_button(ui, theme, IconButton::new(Icon::Prev, sizes[1], "Предыдущий"))).clicked()
        {
            self.ctl.prev();
        }
        let (play_icon, tip) = if self.ctl.is_playing() { (Icon::Pause, "Пауза") } else { (Icon::Play, "Играть") };
        if place_at(ui, centers[2], sizes[2], |ui| {
            icon_button(ui, theme, IconButton::new(play_icon, sizes[2], tip).kind(ButtonKind::Filled))
        })
        .clicked()
        {
            self.ctl.toggle_pause();
        }
        if place_at(ui, centers[3], sizes[3], |ui| icon_button(ui, theme, IconButton::new(Icon::Next, sizes[3], "Следующий"))).clicked()
        {
            self.ctl.next();
        }
        let (rep_icon, rep_active, rep_tip) = match self.ctl.repeat_mode() {
            RepeatMode::Off => (Icon::Repeat, false, "Повтор: выкл"),
            RepeatMode::All => (Icon::Repeat, true, "Повтор: все"),
            RepeatMode::One => (Icon::RepeatOne, true, "Повтор: один трек"),
        };
        if place_at(ui, centers[4], sizes[4], |ui| icon_button(ui, theme, IconButton::new(rep_icon, sizes[4], rep_tip).active(rep_active)))
            .clicked()
        {
            self.ctl.cycle_repeat();
        }
    }

    /// Строка перемотки: время слева, полоса, длительность справа.
    fn pb_seek(&mut self, ui: &mut Ui, theme: &Theme, rect: Rect) {
        let p = &theme.palette;
        let dur = self.ctl.now.as_ref().map(|t| t.duration_secs as f32).unwrap_or(0.0);
        let pos = self.ctl.position().as_secs_f32().min(dur.max(0.0));
        let frac = self.seek_drag.unwrap_or(if dur > 0.0 { pos / dur } else { 0.0 });
        let shown = if self.seek_drag.is_some() { frac * dur } else { pos };

        let seek = Rect::from_min_size(pos2(rect.left(), rect.top() + CONTROLS_H + ROW_GAP), vec2(rect.width(), SEEK_H));
        let time_w = 44.0;
        let font = FontId::proportional(12.0);
        // Время прижато к полосе: левое вправо, правое влево, поэтому ничего не «гуляет» от ширины цифр.
        ui.painter().text(
            pos2(seek.left() + time_w, seek.center().y),
            Align2::RIGHT_CENTER,
            fmt_time(shown as u64),
            font.clone(),
            p.on_surface_variant,
        );
        ui.painter().text(
            pos2(seek.right() - time_w, seek.center().y),
            Align2::LEFT_CENTER,
            fmt_time(dur as u64),
            font,
            p.on_surface_variant,
        );

        let bar_rect = Rect::from_min_max(pos2(seek.left() + time_w, seek.top()), pos2(seek.right() - time_w, seek.bottom()));
        let out = ui
            .scope_builder(UiBuilder::new().max_rect(bar_rect), |ui| slider_bar(ui, theme, bar_rect.width(), frac, dur > 0.0, "Перемотка"))
            .inner;
        if out.response.dragged() {
            if let Some(v) = out.value {
                self.seek_drag = Some(v);
            }
        } else if out.response.has_focus() && !out.response.clicked() {
            // Изменение с клавиатуры применяется сразу, без имитации мышиного drag.
            if let Some(v) = out.value {
                self.ctl.seek_to(v * dur);
                self.seek_drag = None;
            }
        }
        // Перематываем один раз при отпускании (или при клике), а не на каждом кадре.
        if out.response.drag_stopped() || out.response.clicked() {
            let v = out.value.or(self.seek_drag).unwrap_or(frac);
            self.ctl.seek_to(v * dur);
            self.seek_drag = None;
        }
    }

    /// Стрелка «Сейчас играет»: ^ когда экран закрыт, v когда открыт; поворачивается плавно.
    /// Открытие запоминает текущий экран, закрытие возвращает на него.
    fn pb_now_playing_toggle(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        let open = self.view == View::NowPlaying;
        let t = ui.ctx().animate_bool_with_time(Id::unique("np_toggle_arrow"), open, 0.30);
        let e = t * t * (3.0 - 2.0 * t); // smoothstep: плавный старт и остановка
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
        let pressed = resp.is_pointer_button_down_on();
        if resp.hovered() && !pressed {
            ui.painter().circle_filled(rect.center(), 18.0, with_alpha(p.on_surface, 20));
        }
        ripple::state_layer(ui, theme, ui.painter(), &resp, rect, 18.0);
        let (s, c) = (e * std::f32::consts::PI).sin_cos();
        let pts: Vec<Pos2> = [vec2(-7.0, 3.5), vec2(0.0, -3.5), vec2(7.0, 3.5)]
            .iter()
            .map(|v| rect.center() + vec2(v.x * c - v.y * s, v.x * s + v.y * c))
            .collect();
        ui.painter().add(Shape::line(pts, Stroke::new(2.2, p.on_surface)));
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Сейчас играет");
        if resp.clicked() {
            if open {
                self.view = self.view_before_np;
            } else {
                self.view_before_np = self.view;
                self.view = View::NowPlaying;
            }
        }
    }

    fn pb_volume(&mut self, ui: &mut Ui, theme: &Theme, width: f32) {
        // Layout здесь right_to_left: что добавлено первым, то правее.
        self.pb_now_playing_toggle(ui, theme);
        ui.add_space(4.0);
        // Ползунок растёт с панелью, но остаётся в пределах 64..180 px (минимум снижен из-за стрелки справа).
        let slider_w = ((width - 36.0) * 0.45).clamp(64.0, 180.0);
        let out = slider_bar(ui, theme, slider_w, self.ctl.volume.level(), true, "Громкость");
        if let Some(v) = out.value {
            self.ctl.set_volume(v);
        }
        let muted = self.ctl.volume.is_muted();
        let icon = if muted { Icon::VolumeMute } else { Icon::Volume };
        if icon_button(ui, theme, IconButton::new(icon, 36.0, "Громкость")).clicked() {
            self.ctl.toggle_mute();
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
                let r = place_at(ui, pos2(x + d / 2.0, cy), *d, |ui| icon_button(ui, &theme, IconButton::new(icon, *d, "t")));
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

//! Блок «Моя волна» на главной: холст с анимированной волной и её связь с плеером.

use crate::app::McrateApp;
use crate::theme::Theme;
use crate::ui::wave::{self, COL_STEP_PX, WaveInput, WaveSim};
use egui::{Sense, Ui, vec2};
use std::time::Duration;

const WAVE_H: f32 = 200.0; // высота волны
/// Если видимая область ниже этого значения, волна уменьшается до `WAVE_H_COMPACT`.
const WAVE_COMPACT_BELOW_HEIGHT: f32 = 640.0;
const WAVE_H_COMPACT: f32 = 148.0;
/// Число колонок волны кратно этому шагу, чтобы таблица не пересобиралась на каждый пиксель ширины.
const WAVE_COLS_QUANTUM: usize = 20;
const WAVE_COLS_MIN: usize = 180;
const WAVE_COLS_MAX: usize = 900;

impl McrateApp {
    /// «Моя волна»: заголовок, две кнопки и анимированная волна из точек.
    pub(super) fn home_wave(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        let playing = self.ctl.is_playing();
        // Время берём у основного контекста (в закадровом проходе стекла свои часы).
        let ctx_time = ui.input(|i| i.time);
        let now = if self.frame_time > 0.0 { self.frame_time } else { ctx_time };
        // Закадровый проход стекла (его часы не совпадают с основными) рисует облегчённую волну: через стекло видно только размытое пятно.
        let backdrop_pass = (ctx_time - now).abs() > 1e-3;
        let wave_id = ui.scope_id().with("wave_state");
        let reduce_motion = theme.settings.reduce_motion;

        // Заголовок «Моя волна» выведен в строку с поиском (см. `wave_title` и `home_view`).

        // Холст волны слушает курсор (hover), нажатие и клик.
        let wave_height = if ui.clip_rect().height() < WAVE_COMPACT_BELOW_HEIGHT { WAVE_H_COMPACT } else { WAVE_H };
        // Волна идёт от края до края панели в обеих темах: холст шире страницы на её боковые отступы.
        // Нажатие и наведение ловим на всём широком холсте (`interact`), а не на узкой рамке.
        let wave_bleed = self.bleed.left.min(self.bleed.right);
        let (page_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), wave_height), Sense::hover());
        let canvas = page_rect.expand2(vec2(wave_bleed, 0.0));
        let resp = ui.interact(canvas, wave_id.with("hit"), Sense::click());
        // Волна прокручена за экран (или скрыта): не считаем и не рисуем, и не просим перерисовку.
        if !ui.is_rect_visible(canvas) {
            return;
        }

        // Что сейчас играет: из этого волна узнаёт про смену трека и прогресс.
        let duration = self.ctl.now.as_ref().map(|t| t.duration_secs as f32).unwrap_or(0.0);
        let progress = if duration > 0.0 { Some((self.ctl.position().as_secs_f32() / duration).clamp(0.0, 1.0)) } else { None };
        let input = WaveInput {
            now,
            playing,
            track_id: self.ctl.now.as_ref().map(|t| t.id.as_str()),
            progress,
            // Спектр из движка (слева низкие частоты, справа высокие). Пока не играет, волна его игнорирует.
            spectrum: Some(&self.ctl.visualizer().samples[..]),
            reduce_motion,
            pointer: resp.hover_pos().map(|p| p - canvas.min),
            pressed: resp.is_pointer_button_down_on(),
            size: canvas.size(),
        };
        let mut sim: WaveSim = ui.ctx().data_mut(|d| d.get_temp(wave_id)).unwrap_or_default();
        sim.step(&input);

        if !backdrop_pass && !reduce_motion {
            // 60 к/с, пока есть заметное движение; в покое волна дрейфует на ~30 к/с (дешевле для CPU).
            let ms = if sim.busy() { 16 } else { 33 };
            ui.ctx().request_repaint_after(Duration::from_millis(ms));
        }
        // Колонок больше при широком окне; кратно 20, чтобы таблица не пересобиралась на каждый пиксель.
        let cols = ((canvas.width() / COL_STEP_PX / WAVE_COLS_QUANTUM as f32).round() as usize * WAVE_COLS_QUANTUM)
            .clamp(WAVE_COLS_MIN, WAVE_COLS_MAX);
        let table = wave::wave_table(ui.ctx(), cols);
        wave::paint(
            ui.painter(),
            canvas,
            &sim,
            &table,
            wave::WaveColors { base: p.on_surface, accent: p.primary },
            if backdrop_pass { 3 } else { 1 },
        );
        ui.ctx().data_mut(|d| d.insert_temp(wave_id, sim));
    }
}

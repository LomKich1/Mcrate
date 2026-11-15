//! Экран «Сейчас играет».
//! Список «дальше в очереди» пока показать нельзя: у PlayQueue нет публичного доступа
//! к элементам. Если он понадобится, надо согласовать с LomKich метод вроде `items()`.
//!
//! Под обложкой и текстом рисуется визуализация в стиле cava: столбики, которые прыгают в такт музыке.
//! Слои снизу вверх: фон стеклянной панели, столбики, обложка, название и исполнитель.
//! Для этого столбики рисуются САМЫМИ ПЕРВЫМИ в экране: в egui всё, что нарисовано позже, лежит выше.

use std::collections::hash_map::DefaultHasher;
use std::f64::consts::TAU;
use std::hash::{Hash, Hasher};

use egui::{Rect, RichText, Sense, Ui, Vec2, pos2};

use super::layout::cover_side;
use super::widgets::{artists_text, cover_tile};
use crate::app::McrateApp;
use crate::controller::Controller;
use crate::theme::{Theme, with_alpha};

/// Шаг между столбиками по горизонтали, в логических пикселях (от него зависит их число).
const BAR_STEP: f32 = 14.0;
/// Во сколько раз быстро столбик взлетает (чем больше, тем резче).
const ATTACK: f32 = 30.0;
/// Как быстро столбик падает вниз: долей максимальной высоты в секунду.
const FALL_PER_SEC: f32 = 1.3;

impl McrateApp {
    pub fn now_playing_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        let track = self.ctl.now.clone();

        // Визуализация идёт первой, чтобы лежать под всем остальным.
        if let Some(track) = &track {
            self.paint_visualizer(ui, theme, hash_seed(&track.id));
        }

        ui.heading("Сейчас играет");

        let Some(track) = track else {
            ui.add_space(12.0);
            ui.label(RichText::new("Выберите трек в библиотеке").color(p.on_surface_variant));
            return;
        };

        ui.vertical_centered(|ui| {
            ui.add_space(12.0);
            // bottom_inset: в Liquid Glass нижняя часть окна закрыта плавающей панелью плеера
            let side = cover_side(ui.available_width(), ui.available_height() - self.bottom_inset);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
            cover_tile(ui, theme, rect, &track.id, 36.0);
            ui.add_space(18.0);
            ui.label(RichText::new(&track.title).size(26.0).strong());
            ui.label(RichText::new(artists_text(&track)).size(16.0).color(p.on_surface_variant));
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("Свои файлы · в очереди: {}", self.ctl.queue.len()))
                    .size(13.0)
                    .color(p.on_surface_variant),
            );
        });
    }

    /// Столбики cava вдоль нижней кромки экрана.
    fn paint_visualizer(&self, ui: &mut Ui, theme: &Theme, seed: u64) {
        let rect = ui.max_rect();
        if rect.width() < 80.0 || rect.height() < 80.0 {
            return;
        }
        let playing = self.ctl.is_playing();
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        let n = ((rect.width() / BAR_STEP) as usize).clamp(16, 96);

        // Состояние (высота каждого столбика и «музыкальное время») храним в памяти egui.
        // Так не нужно добавлять поля в McrateApp, а состояние у каждого экрана своё.
        let id = ui.scope_id().with("visualizer");
        let mut st: VisState = ui.ctx().data_mut(|d| d.get_temp(id)).unwrap_or_default();
        if st.bars.len() != n {
            st.bars = vec![0.0; n];
        }
        if playing {
            st.time += dt as f64; // на паузе время стоит, столбики плавно опускаются
        }

        // Откуда берём цель для столбиков: настоящий спектр (когда он появится) или имитация.
        let target = match real_spectrum(&self.ctl, n) {
            Some(v) if v.len() == n => v,
            _ if playing => demo_spectrum(n, st.time, seed),
            _ => vec![0.0; n],
        };

        let mut moving = false;
        for (bar, t) in st.bars.iter_mut().zip(target) {
            if t > *bar {
                *bar += (t - *bar) * (1.0 - (-dt * ATTACK).exp()); // быстрый взлёт
            } else {
                *bar = (*bar - dt * FALL_PER_SEC).max(t); // плавное падение
            }
            moving |= *bar > 0.002;
        }

        // ---- рисуем ----
        let p = &theme.palette;
        let max_h = (rect.height() * 0.42).clamp(120.0, 360.0);
        let step = rect.width() / n as f32;
        let bw = (step * 0.62).max(3.0);
        // Столбики растут от самого низа панели: добавляем 20 px, это отступ стеклянной панели.
        let bottom = rect.bottom() + 20.0;
        let clip = ui.clip_rect();
        let painter = ui.painter().with_clip_rect(Rect::from_min_max(clip.min, pos2(clip.max.x, clip.max.y + 20.0)));
        let color = with_alpha(p.primary, if theme.glass_active() { 80 } else { 100 });
        for (i, &v) in st.bars.iter().enumerate() {
            let h = v.clamp(0.0, 1.0) * max_h;
            if h < 1.0 {
                continue;
            }
            let x = rect.left() + step * (i as f32 + 0.5);
            let bar = Rect::from_min_max(pos2(x - bw / 2.0, bottom - h), pos2(x + bw / 2.0, bottom));
            painter.rect_filled(bar, bw / 2.0, color);
        }

        ui.ctx().data_mut(|d| d.insert_temp(id, st));
        if playing || moving {
            ui.ctx().request_repaint(); // пока играет или опускаются столбики, рисуем каждый кадр
        }
    }
}

/// Высоты столбиков и время анимации между кадрами.
#[derive(Clone, Default)]
struct VisState {
    bars: Vec<f32>,
    time: f64,
}

/// ТОЧКА ПОДКЛЮЧЕНИЯ НАСТОЯЩЕГО СПЕКТРА.
/// Сейчас движок звука не отдаёт данных для визуализации, поэтому возвращаем `None`
/// и ниже рисуется имитация. Когда LomKich добавит в `PlaybackEngine` метод вроде
/// `spectrum(bands) -> Vec<f32>` (значения 0..1, от низких частот к высоким),
/// достаточно вернуть его результат здесь через `Controller`, остальной код менять не надо.
fn real_spectrum(_ctl: &Controller, _bands: usize) -> Option<Vec<f32>> {
    None
}

/// Имитация спектра: басы выше, верха ниже, у каждого столбика своя «скорость качания»
/// (она зависит от трека, поэтому у разных треков картинка разная), плюс общий «удар» 2 раза в секунду.
/// Это НЕ анализ звука: движение не связано с тем, что реально играет.
fn demo_spectrum(n: usize, t: f64, seed: u64) -> Vec<f32> {
    let beat = (t * TAU * 2.0).sin() * 0.5 + 0.5;
    let kick = beat.powi(4);
    (0..n)
        .map(|i| {
            let x = i as f64 / (n - 1).max(1) as f64; // 0 = басы, 1 = верха
            let envelope = 0.9 * (1.0 - x).powf(1.3) + 0.12;
            let (h1, h2) = (hash01(seed, i as u64), hash01(seed, 1000 + i as u64));
            let wobble_a = 0.5 + 0.5 * (t * (2.0 + 6.0 * h1) + TAU * h2).sin();
            let wobble_b = 0.5 + 0.5 * (t * (1.0 + 4.0 * h2) + TAU * h1).sin();
            let v = envelope * (0.3 + 0.7 * wobble_a * wobble_b) + kick * 0.45 * (1.0 - x).powi(2);
            v.clamp(0.0, 1.0) as f32
        })
        .collect()
}

fn hash_seed(id: &str) -> u64 {
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    h.finish()
}

/// Псевдослучайное число 0..1 из (seed, i): один и тот же вход всегда даёт один и тот же результат.
fn hash01(seed: u64, i: u64) -> f64 {
    let mut z = seed.wrapping_add(i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

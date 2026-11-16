//! Экран «Сейчас играет».
//!
//! Аудиовизуализатор рисуется отдельным фоновым слоем у нижнего края — как в исходном экране,
//! но теперь это гладкая непрерывная линия спектра (слева низкие частоты, справа высокие) из mcrate-audio.
//! Поверх неё располагаются карточка трека и анимируемая панель текста.

use egui::{
    Align, Layout, Rect, RichText, ScrollArea, Sense, Shape, Stroke, Ui,
    pos2, vec2,
};

use super::widgets::{ButtonKind, artists_text, cover_tile, icon_button, spring_step};
use crate::app::{McrateApp, View};
use crate::controller::provider_label;
use crate::icons::Icon;
use crate::theme::{Surface, Theme, with_alpha};
use super::layout::cover_side;

const PANEL_GAP: f32 = 16.0;
const LEFT_MIN: f32 = 280.0;
const RIGHT_MIN: f32 = 250.0;
const FOOTER_HEIGHT: f32 = 48.0;
/// Пружина двигает число от 0 до этого значения (а не до 1): у `spring_step` порог остановки
/// 0,05 задан для пикселей, и на шкале 0..1 пружина «замирала» бы, не дойдя 5% до цели.
const SPRING_SCALE: f32 = 100.0;

/// Где лежат две карточки экрана в этом кадре.
/// Страница считает их один раз, а рисует либо сама (обычные темы), либо стеклянный слой
/// поверх страницы (Liquid Glass): стекло должно лежать НАД страницей, иначе ему нечего преломлять.
#[derive(Clone, Copy, Debug)]
pub struct NpLayout {
    /// Карточка трека (обложка, название). Её размер плавно меняется, когда уезжает текст.
    pub left: Rect,
    /// Карточка текста уже со сдвигом вправо. Её размер НЕ меняется: она целиком выезжает за окно.
    pub lyrics: Rect,
    /// `false`, когда карточка текста уехала за правый край окна целиком (рисовать нечего).
    pub lyrics_visible: bool,
}

/// Состояние экрана. Лежит в `McrateApp`, а не во временной памяти egui: страница в Liquid Glass
/// рисуется дважды за кадр (на экран и за кадр), а стеклянный слой ещё и отдельно от неё.
#[derive(Clone, Debug)]
pub struct NowPlayingState {
    /// Хочет ли пользователь видеть панель текста (по умолчанию да).
    pub lyrics_open: bool,
    /// Положение панели на шкале 0..=`SPRING_SCALE` (0 спрятана, `SPRING_SCALE` показана).
    /// Двигает его пружина (см. `step_now_playing`), поэтому в пути оно слегка перелетает цель.
    pub lyrics_pos: f32,
    /// Скорость пружины. При повторном нажатии не обнуляется: панель разворачивается плавно.
    pub lyrics_vel: f32,
    /// Раскладка этого кадра. Страница пишет её, стеклянный слой читает. Перед страницей стирается.
    pub layout: Option<NpLayout>,
}

impl Default for NowPlayingState {
    fn default() -> Self {
        Self { lyrics_open: true, lyrics_pos: SPRING_SCALE, lyrics_vel: 0.0, layout: None }
    }
}

/// Раскладка карточек в области `body`. `shown` = 0..1 (у пружины в пути чуть выходит за границы),
/// `window_right` = правый край окна (до него панель текста уезжает).
fn np_layout(body: Rect, shown: f32, window_right: f32) -> NpLayout {
    let side_by_side = body.width() >= LEFT_MIN + RIGHT_MIN + PANEL_GAP;
    // `final_lyrics` = где карточка текста стоит, когда открыта полностью. Размер всегда этот.
    let (left, final_lyrics) = if side_by_side {
        let target_right = (body.width() * 0.55).clamp(RIGHT_MIN, body.width() - LEFT_MIN - PANEL_GAP);
        // Карточка трека занимает всё, что не заняла панель текста (плюс зазор между ними).
        // `.min(..)` нужен из-за перелёта пружины: при закрытии `shown` кратко уходит ниже нуля.
        let left_width = (body.width() - (target_right + PANEL_GAP) * shown).clamp(LEFT_MIN, body.width());
        (
            Rect::from_min_size(body.min, vec2(left_width, body.height())),
            Rect::from_min_max(pos2(body.right() - target_right, body.top()), body.max),
        )
    } else {
        // Узкое окно: карточки друг под другом.
        let target_left = body.height() * 0.54;
        let target_right = (body.height() - target_left - PANEL_GAP).max(0.0);
        let left_height = (body.height() - (target_right + PANEL_GAP) * shown).clamp(0.0, body.height());
        (
            Rect::from_min_size(body.min, vec2(body.width(), left_height)),
            Rect::from_min_max(pos2(body.left(), body.bottom() - target_right), body.max),
        )
    };
    // Сдвиг вправо: при `shown = 1` нулевой, при `shown = 0` левый край карточки как раз у края окна.
    let shift = (1.0 - shown) * (window_right - final_lyrics.left()).max(0.0);
    let lyrics = final_lyrics.translate(vec2(shift, 0.0));
    NpLayout { left, lyrics, lyrics_visible: lyrics.left() < window_right - 0.5 && lyrics.height() > 1.0 }
}

impl McrateApp {
    /// Двигает панель текста той же пружиной, что и пилюлю выделения в рейке (`spring_step`):
    /// небольшой перелёт и остановка, а при повторном нажатии посреди пути скорость сохраняется.
    /// Вызывать РОВНО раз за кадр из `McrateApp::ui` (а не из страницы: её замыкание выполняется
    /// дважды за кадр, и время шло бы вдвое быстрее).
    pub fn step_now_playing(&mut self, ctx: &egui::Context) {
        let target = if self.np.lyrics_open { SPRING_SCALE } else { 0.0 };
        if self.theme.settings.reduce_motion {
            // «Уменьшить анимацию»: без пружины, сразу на месте.
            self.np.lyrics_pos = target;
            self.np.lyrics_vel = 0.0;
            return;
        }
        // `stable_dt` = сглаженная длительность кадра в секундах. Верхний предел такой же, как у
        // пилюли: долгий кадр не должен «взрывать» пружину.
        let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.064);
        if spring_step(&mut self.np.lyrics_pos, &mut self.np.lyrics_vel, target, dt) {
            ctx.request_repaint(); // следующий кадр сразу, не ждём события мыши
        }
    }

    /// Страница экрана: волна, заголовок, кнопка текста и раскладка. Сами карточки она рисует
    /// только без Liquid Glass; в Liquid Glass их рисует `now_playing_glass_cards` поверх страницы.
    pub fn now_playing_view(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;

        // Волна рисуется первой: все карточки и надписи останутся поверх неё.
        self.paint_background_wave(ui, theme);

        ui.heading("Сейчас играет");
        ui.add_space(10.0);

        let available = ui.available_rect_before_wrap();
        let content_bottom = (available.bottom() - self.bottom_inset).max(available.top() + 180.0);
        let footer_top = (content_bottom - FOOTER_HEIGHT).max(available.top() + 120.0);
        let footer_rect = Rect::from_min_max(
            pos2(available.left(), footer_top),
            pos2(available.right(), content_bottom),
        );
        let body_bottom = (footer_top - 8.0).max(available.top() + 100.0);
        let body = Rect::from_min_max(available.min, pos2(available.right(), body_bottom));

        // Кнопка находится в одном и том же месте и при открытом, и при закрытом тексте.
        let lyrics_open = self.np.lyrics_open;
        let mut toggle_clicked = false;
        let mut footer_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("now-playing-lyrics-toggle")
                .max_rect(footer_rect)
                .layout(Layout::top_down(Align::Center)),
        );
        footer_ui.horizontal(|ui| {
            if icon_button(
                ui,
                theme,
                Icon::Note,
                38.0,
                ButtonKind::Tonal,
                false,
                if lyrics_open { "Скрыть текст трека" } else { "Показать текст трека" },
            )
            .clicked()
            {
                toggle_clicked = true;
            }
            ui.label(
                RichText::new(if lyrics_open { "Скрыть текст" } else { "Показать текст" })
                    .size(13.0)
                    .color(p.on_surface_variant),
            );
        });
        if toggle_clicked {
            self.np.lyrics_open = !self.np.lyrics_open;
            ui.ctx().request_repaint(); // чтобы анимация началась в следующем кадре без задержки
        }

        let window_right = ui.ctx().viewport_rect().right();
        let layout = np_layout(body, self.np.lyrics_pos / SPRING_SCALE, window_right);
        self.np.layout = Some(layout);

        // Liquid Glass: карточки нарисует стеклянный слой (он лежит над страницей и преломляет её).
        if !theme.glass_active() {
            self.draw_np_cards(ui, theme, layout, false);
        }
    }

    /// Стеклянные карточки «Сейчас играет» (только Liquid Glass). Вызывается из `glass_layout`
    /// ПОСЛЕ страницы, как рейка и плеер: стекло берёт картинку страницы за собой.
    pub fn now_playing_glass_cards(&mut self, ui: &mut Ui, theme: &Theme) {
        if self.view != View::NowPlaying {
            return;
        }
        if let Some(layout) = self.np.layout {
            self.draw_np_cards(ui, theme, layout, true);
        }
    }

    /// Нарисовать обе карточки. `shield` = поставить невидимый щит под каждой, чтобы клики и
    /// прокрутка не проваливались в страницу под стеклом (как у рейки и плеера).
    fn draw_np_cards(&self, ui: &mut Ui, theme: &Theme, layout: NpLayout, shield: bool) {
        let track = self.ctl.now.clone();

        if shield {
            ui.interact(layout.left, egui::Id::unique("np_left_shield"), Sense::click_and_drag());
        }
        let mut left_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("now-playing-left")
                .max_rect(layout.left)
                .layout(Layout::top_down(Align::Min)),
        );
        self.track_panel(&mut left_ui, theme, track.as_ref());

        if layout.lyrics_visible {
            if shield {
                ui.interact(layout.lyrics, egui::Id::unique("np_lyrics_shield"), Sense::click_and_drag());
            }
            let mut lyrics_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("now-playing-lyrics")
                    .max_rect(layout.lyrics)
                    .layout(Layout::top_down(Align::Min)),
            );
            self.lyrics_panel(&mut lyrics_ui, theme, track.as_ref());
        }
    }

    fn track_panel(&self, ui: &mut Ui, theme: &Theme, track: Option<&mcrate_core::Track>) {
        let p = &theme.palette;
        theme.surface(ui, Surface::Dialog, |ui| {
            ui.set_min_size(ui.available_size());
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(4.0);
                if let Some(track) = track {
                    ui.vertical_centered(|ui| {
                        let compact = ui.available_height() < 560.0;
                        let side = cover_side(ui.available_width(), ui.available_height(), compact)
                            .min(340.0);
                        let (cover, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
                        cover_tile(ui, theme, cover, &track.id, 28.0);
                        ui.add_space(18.0);
                        ui.label(RichText::new(&track.title).size(if compact { 21.0 } else { 25.0 }).strong());
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(artists_text(track))
                                .size(15.0)
                                .color(p.on_surface_variant),
                        );
                        ui.add_space(12.0);
                        let queue_count = self.ctl.queue.len();
                        let queue_position = if queue_count > 0 {
                            format!(
                                "В очереди · {} из {queue_count}",
                                self.ctl.queue.index().saturating_add(1).min(queue_count)
                            )
                        } else {
                            "Очередь пуста".to_owned()
                        };
                        ui.label(
                            RichText::new(format!("{}  ·  {queue_position}", provider_label(track.provider)))
                                .size(12.5)
                                .color(p.on_surface_variant),
                        );
                    });
                    if let Some(percent) = self.ctl.download_percent() {
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new(format!("Загрузка трека · {percent}%"))
                                .size(12.0)
                                .color(p.on_surface_variant),
                        );
                    }
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.label(RichText::new("Сейчас ничего не играет").size(20.0).strong());
                        ui.add_space(8.0);
                        ui.label(RichText::new("Выберите трек в библиотеке или SoundCloud").color(p.on_surface_variant));
                    });
                }
            });
        });
    }

    fn lyrics_panel(&self, ui: &mut Ui, theme: &Theme, track: Option<&mcrate_core::Track>) {
        let p = &theme.palette;
        theme.surface(ui, Surface::Dialog, |ui| {
            ui.set_min_size(ui.available_size());
            ui.label(RichText::new("Текст трека").size(18.0).strong());
            ui.add_space(8.0);
            ui.separator();
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(18.0);
                if let Some(track) = track {
                    ui.label(RichText::new(&track.title).size(15.0).strong());
                    ui.label(RichText::new(artists_text(track)).size(12.5).color(p.on_surface_variant));
                    ui.add_space(32.0);
                }
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    let (icon_rect, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                    crate::icons::paint(ui.painter(), Icon::Note, icon_rect.center(), 38.0, p.on_surface_variant);
                    ui.add_space(14.0);
                    ui.label(RichText::new("Текст пока недоступен").size(16.0).strong());
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("В текущем бэкенде ещё нет источника и API для получения текстов песен.")
                            .size(13.0)
                            .color(p.on_surface_variant),
                    );
                });
            });
        });
    }

    /// Гладкая линия спектра из mcrate-audio (форма стоит на месте и пульсирует, вбок не едет). Рисуется у нижней кромки всей страницы,
    /// позади обложки и панели текста. Никакой прогресс трека здесь не отображается.
    fn paint_background_wave(&self, ui: &mut Ui, theme: &Theme) {
        let rect = ui.max_rect();
        if rect.width() < 80.0 || rect.height() < 80.0 {
            return;
        }

        let target = &self.ctl.visualizer().samples;
        let id = ui.make_persistent_id("now-playing-wave-smoothing");
        let mut smooth = ui.ctx().data_mut(|d| d.get_temp::<Vec<f32>>(id))
            .unwrap_or_else(|| vec![0.0; target.len()]);
        if smooth.len() != target.len() {
            smooth.resize(target.len(), 0.0);
        }

        let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.05);
        let mut moving = false;
        if theme.settings.reduce_motion {
            smooth.copy_from_slice(target);
        } else {
            for (current, &goal) in smooth.iter_mut().zip(target.iter()) {
                let speed = if goal > *current { 15.0 } else { 5.5 };
                *current += (goal - *current) * (1.0 - (-dt * speed).exp());
                moving |= (goal - *current).abs() > 0.002;
            }
        }

        // Одна непрерывная линия без заливки, подложки и второй параллельной дорожки.
        // Низкая амплитуда остаётся у нижнего края, пики поднимаются вверх.
        let baseline = rect.bottom() - 8.0;
        let max_height = (rect.height() * 0.34).clamp(80.0, 260.0);
        let segments = (target.len() * 3).max(192);
        let mut wave = Vec::with_capacity(segments + 1);

        for i in 0..=segments {
            let x = i as f32 / segments as f32;
            let index = x * (smooth.len() - 1) as f32;
            let left = index.floor() as usize;
            let right = (left + 1).min(smooth.len() - 1);
            let t = index - left as f32;
            let level = smooth[left] + (smooth[right] - smooth[left]) * t;
            let y = baseline - level.clamp(0.0, 1.0) * max_height;
            wave.push(pos2(rect.left() + rect.width() * x, y));
        }

        let p = &theme.palette;
        let painter = ui.painter().with_clip_rect(rect);
        let line = with_alpha(p.primary, if theme.glass_active() { 190 } else { 205 });
        painter.add(Shape::line(wave, Stroke::new(2.2, line)));

        ui.ctx().data_mut(|d| d.insert_temp(id, smooth));
        if !theme.settings.reduce_motion && self.ctl.is_playing() && !self.ctl.is_loading() {
            ui.ctx().request_repaint();
        } else if moving && !theme.settings.reduce_motion {
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(20.0, 20.0), vec2(w, h))
    }

    #[test]
    fn lyrics_card_keeps_its_size_while_sliding() {
        // Главное требование: карточка текста не сжимается, а только едет.
        for (w, h) in [(1100.0, 600.0), (420.0, 700.0)] {
            let open = np_layout(body(w, h), 1.0, 1300.0).lyrics;
            for shown in [0.0, 0.25, 0.5, 0.75] {
                let moving = np_layout(body(w, h), shown, 1300.0).lyrics;
                assert!((moving.width() - open.width()).abs() < 0.01);
                assert!((moving.height() - open.height()).abs() < 0.01);
            }
        }
    }

    #[test]
    fn lyrics_card_leaves_through_the_right_window_edge() {
        let open = np_layout(body(1100.0, 600.0), 1.0, 1300.0);
        assert!(open.lyrics_visible);
        let hidden = np_layout(body(1100.0, 600.0), 0.0, 1300.0);
        assert!(!hidden.lyrics_visible);
        assert!(hidden.lyrics.left() >= 1300.0 - 0.5);
    }

    #[test]
    fn track_card_takes_the_freed_space() {
        let open = np_layout(body(1100.0, 600.0), 1.0, 1300.0);
        let closed = np_layout(body(1100.0, 600.0), 0.0, 1300.0);
        assert!(closed.left.width() > open.left.width());
        assert!((closed.left.width() - 1100.0).abs() < 0.01);
    }

    #[test]
    fn spring_opens_with_a_small_overshoot_and_settles_exactly() {
        // Как у пилюли: пружина чуть перелетает цель и останавливается ровно на ней.
        let (mut pos, mut vel) = (0.0_f32, 0.0_f32);
        let (mut max, mut settled) = (0.0_f32, false);
        for _ in 0..600 {
            let moving = spring_step(&mut pos, &mut vel, SPRING_SCALE, 1.0 / 60.0);
            max = max.max(pos);
            if !moving {
                settled = true;
                break;
            }
        }
        assert!(settled);
        assert_eq!(pos, SPRING_SCALE);
        assert!(max > SPRING_SCALE && max < SPRING_SCALE * 1.06, "перелёт {max}");
    }

    #[test]
    fn overshoot_never_breaks_the_layout() {
        // Перелёт в обе стороны: карточка трека остаётся в пределах области, размер текста прежний.
        let reference = np_layout(body(1100.0, 600.0), 1.0, 1300.0).lyrics;
        for shown in [-0.05, 0.0, 1.0, 1.05] {
            let l = np_layout(body(1100.0, 600.0), shown, 1300.0);
            assert!(l.left.width() <= 1100.0 && l.left.width() >= LEFT_MIN);
            assert!((l.lyrics.width() - reference.width()).abs() < 0.01);
        }
    }
}

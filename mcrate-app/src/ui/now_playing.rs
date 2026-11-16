//! Экран «Сейчас играет».
//!
//! Аудиовизуализатор рисуется отдельным фоновым слоем у нижнего края — как в исходном экране,
//! но теперь это гладкая непрерывная линия спектра (слева низкие частоты, справа высокие) из mcrate-audio.
//! Поверх неё располагаются карточка трека и анимируемая панель текста.
//! Между карточками стоит разделитель: его можно тянуть мышью, меняя ширину карточек друг относительно друга
//! (двойной клик возвращает исходное соотношение). Минимумы: `LEFT_MIN` и `RIGHT_MIN`.

use egui::{
    Align, CursorIcon, Layout, Pos2, Rect, RichText, ScrollArea, Sense, Shape, Stroke, Ui,
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
/// Доля ширины области, которую занимает карточка текста, пока пользователь не двигал разделитель.
const DEFAULT_SPLIT: f32 = 0.55;
/// Идентификатор разделителя в памяти egui (один на приложение).
const SPLIT_ID: &str = "np_split_handle";
/// Кнопка «скрыть/показать текст» стоит в правом верхнем углу области страницы: при открытом тексте
/// это шапка карточки текста, при закрытом — угол карточки трека (место на экране одно и то же).
const TOGGLE_SIZE: f32 = 32.0;
const TOGGLE_INSET_RIGHT: f32 = 14.0;
const TOGGLE_INSET_TOP: f32 = 10.0;
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
    /// Центр кнопки «скрыть/показать текст».
    pub toggle_center: Pos2,
    /// Зазор между карточками, за который можно тянуть. Есть только в раскладке «рядом» и когда текст
    /// открыт целиком: пока панель едет, разделителя нет.
    pub divider: Option<Rect>,
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
    /// Какую долю ширины области занимает карточка текста (двигается разделителем, живёт до закрытия приложения).
    pub split: f32,
    /// Раскладка этого кадра. Страница пишет её, стеклянный слой читает. Перед страницей стирается.
    pub layout: Option<NpLayout>,
}

impl Default for NowPlayingState {
    fn default() -> Self {
        Self { lyrics_open: true, lyrics_pos: SPRING_SCALE, lyrics_vel: 0.0, split: DEFAULT_SPLIT, layout: None }
    }
}

/// Допустимая ширина карточки текста при ширине области `width`: не уже `RIGHT_MIN`, и карточке трека остаётся `LEFT_MIN`.
fn right_bounds(width: f32) -> (f32, f32) {
    (RIGHT_MIN, (width - LEFT_MIN - PANEL_GAP).max(RIGHT_MIN))
}

/// Раскладка карточек в области `body`. `shown` = 0..1 (у пружины в пути чуть выходит за границы),
/// `window_right` = правый край окна (до него панель текста уезжает), `split` = доля ширины под карточку текста.
fn np_layout(body: Rect, shown: f32, window_right: f32, split: f32) -> NpLayout {
    let side_by_side = body.width() >= LEFT_MIN + RIGHT_MIN + PANEL_GAP;
    let mut divider = None;
    // `final_lyrics` = где карточка текста стоит, когда открыта полностью. Размер всегда этот.
    let (left, final_lyrics) = if side_by_side {
        let (min_right, max_right) = right_bounds(body.width());
        let target_right = (body.width() * split).clamp(min_right, max_right);
        // Карточка трека занимает всё, что не заняла панель текста (плюс зазор между ними).
        // `.min(..)` нужен из-за перелёта пружины: при закрытии `shown` кратко уходит ниже нуля.
        let left_width = (body.width() - (target_right + PANEL_GAP) * shown).clamp(LEFT_MIN, body.width());
        let left = Rect::from_min_size(body.min, vec2(left_width, body.height()));
        let final_lyrics = Rect::from_min_max(pos2(body.right() - target_right, body.top()), body.max);
        // Разделитель занимает зазор между карточками, когда текст открыт целиком (пружина успокоилась).
        if (shown - 1.0).abs() < 0.01 {
            divider = Some(Rect::from_min_max(pos2(left.right(), body.top()), pos2(final_lyrics.left(), body.bottom())));
        }
        (left, final_lyrics)
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
    // Кнопка текста. В обычной раскладке стоит в правом верхнем углу области. В узкой карточка текста
    // снизу, поэтому кнопка плавно ездит вместе с её открытием: от угла области к шапке карточки.
    let button_top = if side_by_side {
        body.top()
    } else {
        let t = shown.clamp(0.0, 1.0);
        body.top() + (final_lyrics.top() - body.top()) * t
    };
    let toggle_center = pos2(
        body.right() - TOGGLE_INSET_RIGHT - TOGGLE_SIZE / 2.0,
        button_top + TOGGLE_INSET_TOP + TOGGLE_SIZE / 2.0,
    );
    NpLayout {
        left,
        lyrics,
        lyrics_visible: lyrics.left() < window_right - 0.5 && lyrics.height() > 1.0,
        toggle_center,
        divider,
    }
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

    /// Страница экрана: волна, заголовок и раскладка. Сами карточки она рисует
    /// только без Liquid Glass; в Liquid Glass их рисует `now_playing_glass_cards` поверх страницы.
    pub fn now_playing_view(&mut self, ui: &mut Ui, theme: &Theme) {
        // Волна рисуется первой: все карточки и надписи останутся поверх неё.
        self.paint_background_wave(ui, theme);

        ui.heading("Сейчас играет");
        ui.add_space(10.0);

        let available = ui.available_rect_before_wrap();
        let content_bottom = (available.bottom() - self.bottom_inset).max(available.top() + 180.0);
        let body = Rect::from_min_max(available.min, pos2(available.right(), content_bottom));

        let window_right = ui.ctx().viewport_rect().right();
        let shown = self.np.lyrics_pos / SPRING_SCALE;
        let mut layout = np_layout(body, shown, window_right, self.np.split);
        if let Some(divider) = layout.divider {
            // Разделитель двигает долю прямо в этом кадре: пересчитываем раскладку, чтобы карточки шли за курсором без отставания.
            if self.np_divider(ui, theme, divider, body) {
                layout = np_layout(body, shown, window_right, self.np.split);
            }
        }
        self.np.layout = Some(layout);

        // Liquid Glass: карточки нарисует стеклянный слой (он лежит над страницей и преломляет её).
        if !theme.glass_active() && self.draw_np_cards(ui, theme, layout, false) {
            self.toggle_lyrics(ui.ctx());
        }
    }

    /// Разделитель между карточками: тянем мышью, двойной клик возвращает `DEFAULT_SPLIT`.
    /// Положение считаем по курсору (а не накоплением сдвигов), поэтому у границы минимума карточка не «уплывает»
    /// от курсора. Смещение точки захвата относительно середины зазора запоминаем, чтобы при нажатии не было скачка.
    /// Страница в Liquid Glass вызывается дважды за кадр, но событий мыши получает только настоящий проход,
    /// а запись идемпотентна (присваивание, а не прибавка), так что двойной вызов безвреден.
    /// Возвращает `true`, если доля изменилась.
    fn np_divider(&mut self, ui: &mut Ui, theme: &Theme, divider: Rect, body: Rect) -> bool {
        let id = egui::Id::new(SPLIT_ID);
        let resp = ui.interact(divider, id, Sense::click_and_drag());
        let mut changed = false;

        if resp.drag_started() {
            if let Some(pos) = resp.interact_pointer_pos() {
                ui.ctx().data_mut(|d| d.insert_temp(id.with("grab"), pos.x - divider.center().x));
            }
        }
        if resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let grab: f32 = ui.ctx().data_mut(|d| d.get_temp(id.with("grab"))).unwrap_or(0.0);
                // Середина зазора стоит под курсором (с поправкой на захват): справа от неё ширина карточки текста.
                let right = body.right() - (pos.x - grab) - PANEL_GAP / 2.0;
                let (min_right, max_right) = right_bounds(body.width());
                let split = right.clamp(min_right, max_right) / body.width();
                if (split - self.np.split).abs() > 1e-4 {
                    self.np.split = split;
                    changed = true;
                }
            }
        }
        if resp.double_clicked() && (self.np.split - DEFAULT_SPLIT).abs() > 1e-4 {
            self.np.split = DEFAULT_SPLIT;
            changed = true;
        }

        // Ручка: тонкая «таблетка» по центру зазора, при наведении и перетаскивании вытягивается и цветнеет.
        let hot = resp.hovered() || resp.dragged();
        let t = if theme.settings.reduce_motion {
            if hot { 1.0 } else { 0.0 }
        } else {
            ui.ctx().animate_bool_with_time(id.with("hot"), hot, 0.12)
        };
        let p = &theme.palette;
        let color = with_alpha(p.on_surface_variant.lerp_to_gamma(p.primary, t), (110.0 + 110.0 * t) as u8);
        let grip = Rect::from_center_size(divider.center(), vec2(4.0, 36.0 + 20.0 * t));
        ui.painter().rect_filled(grip, 2.0, color);

        let _ = resp.on_hover_cursor(CursorIcon::ResizeHorizontal);
        changed
    }

    /// Стеклянные карточки «Сейчас играет» (только Liquid Glass). Вызывается из `glass_layout`
    /// ПОСЛЕ страницы, как рейка и плеер: стекло берёт картинку страницы за собой.
    pub fn now_playing_glass_cards(&mut self, ui: &mut Ui, theme: &Theme) {
        if self.view != View::NowPlaying {
            return;
        }
        if let Some(layout) = self.np.layout {
            if self.draw_np_cards(ui, theme, layout, true) {
                self.toggle_lyrics(ui.ctx());
            }
        }
    }

    fn toggle_lyrics(&mut self, ctx: &egui::Context) {
        self.np.lyrics_open = !self.np.lyrics_open;
        ctx.request_repaint(); // чтобы анимация началась в следующем кадре без задержки
    }

    /// Нарисовать обе карточки. `shield` = поставить невидимый щит под каждой, чтобы клики и
    /// прокрутка не проваливались в страницу под стеклом (как у рейки и плеера).
    /// Кнопка «скрыть/показать текст» рисуется поверх карточек; `true` = её нажали.
    fn draw_np_cards(&self, ui: &mut Ui, theme: &Theme, layout: NpLayout, shield: bool) -> bool {
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

        let lyrics_open = self.np.lyrics_open;
        let mut toggle_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("now-playing-lyrics-toggle")
                .max_rect(Rect::from_center_size(layout.toggle_center, vec2(TOGGLE_SIZE, TOGGLE_SIZE)))
                .layout(Layout::top_down(Align::Min)),
        );
        icon_button(
            &mut toggle_ui,
            theme,
            Icon::Note,
            TOGGLE_SIZE,
            ButtonKind::Tonal,
            false,
            if lyrics_open { "Скрыть текст трека" } else { "Показать текст трека" },
        )
        .clicked()
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
                    crate::icons::paint(theme, ui.painter(), Icon::Note, icon_rect.center(), 38.0, p.on_surface_variant);
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
            let open = np_layout(body(w, h), 1.0, 1300.0, DEFAULT_SPLIT).lyrics;
            for shown in [0.0, 0.25, 0.5, 0.75] {
                let moving = np_layout(body(w, h), shown, 1300.0, DEFAULT_SPLIT).lyrics;
                assert!((moving.width() - open.width()).abs() < 0.01);
                assert!((moving.height() - open.height()).abs() < 0.01);
            }
        }
    }

    #[test]
    fn lyrics_card_leaves_through_the_right_window_edge() {
        let open = np_layout(body(1100.0, 600.0), 1.0, 1300.0, DEFAULT_SPLIT);
        assert!(open.lyrics_visible);
        let hidden = np_layout(body(1100.0, 600.0), 0.0, 1300.0, DEFAULT_SPLIT);
        assert!(!hidden.lyrics_visible);
        assert!(hidden.lyrics.left() >= 1300.0 - 0.5);
    }

    #[test]
    fn track_card_takes_the_freed_space() {
        let open = np_layout(body(1100.0, 600.0), 1.0, 1300.0, DEFAULT_SPLIT);
        let closed = np_layout(body(1100.0, 600.0), 0.0, 1300.0, DEFAULT_SPLIT);
        assert!(closed.left.width() > open.left.width());
        assert!((closed.left.width() - 1100.0).abs() < 0.01);
    }

    #[test]
    fn divider_exists_only_when_side_by_side_and_fully_open() {
        let open = np_layout(body(1100.0, 600.0), 1.0, 1300.0, DEFAULT_SPLIT);
        let gap = open.divider.expect("при открытом тексте разделитель есть");
        assert!((gap.left() - open.left.right()).abs() < 0.01);
        assert!((gap.right() - open.lyrics.left()).abs() < 0.01);
        assert!((gap.width() - PANEL_GAP).abs() < 0.01);
        assert!(np_layout(body(1100.0, 600.0), 0.5, 1300.0, DEFAULT_SPLIT).divider.is_none()); // панель едет
        assert!(np_layout(body(1100.0, 600.0), 0.0, 1300.0, DEFAULT_SPLIT).divider.is_none()); // текст скрыт
        assert!(np_layout(body(420.0, 700.0), 1.0, 1300.0, DEFAULT_SPLIT).divider.is_none()); // узкое окно: карточки друг под другом
    }

    #[test]
    fn split_changes_widths_and_respects_minimums() {
        let narrow = np_layout(body(1100.0, 600.0), 1.0, 1300.0, 0.3);
        let wide = np_layout(body(1100.0, 600.0), 1.0, 1300.0, 0.7);
        assert!(wide.lyrics.width() > narrow.lyrics.width());
        assert!(wide.left.width() < narrow.left.width());
        // Ширины двух карточек и зазор всегда дают всю область.
        for l in [narrow, wide] {
            assert!((l.left.width() + PANEL_GAP + l.lyrics.width() - 1100.0).abs() < 0.01);
        }
        // Крайние значения упираются в минимумы.
        let tiny = np_layout(body(1100.0, 600.0), 1.0, 1300.0, 0.0);
        assert!((tiny.lyrics.width() - RIGHT_MIN).abs() < 0.01);
        let huge = np_layout(body(1100.0, 600.0), 1.0, 1300.0, 1.0);
        assert!((huge.left.width() - LEFT_MIN).abs() < 0.01);
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
        let reference = np_layout(body(1100.0, 600.0), 1.0, 1300.0, DEFAULT_SPLIT).lyrics;
        for shown in [-0.05, 0.0, 1.0, 1.05] {
            let l = np_layout(body(1100.0, 600.0), shown, 1300.0, DEFAULT_SPLIT);
            assert!(l.left.width() <= 1100.0 && l.left.width() >= LEFT_MIN);
            assert!((l.lyrics.width() - reference.width()).abs() < 0.01);
        }
    }
}


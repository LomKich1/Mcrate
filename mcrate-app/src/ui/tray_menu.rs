//! Меню иконки в трее в цветах приложения.
//!
//! Системное меню Windows перекрасить нельзя, поэтому по правому клику по иконке (`TrayCommand::OpenMenu`) приложение
//! открывает своё маленькое окно без рамки: тот же вид, что у меню трека (Material 3 или Liquid Glass), цвета и тёмная
//! или светлая тема текущие. Окно не показывается в панели задач, лежит поверх остальных и закрывается, как обычное
//! меню: выбором пункта, Esc или щелчком вне него (потеря фокуса).
//!
//! Главное окно при этом может быть скрыто. eframe не вызывает `App::ui` у скрытого окна, а окно меню объявляется
//! оттуда, поэтому, пока меню открыто, `background.rs` просит `RequestPaintWhileHidden`. Как только окно меню
//! появилось, eframe сам продолжает гонять `ui` ради видимого дочернего окна.
//!
//! Окно меню обязано быть отложенным (`show_viewport_deferred`), а не немедленным (`show_viewport_immediate`).
//! В eframe 0.37 кадр скрытого окна, запущенный по таймеру (`new_events`), идёт без доступа к циклу событий:
//! немедленное окно тогда не создаётся, и egui паникует («the user callback was never called»), приложение
//! закрывается. Отложенное окно eframe создаёт сам в начале следующего кадра. Оно рисуется отдельно от приложения,
//! поэтому выбор пользователя передаётся через общую ячейку `Feedback`, после чего будим главное окно.
//!
//! Положение: правый край меню у курсора в момент клика; меню над ним, если панель задач внизу, и под ним, если
//! вверху; от краёв экрана отступ. Координаты клика приходят в физических пикселях, окну нужны точки egui: делим на `pixels_per_point`
//! (масштаб экрана Windows, умноженный на «Размер текста» из настроек). Тем же числом eframe умножает обратно.
//!
//! Скругление, рамку и тень рисует Windows (`platform::style_popup_window`), а карточка заполняет окно целиком:
//! своя скруглённая рамка поверх системной давала светлые уголки и полоску сверху.

use std::sync::{Arc, Mutex, PoisonError};

use egui::{
    CentralPanel, Context, CornerRadius, Frame, Key, Pos2, Shadow, Stroke, Ui, Vec2, ViewportBuilder, ViewportCommand, ViewportId, vec2,
};

use crate::app::McrateApp;
use crate::icons::Icon;
use crate::platform;
use crate::theme::Theme;
use crate::tray::TrayCommand;
use crate::ui::track_menu::{ITEM_HEIGHT, MENU_PADDING, MENU_WIDTH, glass_frame, m3_frame, menu_item};

/// Высота разделителя между группами пунктов.
const SEPARATOR_HEIGHT: f32 = 9.0;
/// Отступ окна от края экрана и от курсора, пт.
const SCREEN_MARGIN: f32 = 8.0;
const CURSOR_GAP: f32 = 12.0;
/// Заголовок окна меню (на экране не виден): по нему `platform` находит окно, поэтому он не как у главного.
const MENU_TITLE: &str = "Mcrate: меню трея";
/// Сколько секунд ждать, пока меню получит фокус. Не получило: закрываем само, иначе щелчок вне него не заметить.
const FOCUS_TIMEOUT_SECS: f64 = 2.0;

/// Открытое меню трея.
pub(crate) struct OpenTrayMenu {
    /// Левый верхний угол окна, логические точки экрана.
    pos: Pos2,
    /// Когда открыли (время egui, с).
    opened_at: f64,
    /// Что сообщило окно меню.
    feedback: Arc<Mutex<Feedback>>,
}

/// Что произошло в окне меню: оно пишет сюда, главное окно читает.
#[derive(Clone, Copy, Default)]
struct Feedback {
    /// Окно уже получало фокус (после этого его потеря значит «щёлкнули вне меню»).
    had_focus: bool,
    /// Меню пора закрыть.
    done: bool,
    /// Выбранный пункт.
    chosen: Option<TrayCommand>,
}

impl Feedback {
    /// Учесть кадр окна меню. `dismissed`: нажали Esc или Windows просит закрыть окно.
    fn record(&mut self, chosen: Option<TrayCommand>, focused: Option<bool>, dismissed: bool) {
        self.had_focus |= focused == Some(true);
        let lost_focus = self.had_focus && focused == Some(false);
        self.chosen = self.chosen.or(chosen);
        self.done |= self.chosen.is_some() || dismissed || lost_focus;
    }
}

/// Один пункт меню.
struct Entry {
    command: TrayCommand,
    icon: Icon,
    label: &'static str,
    /// Перед пунктом рисуется разделитель.
    separator_before: bool,
}

/// Пункты по порядку. Подпись и значок паузы зависят от того, играет ли музыка.
fn entries(playing: bool) -> [Entry; 5] {
    let (pause_icon, pause_label) = if playing { (Icon::Pause, "Пауза") } else { (Icon::Play, "Играть") };
    [
        Entry { command: TrayCommand::Show, icon: Icon::Home, label: "Открыть Mcrate", separator_before: false },
        Entry { command: TrayCommand::TogglePause, icon: pause_icon, label: pause_label, separator_before: true },
        Entry { command: TrayCommand::Previous, icon: Icon::Prev, label: "Предыдущий трек", separator_before: false },
        Entry { command: TrayCommand::Next, icon: Icon::Next, label: "Следующий трек", separator_before: false },
        Entry { command: TrayCommand::Quit, icon: Icon::Close, label: "Выход", separator_before: true },
    ]
}

/// Размер окна меню: пункты, разделители и отступы карточки.
fn menu_size(entries: &[Entry]) -> Vec2 {
    let separators = entries.iter().filter(|e| e.separator_before).count() as f32;
    let height = entries.len() as f32 * ITEM_HEIGHT + separators * SEPARATOR_HEIGHT + 2.0 * f32::from(MENU_PADDING);
    vec2(MENU_WIDTH, height)
}

/// Где поставить окно меню (левый верхний угол в точках egui). `cursor`: физические пиксели, `scale`: пикселей в
/// одной точке, `monitor`: размер экрана в точках, если известен.
fn menu_origin(cursor: (i32, i32), scale: f32, monitor: Option<Vec2>, size: Vec2) -> Pos2 {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let at = Pos2::new(cursor.0 as f32 / scale, cursor.1 as f32 / scale);
    let (screen_w, screen_h) = monitor.map_or((f32::INFINITY, f32::INFINITY), |m| (m.x, m.y));
    let x = (at.x - size.x).clamp(SCREEN_MARGIN, (screen_w - size.x - SCREEN_MARGIN).max(SCREEN_MARGIN));
    // Панель задач чаще внизу: меню над курсором. Курсор в верхней половине экрана: панель вверху, меню под ним.
    let taskbar_at_top = screen_h.is_finite() && at.y < screen_h / 2.0;
    let y = if taskbar_at_top {
        (at.y + CURSOR_GAP).min((screen_h - size.y - SCREEN_MARGIN).max(SCREEN_MARGIN))
    } else {
        (at.y - size.y - CURSOR_GAP).max(SCREEN_MARGIN)
    };
    Pos2::new(x, y)
}

impl McrateApp {
    /// Открывает меню у курсора (правый клик по иконке). Если меню уже открыто, переносит его.
    pub(crate) fn open_tray_menu(&mut self, ctx: &Context, cursor: (i32, i32)) {
        let scale = ctx.pixels_per_point(); // учитывает и масштаб Windows, и zoom_factor («Размер текста»)
        let monitor = ctx.input(|i| i.viewport().monitor_size);
        let size = menu_size(&entries(self.ctl.is_playing()));
        let pos = menu_origin(cursor, scale, monitor, size);
        self.tray_menu = Some(OpenTrayMenu { pos, opened_at: ctx.input(|i| i.time), feedback: Arc::default() });
        ctx.request_repaint();
    }

    /// Открыто ли меню (пока открыто, скрытому главному окну нужно рисовать кадры, см. `background.rs`).
    pub(crate) fn tray_menu_is_open(&self) -> bool {
        self.tray_menu.is_some()
    }

    /// Выполняет выбранное в меню, а пока оно открыто, объявляет его окно. Раз за кадр из `App::ui`.
    pub(crate) fn show_tray_menu(&mut self, ctx: &Context, theme: &Theme) {
        let Some(open) = &self.tray_menu else { return };
        let feedback = *lock(&open.feedback);
        let never_focused = !feedback.had_focus && ctx.input(|i| i.time) - open.opened_at > FOCUS_TIMEOUT_SECS;
        if feedback.done || never_focused {
            self.tray_menu = None; // окно не объявляем: egui закроет его
            ctx.request_repaint();
            if let Some(command) = feedback.chosen {
                self.run_tray_command(ctx, command);
            }
            return;
        }
        declare_menu_window(ctx, theme, open, self.ctl.is_playing());
    }
}

/// Объявляет отложенное окно меню на этот кадр.
fn declare_menu_window(ctx: &Context, theme: &Theme, open: &OpenTrayMenu, playing: bool) {
    let id = ViewportId::from_hash_of("mcrate_tray_menu");
    let builder = ViewportBuilder::default()
        .with_title(MENU_TITLE)
        .with_inner_size(menu_size(&entries(playing)))
        .with_position(open.pos)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_taskbar(false)
        .with_always_on_top()
        .with_active(true);
    let (theme, feedback) = (theme.clone(), Arc::clone(&open.feedback));
    ctx.show_viewport_deferred(id, builder, move |ui, _class| menu_window(ui, &theme, playing, &feedback));
    if !lock(&open.feedback).had_focus {
        ctx.request_repaint_of(id); // пока окно не получило фокус, не даём ему застыть пустым
    }
}

/// Кадр окна меню.
fn menu_window(ui: &mut Ui, theme: &Theme, playing: bool, feedback: &Mutex<Feedback>) {
    if !lock(feedback).had_focus {
        ui.ctx().send_viewport_cmd(ViewportCommand::Focus); // чтобы потеря фокуса означала «щёлкнули вне меню»
        let p = &theme.palette;
        platform::style_popup_window(MENU_TITLE, p.surface_container, p.outline_variant, p.dark);
    }
    let chosen = draw_menu(ui, theme, &entries(playing));
    let (focused, close_requested) = ui.input(|i| (i.viewport().focused, i.viewport().close_requested()));
    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    let done = {
        let mut feedback = lock(feedback);
        feedback.record(chosen, focused, escape || close_requested);
        feedback.done
    };
    if done {
        ui.ctx().request_repaint_of(ViewportId::ROOT); // главное окно выполнит выбор и закроет меню
    }
}

/// Замок на ячейку; после паники в другом кадре берём значение как есть.
fn lock(feedback: &Mutex<Feedback>) -> std::sync::MutexGuard<'_, Feedback> {
    feedback.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Карточка меню на всё окно и пункты в ней. Возвращает выбранную команду.
fn draw_menu(ui: &mut Ui, theme: &Theme, entries: &[Entry]) -> Option<TrayCommand> {
    // Форму окна задаёт Windows, поэтому у карточки нет своих углов, рамки и тени.
    let frame = if theme.glass_active() { glass_frame(theme) } else { m3_frame(theme) };
    let frame = frame.corner_radius(CornerRadius::ZERO).stroke(Stroke::NONE).shadow(Shadow::NONE);
    CentralPanel::default()
        .frame(Frame::NONE)
        .show(ui, |ui| {
            frame
                .show(ui, |ui| {
                    ui.set_width(MENU_WIDTH);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let mut chosen = None;
                    for entry in entries {
                        if entry.separator_before {
                            separator(ui, theme);
                        }
                        if menu_item(ui, theme, entry.icon, entry.label) {
                            chosen = Some(entry.command);
                        }
                    }
                    chosen
                })
                .inner
        })
        .inner
}

/// Тонкая линия между группами пунктов.
fn separator(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(vec2(MENU_WIDTH, SEPARATOR_HEIGHT), egui::Sense::hover());
    let y = rect.center().y;
    ui.painter().hline(rect.x_range(), y, Stroke::new(1.0, theme.palette.outline_variant));
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Vec2 = vec2(1920.0, 1080.0);

    fn size() -> Vec2 {
        menu_size(&entries(false))
    }

    #[test]
    fn menu_with_taskbar_at_bottom_opens_above_cursor() {
        let s = size();
        let pos = menu_origin((1800, 1060), 1.0, Some(SCREEN), s);
        assert!(pos.y + s.y <= 1060.0, "меню целиком над курсором");
        assert!(pos.y >= SCREEN_MARGIN);
    }

    #[test]
    fn menu_with_taskbar_at_top_opens_below_cursor() {
        let s = size();
        let pos = menu_origin((1800, 20), 1.0, Some(SCREEN), s);
        assert!(pos.y >= 20.0, "меню под курсором");
    }

    #[test]
    fn menu_right_edge_is_at_cursor() {
        let s = size();
        let pos = menu_origin((1500, 1060), 1.0, Some(SCREEN), s);
        assert_eq!(pos.x + s.x, 1500.0);
        assert_eq!(pos.y + s.y, 1060.0 - CURSOR_GAP);
    }

    #[test]
    fn menu_stays_inside_the_screen_horizontally() {
        let s = size();
        let right = menu_origin((1919, 1060), 1.0, Some(SCREEN), s);
        assert!(right.x + s.x <= SCREEN.x - SCREEN_MARGIN + 0.01);
        let left = menu_origin((0, 1060), 1.0, Some(SCREEN), s);
        assert_eq!(left.x, SCREEN_MARGIN);
    }

    #[test]
    fn physical_pixels_are_divided_by_screen_scale() {
        // Экран 3840x2160 при масштабе 2 это 1920x1080 точек; курсор в (3000, 2100) пикселей это (1500, 1050) точек.
        let s = size();
        let pos = menu_origin((3000, 2100), 2.0, Some(SCREEN), s);
        assert!((pos.x + s.x - 1500.0).abs() < 0.01);
        assert!(pos.y + s.y <= 1050.0);
    }

    #[test]
    fn unknown_monitor_still_gives_a_sane_position() {
        let s = size();
        let pos = menu_origin((500, 400), 1.0, None, s);
        assert!(pos.x >= SCREEN_MARGIN && pos.y >= SCREEN_MARGIN);
        assert_eq!(menu_origin((500, 400), 0.0, None, s), pos, "нулевой масштаб считается за 1");
    }

    #[test]
    fn menu_closes_on_focus_loss_only_after_it_had_focus() {
        let mut feedback = Feedback::default();
        feedback.record(None, Some(false), false);
        assert!(!feedback.done, "фокуса ещё не было");
        feedback.record(None, Some(true), false);
        assert!(!feedback.done);
        feedback.record(None, Some(false), false);
        assert!(feedback.done);
    }

    #[test]
    fn chosen_item_is_kept_until_the_main_window_reads_it() {
        let mut feedback = Feedback::default();
        feedback.record(Some(TrayCommand::Quit), Some(true), false);
        feedback.record(None, Some(false), false);
        assert!(feedback.done);
        assert_eq!(feedback.chosen, Some(TrayCommand::Quit));
    }

    #[test]
    fn pause_item_follows_playback_state() {
        assert_eq!(entries(true)[1].label, "Пауза");
        assert_eq!(entries(false)[1].label, "Играть");
        assert!(entries(true).iter().all(|e| e.command != TrayCommand::OpenMenu { x: 0, y: 0 }));
    }

    #[test]
    fn menu_size_counts_items_separators_and_padding() {
        let e = entries(false);
        let expected_h = 5.0 * ITEM_HEIGHT + 2.0 * SEPARATOR_HEIGHT + 2.0 * f32::from(MENU_PADDING);
        assert_eq!(menu_size(&e), vec2(MENU_WIDTH, expected_h));
    }
}

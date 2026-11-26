//! Работа в фоне: окно скрывается в трей, музыка продолжает играть.
//!
//! Что здесь решается:
//! - Кнопка закрытия окна (крестик, Alt+F4) прячет окно в трей, если включён переключатель
//!   «Сворачивать в трей при закрытии» (`Preferences::background_play`) и иконка в трее создалась.
//!   Иначе приложение закрывается как раньше.
//! - Настоящий выход: пункт «Выход» в меню иконки.
//! - Команды иконки и глобальные медиаклавиши выполняются и при скрытом окне.
//!
//! Как это работает с eframe 0.37: пока окно скрыто (`ViewportCommand::Visible(false)`), он не рисует интерфейс
//! и не вызывает `App::ui`, но на каждый `request_repaint` зовёт `App::logic`. Поэтому всё, что должно жить в
//! фоне, вызывается из `logic` (`McrateApp::background_logic`). В обычном режиме `logic` идёт в том же проходе
//! прямо перед `ui`. Читать клавиатуру и мышь в `logic` нельзя: у скрытого окна egui отдаёт там события
//! последнего показанного кадра, команды сработали бы повторно. Поэтому ввод остаётся в `ui` (ui/shortcuts.rs).
//!
//! Пока окно скрыто, никто не зовёт `ui`, а значит и `Controller::tick`, который забирает события движка. Его
//! вызывает `background_logic` по таймеру (`HIDDEN_TICK`), пока окно скрыто.

use std::time::Duration;

use eframe::egui::{Context, ViewportCommand};

use crate::app::McrateApp;
use crate::tray::{TrayCommand, tooltip_text};
use crate::ui::format::artists_text;
use crate::ui::shortcuts::Shortcut;

/// Как часто пробуждаем скрытое окно, чтобы забрать события движка и обновить подсказку иконки.
const HIDDEN_TICK: Duration = Duration::from_millis(500);

impl McrateApp {
    /// Всё, что должно работать независимо от того, видно ли окно. Вызывается из `App::logic` перед каждым кадром
    /// и, пока окно скрыто, по `request_repaint`.
    pub(crate) fn background_logic(&mut self, ctx: &Context) {
        self.handle_close_request(ctx);
        self.run_background_commands(ctx);
        self.update_tray_tooltip();
        if self.hidden_to_tray {
            // Окно скрыто: `ui` не вызывается, движок опрашиваем сами и просим разбудить нас снова.
            self.ctl.tick();
            ctx.request_repaint_after(HIDDEN_TICK);
        }
    }

    /// Крестик окна: прячем в трей вместо выхода, если так настроено. Свой выход (`quitting`) не перехватываем.
    fn handle_close_request(&mut self, ctx: &Context) {
        if self.quitting || !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.prefs.background_play && self.tray.is_some() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_to_tray(ctx);
        }
    }

    /// Выполняет команды меню иконки и медиаклавиши с клавиатуры (они приходят из системы и в фоне).
    fn run_background_commands(&mut self, ctx: &Context) {
        let tray_commands = self.tray.as_ref().map(|t| t.poll()).unwrap_or_default();
        for command in tray_commands {
            match command {
                TrayCommand::Show => self.show_from_tray(ctx),
                TrayCommand::TogglePause => self.run_shortcut(Shortcut::TogglePause),
                TrayCommand::Next => self.run_shortcut(Shortcut::NextTrack),
                TrayCommand::Previous => self.run_shortcut(Shortcut::PrevTrack),
                TrayCommand::Quit => self.quit(ctx),
            }
        }
        let media_keys = self.media_keys.as_ref().map(|k| k.poll()).unwrap_or_default();
        for key in media_keys {
            self.run_shortcut(Shortcut::from(key));
        }
    }

    /// Прячет окно: пропадает с экрана и из панели задач, остаётся иконка в трее.
    fn hide_to_tray(&mut self, ctx: &Context) {
        self.hidden_to_tray = true;
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        ctx.request_repaint(); // чтобы `logic` вызвалась сразу и запустила таймер опроса движка
    }

    /// Возвращает окно из трея на передний план (и разворачивает, если оно было свёрнуто).
    fn show_from_tray(&mut self, ctx: &Context) {
        self.hidden_to_tray = false;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        ctx.request_repaint();
    }

    /// Настоящий выход (пункт «Выход» в меню иконки). Помечаем, чтобы `handle_close_request` не спрятал окно снова.
    fn quit(&mut self, ctx: &Context) {
        self.quitting = true;
        ctx.send_viewport_cmd(ViewportCommand::Close);
        ctx.request_repaint(); // скрытому окну нужен ещё один проход, чтобы заметить запрос закрытия
    }

    /// Подсказка иконки: играющий трек или имя приложения.
    fn update_tray_tooltip(&mut self) {
        let Some(tray) = self.tray.as_mut() else { return };
        let now = self.ctl.now.as_ref().map(|t| (artists_text(t), t.title.clone()));
        let text = tooltip_text(now.as_ref().map(|(artists, title)| (artists.as_str(), title.as_str())));
        tray.set_tooltip(&text);
    }
}

//! Иконка в трее (области уведомлений Windows): окно можно скрыть, музыка продолжит играть.
//!
//! Что умеет иконка:
//! - левый клик возвращает окно;
//! - правый клик открывает меню: «Открыть Mcrate», пауза/играть, предыдущий и следующий трек, «Выход»;
//! - подсказка при наведении показывает играющий трек.
//!
//! Меню рисует само приложение (`ui/tray_menu.rs`) в цветах текущей темы: системное меню Windows перекрасить нельзя.
//! Иконка лишь сообщает, что нажали правую кнопку и где (`TrayCommand::OpenMenu`). Запасной вариант: если запустить
//! с переменной окружения `MCRATE_NATIVE_TRAY_MENU=1`, меню будет системным (на случай, если своё не показывается).
//!
//! Этот модуль только про иконку и меню (обвязка ОС, как `platform.rs` и `media_keys.rs`). Что делать со скрытым
//! окном, решает `background.rs`: он же перехватывает закрытие окна.
//!
//! Как события доходят до приложения: меню и клики приходят в главном потоке из цикла сообщений Windows (его даёт
//! winit внутри eframe). Обработчик кладёт команду в канал и зовёт `request_repaint`. Пока окно скрыто, eframe не
//! рисует интерфейс, но на `request_repaint` вызывает `App::logic`, и там команда выполняется (см. `background.rs`).
//!
//! Иконка подбирается под тему панели задач: на тёмной белая «печенька», на светлой чёрная. Тема читается один раз
//! при запуске, на смену темы во время работы иконка не реагирует.
//!
//! Не Windows: `Tray::new` возвращает `None`, окно закрывается как обычно.

/// Есть ли трей на этой ОС (экран настроек прячет переключатель, если его нет).
pub(crate) const SUPPORTED: bool = cfg!(windows);

/// Что просят пункты меню и клики по иконке.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(windows), allow(dead_code))] // на не-Windows иконку никто не создаёт
pub(crate) enum TrayCommand {
    /// Показать окно и вывести его на передний план.
    Show,
    TogglePause,
    Next,
    Previous,
    /// Правый клик по иконке: открыть меню приложения. Координаты курсора в физических пикселях экрана.
    OpenMenu {
        x: i32,
        y: i32,
    },
    /// Закрыть приложение по-настоящему (кнопка «закрыть» в окне только прячет его).
    Quit,
}

/// Идентификаторы пунктов меню: по ним обработчик узнаёт, что выбрали.
const ID_SHOW: &str = "show";
const ID_TOGGLE: &str = "toggle";
const ID_NEXT: &str = "next";
const ID_PREVIOUS: &str = "previous";
const ID_QUIT: &str = "quit";

/// Имя приложения: подсказка, когда ничего не играет.
const APP_NAME: &str = "Mcrate";
/// Windows режет подсказку иконки на 127 символах (128 вместе с нулём в конце).
const TOOLTIP_MAX_CHARS: usize = 127;

/// Какая команда стоит за пунктом меню с таким id (`None`: чужой id).
#[cfg_attr(not(windows), allow(dead_code))]
fn command_for_menu_id(id: &str) -> Option<TrayCommand> {
    match id {
        ID_SHOW => Some(TrayCommand::Show),
        ID_TOGGLE => Some(TrayCommand::TogglePause),
        ID_NEXT => Some(TrayCommand::Next),
        ID_PREVIOUS => Some(TrayCommand::Previous),
        ID_QUIT => Some(TrayCommand::Quit),
        _ => None,
    }
}

/// Текст подсказки: «Исполнитель — Название», а без трека просто имя приложения. Длинный текст обрезается
/// с многоточием по символам (не по байтам: в названиях бывает кириллица).
pub(crate) fn tooltip_text(now: Option<(&str, &str)>) -> String {
    let Some((artists, title)) = now else { return APP_NAME.to_string() };
    let full = if artists.is_empty() { title.to_string() } else { format!("{artists} — {title}") };
    if full.chars().count() <= TOOLTIP_MAX_CHARS {
        return full;
    }
    let mut cut: String = full.chars().take(TOOLTIP_MAX_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// Разбирает вывод `reg query ... /v SystemUsesLightTheme`: строка вида `SystemUsesLightTheme    REG_DWORD    0x1`.
/// `true`, только если значение явно 1; любая неясность считается тёмной темой (так по умолчанию в Windows 11).
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_light_theme(reg_output: &str) -> bool {
    reg_output
        .lines()
        .find(|line| line.contains("SystemUsesLightTheme"))
        .and_then(|line| line.split_whitespace().last())
        .is_some_and(|value| value.eq_ignore_ascii_case("0x1"))
}

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::mpsc::{self, Receiver};

    use tray_icon::menu::{Menu, MenuEvent, MenuEventHandler, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    use super::{APP_NAME, ID_NEXT, ID_PREVIOUS, ID_QUIT, ID_SHOW, ID_TOGGLE, TrayCommand, command_for_menu_id, parse_light_theme};

    /// Иконка 32x32 для тёмной панели задач (белая «печенька») и для светлой (чёрная). Сырой RGBA, см. assets/README.md.
    const ICON_FOR_DARK_TASKBAR: &[u8] = include_bytes!("../assets/tray_white_32.rgba");
    const ICON_FOR_LIGHT_TASKBAR: &[u8] = include_bytes!("../assets/tray_black_32.rgba");
    const ICON_SIZE: u32 = 32;
    /// Флаг запуска процесса без окна консоли (иначе `reg` на миг мигнёт чёрным окном).
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// Иконка в трее. Пока значение живо, иконка видна; при удалении (выход из приложения) исчезает.
    pub(crate) struct Tray {
        icon: TrayIcon,
        commands: Receiver<TrayCommand>,
        /// Какая подсказка показана сейчас: Windows перерисовывать зря не заставляем.
        tooltip: String,
    }

    impl Tray {
        /// Создаёт иконку и меню. `None`, если Windows не дала создать иконку (тогда окно закрывается как обычно).
        /// `ctx` нужен, чтобы разбудить приложение, когда выбрали пункт меню: скрытое окно само не перерисуется.
        pub(crate) fn new(ctx: &egui::Context) -> Option<Self> {
            let native_menu = std::env::var_os("MCRATE_NATIVE_TRAY_MENU").is_some();
            let (tx, commands) = mpsc::channel();
            install_handlers(ctx, tx, native_menu);

            let menu = Menu::new();
            let show = MenuItem::with_id(ID_SHOW, "Открыть Mcrate", true, None);
            let toggle = MenuItem::with_id(ID_TOGGLE, "Пауза / Играть", true, None);
            let previous = MenuItem::with_id(ID_PREVIOUS, "Предыдущий трек", true, None);
            let next = MenuItem::with_id(ID_NEXT, "Следующий трек", true, None);
            let quit = MenuItem::with_id(ID_QUIT, "Выход", true, None);
            let separator = PredefinedMenuItem::separator();
            menu.append_items(&[&show, &separator, &toggle, &previous, &next, &PredefinedMenuItem::separator(), &quit]).ok()?;

            let bytes = if taskbar_uses_light_theme() { ICON_FOR_LIGHT_TASKBAR } else { ICON_FOR_DARK_TASKBAR };
            let icon = Icon::from_rgba(bytes.to_vec(), ICON_SIZE, ICON_SIZE).ok()?;
            let mut builder = TrayIconBuilder::new().with_icon(icon).with_tooltip(APP_NAME);
            if native_menu {
                // Левый клик возвращает окно, системное меню только по правому.
                builder = builder.with_menu(Box::new(menu)).with_menu_on_left_click(false);
            }
            let tray = builder.build().ok()?;
            Some(Self { icon: tray, commands, tooltip: APP_NAME.to_string() })
        }

        /// Видит ли Windows иконку. `tray-icon` при неудачной регистрации (панель задач ещё не готова) молча ждёт
        /// и создаёт объект всё равно, поэтому «объект есть» не значит «иконка на экране». Не видна: окно прятать
        /// нельзя, вернуть его было бы нечем.
        pub(crate) fn is_visible(&self) -> bool {
            self.icon.rect().is_some()
        }

        /// Забирает команды, накопившиеся с прошлого раза.
        pub(crate) fn poll(&self) -> Vec<TrayCommand> {
            self.commands.try_iter().collect()
        }

        /// Меняет подсказку, только если текст другой.
        pub(crate) fn set_tooltip(&mut self, text: &str) {
            if self.tooltip != text && self.icon.set_tooltip(Some(text)).is_ok() {
                self.tooltip = text.to_string();
            }
        }
    }

    /// Подписывается на пункты меню и клики по иконке. Обработчики общие на весь процесс и ставятся один раз.
    fn install_handlers(ctx: &egui::Context, tx: mpsc::Sender<TrayCommand>, native_menu: bool) {
        let (menu_tx, menu_ctx) = (tx.clone(), ctx.clone());
        let on_menu: MenuEventHandler = Arc::new(move |event: MenuEvent| {
            if let Some(command) = command_for_menu_id(&event.id.0) {
                let _ = menu_tx.send(command);
                menu_ctx.request_repaint();
            }
        });
        let _ = MenuEvent::set_event_handler(Some(on_menu));

        let click_ctx = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            // Реагируем на отпускание левой кнопки: нажатие и отпускание приходят двумя событиями.
            let command = match event {
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => Some(TrayCommand::Show),
                TrayIconEvent::Click { button: MouseButton::Right, button_state: MouseButtonState::Up, position, .. } if !native_menu => {
                    Some(TrayCommand::OpenMenu { x: position.x.round() as i32, y: position.y.round() as i32 })
                }
                _ => None,
            };
            if let Some(command) = command {
                let _ = tx.send(command);
                click_ctx.request_repaint();
            }
        }));
    }

    /// Светлая ли тема панели задач Windows (значение `SystemUsesLightTheme` в реестре). Читаем программой `reg`, чтобы
    /// не тянуть крейт для реестра ради одного числа. Не вышло прочитать: считаем тему тёмной (по умолчанию в Windows 11).
    fn taskbar_uses_light_theme() -> bool {
        Command::new("reg")
            .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "/v", "SystemUsesLightTheme"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .is_ok_and(|out| parse_light_theme(&String::from_utf8_lossy(&out.stdout)))
    }
}

#[cfg(not(windows))]
mod imp {
    use super::TrayCommand;

    /// Заглушка: трея вне Windows нет.
    pub(crate) struct Tray;

    impl Tray {
        pub(crate) fn new(_ctx: &egui::Context) -> Option<Self> {
            None
        }

        pub(crate) fn is_visible(&self) -> bool {
            false
        }

        pub(crate) fn poll(&self) -> Vec<TrayCommand> {
            Vec::new()
        }

        pub(crate) fn set_tooltip(&mut self, _text: &str) {}
    }
}

pub(crate) use imp::Tray;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_commands() {
        assert_eq!(command_for_menu_id(ID_SHOW), Some(TrayCommand::Show));
        assert_eq!(command_for_menu_id(ID_TOGGLE), Some(TrayCommand::TogglePause));
        assert_eq!(command_for_menu_id(ID_NEXT), Some(TrayCommand::Next));
        assert_eq!(command_for_menu_id(ID_PREVIOUS), Some(TrayCommand::Previous));
        assert_eq!(command_for_menu_id(ID_QUIT), Some(TrayCommand::Quit));
        assert_eq!(command_for_menu_id("чужой"), None);
    }

    #[test]
    fn tooltip_shows_track_or_app_name() {
        assert_eq!(tooltip_text(None), "Mcrate");
        assert_eq!(tooltip_text(Some(("Земфира", "Хочешь?"))), "Земфира — Хочешь?");
        assert_eq!(tooltip_text(Some(("", "Без исполнителя"))), "Без исполнителя");
    }

    #[test]
    fn long_tooltip_is_cut_by_chars_not_bytes() {
        let long = "я".repeat(300); // 2 байта на символ: по байтам обрезка попала бы в середину буквы
        let text = tooltip_text(Some(("Группа", &long)));
        assert_eq!(text.chars().count(), TOOLTIP_MAX_CHARS);
        assert!(text.ends_with('…'));
        assert!(text.starts_with("Группа — я"));
    }

    #[test]
    fn tooltip_of_exact_limit_is_kept_whole() {
        let exact = "a".repeat(TOOLTIP_MAX_CHARS);
        assert_eq!(tooltip_text(Some(("", &exact))), exact);
    }

    #[test]
    fn light_theme_is_read_from_reg_output() {
        let light = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\r\n    SystemUsesLightTheme    REG_DWORD    0x1\r\n\r\n";
        let dark = light.replace("0x1", "0x0");
        assert!(parse_light_theme(light));
        assert!(!parse_light_theme(&dark));
    }

    #[test]
    fn unclear_reg_output_means_dark_theme() {
        assert!(!parse_light_theme(""));
        assert!(!parse_light_theme("ERROR: The system was unable to find the specified registry key or value."));
        assert!(!parse_light_theme("SystemUsesLightTheme"));
    }
}

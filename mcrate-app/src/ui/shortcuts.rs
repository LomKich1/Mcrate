//! Клавиатура и кнопки мыши: навигация как в браузере и управление воспроизведением. Работает на всех
//! экранах, одинаково в Material 3 и Liquid Glass (экраны о вводе ничего не знают).
//!
//! | Что нажали                                   | Что происходит                                    |
//! |----------------------------------------------|---------------------------------------------------|
//! | кнопка мыши «Назад» (4), Alt+←, клавиша «Назад» | на предыдущий экран (история, см. `nav_history`)  |
//! | кнопка мыши «Вперёд» (5), Alt+→, клавиша «Вперёд» | на следующий экран                              |
//! | Esc                                          | то же, что «Назад»; сначала закрывает окна и меню |
//! | зажатая средняя кнопка мыши                  | прокрутка как в браузере, см. `autoscroll`        |
//! | Пробел, клавиша Play/Pause                   | пауза / играть                                    |
//! | Ctrl+→, клавиша «Следующий трек»             | следующий трек                                    |
//! | Ctrl+←, клавиша «Предыдущий трек»            | предыдущий трек (после 3 с перезапуск, как в движке) |
//! | клавиша Stop                                 | пауза, если играет                                |
//!
//! Чтение ввода (`read_shortcuts`) отделено от действий (`run_shortcut`): первое проверяется тестами на
//! настоящем `egui::InputState`. Звуком управляет только `Controller`, здесь лишь вызываются его методы.
//! Медиаклавиши, когда окно не в фокусе, приходят из `crate::media_keys` и выполняются в `background.rs`
//! (там же команды трея): им нужен `App::logic`, который работает и при скрытом окне.

use egui::{Context, Event, InputState, Key, PointerButton};

use crate::app::{McrateApp, View};
use crate::media_keys::MediaKey;
use crate::ui::nav_history::Place;

/// Что просит сделать нажатая клавиша или кнопка мыши.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shortcut {
    /// Шаг назад по истории экранов.
    Back,
    /// Шаг вперёд по истории экранов.
    Forward,
    /// Esc: выйти с экрана (шаг назад). Отдельно от `Back`, потому что у него свои условия (см. `InputGuard`).
    Escape,
    TogglePause,
    /// Клавиша Stop: ставит на паузу, но не запускает играть.
    Stop,
    NextTrack,
    PrevTrack,
}

/// Состояние интерфейса, от которого зависит, какие нажатия считать командами.
#[derive(Clone, Copy, Default)]
pub(crate) struct InputGuard {
    /// В фокусе какой-то виджет с клавиатурным вводом (поле ввода, слайдер): пробел и Ctrl+стрелки принадлежат ему.
    pub widget_focused: bool,
    /// В фокусе именно текстовое поле (уже точнее, чем `widget_focused`): Esc нужен ему, чтобы закончить ввод.
    pub text_focused: bool,
    /// Текстовое поле было в фокусе в прошлом кадре. egui снимает фокус с поля по Esc ещё до нашего кода, так что
    /// без этого флага тот же Esc, которым закрыли ввод, сразу увёл бы с экрана.
    pub text_focused_last_frame: bool,
    /// Открыто окно или меню: Esc и кнопки мыши принадлежат ему (оно закроется само), экран не трогаем.
    pub overlay_open: bool,
}

/// Читает команды из ввода одного кадра. Повтор клавиши при удержании не считается: иначе зажатый Esc пролистал бы
/// всю историю, а зажатая клавиша «Следующий трек» перескочила бы весь список.
pub(crate) fn read_shortcuts(input: &InputState, guard: &InputGuard) -> Vec<Shortcut> {
    let mut found = Vec::new();
    for event in &input.events {
        match *event {
            Event::Key { key, pressed: true, repeat: false, modifiers, .. } => {
                if let Some(shortcut) = key_shortcut(key, modifiers, guard) {
                    found.push(shortcut);
                }
            }
            Event::PointerButton { button, pressed: true, .. } if !guard.overlay_open => match button {
                PointerButton::Extra1 => found.push(Shortcut::Back),
                PointerButton::Extra2 => found.push(Shortcut::Forward),
                _ => {}
            },
            _ => {}
        }
    }
    found
}

/// Что означает одно нажатие клавиши (`None`: это не наша клавиша).
fn key_shortcut(key: Key, modifiers: egui::Modifiers, guard: &InputGuard) -> Option<Shortcut> {
    let navigation_free = !guard.overlay_open;
    let keys_free = !guard.widget_focused && !guard.overlay_open;
    match key {
        // Медиаклавиши клавиатуры: поле ввода им не мешает, окну тоже.
        Key::MediaPlayPause => Some(Shortcut::TogglePause),
        Key::MediaStop => Some(Shortcut::Stop),
        Key::MediaTrackNext => Some(Shortcut::NextTrack),
        Key::MediaTrackPrevious => Some(Shortcut::PrevTrack),
        // Клавиши «Назад»/«Вперёд» на мультимедийных клавиатурах.
        Key::BrowserBack if navigation_free => Some(Shortcut::Back),
        Key::BrowserForward if navigation_free => Some(Shortcut::Forward),
        Key::Escape if navigation_free && !guard.text_focused && !guard.text_focused_last_frame => Some(Shortcut::Escape),
        // Alt+стрелки, как в браузере. Ctrl+стрелки ниже заняты треками, поэтому Alt без Ctrl.
        Key::ArrowLeft if navigation_free && modifiers.alt && !modifiers.command => Some(Shortcut::Back),
        Key::ArrowRight if navigation_free && modifiers.alt && !modifiers.command => Some(Shortcut::Forward),
        Key::ArrowRight if keys_free && modifiers.command && !modifiers.alt => Some(Shortcut::NextTrack),
        Key::ArrowLeft if keys_free && modifiers.command && !modifiers.alt => Some(Shortcut::PrevTrack),
        // Пробел без модификаторов; прежнее поведение приложения (пауза), но без повтора при удержании.
        // Окна его не блокируют: так было и раньше.
        Key::Space if !guard.widget_focused && modifiers.is_none() => Some(Shortcut::TogglePause),
        _ => None,
    }
}

impl From<MediaKey> for Shortcut {
    fn from(key: MediaKey) -> Self {
        match key {
            MediaKey::PlayPause => Shortcut::TogglePause,
            MediaKey::Stop => Shortcut::Stop,
            MediaKey::Next => Shortcut::NextTrack,
            MediaKey::Previous => Shortcut::PrevTrack,
        }
    }
}

impl McrateApp {
    /// Читает клавиатуру, кнопки мыши и глобальные медиаклавиши и выполняет команды. Раз за кадр, до отрисовки.
    pub(crate) fn handle_shortcuts(&mut self, ctx: &Context) {
        // Сначала записываем переходы прошлого кадра: иначе шаг «назад» сравнил бы историю со старым экраном.
        self.record_navigation();

        let guard = InputGuard {
            widget_focused: ctx.egui_wants_keyboard_input(),
            text_focused: ctx.text_edit_focused(),
            text_focused_last_frame: self.text_focused_last_frame,
            overlay_open: self.overlay_open(ctx),
        };
        // Прокрутка средней кнопкой: Esc сначала останавливает её и не уводит с экрана.
        let escape_stopped_autoscroll = self.step_autoscroll(ctx, !guard.overlay_open);
        let mut shortcuts = ctx.input(|i| read_shortcuts(i, &guard));
        if escape_stopped_autoscroll {
            shortcuts.retain(|s| *s != Shortcut::Escape);
        }
        for shortcut in shortcuts {
            self.run_shortcut(shortcut);
        }
    }

    /// Запоминает, было ли в конце кадра в фокусе текстовое поле (нужно Esc, см. `InputGuard::text_focused_last_frame`).
    pub(crate) fn remember_typing(&mut self, ctx: &Context) {
        self.text_focused_last_frame = ctx.text_edit_focused();
    }

    /// Открыто ли что-то поверх экрана, что само закрывается по Esc: меню трека, окна плейлистов и любое
    /// модальное окно egui (например, «Очистить кэш?»). Модальные окна рисуются позже по кадру, поэтому тут видно
    /// состояние прошлого кадра: этого достаточно, окно не может открыться и закрыться в одном кадре.
    pub(crate) fn overlay_open(&self, ctx: &Context) -> bool {
        self.track_menu.is_some() || self.playlist_dialog.is_some() || ctx.memory(|m| m.top_modal_layer().is_some())
    }

    /// Выполняет одну команду. Её же зовут команды из трея и глобальные медиаклавиши (`background.rs`).
    pub(crate) fn run_shortcut(&mut self, shortcut: Shortcut) {
        match shortcut {
            Shortcut::Back | Shortcut::Escape => self.navigate_back(),
            Shortcut::Forward => self.navigate_forward(),
            Shortcut::TogglePause => self.ctl.toggle_pause(),
            Shortcut::Stop => {
                if self.ctl.is_playing() {
                    self.ctl.toggle_pause();
                }
            }
            Shortcut::NextTrack => self.ctl.next(),
            Shortcut::PrevTrack => self.ctl.prev(),
        }
    }

    /// Сообщает истории, где пользователь сейчас. Экраны переключаются присваиванием `self.view`
    /// где угодно, так что история узнаёт об этом здесь, один раз за кадр.
    fn record_navigation(&mut self) {
        self.nav.observe(Place::new(self.view, self.open_playlist));
    }

    /// Шаг назад по истории экранов (кнопка мыши 4, Alt+←, Esc).
    pub(crate) fn navigate_back(&mut self) {
        let ctl = &self.ctl;
        if let Some(place) = self.nav.back(|p| place_exists(ctl, p)) {
            self.open_place(place);
        }
    }

    /// Шаг вперёд по истории экранов (кнопка мыши 5, Alt+→).
    pub(crate) fn navigate_forward(&mut self) {
        let ctl = &self.ctl;
        if let Some(place) = self.nav.forward(|p| place_exists(ctl, p)) {
            self.open_place(place);
        }
    }

    /// Открывает место, найденное историей. История уже считает его текущим, поэтому следующий кадр
    /// не запишет этот переход как новый.
    fn open_place(&mut self, place: Place) {
        if place.view == View::CollectionPlaylist {
            self.open_playlist = place.playlist;
        }
        if place.view == View::NowPlaying {
            // Пока «Сейчас играет» открыт, в меню подсвечен экран под ним (см. rail.rs): это тот, куда вернёт «назад».
            self.view_before_np = self.nav.previous().map_or(View::Home, |p| p.view);
        }
        self.view = place.view;
    }
}

/// Существует ли ещё место: экран плейлиста без самого плейлиста (его удалили) открывать нельзя.
fn place_exists(ctl: &crate::controller::Controller, place: Place) -> bool {
    place.view != View::CollectionPlaylist || place.playlist.is_some_and(|id| ctl.playlists.get(id).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Modifiers, RawInput, pos2, vec2};

    const NO_GUARD: InputGuard =
        InputGuard { widget_focused: false, text_focused: false, text_focused_last_frame: false, overlay_open: false };

    /// Прогоняет один кадр egui без окна с заданными событиями и читает команды.
    fn read(events: Vec<Event>, guard: InputGuard) -> Vec<Shortcut> {
        let ctx = Context::default();
        let input =
            RawInput { events, screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))), ..Default::default() };
        let mut found = Vec::new();
        let mut out = ctx.run_ui(input, |ui| found = ui.input(|i| read_shortcuts(i, &guard)));
        out.textures_delta.clear();
        found
    }

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn mouse(button: PointerButton) -> Event {
        Event::PointerButton { pos: pos2(10.0, 10.0), button, pressed: true, modifiers: Modifiers::NONE }
    }

    #[test]
    fn side_mouse_buttons_navigate() {
        assert_eq!(read(vec![mouse(PointerButton::Extra1)], NO_GUARD), [Shortcut::Back]);
        assert_eq!(read(vec![mouse(PointerButton::Extra2)], NO_GUARD), [Shortcut::Forward]);
        assert!(read(vec![mouse(PointerButton::Primary)], NO_GUARD).is_empty());
    }

    #[test]
    fn mouse_release_is_not_a_command() {
        let release =
            Event::PointerButton { pos: pos2(1.0, 1.0), button: PointerButton::Extra1, pressed: false, modifiers: Modifiers::NONE };
        assert!(read(vec![release], NO_GUARD).is_empty());
    }

    #[test]
    fn escape_and_alt_arrows_navigate() {
        assert_eq!(read(vec![key(Key::Escape, Modifiers::NONE)], NO_GUARD), [Shortcut::Escape]);
        assert_eq!(read(vec![key(Key::ArrowLeft, Modifiers::ALT)], NO_GUARD), [Shortcut::Back]);
        assert_eq!(read(vec![key(Key::ArrowRight, Modifiers::ALT)], NO_GUARD), [Shortcut::Forward]);
        assert_eq!(read(vec![key(Key::BrowserBack, Modifiers::NONE)], NO_GUARD), [Shortcut::Back]);
        assert_eq!(read(vec![key(Key::BrowserForward, Modifiers::NONE)], NO_GUARD), [Shortcut::Forward]);
    }

    #[test]
    fn media_keys_control_playback() {
        let none = Modifiers::NONE;
        assert_eq!(read(vec![key(Key::MediaPlayPause, none)], NO_GUARD), [Shortcut::TogglePause]);
        assert_eq!(read(vec![key(Key::MediaStop, none)], NO_GUARD), [Shortcut::Stop]);
        assert_eq!(read(vec![key(Key::MediaTrackNext, none)], NO_GUARD), [Shortcut::NextTrack]);
        assert_eq!(read(vec![key(Key::MediaTrackPrevious, none)], NO_GUARD), [Shortcut::PrevTrack]);
    }

    #[test]
    fn space_and_ctrl_arrows_control_playback() {
        assert_eq!(read(vec![key(Key::Space, Modifiers::NONE)], NO_GUARD), [Shortcut::TogglePause]);
        assert_eq!(read(vec![key(Key::ArrowRight, Modifiers::COMMAND)], NO_GUARD), [Shortcut::NextTrack]);
        assert_eq!(read(vec![key(Key::ArrowLeft, Modifiers::COMMAND)], NO_GUARD), [Shortcut::PrevTrack]);
        // Просто стрелки остаются слайдерам.
        assert!(read(vec![key(Key::ArrowRight, Modifiers::NONE)], NO_GUARD).is_empty());
    }

    #[test]
    fn held_key_does_not_repeat() {
        // egui сам решает, что это повтор: клавиша уже зажата с прошлого кадра. Поэтому кадров два, в одном контексте.
        let ctx = Context::default();
        let frame = |events: Vec<Event>| {
            let input =
                RawInput { events, screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))), ..Default::default() };
            let mut found = Vec::new();
            let mut out = ctx.run_ui(input, |ui| found = ui.input(|i| read_shortcuts(i, &NO_GUARD)));
            out.textures_delta.clear();
            found
        };
        assert_eq!(frame(vec![key(Key::Escape, Modifiers::NONE)]), [Shortcut::Escape], "первое нажатие работает");
        assert!(frame(vec![key(Key::Escape, Modifiers::NONE)]).is_empty(), "повтор при удержании игнорируется");
    }

    #[test]
    fn typing_keeps_text_keys_but_not_media_keys() {
        let typing = InputGuard { widget_focused: true, text_focused: true, ..NO_GUARD };
        assert!(read(vec![key(Key::Space, Modifiers::NONE)], typing).is_empty());
        assert!(read(vec![key(Key::Escape, Modifiers::NONE)], typing).is_empty());
        assert!(read(vec![key(Key::ArrowRight, Modifiers::COMMAND)], typing).is_empty());
        // Мышь и медиаклавиши работают и в поле ввода.
        assert_eq!(read(vec![mouse(PointerButton::Extra1)], typing), [Shortcut::Back]);
        assert_eq!(read(vec![key(Key::MediaPlayPause, Modifiers::NONE)], typing), [Shortcut::TogglePause]);
    }

    #[test]
    fn focused_slider_keeps_space_and_ctrl_arrows_but_not_escape() {
        let slider = InputGuard { widget_focused: true, ..NO_GUARD };
        assert!(read(vec![key(Key::Space, Modifiers::NONE)], slider).is_empty());
        assert!(read(vec![key(Key::ArrowRight, Modifiers::COMMAND)], slider).is_empty());
        assert_eq!(read(vec![key(Key::Escape, Modifiers::NONE)], slider), [Shortcut::Escape]);
    }

    #[test]
    fn escape_that_closed_a_text_field_does_not_leave_the_screen() {
        let just_typed = InputGuard { text_focused_last_frame: true, ..NO_GUARD };
        assert!(read(vec![key(Key::Escape, Modifiers::NONE)], just_typed).is_empty());
    }

    #[test]
    fn open_overlay_owns_escape_and_mouse_buttons() {
        let overlay = InputGuard { overlay_open: true, ..NO_GUARD };
        assert!(read(vec![key(Key::Escape, Modifiers::NONE)], overlay).is_empty());
        assert!(read(vec![mouse(PointerButton::Extra1), mouse(PointerButton::Extra2)], overlay).is_empty());
        assert!(read(vec![key(Key::ArrowLeft, Modifiers::ALT)], overlay).is_empty());
        // А воспроизведение медиаклавишами окно не блокирует.
        assert_eq!(read(vec![key(Key::MediaTrackNext, Modifiers::NONE)], overlay), [Shortcut::NextTrack]);
    }

    #[test]
    fn several_events_in_one_frame_keep_their_order() {
        let found = read(vec![mouse(PointerButton::Extra1), mouse(PointerButton::Extra2)], NO_GUARD);
        assert_eq!(found, [Shortcut::Back, Shortcut::Forward]);
    }
}

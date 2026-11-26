//! Медиаклавиши клавиатуры (Play/Pause, Stop, Следующий трек, Предыдущий трек) на уровне системы.
//!
//! Зачем отдельно от `ui/shortcuts.rs`: окно получает эти клавиши только пока оно в фокусе, а плеер обычно
//! играет в фоне, пока человек работает в другой программе. Поэтому на Windows клавиши регистрируются глобально
//! (`RegisterHotKey` через крейт global-hotkey). Когда окно в фокусе, Windows тоже отдаёт клавишу
//! глобальной регистрации, так что команда приходит ровно один раз, а не дважды.
//!
//! Что нужно знать:
//! - Клавиша, уже занятая другой программой (бывает у браузеров и других плееров), не регистрируется. Тогда она
//!   работает только при фокусе на окне (через `ui/shortcuts.rs`), остальные клавиши от этого не страдают.
//! - Пока Mcrate запущен, он забирает эти клавиши у остальных программ. Так ведут себя классические плееры.
//! - Нужен цикл сообщений Windows в главном потоке: его даёт winit внутри eframe. Менеджер создаётся в
//!   `McrateApp::new`, а событие просыпается через `request_repaint`, поэтому команда выполняется сразу, а не
//!   при следующем движении мыши.
//! - Не Windows: плеер ориентирован на Windows 11, `MediaKeys::new` возвращает `None`.

/// Какая медиаклавиша нажата.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(windows), allow(dead_code))] // на не-Windows клавиши никто не создаёт
pub(crate) enum MediaKey {
    PlayPause,
    Stop,
    Next,
    Previous,
}

#[cfg(windows)]
mod imp {
    use std::sync::mpsc::{self, Receiver};

    use global_hotkey::hotkey::{Code, HotKey};
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

    use super::MediaKey;

    /// Какие клавиши слушаем.
    const KEYS: [(Code, MediaKey); 4] = [
        (Code::MediaPlayPause, MediaKey::PlayPause),
        (Code::MediaStop, MediaKey::Stop),
        (Code::MediaTrackNext, MediaKey::Next),
        (Code::MediaTrackPrevious, MediaKey::Previous),
    ];

    /// Глобальная подписка на медиаклавиши. Пока значение живо, клавиши зарегистрированы; при удалении снимаются.
    pub(crate) struct MediaKeys {
        _manager: GlobalHotKeyManager, // держим: без него регистрации исчезают
        events: Receiver<MediaKey>,
    }

    impl MediaKeys {
        /// Регистрирует клавиши. `None`, если система не дала зарегистрировать ни одной.
        /// `ctx` нужен, чтобы разбудить интерфейс, когда нажата клавиша (иначе окно в фоне не перерисуется).
        pub(crate) fn new(ctx: &egui::Context) -> Option<Self> {
            let manager = GlobalHotKeyManager::new().ok()?;
            let mut registered = Vec::new();
            for (code, key) in KEYS {
                let hotkey = HotKey::new(None, code);
                if manager.register(hotkey).is_ok() {
                    registered.push((hotkey.id(), key));
                }
            }
            if registered.is_empty() {
                return None;
            }
            let (tx, events) = mpsc::channel();
            let ctx = ctx.clone();
            GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
                // Windows присылает нажатие и отпускание; команда нужна один раз, по нажатию.
                if event.state != HotKeyState::Pressed {
                    return;
                }
                if let Some(&(_, key)) = registered.iter().find(|(id, _)| *id == event.id) {
                    let _ = tx.send(key);
                    ctx.request_repaint();
                }
            }));
            Some(Self { _manager: manager, events })
        }

        /// Забирает нажатия, накопившиеся с прошлого кадра.
        pub(crate) fn poll(&self) -> Vec<MediaKey> {
            self.events.try_iter().collect()
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::MediaKey;

    /// Заглушка: глобальных медиаклавиш вне Windows нет.
    pub(crate) struct MediaKeys;

    impl MediaKeys {
        pub(crate) fn new(_ctx: &egui::Context) -> Option<Self> {
            None
        }

        pub(crate) fn poll(&self) -> Vec<MediaKey> {
            Vec::new()
        }
    }
}

pub(crate) use imp::MediaKeys;

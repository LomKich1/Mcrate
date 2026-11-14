//! Всё, что зависит от ОС. Сейчас: системный материал окна Windows 11 (Mica/Acrylic) через DWM
//! (Desktop Window Manager, оконный менеджер Windows).

use crate::settings::WindowBackdrop;

/// Есть ли на этой ОС Mica/Acrylic.
pub const BACKDROP_SUPPORTED: bool = cfg!(windows);

/// Применить материал окна. На сборках Windows старше 22621 вызов просто не даст эффекта.
pub fn apply_window_backdrop(frame: &eframe::Frame, kind: WindowBackdrop) -> Result<(), String> {
    #[cfg(windows)]
    {
        use winit::platform::windows::{BackdropType, WindowExtWindows};
        let window = frame.winit_window().ok_or("нет доступа к окну")?;
        let t = match kind {
            WindowBackdrop::Off => BackdropType::None,
            WindowBackdrop::Mica => BackdropType::MainWindow,
            WindowBackdrop::Acrylic => BackdropType::TransientWindow,
        };
        window.set_system_backdrop(t);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (frame, kind);
        Err("Mica и Acrylic работают только в Windows 11".to_string())
    }
}

/// Покрасить заголовок окна (title bar) в цвет темы. Без этого Windows рисует его чёрным.
/// `use_material`: включён Mica/Acrylic. Тогда оставляем системный цвет, и материал
/// виден и в заголовке тоже, а не только под панелями.
/// Нужна Windows 11 сборки 22000+; на старых версиях вызов не даёт эффекта.
pub fn apply_title_bar(frame: &eframe::Frame, background: [u8; 3], text: [u8; 3], use_material: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use winit::platform::windows::{Color, WindowExtWindows};
        let window = frame.winit_window().ok_or("нет доступа к окну")?;
        if use_material {
            window.set_title_background_color(Some(Color::SYSTEM_DEFAULT));
        } else {
            window.set_title_background_color(Some(Color::from_rgb(background[0], background[1], background[2])));
        }
        window.set_title_text_color(Color::from_rgb(text[0], text[1], text[2]));
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (frame, background, text, use_material);
        Ok(())
    }
}

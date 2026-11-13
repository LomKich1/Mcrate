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

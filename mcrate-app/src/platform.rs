//! Всё, что зависит от ОС. Сейчас: системный материал окна Windows 11 (Mica/Acrylic) через DWM
//! (Desktop Window Manager, оконный менеджер Windows).

use crate::settings::WindowBackdrop;

/// Есть ли на этой ОС Mica/Acrylic.
pub const BACKDROP_SUPPORTED: bool = cfg!(windows);

/// Настройка wgpu (графической библиотеки) для окна, сквозь которое виден Mica/Acrylic.
///
/// Почему это нужно: по умолчанию wgpu на Windows показывает окно через обычную цепочку
/// DXGI, у которой нет прозрачности (режим Opaque). Тогда eframe не может включить
/// режим PreMultiplied, а прозрачный фон (clear_color = 0,0,0,0) превращается в чёрный.
/// Прозрачность даёт только DX12 + DirectComposition (`DxgiFromVisual`: окно показывается
/// через «визуал» системного композитора DWM). Поэтому на Windows берём именно его.
///
/// Запасной выход: если видеокарта или драйвер не тянут DX12, запустите приложение
/// с переменной окружения `MCRATE_NO_DCOMP=1`. Тогда настройки по умолчанию
/// (Vulkan/DX12 как выберет wgpu), но без Mica/Acrylic.
pub fn wgpu_configuration() -> eframe::egui_wgpu::WgpuConfiguration {
    #[allow(unused_mut)] // на не-Windows переменная не меняется
    let mut config = eframe::egui_wgpu::WgpuConfiguration::default();
    #[cfg(windows)]
    {
        if std::env::var_os("MCRATE_NO_DCOMP").is_some() {
            return config;
        }
        use eframe::egui_wgpu::{WgpuSetup, WgpuSetupCreateNew, wgpu};
        // display_handle eframe заполнит сам (см. документацию egui-wgpu).
        let mut setup = WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = wgpu::Backends::DX12;
        setup.instance_descriptor.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
        config.wgpu_setup = WgpuSetup::CreateNew(setup);
    }
    config
}

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

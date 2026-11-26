//! Всё, что зависит от ОС. Сейчас: системный материал окна Windows 11 (Mica/Acrylic) через DWM
//! (Desktop Window Manager, оконный менеджер Windows).

use crate::settings::WindowBackdrop;
use eframe::egui::Color32;

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
    use eframe::egui_wgpu::WgpuSetup;
    #[allow(unused_mut)] // на не-Windows переменная не меняется
    let mut config = eframe::egui_wgpu::WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup {
        economize_memory(setup);
    }
    #[cfg(windows)]
    {
        if std::env::var_os("MCRATE_NO_DCOMP").is_some() {
            return config;
        }
        use eframe::egui_wgpu::{WgpuSetupCreateNew, wgpu};
        // display_handle eframe заполнит сам (см. документацию egui-wgpu).
        let mut setup = WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = wgpu::Backends::DX12;
        setup.instance_descriptor.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
        economize_memory(&mut setup);
        config.wgpu_setup = WgpuSetup::CreateNew(setup);
    }
    config
}

/// Просим wgpu экономить память, а не гнаться за скоростью выделений.
///
/// egui_wgpu создаёт устройство с настройками по умолчанию, то есть `MemoryHints::Performance`.
/// Для DX12 и Vulkan это значит, что видеопамять резервируется крупными блоками: от 128 до 256 МБ
/// на ресурсы и от 64 до 128 МБ на загрузку данных, даже если приложению нужны единицы мегабайт
/// (у нас это несколько текстур стекла и атлас шрифтов). `MemoryUsage` уменьшает блоки до 8-64 МБ и
/// 4-32 МБ. Для интерфейса, который перерисовывается по событиям, разницы в скорости не заметно.
fn economize_memory(setup: &mut eframe::egui_wgpu::WgpuSetupCreateNew) {
    use eframe::egui_wgpu::wgpu;
    let base = setup.device_descriptor.clone();
    setup.device_descriptor =
        std::sync::Arc::new(move |adapter| wgpu::DeviceDescriptor { memory_hints: wgpu::MemoryHints::MemoryUsage, ..base(adapter) });
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

/// Оформить всплывающее окно без рамки (меню трея) средствами DWM: скругление Windows 11, цвет рамки и цвет
/// однопиксельной полоски заголовка сверху (eframe включает таким окнам системную тень, и полоска остаётся видна).
/// Окно ищется по заголовку: eframe не даёт доступа к окнам дочерних viewport. На Windows 10 эффекта нет.
#[cfg(windows)]
pub fn style_popup_window(title: &str, fill: Color32, border: Color32, dark: bool) {
    use std::ffi::c_void;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> *mut c_void;
    }
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(window: *mut c_void, attribute: u32, value: *const c_void, size: u32) -> i32;
    }
    // Номера атрибутов из dwmapi.h (DWMWINDOWATTRIBUTE).
    const USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const WINDOW_CORNER_PREFERENCE: u32 = 33;
    const BORDER_COLOR: u32 = 34;
    const CAPTION_COLOR: u32 = 35;
    /// DWMWCP_ROUND: обычное скругление Windows 11.
    const CORNER_ROUND: u32 = 2;

    let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `wide` заканчивается нулём и живёт до конца вызова; нулевой class значит «любой класс окна».
    let window = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) };
    if window.is_null() {
        return;
    }
    let attributes = [
        (USE_IMMERSIVE_DARK_MODE, u32::from(dark)),
        (WINDOW_CORNER_PREFERENCE, CORNER_ROUND),
        (BORDER_COLOR, colorref(border)),
        (CAPTION_COLOR, colorref(fill)),
    ];
    for (attribute, value) in attributes {
        // SAFETY: окно найдено выше; все четыре атрибута принимают 4 байта. Ошибку (старая Windows) пропускаем.
        unsafe { DwmSetWindowAttribute(window, attribute, (&raw const value).cast(), size_of::<u32>() as u32) };
    }
}

/// Вне Windows оформлять нечего.
#[cfg(not(windows))]
pub fn style_popup_window(_title: &str, _fill: Color32, _border: Color32, _dark: bool) {}

/// Цвет в формате COLORREF (число 0x00BBGGRR), прозрачность отбрасывается.
#[cfg_attr(not(windows), allow(dead_code))]
fn colorref(color: Color32) -> u32 {
    u32::from_le_bytes([color.r(), color.g(), color.b(), 0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_is_bgr() {
        assert_eq!(colorref(Color32::from_rgb(0x11, 0x22, 0x33)), 0x0033_2211);
    }
}

//! Mcrate: десктопный плеер. Этот крейт = окно и интерфейс (UI).
//! Ядро (типы, провайдеры, звук) живёт в mcrate-core / mcrate-providers / mcrate-audio.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod controller;
mod fonts;
mod icons;
mod platform;
mod settings;
mod theme;
mod ui;

/// Иконка окна и панели задач: 256x256, сырые байты RGBA (без новых зависимостей).
/// Исходник: assets/icon.svg; пересборка: см. assets/README.md.
fn app_icon() -> eframe::egui::IconData {
    eframe::egui::IconData { rgba: include_bytes!("../assets/icon_256.rgba").to_vec(), width: 256, height: 256 }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,             // egui_glass работает только на wgpu
        wgpu_options: platform::wgpu_configuration(), // DX12 + DirectComposition: без него Mica/Acrylic не видны
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Mcrate")
            .with_icon(app_icon())
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([880.0, 560.0])
            .with_transparent(true), // нужно для Mica/Acrylic
        ..Default::default()
    };
    // Пути из командной строки: `mcrate-app песня.mp3 папка`. Они сразу попадают в библиотеку.
    let open_paths: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    eframe::run_native("Mcrate", options, Box::new(move |cc| Ok(Box::new(app::McrateApp::new(cc, open_paths)))))
}

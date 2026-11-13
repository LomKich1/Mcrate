//! Mcrate: десктопный плеер. Этот крейт = окно и интерфейс (UI).
//! Ядро (типы, провайдеры, звук) живёт в mcrate-core / mcrate-providers / mcrate-audio.

mod app;
mod controller;
mod fonts;
mod icons;
mod platform;
mod settings;
mod theme;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu, // egui_glass работает только на wgpu
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Mcrate")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([880.0, 560.0])
            .with_transparent(true), // нужно для Mica/Acrylic
        ..Default::default()
    };
    // Пути из командной строки: `mcrate-app песня.mp3 папка`. Они сразу попадают в библиотеку.
    let open_paths: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    eframe::run_native(
        "Mcrate",
        options,
        Box::new(move |cc| Ok(Box::new(app::McrateApp::new(cc, open_paths)))),
    )
}

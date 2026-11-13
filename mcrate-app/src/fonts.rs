//! Шрифты. В Windows подключаем системный Segoe UI (родной шрифт Windows 11, есть кириллица).
//! В exe шрифт не вшиваем (лицензия Microsoft), читаем из папки Windows при запуске.
//! Если файла нет (Linux/macOS), остаются встроенные шрифты egui.

use std::sync::Arc;

pub fn install(ctx: &egui::Context) {
    for path in [r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\SegoeUI.ttf"] {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("segoe-ui".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            // insert(0, ..) = ставим первым в списке «Proportional», остальные остаются запасными
            if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                list.insert(0, "segoe-ui".to_owned());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
}

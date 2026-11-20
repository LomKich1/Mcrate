//! Экраны настроек. Главный экран зависит от темы: Material 3 получил новый вид (`main.rs`),
//! а Liquid Glass остаётся с прежним списком разделов (`glass.rs`).
//!
//! - `main.rs`: главный экран Material 3 (профиль, плитки, три блока строк);
//! - `screens.rs`: «Персонализация», «Воспроизведение», «Приватность»;
//! - `more.rs`: «Язык», «Производительность», «Интеграции», «Хранилище», «О приложении», «Поддержка» и экраны «скоро»;
//! - `glass.rs`: экраны Liquid Glass и блоки выбора оформления, общие для обеих тем;
//! - `controls.rs`: общие виджеты настроек (строки, чипы, переключатель, плитки).

mod glass;
mod main;
mod more;
mod screens;

pub(crate) mod controls;

pub(crate) use more::SoonKind;

use egui::Ui;

use crate::app::McrateApp;
use crate::theme::Theme;

impl McrateApp {
    /// Главный экран настроек: в Material 3 новый вид (`settings_main_m3`), в Liquid Glass прежний список.
    pub(crate) fn settings_view(&mut self, ui: &mut Ui, theme: &Theme) {
        if theme.glass_active() {
            self.settings_view_glass(ui, theme);
        } else {
            self.settings_main_m3(ui, theme);
        }
    }
}

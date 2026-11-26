//! Главный экран настроек в Material 3: карточка профиля, четыре плитки быстрого доступа
//! и три блока строк («Оформление», «Подключения», «Прочее»).
//!
//! Liquid Glass этот экран не использует: у него остаётся прежний список (`settings_view_glass`).
//! Каждая строка и плитка ведёт на свой подэкран (ui/settings/screens.rs и ui/settings/more.rs).

use std::sync::OnceLock;

use egui::{ScrollArea, Ui};
use egui_material_icons::icons as mi;

use super::controls::{Glyph, Row, Tile, Trailing, group_label, profile_card, settings_group, settings_tiles};
use crate::app::{McrateApp, View};
use crate::settings::{GraphicsSettings, Preferences};
use crate::theme::Theme;
use crate::ui::layout::{centered_column, pane_scroll, responsive_column, scroll_end_padding};
use crate::ui::widgets::{ButtonKind, pill_button};

/// Имя для карточки профиля: имя пользователя операционной системы. Показывается только на экране,
/// никуда не сохраняется и не отправляется (учётных записей в приложении нет). Без имени: «Гость».
fn local_user_name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        std::env::var("USERNAME") // Windows
            .or_else(|_| std::env::var("USER")) // Linux и macOS
            .ok()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Гость".to_string())
    })
}

impl McrateApp {
    /// Главный экран настроек (Material 3).
    pub(crate) fn settings_main_m3(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, f32::INFINITY, |ui| {
            responsive_column(ui, |ui| {
                ui.heading("Настройки");
                ui.add_space(8.0);
            });
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    responsive_column(ui, |ui| {
                        self.main_profile_and_tiles(ui, theme);
                        self.main_appearance_group(ui, theme);
                        self.main_connections_group(ui, theme);
                        self.main_other_group(ui, theme);

                        // ---------- сброс ----------
                        ui.add_space(20.0);
                        if pill_button(ui, theme, "Сбросить все настройки", None, ButtonKind::Outlined).clicked() {
                            self.settings = GraphicsSettings::default();
                            self.prefs = Preferences::default();
                        }
                        scroll_end_padding(ui, self.bottom_inset);
                    });
                })
            });
        });
    }

    /// Карточка профиля и четыре плитки быстрого доступа.
    fn main_profile_and_tiles(&mut self, ui: &mut Ui, theme: &Theme) {
        // ---------- профиль ----------
        if profile_card(ui, theme, local_user_name(), "Локальный профиль · аккаунт не подключён").clicked()
        {
            self.view = View::SettingsProfile;
        }
        ui.add_space(12.0);

        // ---------- плитки быстрого доступа ----------
        let tiles = [
            Tile {
                icon: mi::ICON_LANGUAGE,
                title: "Язык",
                subtitle: Some(self.prefs.language.label().to_lowercase()),
                glyph: Glyph::Text(self.prefs.language.code()),
            },
            Tile { icon: mi::ICON_PALETTE, title: "Персонализация", subtitle: None, glyph: Glyph::Symbol(mi::ICON_PALETTE) },
            Tile {
                icon: mi::ICON_MUSIC_NOTE,
                title: "Воспроизведение",
                subtitle: Some(self.prefs.stream_quality.label().to_string()),
                glyph: Glyph::Symbol(mi::ICON_MUSIC_NOTE),
            },
            Tile {
                icon: mi::ICON_LOCK_PERSON, title: "Приватность", subtitle: None, glyph: Glyph::Symbol(mi::ICON_LOCK_PERSON)
            },
        ];
        // Куда ведёт каждая плитка (порядок тот же, что в `tiles`).
        let tile_views = [View::SettingsLanguage, View::SettingsAppearance, View::SettingsPlayback, View::SettingsPrivacy];
        if let Some(i) = settings_tiles(ui, theme, &tiles) {
            self.view = tile_views[i];
        }
    }

    /// Блок «Оформление»: производительность и уведомления.
    fn main_appearance_group(&mut self, ui: &mut Ui, theme: &Theme) {
        // ---------- ОФОРМЛЕНИЕ ----------
        group_label(ui, theme, "Оформление");
        // Язык, Персонализация и Воспроизведение лежат плитками выше: здесь их не дублируем.
        let rows = vec![
            Row::new(mi::ICON_SPEED, "Производительность", Trailing::Chevron),
            Row::new(mi::ICON_NOTIFICATIONS, "Уведомления", Trailing::Switch(&mut self.prefs.notifications)),
        ];
        // «Производительность»: материал окна, качество стекла, графический API.
        let views = [Some(View::SettingsPerformance), None];
        if let Some(i) = settings_group(ui, theme, "appearance", rows)
            && let Some(view) = views[i]
        {
            self.view = view;
        }
    }

    /// Блок «Подключения».
    fn main_connections_group(&mut self, ui: &mut Ui, theme: &Theme) {
        // ---------- ПОДКЛЮЧЕНИЯ ----------
        // Подписка, сессии, семья и AI-память требуют сервера, а проект «только клиент»
        // (см. CLAUDE.md): пока это только вид, без логики. Значения справа пока постоянные.
        group_label(ui, theme, "Подключения");
        let rows = vec![
            Row::new(mi::ICON_SETTINGS_INPUT_COMPONENT, "Интеграции", Trailing::Value("SoundCloud".into())),
            Row::new(mi::ICON_CLOUD, "Хранилище и кеш", Trailing::Chevron),
            Row::new(mi::ICON_AUTO_AWESOME, "Музыкальная AI-память", Trailing::Value("Выключена".into())),
            Row::new(mi::ICON_SHIELD, "Сессии", Trailing::Chevron),
            Row::new(mi::ICON_WORKSPACE_PREMIUM, "Подписка", Trailing::Chevron),
            Row::new(mi::ICON_GROUP, "Управление семьёй", Trailing::Chevron),
        ];
        let views = [
            View::SettingsIntegrations,
            View::SettingsStorage,
            View::SettingsAiMemory,
            View::SettingsSessions,
            View::SettingsSubscription,
            View::SettingsFamily,
        ];
        if let Some(i) = settings_group(ui, theme, "connections", rows) {
            self.view = views[i];
        }
    }

    /// Блок «Прочее»: обновления, поддержка, о приложении.
    fn main_other_group(&mut self, ui: &mut Ui, theme: &Theme) {
        // ---------- ПРОЧЕЕ ----------
        group_label(ui, theme, "Прочее");
        let version = format!("V{}", env!("CARGO_PKG_VERSION"));
        let rows = vec![
            Row::new(mi::ICON_DOWNLOAD, "Автоматически скачивать обновления", Trailing::Switch(&mut self.prefs.auto_update)),
            Row::new(mi::ICON_HEADSET_MIC, "Поддержка", Trailing::Chevron),
            Row::new(mi::ICON_INFO, "О приложении", Trailing::Value(version)),
        ];
        let views = [None, Some(View::SettingsSupport), Some(View::SettingsAbout)];
        if let Some(i) = settings_group(ui, theme, "other", rows)
            && let Some(view) = views[i]
        {
            self.view = view;
        }
    }
}

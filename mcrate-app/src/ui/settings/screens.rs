//! Подэкраны настроек в Material 3: «Персонализация», «Воспроизведение», «Приватность».
//!
//! Все значения хранятся в `Preferences` / `GraphicsSettings`. Настройки воспроизведения и приватности
//! пока только сохраняются: движок и сервер их не читают (см. подсказки внизу экранов).

use egui::{Color32, ScrollArea, Ui};
use egui_material_icons::icons as mi;

use super::controls::{Chip, Choice, Row, Trailing, accent_picker, group_label, hint_text, settings_card, settings_group};
use super::glass::{design_cards, glass_options};
use crate::app::{McrateApp, View};
use crate::settings::{CROSSFADE_OPTIONS, Design, StreamQuality, TextSize, ThemePreset, WaveQuality};
use crate::theme::Theme;
use crate::ui::components::back_header;
use crate::ui::layout::{bleed_clip, centered_column, pane_scroll, responsive_column, scroll_end_padding};

/// Цвета акцента после «монохрома» (первый кружок). Остальное в палитре строит `ThemePreset::Custom`.
const ACCENTS: [[u8; 3]; 7] = [
    [0x5B, 0x8C, 0xFF], // синий
    [0x8B, 0x6C, 0xFF], // фиолетовый
    [0x4F, 0xD1, 0x8B], // зелёный
    [0xFF, 0x9F, 0x5A], // оранжевый
    [0xFF, 0x5A, 0x6A], // красный
    [0xFF, 0x6F, 0xB5], // розовый
    [0x3F, 0xC9, 0xB8], // бирюзовый
];

impl McrateApp {
    /// «Оформление» из главного экрана настроек: в Liquid Glass прежний экран, в Material 3 новая «Персонализация».
    pub(crate) fn settings_appearance_view(&mut self, ui: &mut Ui, theme: &Theme) {
        if theme.glass_active() {
            self.settings_appearance_glass(ui, theme);
        } else {
            self.settings_personalization_m3(ui, theme);
        }
    }

    /// «Персонализация»: акцент, размер текста, эффекты и (внизу) прежние выбор дизайна и готовых тем.
    fn settings_personalization_m3(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, f32::INFINITY, |ui| {
            responsive_column(ui, |ui| {
                if back_header(ui, theme, "Персонализация") {
                    self.view = View::Settings;
                }
            });
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    bleed_clip(ui, self.bleed.below_header());
                    responsive_column(ui, |ui| {
                        self.personalization_accent(ui, theme);
                        self.personalization_dark_light(ui, theme);
                        self.personalization_text_size(ui, theme);
                        self.personalization_effects(ui, theme);
                        self.personalization_design(ui, theme);

                        hint_text(
                            ui,
                            theme,
                            "Размер текста меняет масштаб всего интерфейса. Материал окна и графический API: раздел «Производительность».",
                        );
                    });
                    scroll_end_padding(ui, self.bottom_inset);
                })
            });
        });
    }

    /// Блок «Акцент»: ряд кружков с цветами и нейтральная схема.
    fn personalization_accent(&mut self, ui: &mut Ui, theme: &Theme) {
        // ---------- акцент ----------
        group_label(ui, theme, "Акцент");
        let mut swatches = vec![Color32::from_gray(235)];
        swatches.extend(ACCENTS.iter().map(|c| Color32::from_rgb(c[0], c[1], c[2])));
        // Первый кружок: нейтральная схема («Монохром» в тёмной, «Светлая» в светлой). Остальные: свой цвет.
        let current = match self.settings.theme {
            ThemePreset::Amoled | ThemePreset::Light => Some(0),
            ThemePreset::Custom => ACCENTS.iter().position(|c| *c == self.settings.accent).map(|i| i + 1),
            _ => None,
        };
        let dark = self.settings.is_dark();
        if let Some(i) = settings_card(ui, theme, |ui| accent_picker(ui, theme, "accent", &swatches, current)) {
            if i == 0 {
                self.settings.theme = if dark { ThemePreset::Amoled } else { ThemePreset::Light };
            } else {
                self.settings.theme = ThemePreset::Custom;
                self.settings.accent = ACCENTS[i - 1];
                self.settings.dark = dark;
            }
        }
    }

    /// Блок «Тема»: переключатель тёмной и светлой схемы.
    fn personalization_dark_light(&mut self, ui: &mut Ui, theme: &Theme) {
        // ---------- тёмная / светлая ----------
        group_label(ui, theme, "Тема");
        let mut dark_idx = if self.settings.is_dark() { 0 } else { 1 };
        let was = dark_idx;
        let chips = vec![Chip::new("Тёмная"), Chip::new("Светлая")];
        settings_group(
            ui,
            theme,
            "dark_light",
            vec![Row::plain("", Trailing::Choice(Choice { chips, selected: &mut dark_idx, stretch: true }))],
        );
        if dark_idx != was {
            let dark = dark_idx == 0;
            match self.settings.theme {
                ThemePreset::Amoled | ThemePreset::Light => {
                    self.settings.theme = if dark { ThemePreset::Amoled } else { ThemePreset::Light }
                }
                _ => {
                    self.settings.theme = ThemePreset::Custom;
                    self.settings.dark = dark;
                }
            }
        }
    }

    /// Блок «Размер текста».
    fn personalization_text_size(&mut self, ui: &mut Ui, theme: &Theme) {
        // ---------- размер текста ----------
        group_label(ui, theme, "Размер текста");
        let mut size_idx = TextSize::ALL.iter().position(|s| *s == self.prefs.text_size).unwrap_or(1);
        let chips = TextSize::ALL.iter().map(|s| Chip::new(s.label())).collect();
        settings_group(
            ui,
            theme,
            "text_size",
            vec![Row::plain("", Trailing::Choice(Choice { chips, selected: &mut size_idx, stretch: true }))],
        );
        self.prefs.text_size = TextSize::ALL[size_idx];
    }

    /// Блок «Эффекты»: анимации, морфинг обложки, качество волны.
    fn personalization_effects(&mut self, ui: &mut Ui, theme: &Theme) {
        // ---------- эффекты ----------
        group_label(ui, theme, "Эффекты");
        // «Анимации» это инверсия существующей настройки «уменьшить анимацию».
        let mut animations = !self.settings.reduce_motion;
        let mut wave_idx = WaveQuality::ALL.iter().position(|q| *q == self.prefs.wave_quality).unwrap_or(2);
        let wave_chips = WaveQuality::ALL.iter().map(|q| Chip::new(q.label())).collect();
        let rows = vec![
            Row::new(mi::ICON_AUTO_AWESOME, "Анимации", Trailing::Switch(&mut animations))
                .subtitle("Движение волны и визуализатора. Выключите, чтобы оставить спокойную статичную картинку."),
            Row::new(mi::ICON_IMAGE, "Морфинг обложки", Trailing::Switch(&mut self.prefs.cover_morph))
                .subtitle("Мгновенная подмена обложки между мини-плеером и плеером. Без плавного полёта изображения."),
            Row::new(
                mi::ICON_GRAPHIC_EQ,
                "Качество волны",
                Trailing::Choice(Choice { chips: wave_chips, selected: &mut wave_idx, stretch: true }),
            ),
        ];
        settings_group(ui, theme, "effects", rows);
        self.settings.reduce_motion = !animations;
        self.prefs.wave_quality = WaveQuality::ALL[wave_idx];
    }

    /// Блок «Дизайн»: прежний выбор дизайна и параметры стекла (перенесён из экрана «Оформление»).
    fn personalization_design(&mut self, ui: &mut Ui, theme: &Theme) {
        // ---------- дизайн и тема (перенесено из прежнего экрана «Оформление») ----------
        group_label(ui, theme, "Дизайн");
        design_cards(ui, theme, &mut self.settings.design);
        if self.settings.design == Design::LiquidGlass {
            ui.add_space(10.0);
            glass_options(ui, theme, &mut self.settings);
        }
    }

    /// «Воспроизведение»: качество стриминга, переходы между треками, прочее.
    pub(crate) fn settings_playback_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, f32::INFINITY, |ui| {
            responsive_column(ui, |ui| {
                if back_header(ui, theme, "Воспроизведение") {
                    self.view = View::Settings;
                }
            });
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    bleed_clip(ui, self.bleed.below_header());
                    responsive_column(ui, |ui| {
                        // ---------- качество звука ----------
                        group_label(ui, theme, "Качество звука");
                        // Lossless пока недоступен (замок): выбрать его нельзя.
                        let mut quality_idx = match self.prefs.stream_quality {
                            StreamQuality::Low => 0,
                            StreamQuality::Medium => 1,
                            StreamQuality::High | StreamQuality::Lossless => 2,
                        };
                        let chips = vec![Chip::new("Низкое"), Chip::new("Среднее"), Chip::new("Высокое"), Chip::locked("Lossless")];
                        settings_group(
                            ui,
                            theme,
                            "stream_quality",
                            vec![Row::plain(
                                "Качество стриминга",
                                Trailing::Choice(Choice { chips, selected: &mut quality_idx, stretch: false }),
                            )],
                        );
                        self.prefs.stream_quality = [StreamQuality::Low, StreamQuality::Medium, StreamQuality::High][quality_idx.min(2)];

                        // ---------- переходы ----------
                        group_label(ui, theme, "Переходы");
                        let mut fade_idx = CROSSFADE_OPTIONS.iter().position(|s| *s == self.prefs.crossfade_secs).unwrap_or(2);
                        let fade_labels: Vec<String> =
                            CROSSFADE_OPTIONS.iter().map(|s| if *s == 0 { "Выкл".to_string() } else { format!("{s}s") }).collect();
                        let chips = fade_labels.iter().map(|l| Chip::new(l)).collect();
                        let rows = vec![
                            Row::plain("Кроссфейд", Trailing::Choice(Choice { chips, selected: &mut fade_idx, stretch: false })),
                            Row::plain("Бесшовное воспроизведение", Trailing::Switch(&mut self.prefs.gapless)),
                        ];
                        settings_group(ui, theme, "transitions", rows);
                        self.prefs.crossfade_secs = CROSSFADE_OPTIONS[fade_idx];

                        // ---------- прочее ----------
                        group_label(ui, theme, "Прочее");
                        let rows = vec![
                            Row::plain("Нормализация громкости", Trailing::Switch(&mut self.prefs.normalize_volume)),
                            Row::plain("Автовоспроизведение", Trailing::Switch(&mut self.prefs.autoplay)),
                            Row::plain("Воспроизведение в фоне", Trailing::Switch(&mut self.prefs.background_play)),
                        ];
                        settings_group(ui, theme, "playback_other", rows);

                        hint_text(ui, theme, "Эти настройки сохраняются, но движок воспроизведения пока их не использует.");
                    });
                    scroll_end_padding(ui, self.bottom_inset);
                })
            });
        });
    }

    /// «Приватность»: профиль и данные. Сервера и аккаунтов нет, поэтому это только сохраняемые переключатели.
    pub(crate) fn settings_privacy_view(&mut self, ui: &mut Ui, theme: &Theme) {
        centered_column(ui, f32::INFINITY, |ui| {
            responsive_column(ui, |ui| {
                if back_header(ui, theme, "Приватность") {
                    self.view = View::Settings;
                }
            });
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    bleed_clip(ui, self.bleed.below_header());
                    responsive_column(ui, |ui| {
                        group_label(ui, theme, "Профиль");
                        let rows = vec![
                            Row::plain("Публичный профиль", Trailing::Switch(&mut self.prefs.public_profile))
                                .subtitle("Другие пользователи смогут находить этот аккаунт."),
                            Row::plain("Показывать, что я слушаю", Trailing::Switch(&mut self.prefs.show_listening))
                                .subtitle("Текущий трек будет виден друзьям."),
                        ];
                        settings_group(ui, theme, "privacy_profile", rows);

                        group_label(ui, theme, "Данные");
                        let rows = vec![
                            Row::plain("Делиться статистикой", Trailing::Switch(&mut self.prefs.share_stats))
                                .subtitle("Помогает делать рекомендации точнее."),
                            Row::plain("Персональные рекомендации", Trailing::Switch(&mut self.prefs.personal_recs)),
                            Row::plain("Отключить аналитику", Trailing::Switch(&mut self.prefs.disable_analytics)),
                        ];
                        settings_group(ui, theme, "privacy_data", rows);

                        hint_text(
                            ui,
                            theme,
                            "Аккаунтов и сервера в Mcrate пока нет: приложение ничего не отправляет, переключатели только сохраняются.",
                        );
                    });
                    scroll_end_padding(ui, self.bottom_inset);
                })
            });
        });
    }
}

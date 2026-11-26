//! Остальные подэкраны настроек в Material 3: «Язык», «Производительность», «Интеграции»,
//! «Хранилище и кеш», «О приложении», «Поддержка» и экраны-«скоро» (профиль, сессии, подписка,
//! семья, AI-память: им нужен сервер, а проект «только клиент», поэтому это только вид).

use egui::{ScrollArea, Ui, vec2};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons as mi;

use super::controls::{Chip, Choice, Row, Trailing, group_label, hint_text, paint_symbol, settings_card, settings_group};
use crate::app::{McrateApp, View};
use crate::platform;
use crate::settings::{GlassQuality, GraphicsSettings, Language, PaneFill, WindowBackdrop};
use crate::theme::{TextRole, Theme};
use crate::ui::components::{ConfirmDialog, DialogChoice, back_header, confirm_dialog};
use crate::ui::layout::{centered_column, pane_scroll, responsive_column, scroll_end_padding};
use crate::ui::widgets::error_banner;

/// Строка «Фон панели»: варианты кроме обычного доступны только с включённым материалом окна.
fn pane_fill_group(ui: &mut Ui, theme: &Theme, settings: &mut GraphicsSettings) {
    let fills = PaneFill::ALL;
    let material = platform::BACKDROP_SUPPORTED && settings.window_backdrop != WindowBackdrop::Off;
    let mut idx = fills.iter().position(|f| *f == settings.pane_fill).unwrap_or(0);
    let chips =
        fills.iter().map(|f| if *f == PaneFill::Solid || material { Chip::new(f.label()) } else { Chip::locked(f.label()) }).collect();
    let row = Row::new(mi::ICON_OPACITY, "Фон панели", Trailing::Choice(Choice { chips, selected: &mut idx, stretch: true }));
    settings_group(ui, theme, "pane_fill", vec![row]);
    settings.pane_fill = fills[idx];
}

/// Какой экран-«скоро» показать.
#[derive(Clone, Copy)]
pub(crate) enum SoonKind {
    Profile,
    AiMemory,
    Sessions,
    Subscription,
    Family,
}

impl SoonKind {
    fn icon(self) -> MaterialIcon {
        match self {
            Self::Profile => mi::ICON_PERSON,
            Self::AiMemory => mi::ICON_AUTO_AWESOME,
            Self::Sessions => mi::ICON_SHIELD,
            Self::Subscription => mi::ICON_WORKSPACE_PREMIUM,
            Self::Family => mi::ICON_GROUP,
        }
    }

    fn text(self) -> &'static str {
        match self {
            Self::Profile => "Аккаунт и публичный профиль появятся вместе с учётными записями. Сейчас профиль локальный.",
            Self::AiMemory => "Музыкальная память запоминает вкусы и подбирает музыку. Ей нужен сервер, которого у Mcrate пока нет.",
            Self::Sessions => "Здесь будут устройства, на которых выполнен вход. Входа в аккаунт пока нет.",
            Self::Subscription => "Подписка не нужна: Mcrate бесплатный клиент без сервера.",
            Self::Family => "Семейный доступ появится вместе с аккаунтами.",
        }
    }
}

/// Через сколько после «Очистить кэш» перерисовать экран, чтобы показать новый размер: движок чистит в своём потоке.
const CACHE_REFRESH_DELAY: std::time::Duration = std::time::Duration::from_millis(300);

fn format_size(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        format!("{} КБ", bytes / 1024)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} МБ", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} ГБ", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

impl McrateApp {
    /// Общий каркас подэкрана: шапка «назад», прокрутка, ограничение ширины.
    fn settings_page(&mut self, ui: &mut Ui, theme: &Theme, title: &str, body: impl FnOnce(&mut Self, &mut Ui)) {
        centered_column(ui, f32::INFINITY, |ui| {
            responsive_column(ui, |ui| {
                if back_header(ui, theme, title) {
                    self.view = View::Settings;
                }
            });
            pane_scroll(ui, self.bleed.below_header(), ScrollArea::vertical().auto_shrink([false, false]), |area, ui| {
                area.show(ui, |ui| {
                    responsive_column(ui, |ui| {
                        body(self, ui);
                        scroll_end_padding(ui, self.bottom_inset);
                    });
                })
            });
        });
    }

    /// «Язык»: выбор сохраняется, но переводов строк в приложении нет.
    pub(crate) fn settings_language_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.settings_page(ui, theme, "Язык", |app, ui| {
            group_label(ui, theme, "Язык интерфейса");
            let langs = [Language::Russian, Language::English];
            let mut idx = langs.iter().position(|l| *l == app.prefs.language).unwrap_or(0);
            let chips = langs.iter().map(|l| Chip::new(l.label())).collect();
            settings_group(
                ui,
                theme,
                "language",
                vec![Row::plain("", Trailing::Choice(Choice { chips, selected: &mut idx, stretch: true }))],
            );
            app.prefs.language = langs[idx];
            hint_text(ui, theme, "Переводов строк в приложении пока нет: выбор сохраняется, а тексты экранов остаются русскими.");
        });
    }

    /// «Производительность»: материал окна, качество стекла, графический API.
    pub(crate) fn settings_performance_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.settings_page(ui, theme, "Производительность", |app, ui| {
            group_label(ui, theme, "Окно");
            let backdrops = [(WindowBackdrop::Off, "Выкл"), (WindowBackdrop::Mica, "Mica"), (WindowBackdrop::Acrylic, "Acrylic")];
            let mut idx = backdrops.iter().position(|(b, _)| *b == app.settings.window_backdrop).unwrap_or(0);
            let supported = platform::BACKDROP_SUPPORTED;
            let chips = backdrops.iter().enumerate().map(|(i, (_, l))| if i == 0 || supported { Chip::new(l) } else { Chip::locked(l) }).collect();
            settings_group(
                ui,
                theme,
                "backdrop",
                vec![Row::new(mi::ICON_WINDOW, "Материал окна", Trailing::Choice(Choice { chips, selected: &mut idx, stretch: true }))],
            );
            app.settings.window_backdrop = backdrops[idx].0;
            pane_fill_group(ui, theme, &mut app.settings);
            hint_text(
                ui,
                theme,
                if supported {
                    "Нужна Windows 11 сборки 22621+. Без материала фон панели всегда обычный."
                } else {
                    "Материал окна доступен только в Windows 11."
                },
            );
            if let Some(err) = &app.backdrop_error {
                ui.add_space(8.0);
                error_banner(ui, theme, err);
            }

            group_label(ui, theme, "Графика");
            let qualities = GlassQuality::ALL;
            let mut q_idx = qualities.iter().position(|q| *q == app.settings.glass_quality).unwrap_or(1);
            let chips = qualities.iter().map(|q| Chip::new(q.label())).collect();
            settings_group(
                ui,
                theme,
                "glass_quality",
                vec![
                    Row::new(mi::ICON_SPEED, "Качество стекла", Trailing::Choice(Choice { chips, selected: &mut q_idx, stretch: true })),
                    Row::new(mi::ICON_MEMORY, "Графический API", Trailing::Label(app.gpu_backend.clone())),
                ],
            );
            app.settings.glass_quality = qualities[q_idx];
            let note = if app.gpu_backend == "Dx12" {
                "Качество стекла действует в дизайне Liquid Glass: на слабой видеокарте выберите «Быстро». DX12 работает, прозрачность окна доступна.".to_string()
            } else {
                format!("Графический API: {}. Для Mica/Acrylic нужен DX12 (см. MCRATE_NO_DCOMP в CLAUDE.md).", app.gpu_backend)
            };
            hint_text(ui, theme, &note);
        });
    }

    /// «Интеграции»: какие источники музыки доступны.
    pub(crate) fn settings_integrations_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.settings_page(ui, theme, "Интеграции", |_, ui| {
            group_label(ui, theme, "Источники музыки");
            let rows = vec![
                Row::new(mi::ICON_FOLDER, "Свои файлы", Trailing::Label("Доступно".into())),
                Row::new(mi::ICON_CLOUD, "SoundCloud", Trailing::Label("Доступно, вход не нужен".into())),
                Row::new(mi::ICON_LIBRARY_MUSIC, "VK Музыка", Trailing::Label("Скоро".into())),
                Row::new(mi::ICON_PLAY_CIRCLE, "YouTube Music", Trailing::Label("Скоро".into())),
            ];
            settings_group(ui, theme, "integrations", rows);
            hint_text(ui, theme, "SoundCloud открывается на своей вкладке в меню слева.");
        });
    }

    /// «Хранилище и кеш»: где лежит кэш онлайн-треков и сколько он занимает. Данные даёт движок
    /// (`Controller::cache_info`), сам экран диск не читает. Строка «Очистить кэш» просит движок
    /// (`Controller::cache_clear`) удалить скачанные треки, кроме играющего и следующего.
    pub(crate) fn settings_storage_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.settings_page(ui, theme, "Хранилище и кеш", |app, ui| {
            let info = app.ctl.cache_info();
            let (bytes, files, limit) = info.as_ref().map_or((0, 0, 0), |i| (i.stats.bytes, i.stats.files, i.limit));
            group_label(ui, theme, "Кэш потока");
            let rows = vec![
                Row::new(mi::ICON_STORAGE, "Занято", Trailing::Label(format_size(bytes))),
                Row::new(mi::ICON_AUDIO_FILE, "Треков в кэше", Trailing::Label(format!("{files} из {limit}"))),
            ];
            settings_group(ui, theme, "cache", rows);
            let hint = match &info {
                Some(i) => format!("Папка: {}. Старые треки удаляются сами, когда их становится больше {}.", i.dir.display(), i.limit),
                None => "Звуковой движок не запущен, кэш недоступен.".to_string(),
            };
            hint_text(ui, theme, &hint);
            if info.is_some() {
                let clear = vec![
                    Row::new(mi::ICON_DELETE, "Очистить кэш", Trailing::Action)
                        .subtitle("Удалить скачанные треки. Играющий и следующий останутся, остальные скачаются заново."),
                ];
                let pending_id = egui::Id::unique("cache_clear_confirm");
                if settings_group(ui, theme, "cache_clear", clear).is_some() {
                    ui.ctx().data_mut(|d| d.insert_temp(pending_id, true));
                }
                if ui.ctx().data(|d| d.get_temp::<bool>(pending_id)).unwrap_or(false) {
                    app.cache_clear_dialog(ui.ctx(), theme, pending_id);
                }
            }
        });
    }

    /// Окно «Очистить кэш?»: до подтверждения ничего не удаляется. `pending_id` это флаг «окно открыто»
    /// во временной памяти egui; после выбора он снимается.
    fn cache_clear_dialog(&mut self, ctx: &egui::Context, theme: &Theme, pending_id: egui::Id) {
        let spec = ConfirmDialog {
            key: "cache_clear",
            title: "Очистить кэш?",
            text: "Скачанные треки будут удалены, играющий и следующий останутся. Остальные скачаются заново, когда понадобятся.",
            confirm_label: "Очистить",
        };
        let Some(choice) = confirm_dialog(ctx, theme, &spec) else { return };
        ctx.data_mut(|d| d.insert_temp(pending_id, false));
        if choice == DialogChoice::Confirm {
            self.ctl.cache_clear();
            // Движок чистит в своём потоке: через мгновение перерисуемся, чтобы показать новый размер.
            ctx.request_repaint_after(CACHE_REFRESH_DELAY);
        }
    }

    /// «О приложении»: в Liquid Glass прежний экран, в Material 3 новый.
    pub(crate) fn settings_about_view(&mut self, ui: &mut Ui, theme: &Theme) {
        if theme.glass_active() {
            self.settings_about_glass(ui, theme);
            return;
        }
        self.settings_page(ui, theme, "О приложении", |app, ui| {
            group_label(ui, theme, "Mcrate");
            let rows = vec![
                Row::new(mi::ICON_INFO, "Версия", Trailing::Label(env!("CARGO_PKG_VERSION").to_string())),
                Row::new(mi::ICON_MEMORY, "Графический API", Trailing::Label(app.gpu_backend.clone())),
                Row::new(mi::ICON_COMPUTER, "Система", Trailing::Label(std::env::consts::OS.to_string())),
            ];
            settings_group(ui, theme, "about", rows);
            hint_text(ui, theme, "Музыкальный плеер на Rust и egui.");
        });
    }

    /// «Поддержка»: канала поддержки нет, показываем сведения, которые пригодятся при сообщении об ошибке.
    pub(crate) fn settings_support_view(&mut self, ui: &mut Ui, theme: &Theme) {
        self.settings_page(ui, theme, "Поддержка", |app, ui| {
            group_label(ui, theme, "Сведения для сообщения об ошибке");
            let rows = vec![
                Row::new(mi::ICON_INFO, "Версия", Trailing::Label(env!("CARGO_PKG_VERSION").to_string())),
                Row::new(mi::ICON_MEMORY, "Графический API", Trailing::Label(app.gpu_backend.clone())),
                Row::new(mi::ICON_COMPUTER, "Система", Trailing::Label(std::env::consts::OS.to_string())),
            ];
            settings_group(ui, theme, "support", rows);
            hint_text(ui, theme, "Своего канала поддержки у Mcrate пока нет. Приложите эти сведения, когда будете описывать проблему.");
        });
    }

    /// Экран-«скоро»: крупная иконка, пояснение. Логики нет, нужен сервер.
    pub(crate) fn settings_soon_view(&mut self, ui: &mut Ui, theme: &Theme, title: &str, kind: SoonKind) {
        let p = &theme.palette;
        self.settings_page(ui, theme, title, |_, ui| {
            ui.add_space(24.0);
            settings_card(ui, theme, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(14.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(88.0, 88.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 44.0, p.primary_container);
                    paint_symbol(ui.painter(), kind.icon(), rect.center(), 44.0, p.on_primary_container);
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new("Скоро").size(theme.text_size(TextRole::TitleLarge)).strong().color(p.on_surface));
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(egui::RichText::new(kind.text()).color(p.on_surface_variant)).wrap());
                    ui.add_space(14.0);
                });
            });
        });
    }
}

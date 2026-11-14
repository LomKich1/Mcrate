//! Главный тип приложения и раскладка окна.
//!
//! Раскладка (как в проводнике Windows 11): слева навигация, справа основная область
//! со скруглёнными углами, снизу плавающая панель плеера.

use std::time::Duration;

use eframe::egui::{self, Frame, Margin};
use mcrate_core::Track;

use crate::controller::Controller;
use crate::icons;
use crate::ui::layout::Breakpoint;
use crate::ui::transition::ViewTransition;
use crate::settings::{Design, GraphicsSettings, STORAGE_KEY, ThemePreset, WindowBackdrop};
use crate::theme::{Surface, Theme, glass, with_alpha};
use crate::{fonts, platform};

/// Какой экран открыт.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Library,
    NowPlaying,
    Settings,
}

impl View {
    /// Порядок экранов в меню сверху вниз. От него зависит направление анимации перехода
    /// (см. ui/transition.rs): к большему номеру экран выезжает справа, к меньшему слева.
    /// Новые экраны вставляем сюда в том порядке, в каком они стоят в меню.
    pub fn order(self) -> u8 {
        match self {
            View::Library => 0,
            View::NowPlaying => 1,
            View::Settings => 2,
        }
    }
}

pub struct McrateApp {
    pub settings: GraphicsSettings,
    applied: Option<GraphicsSettings>, // какие настройки уже применены к egui
    pub theme: Theme,
    render_state: eframe::egui_wgpu::RenderState, // доступ к GPU (нужен стеклу)
    wallpaper_key: Option<(ThemePreset, [u8; 3], bool)>, // для какой темы уже загружены обои
    pub backdrop_error: Option<String>,
    /// Какой графический API (DX12, Vulkan...) реально работает. Показывается в настройках:
    /// для Mica/Acrylic нужен Dx12.
    pub gpu_backend: String,

    pub ctl: Controller,
    pub view: View,
    /// Экран, который показан (или к которому идёт переход). Сравнивается с `view`,
    /// чтобы заметить смену экрана и запустить анимацию (см. ui/transition.rs).
    pub shown_view: View,
    pub transition: Option<ViewTransition>,
    pub search: String,
    pub filtered: Vec<Track>,
    filter_key: Option<(String, usize)>,
    pub seek_drag: Option<f32>,
    pub volume_before_mute: f32,
}

impl McrateApp {
    pub fn new(cc: &eframe::CreationContext<'_>, open_paths: Vec<std::path::PathBuf>) -> Self {
        // Сохранённые настройки (если есть) или значения по умолчанию.
        let settings: GraphicsSettings = cc
            .storage
            .and_then(|s| eframe::get_value(s, STORAGE_KEY))
            .unwrap_or_default();

        let render_state = cc
            .wgpu_render_state
            .clone()
            .expect("нужен рендерер wgpu (Renderer::Wgpu)");
        let gpu_backend = format!("{:?}", render_state.adapter.get_info().backend);
        egui_glass::init(&render_state, 1); // 1 = без сглаживания MSAA (значение по умолчанию)

        fonts::install(&cc.egui_ctx);
        icons::install(&cc.egui_ctx); // после fonts::install: тот заменяет набор шрифтов целиком

        let mut ctl = Controller::new();
        if !open_paths.is_empty() {
            ctl.add_paths(&open_paths); // файлы/папки из командной строки («Открыть с помощью»)
        }

        Self {
            theme: Theme::new(&settings),
            settings,
            applied: None,
            render_state,
            wallpaper_key: None,
            backdrop_error: None,
            gpu_backend,
            ctl,
            view: View::Library,
            shown_view: View::Library,
            transition: None,
            search: String::new(),
            filtered: Vec::new(),
            filter_key: None,
            seek_drag: None,
            volume_before_mute: 0.8,
        }
    }

    /// Если настройки изменились, пересобираем тему и применяем побочные эффекты.
    fn sync_theme(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.applied.as_ref() == Some(&self.settings) {
            return;
        }
        self.theme = Theme::new(&self.settings);
        self.theme.apply(ctx);

        // Обои для стекла зависят только от палитры.
        let key = self.settings.palette_key();
        if self.settings.design == Design::LiquidGlass && self.wallpaper_key != Some(key) {
            let img = glass::wallpaper(&self.theme.palette);
            egui_glass::set_backdrop(ctx, &self.render_state, &img);
            self.wallpaper_key = Some(key);
        }

        let backdrop_changed = self.applied.as_ref().map(|a| a.window_backdrop) != Some(self.settings.window_backdrop);
        if backdrop_changed {
            self.backdrop_error = platform::apply_window_backdrop(frame, self.settings.window_backdrop).err();
        }

        // Заголовок окна в цвет темы (поверх системной тёмной/светлой темы ниже).
        let pal = &self.theme.palette;
        let _ = platform::apply_title_bar(
            frame,
            [pal.surface.r(), pal.surface.g(), pal.surface.b()],
            [pal.on_surface.r(), pal.on_surface.g(), pal.on_surface.b()],
            self.settings.window_backdrop != WindowBackdrop::Off,
        );

        // Тёмный/светлый заголовок окна.
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(if self.settings.is_dark() {
            egui::SystemTheme::Dark
        } else {
            egui::SystemTheme::Light
        }));

        self.applied = Some(self.settings.clone());
    }

    /// Файлы, брошенные в окно, + пробел как «пауза/играть».
    fn handle_input(&mut self, ctx: &egui::Context) {
        let dropped: Vec<_> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.ctl.add_paths(&dropped);
            self.view = View::Library;
        }
        // wants_keyboard_input = true, когда курсор стоит в поле ввода (тогда пробел не трогаем)
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.ctl.toggle_pause();
        }
    }

    /// Пересчитываем список по поиску только при изменении запроса или библиотеки.
    fn refresh_filter(&mut self) {
        let key = (self.search.clone(), self.ctl.library.len());
        if self.filter_key.as_ref() != Some(&key) {
            self.filtered = self.ctl.search(&self.search);
            self.filter_key = Some(key);
        }
    }

    fn paint_drop_overlay(&self, ctx: &egui::Context) {
        if !ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            return;
        }
        let p = &self.theme.palette;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop_overlay")));
        let r = ctx.content_rect();
        painter.rect_filled(r, 0.0, with_alpha(p.primary, 80));
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            "Отпустите файлы или папку, чтобы добавить в библиотеку",
            egui::FontId::proportional(22.0),
            p.on_primary_container,
        );
    }
}

impl eframe::App for McrateApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.sync_theme(&ctx, frame);
        self.handle_input(&ctx);
        self.ctl.tick();
        self.refresh_filter();
        self.update_transition(&ctx);

        // Копия темы нужна, потому что внутри замыканий мы меняем self (borrow checker
        // не даст держать &self.theme и &mut self одновременно).
        let theme = self.theme.clone();

        let full = ui.max_rect();
        theme.paint_background(ui, full);
        // Размерный класс окна: от него зависит ширина навигации (см. ui/layout.rs).
        let bp = Breakpoint::from_width(full.width());

        // Порядок важен: сначала панели по краям, потом центральная область.
        egui::Panel::bottom("player_bar")
            .frame(Frame::NONE.inner_margin(Margin { left: 12, right: 12, top: 4, bottom: 12 }))
            .resizable(false)
            .show_separator_line(false)
            .show(ui, |ui| {
                theme.surface(ui, Surface::PlayerBar, |ui| self.player_bar(ui, &theme));
            });

        egui::Panel::left("rail")
            .frame(Frame::NONE.inner_margin(Margin { left: 12, right: 4, top: 12, bottom: 4 }))
            .resizable(false)
            .show_separator_line(false)
            .exact_size(bp.rail_width())
            .show(ui, |ui| {
                theme.surface(ui, Surface::Rail, |ui| self.rail_view(ui, &theme, bp.is_compact()));
            });

        egui::CentralPanel::default()
            .frame(Frame::NONE.inner_margin(Margin { left: 4, right: 12, top: 12, bottom: 4 }))
            .show(ui, |ui| {
                // view_host сам выбирает: один экран или два (во время анимации перехода).
                theme.surface(ui, Surface::Content, |ui| self.view_host(ui, &theme));
            });

        self.paint_drop_overlay(&ctx);

        // Пока играет музыка, перерисовываем ~10 раз в секунду (полоса времени, эквалайзер).
        if self.ctl.is_playing() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.settings);
    }

    /// Фон окна полностью прозрачный: всё закрашиваем сами, а при Mica/Acrylic виден материал ОС.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

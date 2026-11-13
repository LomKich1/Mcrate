//! Тема = выбранный дизайн + палитра + параметры блюра.
//! Остальной UI не знает, какой дизайн включён: он просит у темы «поверхность»
//! (`theme.surface(...)`) и цвета, а тема сама решает, нарисовать это
//! как Material 3 или как Liquid Glass.

pub mod glass;
pub mod material3;
pub mod palette;

use egui::{Color32, Context, CornerRadius, Ui};

use crate::settings::{Design, GraphicsSettings, WindowBackdrop};
pub use palette::{Palette, with_alpha, with_alpha_over};

/// Какие бывают поверхности (контейнеры) в интерфейсе.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    /// Левая навигация.
    Rail,
    /// Основная область (список, настройки).
    Content,
    /// Плавающая панель плеера внизу.
    PlayerBar,
    /// Диалоги и всплывающие окна (будущие экраны подключения сервисов).
    #[allow(dead_code)]
    Dialog,
}

#[derive(Clone)]
pub struct Theme {
    pub settings: GraphicsSettings,
    pub palette: Palette,
}

impl Theme {
    pub fn new(settings: &GraphicsSettings) -> Self {
        Self {
            settings: settings.clone(),
            palette: Palette::from_settings(settings),
        }
    }

    pub fn design(&self) -> Design {
        self.settings.design
    }

    /// Рисуем ли настоящее стекло (дизайн Liquid Glass и блюр не выключен).
    pub fn glass_active(&self) -> bool {
        self.settings.design == Design::LiquidGlass && self.settings.blur_enabled
    }

    /// Показывается ли системный материал (Mica/Acrylic) сквозь окно.
    pub fn window_backdrop_active(&self) -> bool {
        self.settings.window_backdrop != WindowBackdrop::Off
    }

    /// Радиус скругления больших панелей.
    pub fn pane_radius(&self) -> u8 {
        match self.settings.design {
            Design::Material3 => 28, // M3 Expressive: extra-large
            Design::LiquidGlass => 26,
        }
    }

    /// Цвет заливки «тональных» элементов (выбранный пункт меню, круглые кнопки).
    pub fn tonal_fill(&self) -> Color32 {
        let p = &self.palette;
        match self.settings.design {
            Design::Material3 => p.secondary_container,
            Design::LiquidGlass => with_alpha(p.on_surface, if p.dark { 28 } else { 22 }),
        }
    }

    /// Цвет текста/иконки на тональной заливке.
    pub fn on_tonal(&self) -> Color32 {
        match self.settings.design {
            Design::Material3 => self.palette.on_secondary_container,
            Design::LiquidGlass => self.palette.on_surface,
        }
    }

    /// Фон выбранного пункта навигации.
    pub fn selected_fill(&self) -> Color32 {
        let p = &self.palette;
        match self.settings.design {
            Design::Material3 => p.secondary_container,
            Design::LiquidGlass => with_alpha(p.primary, if p.dark { 90 } else { 70 }),
        }
    }

    /// Фон строки списка в обычном состоянии (прозрачный) и под курсором.
    pub fn row_hover_fill(&self) -> Color32 {
        with_alpha(self.palette.on_surface, 20)
    }

    /// Применить визуальный стиль к egui (цвета виджетов, шрифты, отступы).
    pub fn apply(&self, ctx: &Context) {
        let p = &self.palette;
        let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };

        v.override_text_color = Some(p.on_surface);
        v.panel_fill = Color32::TRANSPARENT; // фон рисуем сами (см. paint_background)
        v.window_fill = p.surface_container_high;
        v.window_stroke = egui::Stroke::new(1.0, p.outline_variant);
        v.window_corner_radius = CornerRadius::same(28);
        v.menu_corner_radius = CornerRadius::same(16);
        v.extreme_bg_color = p.surface_container_lowest; // фон полей ввода
        v.faint_bg_color = with_alpha(p.on_surface, 10);
        v.hyperlink_color = p.primary;
        v.selection.bg_fill = with_alpha(p.primary, 90);
        v.selection.stroke = egui::Stroke::new(1.0, p.primary);

        let pill = CornerRadius::same(20); // «таблетка»: M3 любит полностью скруглённые кнопки
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = p.surface_container;
        w.noninteractive.fg_stroke = egui::Stroke::new(1.0, p.on_surface_variant);
        w.noninteractive.bg_stroke = egui::Stroke::new(1.0, p.outline_variant);
        for (state, fill) in [
            (&mut w.inactive, p.surface_container_highest),
            (&mut w.hovered, with_alpha_over(p.surface_container_highest, p.on_surface, 0.08)),
            (&mut w.active, with_alpha_over(p.surface_container_highest, p.on_surface, 0.12)),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = egui::Stroke::NONE;
            state.fg_stroke = egui::Stroke::new(1.0, p.on_surface);
            state.corner_radius = pill;
        }
        w.open.corner_radius = pill;

        ctx.set_visuals(v);

        // Типографика по шкале M3 (размеры в пикселях).
        ctx.all_styles_mut(|style| {
            use egui::{FontFamily::Proportional, FontId, TextStyle};
            style.text_styles.insert(TextStyle::Heading, FontId::new(26.0, Proportional));
            style.text_styles.insert(TextStyle::Body, FontId::new(15.0, Proportional));
            style.text_styles.insert(TextStyle::Button, FontId::new(14.0, Proportional));
            style.text_styles.insert(TextStyle::Small, FontId::new(12.0, Proportional));
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(16.0, 8.0);
            style.spacing.interact_size.y = 36.0;
        });
    }

    /// Фон окна: в M3 это цвет surface, в Liquid Glass это обои, сквозь которые видно стекло.
    pub fn paint_background(&self, ui: &mut Ui, rect: egui::Rect) {
        match self.settings.design {
            Design::Material3 => material3::paint_background(self, ui, rect),
            Design::LiquidGlass => glass::paint_background(self, ui, rect),
        }
    }

    /// Нарисовать контейнер нужного вида и положить в него содержимое.
    pub fn surface<R>(&self, ui: &mut Ui, role: Surface, add: impl FnOnce(&mut Ui) -> R) -> R {
        match self.settings.design {
            Design::Material3 => material3::surface(self, ui, role, add),
            Design::LiquidGlass => glass::surface(self, ui, role, add),
        }
    }
}

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

/// Семантические роли формы. Значения задаются в логических пикселях egui.
/// Матрица используется только Material 3; Liquid Glass сохраняет исходную геометрию.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeRole {
    None,
    ExtraSmall,
    Small,
    Medium,
    Large,
    ExtraLarge,
    Full,
}

/// Основные уровни типографической шкалы Material 3.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextRole {
    DisplayLarge,
    DisplayMedium,
    DisplaySmall,
    HeadlineLarge,
    HeadlineMedium,
    HeadlineSmall,
    TitleLarge,
    TitleMedium,
    TitleSmall,
    BodyLarge,
    BodyMedium,
    BodySmall,
    LabelLarge,
    LabelMedium,
    LabelSmall,
}

// Полные таблицы токенов. Помимо единого источника значений они позволяют
// проверить/перебрать всю шкалу, даже если конкретный экран пока не использует роль.
const SHAPE_TOKENS: [(ShapeRole, u8, u8); 7] = [
    (ShapeRole::None, 0, 0),
    (ShapeRole::ExtraSmall, 4, 8),
    (ShapeRole::Small, 8, 12),
    (ShapeRole::Medium, 12, 16),
    (ShapeRole::Large, 16, 20),
    (ShapeRole::ExtraLarge, 28, 28),
    (ShapeRole::Full, 28, 28),
];

const TEXT_TOKENS: [(TextRole, f32, f32); 15] = [
    (TextRole::DisplayLarge, 57.0, 34.0),
    (TextRole::DisplayMedium, 45.0, 32.0),
    (TextRole::DisplaySmall, 36.0, 34.0),
    (TextRole::HeadlineLarge, 32.0, 26.0),
    (TextRole::HeadlineMedium, 28.0, 24.0),
    (TextRole::HeadlineSmall, 24.0, 26.0),
    (TextRole::TitleLarge, 22.0, 22.0),
    (TextRole::TitleMedium, 16.0, 15.0),
    (TextRole::TitleSmall, 14.0, 14.0),
    (TextRole::BodyLarge, 16.0, 16.0),
    (TextRole::BodyMedium, 14.0, 15.0),
    (TextRole::BodySmall, 12.0, 12.5),
    (TextRole::LabelLarge, 14.0, 14.0),
    (TextRole::LabelMedium, 12.0, 12.5),
    (TextRole::LabelSmall, 11.0, 12.0),
];

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
            palette: Palette::from_settings(settings, settings.design),
        }
    }

    pub fn design(&self) -> Design {
        self.settings.design
    }

    /// Рисуем ли настоящее стекло. Блюр и стекло включаются вместе с дизайном Liquid Glass
    /// (отдельного выключателя больше нет).
    pub fn glass_active(&self) -> bool {
        self.settings.design == Design::LiquidGlass
    }

    /// Показывается ли системный материал (Mica/Acrylic) сквозь окно.
    pub fn window_backdrop_active(&self) -> bool {
        self.settings.window_backdrop != WindowBackdrop::Off
    }

    /// Радиус скругления больших панелей (совместимость с существующими вызовами).
    pub fn pane_radius(&self) -> u8 {
        self.shape_radius(ShapeRole::ExtraLarge)
    }

    /// Токен формы. У Liquid Glass намеренно сохранена прежняя шкала радиусов.
    pub fn shape_radius(&self, role: ShapeRole) -> u8 {
        SHAPE_TOKENS
            .iter()
            .find(|(candidate, _, _)| *candidate == role)
            .map(|(_, material3, glass)| {
                if self.design() == Design::Material3 { *material3 } else { *glass }
            })
            .unwrap_or(0)
    }

    /// Размеры текста для компонентов, которые используют общую шкалу.
    /// Значения M3 соответствуют базовым ролям (Display/Headline/Title/Body/Label),
    /// а размеры Liquid Glass оставлены близкими к прежним значениям интерфейса.
    pub fn text_size(&self, role: TextRole) -> f32 {
        TEXT_TOKENS
            .iter()
            .find(|(candidate, _, _)| *candidate == role)
            .map(|(_, material3, glass)| {
                if self.design() == Design::Material3 { *material3 } else { *glass }
            })
            .unwrap_or(14.0)
    }

    /// Высота двухстрочного элемента библиотеки. M3 использует размерный ряд 56/72/88;
    /// Liquid Glass остаётся на прежних 56 px, чтобы не сдвигать его прокрутку и inset.
    pub fn list_item_height(&self, lines: u8) -> f32 {
        if self.design() == Design::LiquidGlass {
            return 56.0;
        }
        match lines {
            0 | 1 => 56.0,
            2 => 72.0,
            _ => 88.0,
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
            // Выбранный пункт остаётся частью стеклянной поверхности: акцент
            // полупрозрачный, без второй стеклянной подложки.
            Design::LiquidGlass => with_alpha(p.primary, if p.dark { 76 } else { 64 }),
        }
    }

    /// Фон строки списка в обычном состоянии (прозрачный) и под курсором.
    pub fn row_hover_fill(&self) -> Color32 {
        with_alpha(self.palette.on_surface, 20)
    }

    /// Цвет и сила подсветки при наведении. В Liquid Glass она «загорается» изнутри:
    /// светлая на тёмном материале и тёмная на светлом. В Material 3 оставляем прежнее затемнение.
    pub fn hover_glide_color(&self) -> Color32 {
        if self.glass_active() {
            self.palette.on_surface
        } else {
            Color32::BLACK
        }
    }

    pub fn hover_glide_alpha(&self) -> u8 {
        match (self.design(), self.palette.dark) {
            (Design::LiquidGlass, true) => 18,
            (Design::LiquidGlass, false) => 24,
            (_, true) => 70,
            (_, false) => 30,
        }
    }

    /// Применить визуальный стиль к egui (цвета виджетов, шрифты, отступы).
    pub fn apply(&self, ctx: &Context) {
        let p = &self.palette;
        let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };

        v.override_text_color = Some(p.on_surface);
        v.panel_fill = Color32::TRANSPARENT; // фон рисуем сами (см. paint_background)
        v.window_fill = p.surface_container_high;
        v.window_stroke = egui::Stroke::new(1.0, p.outline_variant);
        v.window_corner_radius = CornerRadius::same(self.shape_radius(ShapeRole::ExtraLarge));
        v.menu_corner_radius = CornerRadius::same(self.shape_radius(ShapeRole::Medium));
        // Фон полей ввода. Раньше был почти чёрный (surface_container_lowest), из-за чего поиск
        // выглядел чёрной полосой. В M3 поле заполняется тональным цветом поверхности; в стекле
        // это лёгкая полупрозрачная подсветка, сквозь которую видно фон.
        v.extreme_bg_color = match self.design() {
            Design::Material3 => p.surface_container_high,
            Design::LiquidGlass => with_alpha(p.on_surface, if p.dark { 26 } else { 20 }),
        };
        v.faint_bg_color = with_alpha(p.on_surface, 10);
        v.hyperlink_color = p.primary;
        v.selection.bg_fill = with_alpha(p.primary, 90);
        v.selection.stroke = egui::Stroke::new(1.0, p.primary);

        // Универсальные egui-контролы тоже получают тему. В Liquid Glass фон тише,
        // скругление мягче, а наведение/нажатие читаются как внутренний блик, а не как
        // плотная карточка. Ветка Material 3 сохраняет прежние токены без изменений
        // (полный M3-пилюльный радиус рассчитывают компоненты с учётом собственной высоты).
        let glass = self.glass_active();
        let pill = CornerRadius::same(if glass { 18 } else { 20 });
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = if glass { Color32::TRANSPARENT } else { p.surface_container };
        w.noninteractive.fg_stroke = egui::Stroke::new(1.0, p.on_surface_variant);
        w.noninteractive.bg_stroke = if glass {
            egui::Stroke::NONE
        } else {
            egui::Stroke::new(1.0, p.outline_variant)
        };
        if glass {
            for (state, fill_alpha, edge_alpha) in [
                (&mut w.inactive, if p.dark { 30 } else { 42 }, 30),
                (&mut w.hovered, if p.dark { 58 } else { 72 }, 66),
                (&mut w.active, if p.dark { 82 } else { 96 }, 92),
            ] {
                state.bg_fill = with_alpha(p.surface_container_high, fill_alpha);
                state.weak_bg_fill = with_alpha(p.on_surface, fill_alpha / 2);
                state.bg_stroke = egui::Stroke::new(0.8, with_alpha(p.on_surface, edge_alpha));
                state.fg_stroke = egui::Stroke::new(1.0, p.on_surface);
                state.corner_radius = pill;
            }
        } else {
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
        }
        w.open.corner_radius = pill;

        ctx.set_visuals(v);

        // Типографика по шкале M3 (размеры в пикселях).
        ctx.all_styles_mut(|style| {
            use egui::{FontFamily::Proportional, FontId, TextStyle};
            style.text_styles.insert(TextStyle::Heading, FontId::new(self.text_size(TextRole::HeadlineSmall), Proportional));
            style.text_styles.insert(TextStyle::Body, FontId::new(self.text_size(TextRole::BodyMedium), Proportional));
            style.text_styles.insert(TextStyle::Button, FontId::new(self.text_size(TextRole::LabelLarge), Proportional));
            let small_size = if self.glass_active() { 12.0 } else { self.text_size(TextRole::BodySmall) };
            style.text_styles.insert(TextStyle::Small, FontId::new(small_size, Proportional));
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = if self.glass_active() {
                egui::vec2(14.0, 10.0)
            } else {
                egui::vec2(16.0, 8.0)
            };
            // Ближе к минимальной 44 pt зоне нажатия iOS, но без изменения геометрии
            // кастомных плеерных контролов и без влияния на Material 3.
            style.spacing.interact_size.y = if self.glass_active() { 44.0 } else { 40.0 };

            // Тонкий бегунок лежит в свободном правом поле и не перекрывает строки/карточки.
            // Геометрия ScrollArea дополнительно отступает от верхнего и нижнего скруглений;
            // отдельную тень для бегунка не рисуем, чтобы он оставался частью основной поверхности.
            let mut scroll = egui::style::ScrollStyle::floating();
            scroll.bar_width = 8.0;
            scroll.floating_width = 3.0;
            scroll.floating_allocated_width = 0.0;
            scroll.handle_min_length = 30.0;
            scroll.bar_inner_margin = 3.0;
            scroll.bar_outer_margin = 0.0;
            scroll.dormant_handle_opacity = 0.38;
            scroll.active_handle_opacity = 0.72;
            scroll.interact_handle_opacity = 1.0;
            scroll.dormant_background_opacity = 0.0;
            scroll.active_background_opacity = 0.12;
            scroll.interact_background_opacity = 0.22;
            // Встроенное «затухание» краёв ScrollArea (egui 0.37, по умолчанию 50% на 20 px) выключено:
            // у горизонтальной полки оно затемняло правый край карточки до границы страницы, и дальше
            // карточка выглядела светлее, как полоса-тень у края панели.
            scroll.fade = egui::style::ScrollFadeStyle { strength: 0.0, size: 0.0 };
            style.spacing.scroll = scroll;
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


#[cfg(test)]
mod tests {
    use super::{ShapeRole, TextRole, Theme};
    use crate::settings::{Design, GraphicsSettings};

    #[test]
    fn material3_exposes_standard_shape_scale_and_list_heights() {
        let settings = GraphicsSettings { design: Design::Material3, ..GraphicsSettings::default() };
        let theme = Theme::new(&settings);

        assert_eq!(theme.shape_radius(ShapeRole::None), 0);
        assert_eq!(theme.shape_radius(ShapeRole::ExtraSmall), 4);
        assert_eq!(theme.shape_radius(ShapeRole::Small), 8);
        assert_eq!(theme.shape_radius(ShapeRole::Medium), 12);
        assert_eq!(theme.shape_radius(ShapeRole::Large), 16);
        assert_eq!(theme.shape_radius(ShapeRole::ExtraLarge), 28);
        assert_eq!(theme.list_item_height(1), 56.0);
        assert_eq!(theme.list_item_height(2), 72.0);
        assert_eq!(theme.list_item_height(3), 88.0);
    }

    #[test]
    fn liquid_glass_keeps_legacy_list_geometry_and_core_text_sizes() {
        let settings = GraphicsSettings { design: Design::LiquidGlass, ..GraphicsSettings::default() };
        let theme = Theme::new(&settings);

        assert_eq!(theme.list_item_height(1), 56.0);
        assert_eq!(theme.list_item_height(2), 56.0);
        assert_eq!(theme.text_size(TextRole::DisplaySmall), 34.0);
        assert_eq!(theme.text_size(TextRole::BodyMedium), 15.0);
        assert_eq!(theme.text_size(TextRole::LabelSmall), 12.0);
    }
}

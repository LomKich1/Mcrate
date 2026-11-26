//! Иконки приложения.
//!
//! Material 3 использует Material Symbols Rounded (Google, Apache-2.0), а Liquid Glass —
//! Liquid Glass использует Lucide (ISC/MIT), Material 3 — Material Symbols Rounded.

use std::sync::OnceLock;

use egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, pos2, vec2};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons as mi;
use iconflow::{IconRef, Pack, Size, Style, try_icon};

use crate::theme::Theme;

#[repr(usize)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Play,
    Pause,
    Next,
    Prev,
    Shuffle,
    Repeat,
    RepeatOne,
    Volume,
    VolumeMute,
    Note,
    Queue,
    Settings,
    Plus,
    Folder,
    Home,
    Cloud,
    Search,
    Library,
    ChevronBack,
    ChevronForward,
    ChevronDown,
    ChevronUp,
    Checkmark,
    Close,
    /// Залитое сердце: лайк стоит.
    Heart,
    /// Пустое сердце: лайка нет.
    HeartOutline,
    /// Скачать онлайн-трек в постоянную папку.
    Download,
    /// Удалить трек или убрать его из коллекции.
    Trash,
    /// Три точки по вертикали: меню действий трека.
    MoreVertical,
}

impl Icon {
    const COUNT: usize = 29;

    const ALL: [Self; Self::COUNT] = [
        Self::Play,
        Self::Pause,
        Self::Next,
        Self::Prev,
        Self::Shuffle,
        Self::Repeat,
        Self::RepeatOne,
        Self::Volume,
        Self::VolumeMute,
        Self::Note,
        Self::Queue,
        Self::Settings,
        Self::Plus,
        Self::Folder,
        Self::Home,
        Self::Cloud,
        Self::Search,
        Self::Library,
        Self::ChevronBack,
        Self::ChevronForward,
        Self::ChevronDown,
        Self::ChevronUp,
        Self::Checkmark,
        Self::Close,
        Self::Heart,
        Self::HeartOutline,
        Self::Download,
        Self::Trash,
        Self::MoreVertical,
    ];

    fn material_glyph(self) -> MaterialIcon {
        match self {
            Self::Play => mi::ICON_PLAY_ARROW,
            Self::Pause => mi::ICON_PAUSE,
            Self::Next => mi::ICON_SKIP_NEXT,
            Self::Prev => mi::ICON_SKIP_PREVIOUS,
            Self::Shuffle => mi::ICON_SHUFFLE,
            Self::Repeat => mi::ICON_REPEAT,
            Self::RepeatOne => mi::ICON_REPEAT_ONE,
            Self::Volume => mi::ICON_VOLUME_UP,
            Self::VolumeMute => mi::ICON_VOLUME_OFF,
            Self::Note => mi::ICON_MUSIC_NOTE,
            Self::Queue => mi::ICON_QUEUE_MUSIC,
            Self::Settings => mi::ICON_SETTINGS,
            Self::Plus => mi::ICON_ADD,
            Self::Folder => mi::ICON_FOLDER,
            Self::Home => mi::ICON_HOME,
            Self::Cloud => mi::ICON_CLOUD,
            Self::Search => mi::ICON_SEARCH,
            Self::Library => mi::ICON_LIBRARY_MUSIC,
            Self::ChevronBack => mi::ICON_ARROW_BACK,
            Self::ChevronForward => mi::ICON_CHEVRON_RIGHT,
            Self::ChevronDown => mi::ICON_EXPAND_MORE,
            Self::ChevronUp => mi::ICON_EXPAND_LESS,
            Self::Checkmark => mi::ICON_CHECK,
            Self::Close => mi::ICON_CLOSE,
            Self::Heart => mi::ICON_FAVORITE.filled(),
            Self::HeartOutline => mi::ICON_FAVORITE.outlined(),
            Self::Download => mi::ICON_DOWNLOAD,
            Self::Trash => mi::ICON_DELETE,
            Self::MoreVertical => mi::ICON_MORE_VERT,
        }
    }

    /// Имена глифов Lucide.
    fn lucide_name(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Next => "skip-forward",
            Self::Prev => "skip-back",
            Self::Shuffle => "shuffle",
            Self::Repeat => "repeat",
            Self::RepeatOne => "repeat-1",
            Self::Volume => "volume-2",
            Self::VolumeMute => "volume-x",
            Self::Note => "music-2",
            Self::Queue => "list-music",
            Self::Settings => "settings",
            Self::Plus => "plus",
            Self::Folder => "folder",
            Self::Home => "house",
            Self::Cloud => "cloud",
            Self::Search => "search",
            Self::Library => "library",
            Self::ChevronBack => "arrow-left",
            Self::ChevronForward => "chevron-right",
            Self::ChevronDown => "chevron-down",
            Self::ChevronUp => "chevron-up",
            Self::Checkmark => "check",
            Self::Close => "x",
            Self::Heart | Self::HeartOutline => "heart",
            Self::Download => "download",
            Self::Trash => "trash-2",
            Self::MoreVertical => "ellipsis-vertical",
        }
    }

    /// Небольшая поправка оптического центра Material Symbols, измеренная по метрикам шрифта.
    fn material_nudge_up(self) -> f32 {
        match self {
            Self::Queue => 0.042,
            Self::VolumeMute => 0.029,
            Self::Home => 0.010,
            _ => 0.0,
        }
    }

    fn lucide_glyph(self) -> IconRef {
        static GLYPHS: OnceLock<[IconRef; Icon::COUNT]> = OnceLock::new();
        GLYPHS.get_or_init(|| {
            Icon::ALL.map(|icon| {
                try_icon(Pack::Lucide, icon.lucide_name(), Style::Regular, Size::Regular)
                    .unwrap_or_else(|error| panic!("Lucide glyph '{}' is missing: {error}", icon.lucide_name()))
            })
        })[self as usize]
    }
}

/// Зарегистрировать Material Symbols и Lucide в одном наборе шрифтов.
/// Оба набора загружаются при старте, чтобы переключение темы не требовало перезагрузки окна.
pub fn add_to_definitions(fonts: &mut egui::FontDefinitions) {
    use egui::epaint::text::FontPriority;

    // Залитый и контурный наборы: контурный нужен для пустого сердечка.
    for material in [egui_material_icons::font_insert(), egui_material_icons::font_insert_outlined()] {
        for family in &material.families {
            let list = fonts.families.entry(family.family.clone()).or_default();
            match family.priority {
                FontPriority::Highest => list.insert(0, material.name.clone()),
                FontPriority::Lowest => list.push(material.name.clone()),
            }
        }
        fonts.font_data.insert(material.name.clone(), std::sync::Arc::new(material.data));
    }

    // Lucide embedded TTF (единственный стиль в iconflow — Regular).
    let fallback_fonts: Vec<String> = fonts.font_data.keys().cloned().collect();
    for asset in iconflow::fonts() {
        let name = asset.family.to_owned();
        fonts.font_data.insert(name.clone(), std::sync::Arc::new(egui::FontData::from_static(asset.bytes)));
        let family = FontFamily::Name(asset.family.into());
        let family_fonts = fonts.families.entry(family).or_default();
        if !family_fonts.contains(&name) {
            family_fonts.insert(0, name);
        }
        for fallback in &fallback_fonts {
            if !family_fonts.contains(fallback) {
                family_fonts.push(fallback.clone());
            }
        }
    }
}

/// Нарисовать символ из набора, соответствующего активной теме.
/// Lucide используется только в Liquid Glass; Material 3 остаётся визуально неизменным.
pub fn paint(theme: &Theme, painter: &Painter, icon: Icon, center: Pos2, size: f32, color: Color32) {
    if theme.glass_active() {
        let glyph = icon.lucide_glyph();
        let character = char::from_u32(glyph.codepoint).unwrap_or('?').to_string();
        let font = FontId::new(size, FontFamily::Name(glyph.family.into()));

        // У глифов Lucide отличается высота и положение относительно font metrics.
        // CENTER_CENTER центрирует строковый прямоугольник, из-за чего видимые контуры
        // разных иконок кажутся стоящими на разной высоте/ширине. Центрируем tight mesh
        // bounds самого глифа — так все иконки выравниваются по реальному рисунку.
        let galley = painter.layout_no_wrap(character, font, color);
        let mesh_center = galley.mesh_bounds.center();
        painter.galley(center - mesh_center.to_vec2(), galley, color);
    } else {
        let glyph = icon.material_glyph();
        let shift_up = (0.05 + icon.material_nudge_up()) * size;
        painter.text(center - vec2(0.0, shift_up), Align2::CENTER_CENTER, glyph.codepoint, FontId::new(size, glyph.font_family()), color);
    }
}

/// Анимированный «эквалайзер» для играющего трека. `t` = время в секундах.
/// Это не иконка из набора, а анимация из трёх полосок, поэтому рисуется кодом.
pub fn paint_equalizer(painter: &Painter, center: Pos2, size: f32, color: Color32, t: f64, animate: bool) {
    let w = size * 0.18;
    let gap = size * 0.12;
    for i in 0..3 {
        let phase = if animate { ((t * 5.0 + i as f64 * 1.7).sin() * 0.5 + 0.5) as f32 } else { 0.5 };
        let h = size * (0.3 + 0.7 * phase);
        let x = center.x + (i as f32 - 1.0) * (w + gap);
        let rect = Rect::from_min_max(pos2(x - w / 2.0, center.y + size / 2.0 - h), pos2(x + w / 2.0, center.y + size / 2.0));
        painter.rect_filled(rect, w / 2.0, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Design, GraphicsSettings};

    #[test]
    fn heart_is_filled_when_liked_and_outlined_when_not() {
        use egui_material_icons::IconStyle;
        assert_eq!(Icon::Heart.material_glyph().style, IconStyle::Filled);
        assert_eq!(Icon::HeartOutline.material_glyph().style, IconStyle::Outlined);
        assert_eq!(Icon::Heart.material_glyph().codepoint, Icon::HeartOutline.material_glyph().codepoint, "тот же символ, разный стиль");
    }

    #[test]
    fn both_icon_packs_render_every_shared_icon() {
        let ctx = egui::Context::default();
        let mut defs = egui::FontDefinitions::default();
        add_to_definitions(&mut defs);
        ctx.set_fonts(defs);

        for design in [Design::Material3, Design::LiquidGlass] {
            let theme = Theme::new(&GraphicsSettings { design, ..GraphicsSettings::default() });
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                for icon in Icon::ALL {
                    paint(&theme, ui.painter(), icon, pos2(64.0, 64.0), 24.0, Color32::WHITE);
                }
                if design == Design::LiquidGlass {
                    for icon in Icon::ALL {
                        let glyph = icon.lucide_glyph();
                        let galley = ui.painter().layout_no_wrap(
                            char::from_u32(glyph.codepoint).unwrap_or('?').to_string(),
                            FontId::new(24.0, FontFamily::Name(glyph.family.into())),
                            Color32::WHITE,
                        );
                        assert!(galley.size().x >= 1.0 && galley.size().y >= 1.0, "{icon:?}: Lucide glyph did not render");
                        assert!(
                            galley.mesh_bounds.width() > 0.0 && galley.mesh_bounds.height() > 0.0,
                            "{icon:?}: Lucide glyph has no visible mesh bounds"
                        );
                    }
                }
            });
            output.textures_delta.clear();
        }
    }
}

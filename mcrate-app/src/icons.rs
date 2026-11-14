//! Иконки: готовый набор Google Material Symbols (Rounded, Filled) через крейт `egui_material_icons`.
//! Шрифт иконок вшит в exe, а каждая иконка это один символ шрифта: она чёткая на любом
//! размере и красится любым цветом, как обычный текст.
//!
//! Лицензии: крейт под MIT, сам шрифт Material Symbols под Apache 2.0 (см. THIRD_PARTY_NOTICES.md).

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, pos2, vec2};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons as mi;

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
}

impl Icon {
    fn glyph(self) -> MaterialIcon {
        match self {
            Icon::Play => mi::ICON_PLAY_ARROW,
            Icon::Pause => mi::ICON_PAUSE,
            Icon::Next => mi::ICON_SKIP_NEXT,
            Icon::Prev => mi::ICON_SKIP_PREVIOUS,
            Icon::Shuffle => mi::ICON_SHUFFLE,
            Icon::Repeat => mi::ICON_REPEAT,
            Icon::RepeatOne => mi::ICON_REPEAT_ONE,
            Icon::Volume => mi::ICON_VOLUME_UP,
            Icon::VolumeMute => mi::ICON_VOLUME_OFF,
            Icon::Note => mi::ICON_MUSIC_NOTE,
            Icon::Queue => mi::ICON_QUEUE_MUSIC,
            Icon::Settings => mi::ICON_TUNE,
            Icon::Plus => mi::ICON_ADD,
            Icon::Folder => mi::ICON_FOLDER,
        }
    }

    /// Поправка по вертикали (доля размера, плюс = вверх) для иконок, которые в самом шрифте
    /// нарисованы ниже центра квадрата. Значения вычислены по контурам глифов шрифта.
    fn nudge_up(self) -> f32 {
        match self {
            Icon::Queue => 0.042,
            Icon::VolumeMute => 0.029,
            _ => 0.0,
        }
    }
}

/// Крейт иконок сдвигает все глифы на 5% вниз (подгонка под текст кнопок). Для точной
/// центровки в круглых кнопках этот сдвиг компенсируем. Тест `glyph_offset_matches_constant`
/// следит, что после обновления крейта число не изменилось.
const CRATE_Y_OFFSET: f32 = 0.05;

/// Добавить шрифт иконок в набор шрифтов (после `fonts::definitions`). Делает то же, что
/// `egui_material_icons::initialize` (`Context::add_font`), но на уровне `FontDefinitions`:
/// так один и тот же набор можно отдать и основному, и закадровому контексту стекла.
pub fn add_to_definitions(fonts: &mut egui::FontDefinitions) {
    use egui::epaint::text::FontPriority;
    let insert = egui_material_icons::font_insert();
    for fam in &insert.families {
        let list = fonts.families.entry(fam.family.clone()).or_default();
        match fam.priority {
            FontPriority::Highest => list.insert(0, insert.name.clone()), // перед всеми остальными
            FontPriority::Lowest => list.push(insert.name.clone()),       // запасной, в самый конец
        }
    }
    fonts.font_data.insert(insert.name.clone(), std::sync::Arc::new(insert.data));
}

/// Нарисовать иконку с центром в `center`. `size` = размер шрифта (как «24dp» в Material:
/// иконка занимает квадрат size x size, сама картинка чуть меньше с полями внутри).
pub fn paint(painter: &Painter, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let g = icon.glyph();
    let shift_up = (CRATE_Y_OFFSET + icon.nudge_up()) * size;
    painter.text(center - vec2(0.0, shift_up), Align2::CENTER_CENTER, g.codepoint, FontId::new(size, g.font_family()), color);
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
        let r = Rect::from_min_max(pos2(x - w / 2.0, center.y + size / 2.0 - h), pos2(x + w / 2.0, center.y + size / 2.0));
        painter.rect_filled(r, w / 2.0, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Все глифы должны существовать в шрифте (иначе вместо иконки будет «квадратик»),
    /// а смещение глифа относительно центра строки должно остаться ~5% размера.
    #[test]
    fn glyph_offset_matches_constant() {
        let ctx = egui::Context::default();
        // Тот же порядок, что в приложении: сначала свой набор шрифтов, потом иконки.
        let mut defs = egui::FontDefinitions::default();
        add_to_definitions(&mut defs);
        ctx.set_fonts(defs);

        let all = [
            Icon::Play, Icon::Pause, Icon::Next, Icon::Prev, Icon::Shuffle, Icon::Repeat, Icon::RepeatOne,
            Icon::Volume, Icon::VolumeMute, Icon::Note, Icon::Queue, Icon::Settings, Icon::Plus, Icon::Folder,
        ];
        let mut failures: Vec<String> = Vec::new();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            for icon in all {
                for size in [20.0_f32, 24.0, 40.0] {
                    let g = icon.glyph();
                    let galley = ui.painter().layout_no_wrap(g.codepoint.to_string(), FontId::new(size, g.font_family()), Color32::WHITE);
                    let ink = galley.rows[0].visuals.mesh_bounds;
                    if ink.width() < 1.0 || ink.height() < 1.0 {
                        failures.push(format!("{icon:?}: глиф не нарисовался"));
                        continue;
                    }
                    // Положение иконки после нашей компенсации: ink_center - сдвиг вверх.
                    let after = (ink.center().y - galley.rect.center().y) - (CRATE_Y_OFFSET + icon.nudge_up()) * size;
                    if after.abs() > 0.2 * size * 0.1 + 0.75 {
                        failures.push(format!("{icon:?} при {size}: ink смещён на {after:+.2}px"));
                    }
                }
            }
        });
        out.textures_delta.clear();
        assert!(failures.is_empty(), "{failures:#?}");
    }
}

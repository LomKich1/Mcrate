//! Дизайн Liquid Glass. Стекло рисует крейт egui_glass (шейдер на wgpu: блюр + преломление
//! краёв + блик). LiveBackdrop рисует страницу (обои + плотную контентную панель) за кадром.
//! Рейка плавает над обоями; плеер — над содержимым страницы. Раскладка собрана в
//! `app.rs` (glass_layout), здесь только материал и кирпичики фоновых поверхностей.

use egui::{Color32, ColorImage, Margin, Painter, Rect, Stroke, StrokeKind, TextureId, Ui, pos2, vec2};
use egui_glass::{Glass, GlassStyle, LiveBackdropQuality};

use super::{Palette, Surface, Theme, material3, with_alpha};
use crate::settings::GlassQuality;

/// Наши настройки качества -> перечисление библиотеки.
pub fn lib_quality(q: GlassQuality) -> LiveBackdropQuality {
    match q {
        GlassQuality::Full => LiveBackdropQuality::Full,
        GlassQuality::Balanced => LiveBackdropQuality::Balanced,
        GlassQuality::Performance => LiveBackdropQuality::Performance,
    }
}

/// Нарисовать обои на весь `rect` (с обрезкой по краям, как `background-size: cover`).
/// Вызывается внутри страницы живого фона, поэтому картинку берём по `TextureId`, который
/// вернул `set_backdrop`: закадровый контекст знает эти текстуры, а вот состояние
/// `show_backdrop` в нём не хранится.
/// `translucent`: включён Mica/Acrylic. Тогда обои полупрозрачные и без сплошной подложки,
/// и системный материал виден сквозь них.
pub fn paint_wallpaper(painter: &Painter, rect: Rect, tex: Option<TextureId>, base: Color32, translucent: bool) {
    let alpha = if translucent { 110 } else { 255 };
    if !translucent {
        painter.rect_filled(rect, 0.0, base);
    }
    if let Some(id) = tex {
        let size = vec2(W as f32, H as f32);
        let scale = (rect.width() / size.x).max(rect.height() / size.y);
        let img = Rect::from_center_size(rect.center(), size * scale);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        // from_white_alpha = «премультипликативный» белый: так в egui задаётся прозрачность картинки
        painter.with_clip_rect(rect).image(id, img, uv, Color32::from_white_alpha(alpha));
    }
}

/// Контентная панель — обычная плотная поверхность, не Liquid Glass.
/// Стекло предназначено для навигации и постоянных контролов; списки и текст
/// должны оставаться резкими и не конкурировать с декоративным материалом.
pub fn paint_pane(theme: &Theme, painter: &Painter, rect: Rect) {
    let p = &theme.palette;
    let radius = theme.pane_radius() as f32;
    painter.rect_filled(rect, radius, p.surface_container_low);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, with_alpha(p.outline_variant, if p.dark { 58 } else { 92 })),
        StrokeKind::Inside,
    );
}

const W: usize = 512;
const H: usize = 320;

/// Процедурные обои: несколько мягких цветных пятен на тёмной/светлой основе.
/// Чем контрастнее картинка, тем лучше видно преломление.
pub fn wallpaper(p: &Palette) -> ColorImage {
    let base = p.surface_container_lowest;
    // (x, y, радиус, цвет, сила), координаты в долях от размера картинки
    let blobs = [
        (0.18_f32, 0.28_f32, 0.55_f32, p.primary, 0.85_f32),
        (0.82, 0.22, 0.50, p.tertiary_container, 0.95),
        (0.55, 0.88, 0.60, p.secondary_container, 0.90),
        (0.92, 0.80, 0.35, p.primary_container, 0.80),
    ];
    let mut img = ColorImage::filled([W, H], base);
    for y in 0..H {
        for x in 0..W {
            let (fx, fy) = (x as f32 / W as f32, y as f32 / H as f32);
            let mut col = base;
            for (bx, by, r, color, strength) in blobs {
                // Расстояние учитывает соотношение сторон, чтобы пятна были круглыми.
                let d = (((fx - bx) * 1.6).powi(2) + (fy - by).powi(2)).sqrt() / r;
                if d < 1.0 {
                    let t = 1.0 - d;
                    let falloff = t * t * (3.0 - 2.0 * t); // smoothstep: плавный спад к краю
                    col = col.lerp_to_gamma(color, falloff * strength);
                }
            }
            img[(x, y)] = col;
        }
    }
    img
}

/// Фон окна, когда стекло НЕ активно (блюр выключен): обычный фон Material 3.
/// Активное стекло рисует фон само, внутри живой страницы (см. `app.rs`, glass_layout).
pub fn paint_background(theme: &Theme, ui: &mut Ui, rect: egui::Rect) {
    material3::paint_background(theme, ui, rect);
}

pub fn surface<R>(theme: &Theme, ui: &mut Ui, role: Surface, add: impl FnOnce(&mut Ui) -> R) -> R {
    // Контентный слой никогда не становится стеклянным: это сохраняет читаемость
    // больших списков и экранов настроек. Остальные поверхности используют материал.
    if !theme.glass_active() || role == Surface::Content {
        return material3::surface(theme, ui, role, add);
    }

    let s = &theme.settings;
    let p = &theme.palette;

    let base = match (role, p.dark) {
        (Surface::PlayerBar, true) => GlassStyle::dark(),
        (Surface::PlayerBar, false) => GlassStyle::regular(),
        (_, true) => GlassStyle::panel_dark(),
        (_, false) => GlassStyle::panel(),
    };
    let tint_color = match role {
        Surface::PlayerBar | Surface::Dialog => p.surface_container_high,
        Surface::Rail => p.surface_container,
        Surface::Content => unreachable!("content layer is never glass"),
    };
    // Большая рейка использует спокойный panel preset, а плеер — более оптический
    // regular preset. У обоих остаётся один вариант материала (Regular), без Clear.
    let (blur_scale, refraction_scale) = match role {
        Surface::Rail => (1.18, 0.82),
        Surface::PlayerBar => (0.92, 1.35),
        Surface::Dialog => (1.0, 1.0),
        Surface::Content => unreachable!("content layer is never glass"),
    };
    let style = GlassStyle {
        blur: s.blur_radius * blur_scale,
        refraction: s.refraction * refraction_scale,
        corner_radius: match role {
            Surface::Rail | Surface::Dialog => 28.0,
            Surface::PlayerBar => 26.0,
            Surface::Content => unreachable!("content layer is never glass"),
        },
        // Значение настройки задаёт плотность цветной плёнки; при нуле остаётся
        // чистый блюр, при единице пользователь может сделать поверхность почти глухой.
        tint: with_alpha(tint_color, (s.glass_opacity.clamp(0.0, 1.0) * 255.0) as u8),
        ..base
    };

    let margin = match role {
        Surface::Rail => Margin::symmetric(12, 14),
        Surface::Content => Margin::same(20),
        Surface::PlayerBar => Margin::symmetric(24, 12),
        Surface::Dialog => Margin::same(24),
    };

    // Не даём контейнеру стекла перезаписать палитру дочерних контролов приложения.
    Glass::new(style)
        .inner_margin(margin)
        .preserve_theme(true)
        .show(ui, |ui| {
            match role {
                Surface::Rail | Surface::Content => ui.set_min_size(ui.available_size()),
                Surface::PlayerBar => ui.set_min_width(ui.available_width()),
                Surface::Dialog => {}
            }
            add(ui)
        })
        .inner
}

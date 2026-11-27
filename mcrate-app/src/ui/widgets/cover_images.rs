//! Настоящие картинки обложек в egui: превращение пикселей из хаба (`crate::covers`) в текстуры и рисование.
//!
//! Главное, что надо знать: у приложения два контекста egui. Основной рисует рейку и плеер, а в Liquid Glass
//! весь экран со списками рисуется ещё раз в закадровом («близнеце», см. `app.rs::glass_layout`) со своим рендерером.
//! Номера управляемых текстур в двух контекстах начинаются с нуля независимо, поэтому текстуру, созданную в одном,
//! другой покажет как чужую или пустую. Поэтому кэш текстур лежит в `ctx.data` КАЖДОГО контекста (данные у каждого свои),
//! а пиксели (`Arc<CoverImage>`) общие: каждый контекст загружает себе копию сам, когда впервые рисует обложку.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use egui::epaint::RectShape;
use egui::{Color32, ColorImage, Context, CornerRadius, Id, Rect, TextureHandle, TextureId, TextureOptions, Ui, pos2};
use mcrate_core::Track;
use mcrate_providers::{CoverImage, CoverSize};

use crate::covers::{CoverHub, CoverState};

/// Сколько текстур обложек держит один контекст. Маленькая около 100 КБ видеопамяти, большая около 1 МБ (их мало).
const MAX_TEXTURES: usize = 400;

/// Хаб обложек, общий для всего приложения. Глобальный, потому что рисуют обложки десятки мест
/// (строки, карточки, плеер) в двух контекстах, и тянуть хаб через все их параметры незачем.
static HUB: OnceLock<Arc<CoverHub>> = OnceLock::new();

/// Подключает хаб (один раз при запуске окна). До этого, и если хаба нет, везде рисуются заглушки.
pub(crate) fn install(hub: Arc<CoverHub>) {
    let _ = HUB.set(hub);
}

/// Одна текстура в кэше контекста.
struct Entry {
    /// Держим пиксели: пока запись жива, адрес `Arc` не может достаться другой картинке (по адресу ищем).
    _image: Arc<CoverImage>,
    texture: TextureHandle,
    /// Номер последнего показа (по нему вытесняем).
    used: u64,
}

/// Кэш текстур одного контекста. `Clone` дешёвый (общий `Arc`): так его можно положить в `ctx.data`.
#[derive(Clone, Default)]
struct TextureCache(Arc<Mutex<HashMap<usize, Entry>>>);

/// Рисует картинку обложки трека в `rect` с заданными радиусами углов.
/// `false`, если картинки (пока) нет: вызывающий рисует заглушку.
pub(crate) fn paint_track_image(ui: &Ui, rect: Rect, track: &Track, radius: CornerRadius) -> bool {
    let Some(hub) = HUB.get() else { return false };
    let ctx = ui.ctx();
    let size = CoverSize::for_pixels(rect.width().max(rect.height()) * ctx.pixels_per_point());
    let image = match hub.get(track, size) {
        CoverState::Ready(image) => Some(image),
        // Большая ещё грузится, а маленькая (из списка) уже есть: лучше чуть мягкая картинка, чем пустая заглушка.
        _ if size == CoverSize::Large => hub.peek(track, CoverSize::Small),
        _ => None,
    };
    let Some(image) = image else { return false };
    let texture = texture_id(ctx, &image);
    ui.painter().add(RectShape::filled(rect, radius, Color32::WHITE).with_texture(texture, cover_uv(rect)));
    true
}

/// Текстура для картинки в этом контексте: из кэша, а если нет, загружаем.
fn texture_id(ctx: &Context, image: &Arc<CoverImage>) -> TextureId {
    let cache: TextureCache = ctx.data_mut(|d| d.get_temp_mut_or_default::<TextureCache>(Id::NULL).clone());
    let mut entries = cache.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let used = ctx.cumulative_pass_nr();
    let key = Arc::as_ptr(image) as usize;
    if let Some(entry) = entries.get_mut(&key) {
        entry.used = used;
        return entry.texture.id();
    }
    let color = ColorImage::from_rgba_unmultiplied(image.size, &image.rgba);
    let texture = ctx.load_texture("cover", color, TextureOptions::LINEAR);
    let id = texture.id();
    entries.insert(key, Entry { _image: Arc::clone(image), texture, used });
    evict_oldest(&mut entries, MAX_TEXTURES, key);
    id
}

/// Пока записей больше `cap`, выбрасывает самую давно показанную (кроме `keep`). Выброшенная текстура освобождается сама.
fn evict_oldest(entries: &mut HashMap<usize, Entry>, cap: usize, keep: usize) {
    while entries.len() > cap {
        let Some(victim) = entries.iter().filter(|(key, _)| **key != keep).min_by_key(|(_, e)| e.used).map(|(key, _)| *key) else { break };
        entries.remove(&victim);
    }
}

/// Какую часть квадратной картинки показать в `rect`: у неквадратной плитки (половинки коллажа) берём середину, а не сплющиваем.
fn cover_uv(rect: Rect) -> Rect {
    let (w, h) = (rect.width(), rect.height());
    if w <= 0.0 || h <= 0.0 {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    let (inset_x, inset_y) = if w < h { ((1.0 - w / h) / 2.0, 0.0) } else { (0.0, (1.0 - h / w) / 2.0) };
    Rect::from_min_max(pos2(inset_x, inset_y), pos2(1.0 - inset_x, 1.0 - inset_y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn image(shade: u8) -> Arc<CoverImage> {
        Arc::new(CoverImage { size: [2, 2], rgba: vec![shade; 16] })
    }

    #[test]
    fn square_tile_shows_the_whole_picture() {
        let uv = cover_uv(Rect::from_min_size(pos2(5.0, 5.0), vec2(40.0, 40.0)));
        assert_eq!((uv.min, uv.max), (pos2(0.0, 0.0), pos2(1.0, 1.0)));
    }

    #[test]
    fn tall_tile_shows_the_middle_columns() {
        // Половинка коллажа 25 на 50: видна половина ширины картинки, строго по центру, высота целиком.
        let uv = cover_uv(Rect::from_min_size(pos2(0.0, 0.0), vec2(25.0, 50.0)));
        assert_eq!((uv.min, uv.max), (pos2(0.25, 0.0), pos2(0.75, 1.0)));
    }

    #[test]
    fn wide_tile_shows_the_middle_rows() {
        let uv = cover_uv(Rect::from_min_size(pos2(0.0, 0.0), vec2(50.0, 25.0)));
        assert_eq!((uv.min, uv.max), (pos2(0.0, 0.25), pos2(1.0, 0.75)));
    }

    #[test]
    fn degenerate_rect_does_not_divide_by_zero() {
        let uv = cover_uv(Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 10.0)));
        assert!(uv.min.x.is_finite() && uv.max.x.is_finite());
    }

    #[test]
    fn each_context_has_its_own_textures() {
        // Это и есть причина кэша «на контекст»: основной контекст и закадровый не делят номера текстур.
        let (main, twin) = (Context::default(), Context::default());
        let pic = image(7);
        let _ = texture_id(&main, &pic);
        let in_twin = |ctx: &Context| ctx.data_mut(|d| d.get_temp_mut_or_default::<TextureCache>(Id::NULL).clone()).0.lock().unwrap().len();
        assert_eq!(in_twin(&main), 1);
        assert_eq!(in_twin(&twin), 0, "в другом контексте текстуры нет, пока он сам её не загрузит");
        let _ = texture_id(&twin, &pic);
        assert_eq!(in_twin(&twin), 1);
    }

    #[test]
    fn same_picture_reuses_its_texture() {
        let ctx = Context::default();
        let pic = image(1);
        assert_eq!(texture_id(&ctx, &pic), texture_id(&ctx, &pic));
        assert_ne!(texture_id(&ctx, &pic), texture_id(&ctx, &image(2)));
    }

    #[test]
    fn eviction_drops_oldest_but_keeps_the_new_one() {
        let ctx = Context::default();
        let mut entries = HashMap::new();
        for (key, used) in [(1usize, 10u64), (2, 5), (3, 20)] {
            let texture = ctx.load_texture("t", ColorImage::example(), TextureOptions::LINEAR);
            entries.insert(key, Entry { _image: image(0), texture, used });
        }
        evict_oldest(&mut entries, 2, 2); // самая давняя (2) сохраняется как «только что пришедшая»
        let mut left: Vec<usize> = entries.keys().copied().collect();
        left.sort();
        assert_eq!(left, [2, 3], "из остальных выброшена давняя (1)");
    }
}

//! Обложка-заглушка и контур загрузки вокруг неё.

use super::cover_images::paint_track_image;
use crate::icons::{self, Icon};
use crate::settings::Design;
use crate::theme::{Theme, with_alpha};
use egui::{CornerRadius, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};
use mcrate_core::{ProviderKind, Track};

/// Радиус скругления настолько велик, что прямоугольник превращается в круг (у круглых обложек исполнителей он равен
/// половине стороны). Такой `rect_filled` у egui строит вырожденный контур, и полупрозрачная заливка показывает
/// тонкую полосу (шов треугольников): круг рисуют через `circle_filled`.
pub(crate) fn is_circle(rect: Rect, radius: CornerRadius) -> bool {
    let half = rect.width().min(rect.height()) / 2.0;
    rect.width() == rect.height() && [radius.nw, radius.ne, radius.sw, radius.se].iter().all(|&r| f32::from(r) >= half - 0.5)
}

/// Цветная плитка-заглушка: вместо обложки, пока она грузится или её нет (и там, где обложка не трек: рейка, коллекции).
pub(crate) fn cover_tile(ui: &mut Ui, theme: &Theme, rect: Rect, seed: &str, radius: f32) {
    cover_tile_shaped(ui, theme, rect, seed, CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8));
}

/// Обложка трека: настоящая картинка, а пока она грузится, у трека её нет или не вышло загрузить, цветная заглушка.
/// Загрузку запускает сама (см. `crate::covers`), так что вызывающему достаточно показать трек.
pub(crate) fn track_cover(ui: &mut Ui, theme: &Theme, rect: Rect, track: &Track, radius: f32) {
    let radius = radius.min(rect.width().min(rect.height()) / 2.0 - 1.0).max(0.0); // см. `cover_tile`: полный радиус даёт шов
    track_cover_shaped(ui, theme, rect, track, CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8));
}

/// То же, но с отдельным радиусом у каждого угла (плитки коллажа).
fn track_cover_shaped(ui: &mut Ui, theme: &Theme, rect: Rect, track: &Track, radius: CornerRadius) {
    if !paint_track_image(ui, rect, track, radius) {
        cover_tile_shaped(ui, theme, rect, &track.id, radius);
    }
}

/// То же, что `cover_tile`, но с отдельным радиусом у каждого угла (плитки составной обложки скруглены только по внешним углам).
fn cover_tile_shaped(ui: &mut Ui, theme: &Theme, rect: Rect, seed: &str, radius: CornerRadius) {
    let p = &theme.palette;
    let h = seed.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let (mut bg, fg) = match h % 3 {
        0 => (p.primary_container, p.on_primary_container),
        1 => (p.secondary_container, p.on_secondary_container),
        _ => (p.tertiary_container, if theme.design() == Design::Material3 { p.on_tertiary_container } else { p.on_surface }),
    };
    // Эти цветные плитки — временная замена обложек, а не M3-контейнеры.
    // В Liquid Glass слегка приглушаем тон, чтобы заглушки не спорили с альбомным рядом.
    if theme.glass_active() {
        bg = bg.lerp_to_gamma(p.surface_container_lowest, 0.26);
    }
    let painter = ui.painter();
    if is_circle(rect, radius) {
        painter.circle_filled(rect.center(), rect.width() / 2.0, bg);
    } else {
        painter.rect_filled(rect, radius, bg);
    }
    icons::paint(theme, painter, Icon::Note, rect.center(), rect.height() * 0.5, with_alpha(fg, 200));
}

/// Зазор между плитками составной обложки плейлиста.
const COLLAGE_GAP: f32 = 2.0;
/// Сколько первых треков плейлиста попадает в обложку.
pub(crate) const COLLAGE_MAX_TILES: usize = 4;
/// Сколько первых треков просматриваем, подбирая плитки коллажа. Список может быть в тысячи треков, а коллажу нужно четыре.
const COLLAGE_SCAN: usize = 24;

/// Треки для коллажа коллекции или плейлиста: первые с РАЗНЫМИ обложками. Четыре трека одного альбома дали бы
/// четыре одинаковые плитки. Пропускаются и онлайн-треки без обложки (показывать там нечего). Если подходящих нет,
/// остаётся один первый трек (одна заглушка), у пустого списка результат пустой.
pub(crate) fn collage_sample(tracks: &[Track]) -> Vec<Track> {
    let mut picked: Vec<Track> = Vec::with_capacity(COLLAGE_MAX_TILES);
    for track in tracks.iter().take(COLLAGE_SCAN) {
        if picked.len() == COLLAGE_MAX_TILES {
            break;
        }
        let has_picture = track.provider == ProviderKind::Local || track.cover_url.is_some(); // у своего файла она может быть в теге
        let repeats = track.cover_url.is_some() && picked.iter().any(|p| p.cover_url == track.cover_url);
        if has_picture && !repeats {
            picked.push(track.clone());
        }
    }
    if picked.is_empty() {
        picked.extend(tracks.first().cloned());
    }
    picked
}

/// Плитка составной обложки: где она лежит и какие её углы внешние (верхний левый, верхний правый, нижний левый,
/// нижний правый): только внешние скругляются.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CollageTile {
    rect: Rect,
    outer: [bool; 4],
}

/// Раскладка плиток в `rect` для `count` треков (от 1 до 4, больше четырёх считается как четыре):
/// 1: одна обложка на весь квадрат; 2: две колонки; 3: слева большая, справа две друг под другом; 4: сетка 2 на 2.
fn collage_tiles(rect: Rect, count: usize) -> Vec<CollageTile> {
    let count = count.clamp(1, COLLAGE_MAX_TILES);
    let half = (COLLAGE_GAP / 2.0).min(rect.width() / 4.0);
    let (cx, cy) = (rect.center().x, rect.center().y);
    let left = Rect::from_min_max(rect.min, pos2(cx - half, rect.bottom()));
    let right = Rect::from_min_max(pos2(cx + half, rect.top()), rect.max);
    let quad = |x: bool, y: bool| {
        let (x0, x1) = if x { (cx + half, rect.right()) } else { (rect.left(), cx - half) };
        let (y0, y1) = if y { (cy + half, rect.bottom()) } else { (rect.top(), cy - half) };
        Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
    };
    match count {
        1 => vec![CollageTile { rect, outer: [true; 4] }],
        2 => vec![
            CollageTile { rect: left, outer: [true, false, true, false] },
            CollageTile { rect: right, outer: [false, true, false, true] },
        ],
        3 => vec![
            CollageTile { rect: left, outer: [true, false, true, false] },
            CollageTile { rect: quad(true, false), outer: [false, true, false, false] },
            CollageTile { rect: quad(true, true), outer: [false, false, false, true] },
        ],
        _ => vec![
            CollageTile { rect: quad(false, false), outer: [true, false, false, false] },
            CollageTile { rect: quad(true, false), outer: [false, true, false, false] },
            CollageTile { rect: quad(false, true), outer: [false, false, true, false] },
            CollageTile { rect: quad(true, true), outer: [false, false, false, true] },
        ],
    }
}

/// Обложка плейлиста из обложек его первых четырёх треков: чем меньше треков, тем меньше плиток (см. `collage_tiles`).
/// Пустой плейлист получает одну плитку-заглушку по названию. Плитка трека без обложки (или пока она грузится)
/// это цветная заглушка по id трека.
pub(crate) fn playlist_cover(ui: &mut Ui, theme: &Theme, rect: Rect, (name, tracks): (&str, &[Track]), radius: f32) {
    let r = radius.round().clamp(0.0, 255.0) as u8;
    let corner = |outer: [bool; 4]| {
        let pick = |on: bool| if on { r } else { 0 };
        CornerRadius { nw: pick(outer[0]), ne: pick(outer[1]), sw: pick(outer[2]), se: pick(outer[3]) }
    };
    if tracks.is_empty() {
        cover_tile_shaped(ui, theme, rect, name, corner([true; 4]));
        return;
    }
    for (tile, track) in collage_tiles(rect, tracks.len()).into_iter().zip(tracks) {
        track_cover_shaped(ui, theme, tile.rect, track, corner(tile.outer));
    }
}

/// Замкнутая ломаная по скруглённому прямоугольнику, по часовой стрелке (ось y экрана смотрит вниз).
/// Последняя точка не повторяет первую.
fn rounded_rect_ring(rect: Rect, r: f32) -> Vec<Pos2> {
    const STEPS: usize = 6; // сегментов на каждый скруглённый угол
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0_f32),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    let mut pts: Vec<Pos2> = Vec::with_capacity(4 * (STEPS + 1) + 1);
    for (center, start) in corners {
        for i in 0..=STEPS {
            let a = (start + 90.0 * i as f32 / STEPS as f32).to_radians();
            pts.push(center + vec2(a.cos(), a.sin()) * r);
        }
    }
    pts
}

/// Контур загрузки: яркий отрезок с затухающим хвостом бежит по периметру обложки, пока трек грузится.
/// Чисел нет. Контур рисуется чуть внутри обложки (`rect`), чтобы его не обрезала область панели.
/// При включённом «уменьшении движения» вместо бега показывается неподвижная рамка.
pub(crate) fn loading_outline(ui: &Ui, theme: &Theme, cover: Rect, radius: f32, time: f64) {
    const TRAIL: f32 = 0.30; // длина бегущего отрезка, доля периметра
    const LAP_SECS: f64 = 1.4; // один круг по периметру
    const SEGMENTS: usize = 28; // на сколько кусочков режем отрезок для затухания хвоста

    let p = &theme.palette;
    let rect = cover.shrink(1.5);
    let r = (radius - 1.5).clamp(0.0, rect.width().min(rect.height()) * 0.5);

    let ring = rounded_rect_ring(rect, r);
    let mut pts = ring.clone();
    pts.push(pts[0]); // замыкаем, чтобы последний отрезок периметра тоже считался

    // Накопленная длина вдоль периметра: по ней находим точку на расстоянии `s` от старта.
    let mut cum: Vec<f32> = Vec::with_capacity(pts.len());
    cum.push(0.0);
    for w in pts.windows(2) {
        let last = *cum.last().unwrap_or(&0.0);
        cum.push(last + (w[1] - w[0]).length());
    }
    let total = *cum.last().unwrap_or(&0.0);
    if total <= f32::EPSILON {
        return;
    }
    let point_at = |s: f32| -> Pos2 {
        let s = s.rem_euclid(total); // за пределами круга заворачиваем в начало
        let i = cum.partition_point(|&c| c <= s).saturating_sub(1).min(pts.len() - 2);
        let seg = cum[i + 1] - cum[i];
        if seg <= f32::EPSILON { pts[i] } else { pts[i].lerp(pts[i + 1], (s - cum[i]) / seg) }
    };

    let painter = ui.painter();
    if theme.settings.reduce_motion {
        let _ = painter.add(Shape::closed_line(ring, Stroke::new(2.0, with_alpha(p.primary, 170))));
        return;
    }
    // Тусклая дорожка по всему периметру, чтобы было видно, где бежит отрезок.
    let _ = painter.add(Shape::closed_line(ring, Stroke::new(1.5, with_alpha(p.primary, 60))));
    let head = ((time / LAP_SECS).fract() as f32) * total;
    let trail = total * TRAIL;
    for k in 0..SEGMENTS {
        let a = k as f32 / SEGMENTS as f32;
        let b = (k + 1) as f32 / SEGMENTS as f32;
        let alpha = (255.0 * b).round() as u8; // хвост тает, голова самая яркая
        painter.line_segment(
            [point_at(head - trail * (1.0 - a)), point_at(head - trail * (1.0 - b))],
            Stroke::new(2.5, with_alpha(p.primary, alpha)),
        );
    }
    ui.ctx().request_repaint(); // анимация идёт сама, без движения мыши
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), vec2(100.0, 100.0))
    }

    fn online(id: &str, cover: Option<&str>) -> Track {
        Track {
            id: id.into(),
            provider: ProviderKind::SoundCloud,
            title: id.into(),
            artists: vec![],
            duration_secs: 1,
            cover_url: cover.map(str::to_string),
        }
    }

    fn local(id: &str) -> Track {
        Track { provider: ProviderKind::Local, ..online(id, None) }
    }

    fn ids(tracks: &[Track]) -> Vec<&str> {
        tracks.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn collage_sample_takes_distinct_covers_first() {
        // Первые три трека одного альбома: плитка нужна одна, остальные места занимают другие обложки.
        let tracks =
            [online("1", Some("a")), online("2", Some("a")), online("3", Some("a")), online("4", Some("b")), online("5", Some("c"))];
        assert_eq!(ids(&collage_sample(&tracks)), ["1", "4", "5"]);
    }

    #[test]
    fn collage_sample_stops_at_four_and_skips_pictureless_online() {
        let tracks: Vec<Track> = (0..30).map(|i| online(&i.to_string(), Some(&format!("c{i}")))).collect();
        assert_eq!(collage_sample(&tracks).len(), COLLAGE_MAX_TILES);
        let mixed = [online("x", None), online("y", Some("a")), local("z")];
        assert_eq!(ids(&collage_sample(&mixed)), ["y", "z"], "онлайн без обложки пропущен, свой файл берём (обложка может быть в теге)");
    }

    #[test]
    fn collage_sample_falls_back_to_one_placeholder() {
        let bare = [online("x", None), online("y", None)];
        assert_eq!(ids(&collage_sample(&bare)), ["x"]);
        assert!(collage_sample(&[]).is_empty());
    }

    #[test]
    fn collage_sample_does_not_scan_a_huge_list() {
        // Подходящие треки только после окна просмотра: берём заглушку, а не перебираем тысячи.
        let mut tracks: Vec<Track> = (0..COLLAGE_SCAN).map(|i| online(&i.to_string(), None)).collect();
        tracks.push(online("late", Some("a")));
        assert_eq!(ids(&collage_sample(&tracks)), ["0"]);
    }

    #[test]
    fn collage_has_one_tile_per_track_up_to_four() {
        for (count, tiles) in [(1, 1), (2, 2), (3, 3), (4, 4), (9, 4)] {
            assert_eq!(collage_tiles(square(), count).len(), tiles, "треков: {count}");
        }
        assert_eq!(collage_tiles(square(), 0).len(), 1, "ноль считается как один");
    }

    #[test]
    fn collage_tiles_stay_inside_and_do_not_overlap() {
        for count in 1..=4 {
            let tiles = collage_tiles(square(), count);
            for (i, a) in tiles.iter().enumerate() {
                assert!(square().contains_rect(a.rect), "плитка вышла за обложку: {a:?}");
                assert!(a.rect.width() > 1.0 && a.rect.height() > 1.0);
                for b in &tiles[i + 1..] {
                    assert!(!a.rect.intersects(b.rect) || a.rect.intersect(b.rect).area() < 0.01, "плитки пересекаются: {a:?} {b:?}");
                }
            }
        }
    }

    #[test]
    fn collage_fills_the_whole_square() {
        for count in 1..=4 {
            let tiles = collage_tiles(square(), count);
            let area: f32 = tiles.iter().map(|t| t.rect.area()).sum();
            let gaps = square().area() - area;
            assert!(gaps >= 0.0 && gaps <= square().area() * 0.06, "треков {count}: зазоры занимают {gaps}");
        }
    }

    #[test]
    fn each_square_corner_is_rounded_by_exactly_one_tile() {
        for count in 1..=4 {
            let tiles = collage_tiles(square(), count);
            for corner in 0..4 {
                let owners = tiles.iter().filter(|t| t.outer[corner]).count();
                assert_eq!(owners, 1, "треков {count}, угол {corner}: внешний угол должен скруглять ровно одна плитка");
            }
        }
    }

    #[test]
    fn tiles_are_separated_by_the_gap() {
        let two = collage_tiles(square(), 2);
        assert_eq!(two[1].rect.left() - two[0].rect.right(), COLLAGE_GAP);
        let grid = collage_tiles(square(), 4);
        assert_eq!(grid[1].rect.left() - grid[0].rect.right(), COLLAGE_GAP);
        assert_eq!(grid[2].rect.top() - grid[0].rect.bottom(), COLLAGE_GAP);
        let three = collage_tiles(square(), 3);
        assert_eq!(three[2].rect.top() - three[1].rect.bottom(), COLLAGE_GAP);
    }
}

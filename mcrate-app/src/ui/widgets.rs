//! Общие виджеты. Рисуются вручную (painter), цвета берут из темы,
//! поэтому выглядят правильно и в Material 3, и в Liquid Glass.

use egui::{Align2, Color32, CursorIcon, FontId, Id, Rect, Response, Sense, Shape, Ui, Vec2, pos2, vec2, layers::ShapeIdx};
use mcrate_core::Track;

use crate::icons::{self, Icon};
use crate::settings::Design;
use crate::theme::{Theme, with_alpha};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Plain,  // только иконка
    Tonal,  // приглушённая заливка
    Filled, // главный акцент (кнопка «играть»)
}


// ---------------------------------------------------------------------------------------------
// Анимации по образцу демо «Анимации плеера в стиле Riff»
// ---------------------------------------------------------------------------------------------

/// CSS `cubic-bezier(x1, y1, x2, y2)`: превращает время 0..1 в прогресс (может выходить за 1: «пружина»).
/// Решаем x(s) = t бисекцией (деление отрезка пополам), затем берём y(s).
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let at = |a: f32, b: f32, s: f32| 3.0 * (1.0 - s) * (1.0 - s) * s * a + 3.0 * (1.0 - s) * s * s * b + s * s * s;
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        if at(x1, x2, mid) < t { lo = mid } else { hi = mid }
    }
    at(y1, y2, (lo + hi) / 2.0)
}

/// Кривая «нажатия» из демо: `cubic-bezier(.3, 1.6, .5, 1)`, с небольшим перелётом.
fn press_curve(t: f32) -> f32 {
    cubic_bezier(0.3, 1.6, 0.5, 1.0, t)
}

/// Плавное «сжатие» при нажатии (в демо `scale(.88, .94)` у кнопки): прямоугольник уменьшается
/// вокруг центра на `sx`/`sy` (доли) с перелётом при нажатии и отпускании.
fn squish(ui: &Ui, id: Id, resp: &Response, rect: Rect, sx: f32, sy: f32) -> Rect {
    let t = ui.ctx().animate_bool_with_time(id.with("press"), resp.is_pointer_button_down_on(), 0.18);
    let k = press_curve(t);
    Rect::from_center_size(rect.center(), vec2(rect.width() * (1.0 - sx * k), rect.height() * (1.0 - sy * k)))
}

/// Состояние скользящей подсветки (хранится в памяти egui).
///
/// `from_*` — смещение подсветки относительно цели в момент старта пружины.
/// Пока пружина идёт, рисуем `цель + смещение * (1 - прогресс)`. Прогресс больше 1
/// значит перелёт за цель, меньше 1 после этого — возврат. Если цель просто
/// прокрутилась (тот же `key`), смещение не сбрасываем: подсветка едет вместе со строкой.
#[derive(Clone, Copy)]
struct Glide {
    key: u64,
    target: Rect,
    from_min: Vec2,
    from_max: Vec2,
    t0: f64,
    alpha: f32,
    alive: bool,
}

impl Default for Glide {
    fn default() -> Self {
        Self {
            key: 0,
            target: Rect::ZERO,
            from_min: Vec2::ZERO,
            from_max: Vec2::ZERO,
            t0: 0.0,
            alpha: 0.0,
            alive: false,
        }
    }
}

/// Пружина перемещения подсветки. `t` — секунды с начала хода.
///
/// До первого пика (0.23 с, перелёт около 6%) это прежняя кривая меню
/// `cubic-bezier(.2, .9, .3, 1.2)` за 0.35 с, поэтому скорость разгона та же.
/// Раньше после пика был один плавный возврат и подсветка замирала. Теперь в этот
/// момент несколько колебаний вокруг цели, каждое следующее примерно на треть слабее.
pub fn spring_highlight(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    // Секунда первого пика старой кривой.
    const PEAK_T: f32 = 0.2282;
    if t < PEAK_T {
        return cubic_bezier(0.2, 0.9, 0.3, 1.2, t / 0.35);
    }
    // Затухающий косинус: полпериода 0.11 с, следующее полуколебание = 0.68 от предыдущего.
    const A: f32 = 0.05922308; // высота первого перелёта
    const B: f32 = 0.007270; // слагаемое синуса, чтобы скорость в пике не прыгала
    const DECAY: f32 = 3.506023;
    const OMEGA: f32 = 28.559933; // π / 0.11
    let dt = t - PEAK_T;
    let env = (-DECAY * dt).exp();
    1.0 + env * (A * (OMEGA * dt).cos() + B * (OMEGA * dt).sin())
}

/// Пружина уже не видна: хвост меньше ~0.3% пути.
pub fn spring_highlight_resting(t: f32) -> bool {
    t >= 1.08
}

/// Скользящая подсветка под курсором: одна на весь список. Когда курсор переходит со строки на строку,
/// она не прыгает, а едет пружиной (`spring_highlight`: перелёт и несколько затухающих колебаний).
/// Рисуется ПОД содержимым пунктов: сначала `glide_begin` резервирует место, после отрисовки `glide_end`
/// кладёт в него фигуру.
///
/// `target` — id строки и её прямоугольник, либо `None`, если курсор не над списком.
/// Id отличает смену строки от прокрутки той же строки.
pub fn glide_begin(ui: &Ui) -> ShapeIdx {
    ui.painter().add(Shape::Noop)
}

pub fn glide_end(ui: &Ui, theme: &Theme, slot: ShapeIdx, salt: Id, target: Option<(u64, Rect)>, radius: f32) {
    let now = ui.input(|i| i.time);
    let dt = ui.input(|i| i.stable_dt).min(0.05);
    let mut g: Glide = ui.ctx().data_mut(|d| d.get_temp(salt)).unwrap_or_default();
    let mut busy = false;

    if let Some((key, rect)) = target {
        if !g.alive || g.alpha < 0.02 {
            // Погасла: появляется сразу на строке, без полёта через весь список.
            g.alive = true;
            g.key = key;
            g.target = rect;
            g.from_min = Vec2::ZERO;
            g.from_max = Vec2::ZERO;
            g.t0 = now - 10.0;
        } else if g.key != key {
            let k = spring_highlight((now - g.t0) as f32);
            let disp_min = g.target.min + g.from_min * (1.0 - k);
            let disp_max = g.target.max + g.from_max * (1.0 - k);
            g.from_min = disp_min - rect.min;
            g.from_max = disp_max - rect.max;
            g.target = rect;
            g.key = key;
            g.t0 = now;
            busy = true;
        } else {
            g.target = rect;
            if spring_highlight_resting((now - g.t0) as f32) {
                g.from_min = Vec2::ZERO;
                g.from_max = Vec2::ZERO;
            } else {
                busy = true;
            }
        }
    }

    if target.is_some() {
        g.alpha += (1.0 - g.alpha) * (1.0 - (-dt * 20.0).exp());
        if g.alpha > 0.99 {
            g.alpha = 1.0;
        } else {
            busy = true;
        }
    } else {
        g.alpha -= g.alpha * (1.0 - (-dt * 14.0).exp());
        if g.alpha < 0.01 {
            g.alpha = 0.0;
        } else {
            busy = true;
        }
    }

    if g.alive && g.alpha > 0.0 {
        let k = spring_highlight((now - g.t0) as f32);
        let shown = Rect::from_min_max(g.target.min + g.from_min * (1.0 - k), g.target.max + g.from_max * (1.0 - k));
        let a = (theme.hover_glide_alpha() as f32 * g.alpha) as u8;
        ui.painter().set(slot, Shape::rect_filled(shown, radius, Color32::from_black_alpha(a)));
    }
    if busy {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(salt, g));
}

/// «Слой состояния» из M3: при наведении поверх кладётся 8% цвета контента, при нажатии 12%.
fn state_alpha(r: &Response) -> u8 {
    if r.is_pointer_button_down_on() {
        30
    } else if r.hovered() {
        20
    } else {
        0
    }
}

/// Круглая кнопка с иконкой.
pub fn icon_button(ui: &mut Ui, theme: &Theme, icon: Icon, diameter: f32, kind: ButtonKind, active: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    let p = &theme.palette;
    let (bg, fg) = match kind {
        ButtonKind::Filled => (Some(p.primary), p.on_primary),
        ButtonKind::Tonal => (Some(theme.tonal_fill()), theme.on_tonal()),
        ButtonKind::Plain => (None, if active { p.primary } else { p.on_surface }),
    };
    let painter = ui.painter();
    let r = diameter / 2.0;
    if let Some(bg) = bg {
        painter.circle_filled(rect.center(), r, bg);
    }
    let a = state_alpha(&resp);
    if a > 0 {
        painter.circle_filled(rect.center(), r, with_alpha(fg, a));
    }
    icons::paint(painter, icon, rect.center(), diameter * 0.6, fg);
    resp.on_hover_text(tip).on_hover_cursor(CursorIcon::PointingHand)
}

/// Кнопка-«таблетка» с текстом и необязательной иконкой.
pub fn pill_button(ui: &mut Ui, theme: &Theme, label: &str, icon: Option<Icon>, kind: ButtonKind) -> Response {
    let p = &theme.palette;
    let (bg, fg) = match kind {
        ButtonKind::Filled => (p.primary, p.on_primary),
        _ => (theme.tonal_fill(), theme.on_tonal()),
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(14.0), fg);
    let icon_w = if icon.is_some() { 20.0 + 8.0 } else { 0.0 };
    let size = vec2(galley.size().x + icon_w + 36.0, 40.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 20.0, bg);
    let a = state_alpha(&resp);
    if a > 0 {
        painter.rect_filled(rect, 20.0, with_alpha(fg, a));
    }
    let mut x = rect.left() + 18.0;
    if let Some(icon) = icon {
        icons::paint(painter, icon, pos2(x + 10.0, rect.center().y), 18.0, fg);
        x += icon_w;
    }
    painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Пункт навигации (ширина = вся доступная, высота 48, форма «таблетка»).
/// `compact`: только иконка по центру (узкая панель), подпись показывается во всплывающей подсказке.
pub fn nav_item(ui: &mut Ui, theme: &Theme, icon: Icon, label: &str, selected: bool, enabled: bool, trailing: Option<&str>, compact: bool) -> Response {
    let p = &theme.palette;
    let size = vec2(ui.available_width(), 48.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let id = resp.id;
    let rect = if enabled { squish(ui, id, &resp, rect, 0.04, 0.10) } else { rect };
    let painter = ui.painter();

    // Тёмная заливка только пока кнопка мыши зажата. Простое наведение меню не подсвечивает.
    // Фон уже выбранного пункта рисует rail.rs отдельной пилюлей.
    if enabled && resp.is_pointer_button_down_on() {
        painter.rect_filled(rect, 24.0, Color32::from_black_alpha(theme.hover_glide_alpha()));
    }

    let fg = if !enabled {
        with_alpha(p.on_surface_variant, 120)
    } else if selected {
        match theme.design() {
            Design::Material3 => theme.on_tonal(),
            Design::LiquidGlass => p.on_surface,
        }
    } else {
        p.on_surface_variant
    };

    // Фон выбранного пункта рисует rail.rs: пилюля, которая плавно скользит между пунктами.
    if compact {
        icons::paint(painter, icon, rect.center(), 24.0, fg);
    } else {
        icons::paint(painter, icon, pos2(rect.left() + 28.0, rect.center().y), 24.0, fg);
        painter.text(pos2(rect.left() + 52.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(15.0), fg);
        if let Some(t) = trailing {
            painter.text(pos2(rect.right() - 14.0, rect.center().y), Align2::RIGHT_CENTER, t, FontId::proportional(11.0), fg);
        }
    }
    let resp = if compact { resp.on_hover_text(label) } else { resp };
    if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

/// Результат ползунка: `value` = Some(доля 0..1), пока пользователь тянет или кликает.
pub struct BarOutput {
    pub response: Response,
    pub value: Option<f32>,
}

/// Ползунок для перемотки и громкости. В M3 ручка в виде вертикальной «таблетки» (Expressive),
/// в Liquid Glass круглая.
pub fn slider_bar(ui: &mut Ui, theme: &Theme, width: f32, frac: f32, enabled: bool) -> BarOutput {
    let p = &theme.palette;
    let sense = if enabled { Sense::click_and_drag() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 24.0), sense);
    let pad = 10.0;
    let (left, right) = (rect.left() + pad, rect.right() - pad);
    let cy = rect.center().y;
    let active = resp.hovered() || resp.dragged();
    let th = if active { 8.0 } else { 6.0 };
    let frac = frac.clamp(0.0, 1.0);
    let x = left + (right - left) * frac;

    let painter = ui.painter();
    let track_bg = with_alpha(p.on_surface, if p.dark { 50 } else { 40 });
    let fg = if enabled { p.primary } else { with_alpha(p.on_surface_variant, 120) };

    match theme.design() {
        Design::Material3 => {
            // Активная часть, зазор, ручка-«таблетка», неактивная часть.
            let gap = 6.0;
            let a_end = (x - gap).max(left);
            if a_end > left {
                painter.rect_filled(Rect::from_min_max(pos2(left, cy - th / 2.0), pos2(a_end, cy + th / 2.0)), th / 2.0, fg);
            }
            let i_start = (x + gap).min(right);
            if right > i_start {
                painter.rect_filled(Rect::from_min_max(pos2(i_start, cy - th / 2.0), pos2(right, cy + th / 2.0)), th / 2.0, track_bg);
            }
            let hw = if resp.dragged() { 3.0 } else { 4.0 };
            painter.rect_filled(Rect::from_center_size(pos2(x, cy), vec2(hw, 22.0)), hw / 2.0, fg);
        }
        Design::LiquidGlass => {
            painter.rect_filled(Rect::from_min_max(pos2(left, cy - th / 2.0), pos2(right, cy + th / 2.0)), th / 2.0, track_bg);
            if x > left {
                painter.rect_filled(Rect::from_min_max(pos2(left, cy - th / 2.0), pos2(x, cy + th / 2.0)), th / 2.0, fg);
            }
            let r = if active { 9.0 } else { 7.5 };
            painter.circle_filled(pos2(x, cy + 1.0), r, egui::Color32::from_black_alpha(50));
            painter.circle_filled(pos2(x, cy), r, egui::Color32::WHITE);
        }
    }

    let value = if resp.dragged() || resp.clicked() || resp.drag_stopped() {
        resp.interact_pointer_pos().map(|pos| ((pos.x - left) / (right - left)).clamp(0.0, 1.0))
    } else {
        None
    };
    BarOutput { response: resp, value }
}

/// Цветная плитка-заглушка вместо обложки (обложек в ядре пока нет).
pub fn cover_tile(ui: &mut Ui, theme: &Theme, rect: Rect, seed: &str, radius: f32) {
    let p = &theme.palette;
    let h = seed.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let (bg, fg) = match h % 3 {
        0 => (p.primary_container, p.on_primary_container),
        1 => (p.secondary_container, p.on_secondary_container),
        _ => (p.tertiary_container, p.on_surface),
    };
    let painter = ui.painter();
    painter.rect_filled(rect, radius, bg);
    icons::paint(painter, Icon::Note, rect.center(), rect.height() * 0.5, with_alpha(fg, 200));
}

pub fn fmt_time(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub fn artists_text(track: &Track) -> String {
    if track.artists.is_empty() {
        "Неизвестный исполнитель".to_string()
    } else {
        track.artists.join(", ")
    }
}

/// Строка списка треков. Высота фиксированная (56), чтобы работала виртуализация списка.
pub fn track_row(ui: &mut Ui, theme: &Theme, index: usize, track: &Track, is_current: bool, is_playing: bool, time: f64) -> Response {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
    // Подсветку под курсором рисует список (`glide_end`: она скользит между строками); здесь только сжатие при нажатии.
    if !ui.is_rect_visible(rect) {
        return resp; // не в кадре: ничего не считаем и не рисуем
    }
    let id = ui.scope_id().with(("track_row", &track.id));
    let rect = squish(ui, id, &resp, rect, 0.015, 0.07);
    let painter = ui.painter();

    if is_current {
        painter.rect_filled(rect, 18.0, with_alpha(p.primary, if p.dark { 34 } else { 28 }));
    }

    let cover = Rect::from_min_size(pos2(rect.left() + 10.0, rect.center().y - 20.0), Vec2::splat(40.0));
    cover_tile(ui, theme, cover, &track.id, 12.0);
    let painter = ui.painter();
    if is_current {
        painter.rect_filled(cover, 12.0, egui::Color32::from_black_alpha(110));
        icons::paint_equalizer(painter, cover.center(), 18.0, egui::Color32::WHITE, time, is_playing);
    }

    let text_left = cover.right() + 14.0;
    let dur_x = rect.right() - 16.0;
    let clip = Rect::from_min_max(pos2(text_left, rect.top()), pos2(dur_x - 56.0, rect.bottom()));
    let clipped = painter.with_clip_rect(clip);
    let title_color = if is_current { p.primary } else { p.on_surface };
    clipped.text(pos2(text_left, rect.center().y - 9.0), Align2::LEFT_CENTER, &track.title, FontId::proportional(15.0), title_color);
    clipped.text(pos2(text_left, rect.center().y + 10.0), Align2::LEFT_CENTER, artists_text(track), FontId::proportional(12.5), p.on_surface_variant);
    painter.text(pos2(dur_x, rect.center().y), Align2::RIGHT_CENTER, fmt_time(track.duration_secs as u64), FontId::proportional(13.0), p.on_surface_variant);

    let _ = index;
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Маленький заголовок группы (например «Сервисы»).
pub fn section_label(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(egui::RichText::new(text).size(12.5).color(theme.palette.on_surface_variant));
    });
}

#[cfg(test)]
mod tests {
    use super::spring_highlight;

    /// После первого перелёта пружина пересекает цель ещё несколько раз, а не садится сразу.
    #[test]
    fn highlight_spring_rings_more_than_once() {
        assert!(spring_highlight(0.0).abs() < 1e-3);
        let peak = spring_highlight(0.2282);
        assert!((peak - 1.059).abs() < 0.01, "первый пик уехал: {peak}");

        let mut prev = 0.0_f32;
        let mut prev_d = 0.0_f32;
        let mut crosses = 0;
        let mut extrema = 0;
        for i in 1..1200 {
            let y = spring_highlight(i as f32 * 0.001);
            if (prev - 1.0) * (y - 1.0) < 0.0 {
                crosses += 1;
            }
            let d = y - prev;
            if i > 2 && prev_d * d < 0.0 && (prev - 1.0).abs() > 0.008 {
                extrema += 1;
            }
            assert!((y - prev).abs() < 0.05, "скачок на шаге {i}: {prev} -> {y}");
            prev_d = d;
            prev = y;
        }
        assert!(crosses >= 4, "мало пересечений цели: {crosses}");
        assert!(extrema >= 4, "мало колебаний: {extrema}");
        assert!((spring_highlight(1.2) - 1.0).abs() < 0.01);
    }
}

//! Скользящая подсветка пункта под курсором (`glide_*`) и «продавливание» при нажатии (`squish`).

use crate::theme::Theme;
use crate::ui::anim::{PRESS, cubic_bezier, spring_step};
use egui::{Id, Pos2, Rect, Response, Shape, Ui, Vec2, layers::ShapeIdx, pos2, vec2};

/// Плавное «сжатие» при нажатии (в демо `scale(.88, .94)` у кнопки): прямоугольник уменьшается
/// вокруг центра на `sx`/`sy` (доли) с перелётом при нажатии и отпускании.
pub(in crate::ui) fn squish(ui: &Ui, id: Id, resp: &Response, rect: Rect, sx: f32, sy: f32) -> Rect {
    let t = ui.ctx().animate_bool_with_time(id.with("press"), resp.is_pointer_button_down_on(), 0.18);
    let k = cubic_bezier(PRESS, t);
    Rect::from_center_size(rect.center(), vec2(rect.width() * (1.0 - sx * k), rect.height() * (1.0 - sy * k)))
}

/// Состояние скользящей подсветки (хранится в памяти egui).
/// Положение и скорость разделены для двух углов прямоугольника — так подсветка
/// корректно анимируется и при изменении размеров целевой строки.
#[derive(Clone, Copy)]
struct Glide {
    key: u64,
    target: Rect,
    min: Pos2,
    max: Pos2,
    velocity_min: Vec2,
    velocity_max: Vec2,
    alpha: f32,
    alive: bool,
}

impl Default for Glide {
    fn default() -> Self {
        Self {
            key: 0,
            target: Rect::ZERO,
            min: pos2(0.0, 0.0),
            max: pos2(0.0, 0.0),
            velocity_min: Vec2::ZERO,
            velocity_max: Vec2::ZERO,
            alpha: 0.0,
            alive: false,
        }
    }
}

/// Резервирует слой, на котором после отрисовки строк будет нарисована подсветка.
pub(crate) fn glide_begin(ui: &Ui) -> ShapeIdx {
    ui.painter().add(Shape::Noop)
}

/// Новая цель подсветки: после полного исчезновения она появляется сразу под новым пунктом,
/// иначе меняется только цель (положение и импульс сохраняются).
fn glide_retarget(g: &mut Glide, key: u64, rect: Rect) {
    if !g.alive || g.alpha < 0.02 {
        // После полного исчезновения подсветка появляется сразу под новым пунктом.
        g.key = key;
        g.target = rect;
        g.min = rect.min;
        g.max = rect.max;
        g.velocity_min = Vec2::ZERO;
        g.velocity_max = Vec2::ZERO;
        g.alive = true;
    } else {
        // Меняем только цель. Текущее положение и импульс сохраняются.
        if g.key != key {
            g.key = key;
        }
        g.target = rect;
    }
}

/// Шаг пружин границ и прозрачности подсветки; `has_target`: курсор над пунктом (подсветка проявляется, иначе гаснет).
/// Возвращает `true`, пока что-то ещё движется и нужен следующий кадр.
fn glide_step(g: &mut Glide, has_target: bool, dt: f32) -> bool {
    let mut moving = false;
    if g.alive {
        moving |= spring_step(&mut g.min.x, &mut g.velocity_min.x, g.target.min.x, dt);
        moving |= spring_step(&mut g.min.y, &mut g.velocity_min.y, g.target.min.y, dt);
        moving |= spring_step(&mut g.max.x, &mut g.velocity_max.x, g.target.max.x, dt);
        moving |= spring_step(&mut g.max.y, &mut g.velocity_max.y, g.target.max.y, dt);
    }

    if has_target {
        g.alpha += (1.0 - g.alpha) * (1.0 - (-dt * 20.0).exp());
        if g.alpha > 0.99 {
            g.alpha = 1.0;
        } else {
            moving = true;
        }
    } else {
        g.alpha -= g.alpha * (1.0 - (-dt * 14.0).exp());
        if g.alpha < 0.01 {
            g.alpha = 0.0;
            g.alive = false;
        } else {
            moving = true;
        }
    }
    moving
}

/// Скользящая подсветка под курсором: одна на весь список. При переходе на другую
/// строку пружина получает новую цель, сохраняя текущую скорость — даже при быстрых
/// перемещениях курсора она не телепортируется и не начинает движение с нуля.
///
/// Только Liquid Glass: стеклянная подложка. В Material 3 скользящей подсветки нет (нажатие рисует `ripple.rs`).
pub(crate) fn glide_end(ui: &Ui, theme: &Theme, slot: ShapeIdx, salt: Id, target: Option<(u64, Rect)>, radius: f32) {
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.064);
    let mut g: Glide = ui.ctx().data_mut(|d| d.get_temp(salt)).unwrap_or_default();

    if let Some((key, rect)) = target {
        glide_retarget(&mut g, key, rect);
    }
    let moving = glide_step(&mut g, target.is_some(), dt);

    if g.alive && g.alpha > 0.0 {
        let shown = Rect::from_min_max(g.min, g.max);
        // В Liquid Glass анимируется не цветная подсветка, а сама стеклянная
        // поверхность выбранной строки: материал остаётся настоящим GPU glass.
        // Выделение = только эффекты стекла (как у мини-плеера), без заливки. Кладём в слот,
        // который резервирует glide_begin: фигура оказывается ПОД текстом и обложками.
        let mut style = crate::theme::glass::selection_style(theme, radius);
        style.border *= g.alpha;
        style.shadow *= g.alpha;
        ui.painter().set(slot, egui_glass::glass_shape(ui, shown, &style));
    }
    if moving {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(salt, g));
}

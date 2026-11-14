//! Анимация переключения экранов.
//!
//! Основа: вкладки Files/Bells в проекте sheduleappreload (ui/AppScaffold.kt, режим PARALLAX
//! плюс длительность `DEFAULT_NAV_DURATION_MS = 340` и кривая `FastOutSlowInEasing`).
//! Два экрана рисуются одновременно: «нижний» (с меньшим `View::order()`) уезжает влево
//! и тускнеет, «верхний» выезжает справа с параллаксом (степень `1.6`). При возврате
//! всё идёт в обратную сторону, поэтому направление зависит от порядка экранов, а не от клика.
//!
//! Что НЕ перенесено из Kotlin-версии: масштаб слоёв (`scaleX/scaleY` 0.94/0.96). egui не умеет
//! масштабировать обычный `Ui` без отдельных слоёв (Area), а это усложнит стекло. Если понадобится,
//! делаем отдельной задачей.

use egui::{Rect, Ui, UiBuilder, vec2};

use crate::app::{McrateApp, View};
use crate::theme::Theme;

/// Длительность перехода, секунд (в Kotlin: 340 мс).
const DURATION: f64 = 0.34;
/// Степень параллакса для верхнего слоя (в Kotlin: `parallaxPower = 1.6`).
const PARALLAX_POWER: f32 = 1.6;
/// Прозрачность нижнего слоя в конце перехода (в Kotlin: 0.55).
const BACK_ALPHA_END: f32 = 0.55;
/// Прозрачность верхнего слоя в начале перехода (в Kotlin: 0.6).
const FRONT_ALPHA_START: f32 = 0.6;

/// Идущий сейчас переход.
#[derive(Clone, Copy)]
pub struct ViewTransition {
    pub from: View,
    pub to: View,
    /// Время старта по часам egui (`InputState::time`), секунды.
    pub started: f64,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Кривая Material «FastOutSlowIn» = cubic-bezier(0.4, 0.0, 0.2, 1.0).
/// По времени `x` ищем параметр кривой бисекцией (деление отрезка пополам), возвращаем `y`.
fn fast_out_slow_in(x: f32) -> f32 {
    const P1X: f32 = 0.4;
    const P1Y: f32 = 0.0;
    const P2X: f32 = 0.2;
    const P2Y: f32 = 1.0;
    // Кубическая кривая Безье с концами (0,0) и (1,1).
    fn bez(t: f32, a: f32, b: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    }
    let x = x.clamp(0.0, 1.0);
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(mid, P1X, P2X) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez((lo + hi) / 2.0, P1Y, P2Y)
}

impl McrateApp {
    /// Вызывать раз в кадр до отрисовки: замечает смену `self.view` (откуда бы её ни сделали:
    /// клик в меню, drag&drop, кнопка внутри экрана) и запускает переход.
    pub fn update_transition(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if self.view != self.shown_view {
            self.transition = Some(ViewTransition { from: self.shown_view, to: self.view, started: now });
            self.shown_view = self.view;
        }
        if let Some(t) = self.transition {
            if now - t.started >= DURATION {
                self.transition = None;
            }
        }
    }

    /// Рисует текущий экран или два экрана во время перехода. Заменяет прямой `match self.view`.
    pub fn view_host(&mut self, ui: &mut Ui, theme: &Theme) {
        let rect = ui.available_rect_before_wrap();
        let now = ui.input(|i| i.time);

        let Some(tr) = self.transition else {
            self.view_layer(ui, theme, self.view, rect, 0.0, 1.0, true);
            return;
        };

        // Пока идёт переход, просим перерисовывать каждый кадр.
        ui.ctx().request_repaint();

        let raw = ((now - tr.started) / DURATION).clamp(0.0, 1.0) as f32;
        let t = fast_out_slow_in(raw);

        // `p`: 0 = виден «нижний» экран (меньший order), 1 = виден «верхний».
        let forward = tr.to.order() >= tr.from.order();
        let p = if forward { t } else { 1.0 - t };
        let (low, high) = if forward { (tr.from, tr.to) } else { (tr.to, tr.from) };

        let w = rect.width();
        // Верхний слой едет быстрее в начале (параллакс), как `bellsProgress` в Kotlin.
        let high_p = 1.0 - (1.0 - p).powf(PARALLAX_POWER);

        let low_dx = -w * p;
        let low_alpha = lerp(1.0, BACK_ALPHA_END, p);
        let high_dx = w * (1.0 - high_p);
        let high_alpha = lerp(FRONT_ALPHA_START, 1.0, high_p);

        // Верхний рисуется последним (поверх). Кликать можно только по целевому экрану.
        self.view_layer(ui, theme, low, rect, low_dx, low_alpha, low == tr.to);
        self.view_layer(ui, theme, high, rect, high_dx, high_alpha, high == tr.to);
    }

    /// Один слой: экран, сдвинутый по X и с заданной прозрачностью, обрезанный по `rect`.
    fn view_layer(&mut self, ui: &mut Ui, theme: &Theme, view: View, rect: Rect, dx: f32, alpha: f32, interactive: bool) {
        if alpha <= 0.0 {
            return;
        }
        // `id_salt` у каждого экрана свой и не зависит от того, идёт ли переход: иначе после
        // перехода у экрана менялись бы id виджетов и позиция прокрутки сбрасывалась бы.
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(("view_layer", view as u8))
                .max_rect(rect.translate(vec2(dx, 0.0)))
                .layout(*ui.layout()),
        );
        // Не даём содержимому вылезать за пределы области (при сдвиге оно уезжает под край).
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        child.set_opacity(alpha);
        if !interactive {
            child.disable(); // уходящий экран не реагирует на мышь
        }
        // Сам `match` по экранам лежит в app.rs (`draw_view`): так новые экраны добавляются в одном месте.
        self.draw_view(&mut child, theme, view);
    }
}

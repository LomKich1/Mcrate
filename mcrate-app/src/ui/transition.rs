//! Анимация переключения экранов.
//!
//! Идея (в духе Windows 11 и Material 3 «shared axis»): уходящий экран быстро растворяется
//! и чуть сдвигается назад, а новый выезжает с небольшим сдвигом и плавно проявляется. Сдвиг
//! маленький (несколько десятков пикселей), поэтому на большом окне экран не «пролетает» всю панель.
//!
//! Ось зависит от экранов:
//! - переходы в «Сейчас играет» и обратно идут по вертикали: открытие поднимает экран снизу,
//!   закрытие опускает вниз (в паре со стрелкой в мини-плеере);
//! - остальные идут по горизонтали, направление задаёт порядок экранов (`View::order()`), а не то,
//!   куда кликнули: вперёд экран приходит справа, назад слева.
//!
//! Что важно не сломать:
//! 1. Смена экрана замечается прямо в `view_host`, а не в начале кадра. Клик по меню происходит
//!    позже, при отрисовке рейки, и иначе в кадре клика мелькнул бы новый экран, а потом старый.
//! 2. `Ui::disable()` умножает прозрачность на 0.5, поэтому уходящий слой не отключают, а
//!    перекрывают невидимым «щитом», который гасит клики (иначе экран тускнеет скачком).
//! 3. Время идёт не по часам стены: шаг за кадр ограничен (`MAX_DT`), иначе один долгий кадр
//!    (первая отрисовка экрана) съест кусок анимации.
//! 4. Слоёв всегда два слота (см. `host`): от этого зависят автоматические id виджетов.
//!
//! Масштаб слоёв не делаем: egui не умеет масштабировать обычный `Ui` без отдельных слоёв (`Area`),
//! а это усложнило бы стекло.

use egui::{Rect, Sense, Ui, UiBuilder, vec2};

use super::anim::{EMPHASIZED_DECELERATE, STANDARD, STANDARD_ACCELERATE, cubic_bezier};
use super::layout::Bleed;
use crate::app::{McrateApp, View};
use crate::theme::Theme;

/// Длительность перехода, секунд.
const DURATION: f64 = 0.40;
/// Максимальный шаг времени за один кадр. Если кадр длился дольше (подвисание), анимация
/// всё равно сдвинется только на столько: иначе она «перескочит» и покажется рваной.
const MAX_DT: f64 = 0.04;

/// Уходящий экран растворяется за первую половину перехода.
const OUT_END: f32 = 0.50;
/// Новый экран начинает проявляться почти сразу: так экраны перекрываются (кроссфейд) и
/// посередине перехода панель не выглядит пустой. Тест `motion_is_smooth_and_never_blank`
/// следит, чтобы суммарная непрозрачность не проваливалась.
const IN_START: f32 = 0.05;
/// Какая часть оставшегося времени уходит на проявление нового экрана (остальное он только доезжает).
const IN_FADE_SPAN: f32 = 0.50;
/// Сдвиг в долях размера панели вдоль оси перехода (ширина или высота) и его границы в пикселях.
const SLIDE_FRACTION: f32 = 0.06;
const SLIDE_MIN: f32 = 28.0;
const SLIDE_MAX: f32 = 72.0;
/// Уходящий экран сдвигается на эту долю от сдвига нового.
const OUT_SLIDE_SHARE: f32 = 0.6;

/// Идущий сейчас переход.
#[derive(Clone, Copy)]
pub(crate) struct ViewTransition {
    pub from: View,
    pub to: View,
    /// Сколько секунд анимации уже прошло (с ограничением шага, см. `MAX_DT`).
    elapsed: f64,
    /// Время последнего обновления по часам egui. Нужно, чтобы повторный вызов `sync`
    /// в том же кадре не продвигал анимацию второй раз.
    last: f64,
}

impl ViewTransition {
    fn progress(&self) -> f32 {
        (self.elapsed / DURATION).clamp(0.0, 1.0) as f32
    }
}

/// Положение одного слоя: сдвиг вдоль оси перехода (пиксели; `dx` и для вертикальных переходов) и прозрачность.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LayerPose {
    dx: f32,
    alpha: f32,
}

/// Где и в каком направлении рисуется слой перехода (одинаково для обоих слоёв).
#[derive(Clone, Copy)]
struct LayerFrame {
    /// Область, которую занимает экран.
    rect: Rect,
    bleed: Bleed,
    /// Переход идёт по вертикали (открытие и закрытие «Сейчас играет»).
    vertical: bool,
}

/// Позы уходящего и приходящего экранов. `raw` = доля времени 0..1, `dir` = +1, если новый
/// экран приходит справа (вперёд по меню), -1, если слева, `width` = ширина области.
fn poses(raw: f32, dir: f32, width: f32) -> (LayerPose, LayerPose) {
    let slide = (width * SLIDE_FRACTION).clamp(SLIDE_MIN, SLIDE_MAX);

    // Уходящий: растворяется и немного уезжает в сторону, откуда «пришёл бы» предыдущий.
    let t_out = (raw / OUT_END).clamp(0.0, 1.0);
    let out = LayerPose {
        dx: -dir * slide * OUT_SLIDE_SHARE * cubic_bezier(STANDARD_ACCELERATE, t_out),
        alpha: 1.0 - cubic_bezier(STANDARD, t_out),
    };

    // Приходящий: быстро выезжает и долго «садится» на место, проявляется мягче, чем едет.
    let t_pos = ((raw - IN_START) / (1.0 - IN_START)).clamp(0.0, 1.0);
    let t_fade = ((raw - IN_START) / IN_FADE_SPAN).clamp(0.0, 1.0);
    let inc = LayerPose { dx: dir * slide * (1.0 - cubic_bezier(EMPHASIZED_DECELERATE, t_pos)), alpha: cubic_bezier(STANDARD, t_fade) };
    (out, inc)
}

/// Замечает смену экрана и двигает время перехода. Безопасно вызывать несколько раз за кадр.
/// Возвращает `true`, если в этом кадре нужно рисовать анимацию (переход идёт).
fn sync(transition: &mut Option<ViewTransition>, shown: &mut View, view: View, now: f64) -> bool {
    if view != *shown {
        *transition = Some(ViewTransition { from: *shown, to: view, elapsed: 0.0, last: now });
        *shown = view;
    } else if let Some(t) = transition {
        t.elapsed += (now - t.last).clamp(0.0, MAX_DT);
        t.last = now;
        if t.elapsed >= DURATION {
            *transition = None;
        }
    }
    transition.is_some()
}

/// Рисует экран или два экрана (во время перехода). Не зависит от `McrateApp`, чтобы это
/// можно было проверить тестами без видеокарты.
fn host(ui: &mut Ui, transition: Option<ViewTransition>, view: View, bleed: Bleed, draw: &mut dyn FnMut(&mut Ui, View)) {
    let rect = ui.available_rect_before_wrap();

    // ВАЖНО: `new_child` строит id слоя из счётчика родителя, а счётчик растёт с каждым созданным
    // слоем. Если слоёв то один, то два (или нижний слой исчез), у виджетов нового экрана меняются
    // автоматические id: в этот кадр теряются наведение и клик, сбрасывается перетаскивание.
    // Поэтому слоёв ВСЕГДА два слота: нижний (уходящий экран или пустой) и верхний (текущий или
    // приходящий экран). Пустой слот просто пропускает номер (`skip_ahead_auto_ids`).
    let Some(tr) = transition else {
        ui.skip_ahead_auto_ids(1); // пустой нижний слот
        layer(ui, view, LayerFrame { rect, bleed, vertical: false }, LayerPose { dx: 0.0, alpha: 1.0 }, draw);
        return;
    };

    // Пока идёт переход, просим перерисовывать каждый кадр.
    ui.ctx().request_repaint();

    // Переходы в «Сейчас играет» и обратно идут по вертикали: открытие поднимает экран снизу,
    // закрытие опускает его вниз (в паре со стрелкой в мини-плеере). Остальные идут по горизонтали.
    let vertical = tr.from == View::NowPlaying || tr.to == View::NowPlaying;
    let dir = if vertical {
        if tr.to == View::NowPlaying { 1.0 } else { -1.0 }
    } else if tr.to.order() >= tr.from.order() {
        1.0
    } else {
        -1.0
    };
    let extent = if vertical { rect.height() } else { rect.width() };
    let (out_pose, in_pose) = poses(tr.progress(), dir, extent);

    // Уходящий рисуется первым (под низом), новый поверх.
    layer(ui, tr.from, LayerFrame { rect, bleed, vertical }, out_pose, draw);
    // «Щит»: невидимая область поверх уходящего экрана. Клики и наведение достаются ему,
    // а не кнопкам под ним. Виджеты нового экрана добавляются позже, поэтому они выше щита.
    ui.interact(rect, ui.scope_id().with("view_transition_shield"), Sense::click_and_drag());
    layer(ui, tr.to, LayerFrame { rect, bleed, vertical }, in_pose, draw);
}

/// Один слой: экран, сдвинутый вдоль оси перехода и с заданной прозрачностью, обрезанный по `rect`.
fn layer(ui: &mut Ui, view: View, frame: LayerFrame, pose: LayerPose, draw: &mut dyn FnMut(&mut Ui, View)) {
    let LayerFrame { rect, bleed, vertical } = frame;
    // Слой прозрачнее одного шага 8-битного канала не виден: не тратим на него кадр.
    // Но номер в нумерации id занимаем (см. комментарий в `host`).
    if pose.alpha < 1.0 / 255.0 {
        ui.skip_ahead_auto_ids(1);
        return;
    }
    // `id_salt` у каждого экрана свой и не зависит от того, идёт ли переход: иначе после
    // перехода у экрана менялись бы id виджетов и позиция прокрутки сбрасывалась бы.
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt(("view_layer", view as u8))
            .max_rect(rect.translate(if vertical { vec2(0.0, pose.dx) } else { vec2(pose.dx, 0.0) }))
            .layout(*ui.layout()),
    );
    // Не даём содержимому вылезать за пределы области (при сдвиге оно чуть уезжает под край).
    // Область отсечения шире рамки страницы на вылет: так прокрутка может дойти до краёв панели.
    child.set_clip_rect(bleed.expand(rect).intersect(ui.clip_rect()));
    child.set_opacity(pose.alpha);
    // Сам `match` по экранам лежит в app.rs (`draw_view`): так новые экраны добавляются в одном месте.
    draw(&mut child, view);
}

impl McrateApp {
    /// Вызывать раз в кадр до отрисовки: запоминает время кадра основного контекста и двигает
    /// анимацию. Основная работа по замечанию смены экрана всё равно делается в `view_host`
    /// (в раскладке с боковой панелью клик по меню случается уже после этого вызова).
    pub(crate) fn update_transition(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.frame_time = now; // view_host берёт время отсюда (см. поле `frame_time`)
        sync(&mut self.transition, &mut self.shown_view, self.view, now);
    }

    /// Рисует текущий экран или два экрана во время перехода. Заменяет прямой `match self.view`.
    ///
    /// Живое стекло вызывает это замыкание ДВАЖДЫ за кадр (на экран и в закадровый контекст со своими
    /// часами), поэтому здесь нельзя полагаться на `ui.input(|i| i.time)`: время берётся из
    /// `frame_time` основного контекста. Повторный вызов в том же кадре безопасен: `sync` с тем же
    /// временем ничего не двигает.
    pub(crate) fn view_host(&mut self, ui: &mut Ui, theme: &Theme) {
        // Смену экрана замечаем ЗДЕСЬ: клик по меню мог произойти только что, в этом же кадре.
        sync(&mut self.transition, &mut self.shown_view, self.view, self.frame_time);

        let (transition, view) = (self.transition, self.view);
        let bleed = self.bleed;
        host(ui, transition, view, bleed, &mut |ui, v| self.draw_view(ui, theme, v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Button, Context, Event, Modifiers, PointerButton, RawInput, pos2};
    use std::cell::RefCell;

    /// Один кадр egui без окна и видеокарты.
    fn frame(ctx: &Context, time: f64, events: Vec<Event>, body: impl FnMut(&mut Ui)) {
        let input = RawInput {
            time: Some(time),
            events,
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, body);
        out.textures_delta.clear(); // иначе egui в режиме отладки паникует из-за неприменённых текстур
    }

    /// `View` не реализует `Debug`, поэтому в сообщениях тестов сравниваем номера экранов.
    fn orders(drawn: &RefCell<Vec<View>>) -> Vec<u8> {
        drawn.borrow().iter().map(|v| v.order()).collect()
    }

    #[test]
    fn poses_begin_at_rest_and_end_at_rest() {
        for dir in [1.0_f32, -1.0] {
            let (out, inc) = poses(0.0, dir, 900.0);
            assert!(out.dx.abs() < 1e-3 && (out.alpha - 1.0).abs() < 1e-3);
            assert_eq!(inc.alpha, 0.0);
            assert!(inc.dx * dir > 0.0, "новый экран приходит со стороны dir");

            let (out, inc) = poses(1.0, dir, 900.0);
            assert_eq!(out.alpha, 0.0);
            assert!(inc.dx.abs() < 1e-3 && (inc.alpha - 1.0).abs() < 1e-3);
        }
    }

    #[test]
    fn motion_is_smooth_and_never_blank() {
        let mut prev_out = poses(0.0, 1.0, 900.0).0.alpha;
        let mut prev_in = 0.0_f32;
        let mut prev_dx = f32::MAX;
        let mut min_sum = f32::MAX;
        for i in 0..=400 {
            let raw = i as f32 / 400.0;
            let (out, inc) = poses(raw, 1.0, 900.0);
            assert!(out.alpha <= prev_out + 1e-4 && inc.alpha + 1e-4 >= prev_in, "прозрачность монотонна");
            assert!(inc.dx <= prev_dx + 1e-4, "новый экран только приближается");
            // Шаг между кадрами не должен быть скачком (при 400 кадрах на переход).
            assert!((out.alpha - prev_out).abs() < 0.05 && (inc.alpha - prev_in).abs() < 0.05);
            (prev_out, prev_in, prev_dx) = (out.alpha, inc.alpha, inc.dx);
            min_sum = min_sum.min(out.alpha + inc.alpha);
        }
        // Нет момента, когда оба экрана почти прозрачны и панель выглядит пустой.
        assert!(min_sum > 0.7, "провал яркости: {min_sum}");
    }

    #[test]
    fn slide_is_small_even_in_a_wide_window() {
        let (_, inc) = poses(0.0, 1.0, 3000.0);
        assert!(inc.dx <= SLIDE_MAX + 1e-3);
        let (_, inc) = poses(0.0, 1.0, 300.0);
        assert!(inc.dx >= SLIDE_MIN - 1e-3);
    }

    #[test]
    fn long_frame_does_not_skip_the_animation() {
        let (mut tr, mut shown) = (None, View::Library);
        sync(&mut tr, &mut shown, View::Settings, 10.0);
        // Подвисание на полсекунды: анимация сдвигается не больше, чем на MAX_DT.
        sync(&mut tr, &mut shown, View::Settings, 10.5);
        let t = tr.expect("переход ещё идёт");
        assert!(t.elapsed <= MAX_DT + 1e-9, "elapsed = {}", t.elapsed);
    }

    #[test]
    fn second_sync_in_the_same_frame_does_not_advance_time() {
        let (mut tr, mut shown) = (None, View::Library);
        sync(&mut tr, &mut shown, View::Settings, 1.0);
        sync(&mut tr, &mut shown, View::Settings, 1.016);
        let once = tr.unwrap().elapsed;
        sync(&mut tr, &mut shown, View::Settings, 1.016);
        assert_eq!(tr.unwrap().elapsed, once);
    }

    #[test]
    fn transition_finishes_in_about_duration_at_60fps() {
        let (mut tr, mut shown) = (None, View::Library);
        sync(&mut tr, &mut shown, View::NowPlaying, 0.0);
        let mut frames = 0;
        while tr.is_some() {
            frames += 1;
            sync(&mut tr, &mut shown, View::NowPlaying, frames as f64 / 60.0);
            assert!(frames < 100);
        }
        assert!((23..=27).contains(&frames), "кадров: {frames}");
    }

    /// Главная ошибка прошлой версии: клик по меню происходит ПОСЛЕ вызова в начале кадра.
    /// В кадре клика не должен мелькать новый экран.
    #[test]
    fn click_frame_still_shows_the_old_screen() {
        let ctx = Context::default();
        let (mut tr, mut shown, mut view) = (None, View::Library, View::Library);
        let drawn: RefCell<Vec<View>> = RefCell::new(Vec::new());

        frame(&ctx, 0.0, vec![], |ui| {
            sync(&mut tr, &mut shown, view, 0.0);
            host(ui, tr, view, Bleed::default(), &mut |_, v| drawn.borrow_mut().push(v));
        });
        drawn.borrow_mut().clear();

        frame(&ctx, 0.1, vec![], |ui| {
            sync(&mut tr, &mut shown, view, 0.1); // update_transition в начале кадра
            view = View::Settings; // клик по рейке
            sync(&mut tr, &mut shown, view, 0.1); // начало view_host
            host(ui, tr, view, Bleed::default(), &mut |_, v| drawn.borrow_mut().push(v));
        });
        assert_eq!(orders(&drawn), vec![View::Library.order()], "в кадре клика виден только старый экран");

        // Дальше оба экрана рисуются, а в конце остаётся один новый.
        let mut both = false;
        for i in 1..=40 {
            drawn.borrow_mut().clear();
            let now = 0.1 + i as f64 / 60.0;
            frame(&ctx, now, vec![], |ui| {
                sync(&mut tr, &mut shown, view, now);
                host(ui, tr, view, Bleed::default(), &mut |_, v| drawn.borrow_mut().push(v));
            });
            both |= drawn.borrow().len() == 2;
        }
        assert!(both, "во время перехода рисуются два экрана");
        assert!(tr.is_none());
        assert_eq!(orders(&drawn), vec![View::Settings.order()]);
    }

    /// Кнопки уходящего экрана не должны нажиматься, кнопки нового работают.
    /// Проигрываем весь переход и в КАЖДЫЙ его момент пробуем кликнуть по кнопкам обоих экранов.
    /// Кнопка нового экрана должна нажиматься всегда (в том числе в кадры, когда слой уходящего
    /// экрана исчезает), а кнопка уходящего никогда.
    #[test]
    fn clicks_work_at_every_moment_of_the_transition() {
        let press = |p: egui::Pos2, down: bool| Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed: down,
            modifiers: Modifiers::NONE,
        };

        for (name, target) in [("новый", true), ("уходящий", false)] {
            for warm in 1..=30_usize {
                let ctx = Context::default();
                let (mut tr, mut shown) = (None, View::Library);
                let view = View::Settings;
                let clicked: RefCell<Vec<&'static str>> = RefCell::new(Vec::new());
                let centers: RefCell<[egui::Pos2; 2]> = RefCell::new([pos2(0.0, 0.0); 2]);
                let mut t = 0.0;

                let mut run = |events: Vec<Event>, t: f64, tr: &mut Option<ViewTransition>| {
                    frame(&ctx, t, events, |ui| {
                        sync(tr, &mut shown, view, t);
                        host(ui, *tr, view, Bleed::default(), &mut |ui, v| {
                            let (label, idx, x) = match v {
                                View::Library => ("old", 0, 10.0),
                                _ => ("new", 1, 400.0),
                            };
                            let r = Rect::from_min_size(ui.max_rect().min + vec2(x, 10.0), vec2(100.0, 30.0));
                            let resp = ui.put(r, Button::new(label));
                            centers.borrow_mut()[idx] = resp.rect.center();
                            if resp.clicked() {
                                clicked.borrow_mut().push(label);
                            }
                        });
                    });
                };

                for _ in 0..warm {
                    run(vec![], t, &mut tr);
                    t += 1.0 / 60.0;
                }
                if tr.is_none() {
                    continue; // переход уже закончился: проверять нечего
                }
                // Целимся в кнопку, как её видит пользователь в этот момент.
                let c = centers.borrow()[if target { 1 } else { 0 }];
                if c == pos2(0.0, 0.0) {
                    continue; // слой ещё/уже не нарисован (полностью прозрачен): кликать не по чему
                }
                run(vec![Event::PointerMoved(c)], t, &mut tr);
                t += 1.0 / 60.0;
                run(vec![press(c, true)], t, &mut tr);
                t += 1.0 / 60.0;
                run(vec![press(c, false)], t, &mut tr);

                let got = clicked.borrow().clone();
                if target {
                    assert_eq!(got, vec!["new"], "клик по {name} экрану потерян (прогрев {warm} кадров)");
                } else {
                    assert!(got.is_empty(), "клик по {name} экрану прошёл (прогрев {warm} кадров): {got:?}");
                }
            }
        }
    }
}

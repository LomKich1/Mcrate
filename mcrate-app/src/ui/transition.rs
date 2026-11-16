//! Анимация переключения экранов.
//!
//! Идея (в духе Windows 11 и Material 3 «shared axis»): уходящий экран быстро растворяется
//! и чуть сдвигается назад, а новый выезжает с небольшим сдвигом по горизонтали и плавно
//! проявляется. Сдвиг маленький (несколько десятков пикселей), поэтому на широком окне
//! не «пролетает» через весь экран, как раньше. Направление зависит от порядка экранов
//! (`View::order()`), а не от того, куда кликнули: вперёд экран приходит справа, назад слева.
//!
//! Что было сломано в прошлой версии (портированной из Kotlin-проекта sheduleappreload):
//! 1. Переход замечался в начале кадра, а клик по меню происходит позже, при отрисовке рейки.
//!    В кадре клика `view_host` рисовал уже НОВЫЙ экран, а на следующем кадре откатывался
//!    к старому и только потом анимировал. Теперь смена экрана замечается прямо в `view_host`.
//! 2. `Ui::disable()` умножает прозрачность на 0.5, поэтому уходящий экран тускнел скачком.
//!    Теперь уходящий слой перекрывается невидимым «щитом», который гасит клики.
//! 3. Время шло по часам стены: один долгий кадр (первая отрисовка экрана) съедал кусок
//!    анимации. Теперь шаг времени за кадр ограничен (`MAX_DT`).
//! 4. Резкая кривая и сдвиг на всю ширину панели при длительности 340 мс.
//!
//! Что по-прежнему НЕ перенесено из Kotlin-версии: масштаб слоёв. egui не умеет масштабировать
//! обычный `Ui` без отдельных слоёв (Area), а это усложнит стекло.

use egui::{Rect, Sense, Ui, UiBuilder, vec2};

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
/// Сдвиг в долях ширины панели и его границы в пикселях.
const SLIDE_FRACTION: f32 = 0.06;
const SLIDE_MIN: f32 = 28.0;
const SLIDE_MAX: f32 = 72.0;
/// Уходящий экран сдвигается на эту долю от сдвига нового.
const OUT_SLIDE_SHARE: f32 = 0.6;

/// Кривые Material (cubic-bezier x1, y1, x2, y2).
const STANDARD: [f32; 4] = [0.4, 0.0, 0.2, 1.0];
const STANDARD_ACCELERATE: [f32; 4] = [0.3, 0.0, 1.0, 1.0];
const EMPHASIZED_DECELERATE: [f32; 4] = [0.05, 0.7, 0.1, 1.0];

/// Идущий сейчас переход.
#[derive(Clone, Copy)]
pub struct ViewTransition {
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

/// Кубическая кривая Безье с концами (0,0) и (1,1): по времени `x` ищем параметр бисекцией,
/// возвращаем `y`.
fn cubic_bezier([x1, y1, x2, y2]: [f32; 4], x: f32) -> f32 {
    fn bez(t: f32, a: f32, b: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    }
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez((lo + hi) / 2.0, y1, y2)
}

/// Положение одного слоя: сдвиг по X (пиксели) и прозрачность.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LayerPose {
    dx: f32,
    alpha: f32,
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
    let inc = LayerPose {
        dx: dir * slide * (1.0 - cubic_bezier(EMPHASIZED_DECELERATE, t_pos)),
        alpha: cubic_bezier(STANDARD, t_fade),
    };
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
        layer(ui, view, rect, bleed, LayerPose { dx: 0.0, alpha: 1.0 }, draw);
        return;
    };

    // Пока идёт переход, просим перерисовывать каждый кадр.
    ui.ctx().request_repaint();

    let forward = tr.to.order() >= tr.from.order();
    let dir = if forward { 1.0 } else { -1.0 };
    let (out_pose, in_pose) = poses(tr.progress(), dir, rect.width());

    // Уходящий рисуется первым (под низом), новый поверх.
    layer(ui, tr.from, rect, bleed, out_pose, draw);
    // «Щит»: невидимая область поверх уходящего экрана. Клики и наведение достаются ему,
    // а не кнопкам под ним. Виджеты нового экрана добавляются позже, поэтому они выше щита.
    ui.interact(rect, ui.scope_id().with("view_transition_shield"), Sense::click_and_drag());
    layer(ui, tr.to, rect, bleed, in_pose, draw);
}

/// Один слой: экран, сдвинутый по X и с заданной прозрачностью, обрезанный по `rect`.
fn layer(ui: &mut Ui, view: View, rect: Rect, bleed: Bleed, pose: LayerPose, draw: &mut dyn FnMut(&mut Ui, View)) {
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
            .max_rect(rect.translate(vec2(pose.dx, 0.0)))
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
    pub fn update_transition(&mut self, ctx: &egui::Context) {
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
    pub fn view_host(&mut self, ui: &mut Ui, theme: &Theme) {
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
        let mut input = RawInput::default();
        input.time = Some(time);
        input.events = events;
        input.screen_rect = Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)));
        let mut out = ctx.run_ui(input, body);
        out.textures_delta.clear(); // иначе egui в режиме отладки паникует из-за неприменённых текстур
    }

    /// `View` не реализует `Debug`, поэтому в сообщениях тестов сравниваем номера экранов.
    fn orders(drawn: &RefCell<Vec<View>>) -> Vec<u8> {
        drawn.borrow().iter().map(|v| v.order()).collect()
    }

    #[test]
    fn curves_start_and_end_cleanly() {
        for curve in [STANDARD, STANDARD_ACCELERATE, EMPHASIZED_DECELERATE] {
            assert!(cubic_bezier(curve, 0.0).abs() < 1e-3);
            assert!((cubic_bezier(curve, 1.0) - 1.0).abs() < 1e-3);
            let mut prev = 0.0;
            for i in 0..=100 {
                let y = cubic_bezier(curve, i as f32 / 100.0);
                assert!(y + 1e-4 >= prev, "кривая не должна идти назад");
                prev = y;
            }
        }
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
        let press = |p: egui::Pos2, down: bool| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: down, modifiers: Modifiers::NONE };

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

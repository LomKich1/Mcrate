//! Общая математика анимаций: кубическая кривая Безье (CSS `cubic-bezier`) и физическая пружина.
//! Здесь только вычисления, без рисования; экраны и виджеты берут отсюда кривые и шаг пружины.

/// Кривые Material (cubic-bezier x1, y1, x2, y2).
pub(super) const STANDARD: [f32; 4] = [0.4, 0.0, 0.2, 1.0];
pub(super) const STANDARD_ACCELERATE: [f32; 4] = [0.3, 0.0, 1.0, 1.0];
pub(super) const EMPHASIZED_DECELERATE: [f32; 4] = [0.05, 0.7, 0.1, 1.0];
/// Кривая «нажатия» из демо плеера: `cubic-bezier(.3, 1.6, .5, 1)`, с небольшим перелётом за 1.
pub(super) const PRESS: [f32; 4] = [0.3, 1.6, 0.5, 1.0];

/// Кубическая кривая Безье с концами (0,0) и (1,1): по времени `x` ищем параметр бисекцией,
/// возвращаем `y` (он может выходить за 1: «пружина»). Вне 0..1 `x` обрезается до краёв.
pub(super) fn cubic_bezier([x1, y1, x2, y2]: [f32; 4], x: f32) -> f32 {
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

/// Параметры физической пружины, как в HTML-демо: F = -k * (x - target) - c * velocity.
const SPRING_STIFFNESS: f32 = 500.0;
const SPRING_DAMPING: f32 = 34.0;
/// Ближе этого к цели (и медленнее этого) пружина считается остановившейся.
const SPRING_EPSILON: f32 = 0.05;

/// Один шаг пружины; `true`, пока она ещё движется. Скорость намеренно не обнуляется при смене цели:
/// быстрые перемещения разворачивают текущий импульс, а не начинают анимацию заново.
pub(super) fn spring_step(value: &mut f32, velocity: &mut f32, target: f32, dt: f32) -> bool {
    if dt > 0.0 {
        let force = -SPRING_STIFFNESS * (*value - target) - SPRING_DAMPING * *velocity;
        *velocity += force * dt;
        *value += *velocity * dt;
    }

    if (*value - target).abs() < SPRING_EPSILON && (*velocity).abs() < SPRING_EPSILON {
        *value = target;
        *velocity = 0.0;
        false
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn press_curve_overshoots_and_lands_on_one() {
        let peak = (0..=100).map(|i| cubic_bezier(PRESS, i as f32 / 100.0)).fold(0.0_f32, f32::max);
        assert!(peak > 1.0, "кривая нажатия должна слегка перелетать за 1, пик {peak}");
        assert_eq!(cubic_bezier(PRESS, 1.0), 1.0);
    }

    #[test]
    fn spring_overshoots_and_settles() {
        let (mut value, mut velocity, target) = (0.0_f32, 0.0_f32, 100.0_f32);
        let mut overshot = false;
        let mut settled = false;

        for _ in 0..600 {
            let moving = spring_step(&mut value, &mut velocity, target, 1.0 / 60.0);
            assert!(value.is_finite() && velocity.is_finite());
            overshot |= value > target;
            if !moving {
                settled = true;
                break;
            }
        }

        assert!(overshot, "пружина должна слегка перелететь за цель");
        assert!(settled, "пружина должна остановиться у цели");
        assert_eq!(value, target);
        assert_eq!(velocity, 0.0);
    }

    /// Новая цель не сбрасывает скорость. Целимся ровно в текущее положение: если бы скорость
    /// обнулилась, сила была бы нулевой и после шага скорость осталась бы 0, а с сохранённой
    /// инерцией она положительна (затухание лишь частично гасит её).
    #[test]
    fn retargeting_keeps_existing_velocity() {
        let (mut value, mut velocity) = (0.0_f32, 0.0_f32);
        for _ in 0..5 {
            spring_step(&mut value, &mut velocity, 100.0, 1.0 / 60.0);
        }
        assert!(velocity > 0.0);

        let target = value;
        spring_step(&mut value, &mut velocity, target, 1.0 / 60.0);
        assert!(velocity > 0.0, "новая цель не должна сбрасывать инерцию");
    }
}

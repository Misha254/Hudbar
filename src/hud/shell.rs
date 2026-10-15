//! Общие куски цикла окон: шаг колеса и дожимание проявления.
//!
//! Вынесено, чтобы один и тот же баг не чинить в пяти бинарях: инверсия
//! колеса жила в трёх копиях, залипшее проявление — в пяти. Теперь обе
//! формулы живут здесь, а окна только зовут их.

use nix::poll::{PollFd, PollFlags, poll};
use smithay_client_toolkit::seat::pointer::AxisScroll;
use std::os::fd::BorrowedFd;
use wayland_client::EventQueue;

/// Дельта колеса в целых строках. Тачпад шлёт `value120` меньше 120, поэтому
/// дробный остаток копится в `acc` и отдаётся по целой строке: без накопления
/// одно деление давало бы ноль строк и скролл молчал бы.
///
/// Знак как у композитора: положительное — вниз. Инвертировать здесь нечего:
/// инверсия колеса чинилась удалением минуса сразу в трёх окнах.
///
/// Окна берут из модуля подмножествами (кому шаг, кому дожимание), поэтому
/// мёртвый код здесь не шумит: неиспользуемая соседом функция — норма.
#[allow(dead_code)]
pub fn axis_steps(acc: &mut f32, v: &AxisScroll) -> isize {
    let delta = if v.value120 != 0 {
        v.value120 as f32 / 120.0
    } else if v.discrete != 0 {
        v.discrete as f32
    } else if v.absolute != 0.0 {
        v.absolute.signum() as f32 * 3.0
    } else {
        0.0
    };
    *acc += delta;
    let steps = acc.trunc();
    *acc -= steps;
    steps as isize
}

/// Ожидание сокета с учётом автоповтора: удержанная стрелка будит цикл не
/// реже своего интервала, иначе повтор шёл бы рывками. Ждать дольше `idle`
/// нельзя: иначе проявление и подсветка шли бы ступенями.
///
/// Окна без повтора (бинды) зовут `poll_read` с готовым числом и сюда не
/// ходят — отсюда и разрешение на мёртвый код для их крейтов.
#[allow(dead_code)]
pub fn wait_ms(repeat_hint: Option<std::time::Duration>, idle: u16) -> u16 {
    repeat_hint
        .map(|left| left.as_millis().clamp(1, u16::MAX as u128) as u16)
        .unwrap_or(idle)
        .min(idle)
}

/// Один сон на сокете: опрос с таймаутом и чтение, если есть что читать.
/// Некого будить — очередь молча отпускается, крутить цикл вхолостую не надо.
///
/// Обои сюда не ходят (у них свой каденс 8 мс) — отсюда разрешение.
#[allow(dead_code)]
pub fn poll_read<D>(queue: &mut EventQueue<D>, conn: BorrowedFd<'_>, wait: u16) {
    let Some(guard) = queue.prepare_read() else {
        return;
    };
    let mut fds = [PollFd::new(conn, PollFlags::POLLIN)];
    let _ = poll(&mut fds, wait);
    let readable = fds[0]
        .revents()
        .is_some_and(|events| events.intersects(PollFlags::POLLIN));
    if readable {
        let _ = guard.read();
    }
}
/// Дожимание проявления до непрозрачного кадра. Первый кадр может рисоваться
/// дольше всей анимации (холодный кэш шрифтов и иконок): к следующему тику
/// твин уже done, а на экране остался бы полупрозрачный кадр до первого
/// ввода. Если ушёл недожатый кадр, требуем следующий — он уже будет
/// непрозрачным, и цикл после него встанет сам.
///
/// Берёт флаг, а не контекст слоя, чтобы модуль не зависел от `layer`:
/// окна без слоя (бинды) берут отсюда только шаг колеса.
#[allow(dead_code)]
pub fn finish_fade(dirty: &mut bool, openness: f32) {
    if openness < 1.0 {
        *dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll(value120: i32, discrete: i32, absolute: f64) -> AxisScroll {
        AxisScroll {
            value120,
            discrete,
            absolute,
            ..Default::default()
        }
    }

    /// Одно деление колеса — ровно одна строка, вниз и вверх.
    #[test]
    fn one_detent_is_one_row() {
        let mut acc = 0.0;
        assert_eq!(axis_steps(&mut acc, &scroll(120, 0, 0.0)), 1);
        assert_eq!(axis_steps(&mut acc, &scroll(-120, 0, 0.0)), -1);
        assert_eq!(acc, 0.0, "целые деления остатка не оставляют");
    }

    /// Дробь тачпада копится и отдаётся целой строкой на четвёртом тике.
    #[test]
    fn touchpad_fractions_accumulate() {
        let mut acc = 0.0;
        assert_eq!(axis_steps(&mut acc, &scroll(30, 0, 0.0)), 0);
        assert_eq!(axis_steps(&mut acc, &scroll(30, 0, 0.0)), 0);
        assert_eq!(axis_steps(&mut acc, &scroll(30, 0, 0.0)), 0);
        assert_eq!(axis_steps(&mut acc, &scroll(30, 0, 0.0)), 1);
        assert_eq!(acc, 0.0);
    }

    /// Разные знаки не складываются в чужую строку: вниз и вверх копятся
    /// честно через ноль.
    #[test]
    fn opposite_directions_cancel_out() {
        let mut acc = 0.0;
        assert_eq!(axis_steps(&mut acc, &scroll(60, 0, 0.0)), 0);
        assert_eq!(axis_steps(&mut acc, &scroll(-60, 0, 0.0)), 0);
        assert_eq!(acc, 0.0, "полделения вверх и вниз взаимно гасятся");
    }

    /// Без `value120` берётся `discrete`, без него — знак `absolute`.
    #[test]
    fn fallbacks_follow_the_same_sign() {
        let mut acc = 0.0;
        assert_eq!(axis_steps(&mut acc, &scroll(0, 2, 0.0)), 2);
        assert_eq!(axis_steps(&mut acc, &scroll(0, -1, 0.0)), -1);
        assert_eq!(axis_steps(&mut acc, &scroll(0, 0, 5.0)), 3);
        assert_eq!(axis_steps(&mut acc, &scroll(0, 0, -5.0)), -3);
    }

    /// Пустое событие ничего не двигает и остаток не трогает.
    #[test]
    fn empty_event_is_a_no_op() {
        let mut acc = 0.5;
        assert_eq!(axis_steps(&mut acc, &scroll(0, 0, 0.0)), 0);
        assert_eq!(acc, 0.5, "накопленное добро не теряется");
    }

    /// Приоритет: `value120` бьёт `discrete`, даже если оба не нули.
    #[test]
    fn value120_wins_over_discrete() {
        let mut acc = 0.0;
        assert_eq!(axis_steps(&mut acc, &scroll(120, 5, 0.0)), 1);
    }

    /// Без повтора ждём весь простой.
    #[test]
    fn idle_wait_without_repeat() {
        assert_eq!(wait_ms(None, 250), 250);
        assert_eq!(wait_ms(None, 16), 16);
    }

    /// Удержанная стрелка будит цикл своим интервалом, но не чаще простоя.
    #[test]
    fn repeat_hint_caps_the_wait() {
        use std::time::Duration;
        assert_eq!(wait_ms(Some(Duration::from_millis(5)), 200), 5);
        assert_eq!(
            wait_ms(Some(Duration::from_millis(500)), 200),
            200,
            "повтор реже простоя не усыпляет цикл"
        );
    }

    /// Нулевая подсказка не даёт нулевое ожидание: иначе цикл крутился бы
    /// вхолостую без сна.
    #[test]
    fn zero_hint_clamps_to_one() {
        use std::time::Duration;
        assert_eq!(wait_ms(Some(Duration::ZERO), 200), 1);
    }
}

//! Автоповтор клавиш для окон с навигацией.
//!
//! `wl_keyboard` повторов не шлёт. Композитор сообщает клиенту rate и delay
//! событием `repeat_info`, а повторные нажатия обязан выдавать сам клиент, пока
//! клавиша удерживается. `smithay-client-toolkit` умеет это только через
//! calloop, которого в проекте нет, поэтому повтор живёт здесь: окна и так
//! крутятся обычным циклом с `poll`.
//!
//! Отслеживается одна клавиша — этого хватает меню, где удерживают одну
//! стрелку. Вторая нажатая клавиша просто вытесняет первую.

use std::time::{Duration, Instant};

use smithay_client_toolkit::seat::keyboard::{KeyEvent, RepeatInfo};

/// Потолок частоты повтора в списках.
///
/// Композитор присылает rate, настроенный для набора текста, где 25 Гц
/// привычны. В списке это слишком быстро: строка проносится мимо, и выбрать
/// нужную не получается. Поэтому частота ограничена сверху, а `delay`
/// остаётся композиторский — пауза перед стартом помогает попасть в начало.
const MAX_RATE: u32 = 12;

/// Задержка до первого повтора, если композитор ещё не прислал `repeat_info`.
/// Нулём быть не должна: иначе первое же удержание пронесло бы курсор через
/// весь список.
const DEFAULT_DELAY_MS: u32 = 400;

#[derive(Debug)]
pub struct Repeat {
    interval: Duration,
    delay: Duration,
    held: Option<KeyEvent>,
    next: Option<Instant>,
}

impl Default for Repeat {
    fn default() -> Self {
        Self {
            // Тот же потолок, что и в `set_info`: иначе до первого
            // `repeat_info` список листался бы вдвое быстрее, чем потом.
            interval: Duration::from_millis(u64::from(1000 / MAX_RATE)),
            delay: Duration::from_millis(u64::from(DEFAULT_DELAY_MS)),
            held: None,
            next: None,
        }
    }
}

impl Repeat {
    /// Клавиатура сообщила rate и delay. `rate == 0` композитор трактует как
    /// «повтор выключен», и тогда удержание не двигает курсор вовсе.
    pub fn set_info(&mut self, info: RepeatInfo) {
        match info {
            RepeatInfo::Disable => {
                self.held = None;
                self.next = None;
            }
            RepeatInfo::Repeat { rate, delay } => {
                let millis = 1000 / rate.get().clamp(1, MAX_RATE);
                self.interval = Duration::from_millis(u64::from(millis));
                self.delay = Duration::from_millis(u64::from(delay));
            }
        }
    }

    /// Клавиша нажата: ждём `delay` и начинаем повторять с `rate`.
    pub fn press(&mut self, event: &KeyEvent) {
        self.held = Some(event.clone());
        self.next = Some(Instant::now() + self.delay);
    }

    /// Клавиша отпущена. Сравнение по `raw_code`: у отпускания `utf8` пустой,
    /// а вот keysym может отличаться из-за смены раскладки в момент отпускания.
    pub fn release(&mut self, event: &KeyEvent) {
        if self
            .held
            .as_ref()
            .is_some_and(|held| held.raw_code == event.raw_code)
        {
            self.held = None;
            self.next = None;
        }
    }

    /// Повтор, если подошло время. Повтор отдаёт то же нажатие, поэтому
    /// обработчики видят обычный `press_key` без отдельной ветки.
    pub fn poll(&mut self) -> Option<KeyEvent> {
        let now = Instant::now();
        let next = self.next?;
        if now < next {
            return None;
        }
        self.next = Some(now + self.interval);
        self.held.clone()
    }

    /// Сколько ждать до следующего повтора. `None`, если повтор не нужен и
    /// цикл может спать дольше обычного.
    pub fn wait_hint(&self) -> Option<Duration> {
        let next = self.next?;
        Some(next.saturating_duration_since(Instant::now()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay_client_toolkit::seat::keyboard::Keysym;

    fn key(raw_code: u32) -> KeyEvent {
        KeyEvent {
            time: 0,
            raw_code,
            keysym: Keysym::Right,
            utf8: None,
        }
    }

    fn repeat_info(rate: u32, delay: u32) -> RepeatInfo {
        RepeatInfo::Repeat {
            rate: std::num::NonZeroU32::new(rate).unwrap(),
            delay,
        }
    }

    #[test]
    fn nothing_repeats_until_a_key_is_pressed() {
        let mut repeat = Repeat::default();
        repeat.set_info(repeat_info(25, 400));
        assert!(repeat.poll().is_none());
        assert!(repeat.wait_hint().is_none());
    }

    #[test]
    fn press_waits_for_the_delay() {
        let mut repeat = Repeat::default();
        repeat.set_info(repeat_info(100, 300));
        repeat.press(&key(30));
        assert!(
            repeat.poll().is_none(),
            "сразу после нажатия повтора быть не должно"
        );
        assert!(
            repeat.wait_hint().is_some(),
            "цикл должен проснуться по таймеру"
        );
    }

    #[test]
    fn release_stops_the_repeat_of_that_key() {
        let mut repeat = Repeat::default();
        repeat.set_info(repeat_info(1000, 0));
        repeat.press(&key(30));
        repeat.release(&key(30));
        assert!(repeat.poll().is_none());
        assert!(repeat.wait_hint().is_none());
    }

    #[test]
    fn release_of_another_key_keeps_the_repeat() {
        let mut repeat = Repeat::default();
        repeat.set_info(repeat_info(1000, 0));
        repeat.press(&key(30));
        repeat.release(&key(31));
        assert!(repeat.wait_hint().is_some());
    }

    #[test]
    fn disabled_repeat_stops_a_held_key() {
        let mut repeat = Repeat::default();
        repeat.set_info(repeat_info(1000, 0));
        repeat.press(&key(30));
        repeat.set_info(RepeatInfo::Disable);
        assert!(repeat.poll().is_none());
        assert!(repeat.wait_hint().is_none());
    }

    #[test]
    fn fast_rate_is_capped_so_rows_stay_readable() {
        // Композитор обычно присылает rate около 25: для набора текста
        // нормально, для списка строка проносится мимо. Режем до MAX_RATE.
        // Проверяем сам интервал, а не остаток до следующего тика: замер
        // времени копит микросекунды и сравнение в миллисекундах дрожит.
        let mut repeat = Repeat::default();
        repeat.set_info(repeat_info(25, 0));
        assert_eq!(
            repeat.interval,
            Duration::from_millis(u64::from(1000 / MAX_RATE)),
            "частота выше потолка должна быть срезана"
        );
        assert_eq!(repeat.interval, Duration::from_millis(83));
    }

    #[test]
    fn absurd_rate_does_not_collapse_the_interval() {
        let mut repeat = Repeat::default();
        repeat.set_info(repeat_info(100_000, 0));
        repeat.press(&key(30));
        repeat.poll();
        let hint = repeat
            .wait_hint()
            .expect("клавиша удерживается, значит есть следующий повтор");
        assert!(
            hint >= Duration::from_millis(8),
            "интервал не должен схлопнуться в ноль: {hint:?}"
        );
    }

    #[test]
    fn slow_rate_is_left_alone() {
        // Потолок только сверху: если rate меньше, это выбор пользователя,
        // и ускорять его нельзя.
        let mut repeat = Repeat::default();
        repeat.set_info(repeat_info(4, 0));
        assert_eq!(repeat.interval, Duration::from_millis(250));
    }

    #[test]
    fn repeated_event_is_the_pressed_one() {
        let mut repeat = Repeat::default();
        repeat.set_info(repeat_info(1000, 0));
        let pressed = key(30);
        repeat.press(&pressed);
        // Нулевая задержка: повтор пора сразу.
        let fired = repeat.poll().expect("повтор должен произойти");
        assert_eq!(fired.raw_code, pressed.raw_code);
        assert_eq!(fired.keysym, pressed.keysym);
    }

    #[test]
    fn defaults_never_collapse_to_zero() {
        // До первого `repeat_info` интервалы обязаны быть осмысленными:
        // нулевая задержка означала бы мгновенный пролёт списка.
        let repeat = Repeat::default();
        assert!(
            repeat.interval > Duration::ZERO,
            "интервал не может быть нулём"
        );
        assert!(
            repeat.delay > Duration::ZERO,
            "задержка не может быть нулём"
        );
        assert_eq!(
            repeat.interval,
            Duration::from_millis(u64::from(1000 / MAX_RATE)),
            "дефолт обязан совпадать с потолком, иначе список до первого \
             repeat_info листался бы быстрее, чем после"
        );
        assert_eq!(
            repeat.delay,
            Duration::from_millis(u64::from(DEFAULT_DELAY_MS))
        );
    }
}

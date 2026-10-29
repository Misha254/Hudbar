//! Общая основа анимаций: кривые, твин, набор значений по ключу и настройки.
//!
//! Модуль ничего не знает ни про Wayland, ни про панель: значение — обычный
//! `f32`, время приходит параметром. `Instant::now()` внутри не вызывается
//! нигде — иначе тест, задающий время руками, не проверял бы ничего.
//!
//! Один [`Tween`] — одно число, идущее от `from` к `to` за `duration` по
//! кривой `easing`. [`AnimSet`] держит по твину на ключ и нужен там, где
//! одновременно живут несколько независимых значений: подсветки чипов бара.
//! Ключи двигаются независимо, поэтому переход указателя с одного чипа на
//! другой не обрывает затухание старого.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

/// Кривая: нормализованное время 0..1 → нормализованное значение 0..1.
pub type Easing = fn(f32) -> f32;

/// Сколько длится уход карточки с экрана блокировки и экрана входа.
///
/// Это не украшение поверх чужой анимации: `ext-session-lock` снимается
/// мгновенно, а гритер обязан выйти до старта сеанса, поэтому единственный
/// кадр перехода, который мы можем показать, — свой собственный. 240 мс
/// хватает, чтобы карточка успела уйти, и мало, чтобы человек ждал входа.
///
/// Модуль общий у девяти бинарей, а уход нужен только двум; для остальных
/// константа не используется, поэтому `dead_code` здесь ожидаем.
#[allow(dead_code)]
pub const EXIT_DURATION: Duration = Duration::from_millis(240);

/// Насколько карточка увеличивается за время ухода: 4% читаются глазом, но
/// текст не успевает расплыться.
#[allow(dead_code)]
pub const EXIT_SCALE: f32 = 0.04;

/// Приведение входа к 0..1. `f32::clamp` при значении NaN возвращает сам NaN,
/// а NaN в альфе уехал бы в цвет пикселя и испортил кадр, поэтому вход
/// нормализуется явно: всё, что не число, считается нулём.
fn unit(t: f32) -> f32 {
    if t.is_nan() {
        return 0.0;
    }
    t.clamp(0.0, 1.0)
}

/// Затухание с ускорением в конце: быстро стартует, мягко садится.
pub fn ease_out_cubic(t: f32) -> f32 {
    (1.0 - (1.0 - unit(t)).powi(3)).clamp(0.0, 1.0)
}

/// Разгон с торможением в конце — зеркало `ease_out_cubic`.
pub fn ease_in_cubic(t: f32) -> f32 {
    (unit(t).powi(3)).clamp(0.0, 1.0)
}

/// Приведение значения к ближайшей из `steps` ступеней: для Pixel-темы, где
/// промежуточных значений быть не должно. `steps == 0` — значение как есть.
/// Общие настройки анимаций: включены ли они и с какой скоростью идут.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimSettings {
    pub enabled: bool,
    pub speed: f32,
}

impl Default for AnimSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            speed: 1.0,
        }
    }
}

impl AnimSettings {
    /// Настройки из значений конфига. Границы скорости держит `settings.rs`
    /// при разборе файла, а здесь стоит защита от мусора: панель не должна
    /// делить длительность на ноль или на NaN.
    pub fn new(enabled: bool, speed: f32) -> Self {
        let speed = if speed.is_finite() && speed > 0.0 {
            speed
        } else {
            1.0
        };
        Self { enabled, speed }
    }

    /// Длительность с учётом скорости. Выключенные анимации дают нулевую
    /// длительность, а нулевая длительность означает «значение уже достигнуто
    /// цели» — выключатель не должен оставлять за себя последний кадр.
    ///
    /// Делится наносекунды целочисленно, а не через `Duration::from_secs_f32`:
    /// тот округляет 100 мс до 100.000001 мс, и любое сравнение длительностей
    /// в тестах после этого перестаёт быть точным.
    pub fn scaled(&self, duration: Duration) -> Duration {
        if !self.enabled || duration.is_zero() {
            return Duration::ZERO;
        }
        // Поля публичные, поэтому `AnimSettings` можно собрать и в обход
        // `new`: ноль и нечисло не должны превращать длительность в вечность.
        let speed = if self.speed.is_finite() && self.speed > 0.0 {
            self.speed
        } else {
            1.0
        };
        let nanos = duration.as_nanos() as f64 / speed as f64;
        if !nanos.is_finite() || nanos <= 0.0 {
            return Duration::ZERO;
        }
        Duration::from_nanos(nanos.round() as u64)
    }
}

/// Движение одного числа от `from` к `to` по кривой `easing`.
///
/// `PartialEq` здесь не выводится намеренно: поле `easing` — указатель на
/// функцию, а сравнение указателей не значит ничего полезного (адреса не
/// обязаны быть уникальными), и такое сравнение молча даёт неверный ответ.
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    pub started: Instant,
    pub duration: Duration,
    pub easing: Easing,
}

impl Tween {
    pub fn new(from: f32, to: f32, started: Instant, duration: Duration, easing: Easing) -> Self {
        Self {
            from,
            to,
            started,
            duration,
            easing,
        }
    }

    /// Значение, которое уже достигнуто и больше не меняется.
    pub fn settled(value: f32, now: Instant) -> Self {
        Self::new(value, value, now, Duration::ZERO, ease_out_cubic)
    }

    /// Текущее значение. Нулевая длительность — это не «мгновенно», а
    /// «уже на месте»: именно так выключенные анимации догоняют цель сразу.
    pub fn value(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return self.to;
        }
        let elapsed = now.saturating_duration_since(self.started).as_secs_f32();
        let raw = (elapsed / self.duration.as_secs_f32()).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * (self.easing)(raw)
    }

    pub fn done(&self, now: Instant) -> bool {
        self.duration.is_zero() || now.saturating_duration_since(self.started) >= self.duration
    }

    /// Смена цели продолжает движение с текущего значения: без скачка.
    /// Длительность и кривая остаются прежними — их задаёт вызывающий.
    ///
    /// Повтор с той же целью — no-op. Иначе вызывающий, который дёргает
    /// `retarget` каждый кадр (цикл окна, сверка состояния), сдвигал бы
    /// отсчёт с нуля непрерывно: движение ползло бы к цели бесконечно
    /// медленно и `done()` не наступало бы никогда.
    pub fn retarget(&mut self, now: Instant, to: f32) {
        if to == self.to {
            return;
        }
        let current = self.value(now);
        self.from = current;
        self.to = to;
        self.started = now;
    }
}

/// Набор независимых движущихся значений по ключу: у каждого своя цель 0 или 1.
pub struct AnimSet<K: Eq + Hash> {
    entries: HashMap<K, Tween>,
    /// Момент последнего `tick`: значения читаются без параметра времени,
    /// поэтому источник времени живёт здесь.
    now: Instant,
    duration: Duration,
    settings: AnimSettings,
}

impl<K: Eq + Hash> AnimSet<K> {
    pub fn new(now: Instant, duration: Duration, settings: AnimSettings) -> Self {
        Self {
            entries: HashMap::new(),
            now,
            duration,
            settings,
        }
    }

    /// Смена настроек на лету. Идущие движения пересчитываются по новой
    /// скорости, а при выключении их длительность схлопывается в ноль:
    /// иначе выключатель ждал бы конца текущей анимации, то есть не выключал
    /// бы ничего.
    pub fn set_settings(&mut self, settings: AnimSettings) {
        self.settings = settings;
        for tween in self.entries.values_mut() {
            tween.duration = settings.scaled(tween.duration);
        }
    }

    /// Поставить цель: 0 или 1. Любое другое значение трактуется как 1 —
    /// подсветка либо есть, либо нет.
    pub fn set_target(&mut self, key: K, target: f32, now: Instant) {
        self.now = now;
        let target = if target > 0.5 { 1.0 } else { 0.0 };
        // Цель уже та же и движение идёт: трогать нечего. Без этой проверки
        // вызывающий, дёргающий цель каждый кадр, пересчитывал бы длительность
        // и кривую на идущей анимации.
        if self
            .entries
            .get(&key)
            .is_some_and(|tween| tween.to == target)
        {
            return;
        }
        let current = self.value(&key);
        if current == target {
            // Уже на месте: незачем начинать движение заново. Запись при этом
            // остаётся — из неё читается достигнутая единица.
            return;
        }
        // Появление мягко садится, исчезновение — быстро уходит. Направление
        // выбирается по цели, а не по прошлому движению: перескок указателя с
        // чипа на чип меняет цель на противоположную ровно один раз.
        let easing = if target > current {
            ease_out_cubic
        } else {
            ease_in_cubic
        };
        let duration = self.settings.scaled(self.duration);
        match self.entries.get_mut(&key) {
            Some(tween) => {
                tween.retarget(now, target);
                tween.duration = duration;
                tween.easing = easing;
            }
            None => {
                self.entries
                    .insert(key, Tween::new(current, target, now, duration, easing));
            }
        }
    }

    /// Текущее значение по ключу; для неизвестного ключа — ноль.
    pub fn value(&self, key: &K) -> f32 {
        match self.entries.get(key) {
            Some(tween) => tween.value(self.now),
            None => 0.0,
        }
    }

    /// Есть ли незаконченные движения. Записи, дошедшие до единицы, считаются
    /// движением: их значение ещё нужно читать, пока цель не сменится.
    pub fn active(&self) -> bool {
        self.entries.values().any(|tween| !tween.done(self.now))
    }

    /// Обновить время и выбросить записи, дошедшие до нуля: для них покойное
    /// значение совпадает с отсутствующей записью. Дошедшие единицы
    /// остаются — иначе подсветка чипа гасла бы сама, без новой цели.
    /// `true`, если что-то ещё двигается и нужен следующий кадр.
    pub fn tick(&mut self, now: Instant) -> bool {
        self.now = now;
        self.entries
            .retain(|_, tween| !(tween.done(now) && tween.to == 0.0));
        self.active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// База времени для тестов. Одна на весь прогон: если звать `Instant::now()`
    /// в каждом вызове, база плывёт между строками одного теста и «момент
    /// retarget» оказывается позже самого retarget.
    fn base() -> Instant {
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now)
    }

    fn ms(value: u32) -> Instant {
        base() + Duration::from_millis(value as u64)
    }

    /// Кривая под именем: имя нужно только сообщению теста.
    type NamedEasing = (&'static str, Easing);

    const EASINGS: [NamedEasing; 2] = [
        ("ease_out_cubic", ease_out_cubic),
        ("ease_in_cubic", ease_in_cubic),
    ];

    #[test]
    fn easing_pins_both_ends() {
        for (name, easing) in EASINGS {
            assert!((easing(0.0) - 0.0).abs() < 1e-6, "{name}: начало не ноль");
            assert!((easing(1.0) - 1.0).abs() < 1e-6, "{name}: конец не единица");
        }
    }

    #[test]
    fn easing_clamps_out_of_range_input() {
        for (name, easing) in EASINGS {
            assert!((easing(-1.0) - 0.0).abs() < 1e-6, "{name}: снизу не ноль");
            assert!(
                (easing(2.0) - 1.0).abs() < 1e-6,
                "{name}: сверху не единица"
            );
            assert!((easing(f32::NAN) - 0.0).abs() < 1e-6, "{name}: NaN не ноль");
        }
    }

    #[test]
    fn easing_is_monotonic_and_bounded() {
        for (name, easing) in EASINGS {
            let mut previous = 0.0;
            for step in 0..=100 {
                let t = step as f32 / 100.0;
                let value = easing(t);
                assert!(
                    value >= previous - 1e-6 && (0.0..=1.0).contains(&value),
                    "{name} не монотонна на {t}: {value} после {previous}"
                );
                previous = value;
            }
        }
    }

    #[test]
    fn easing_shapes_match_their_names() {
        // ease_out_cubic идёт выше прямой: стартует быстро и дотягивает конец.
        assert!(ease_out_cubic(0.25) > 0.25);
        assert!(ease_out_cubic(0.75) > 0.75);
        assert!(ease_out_cubic(0.5) > ease_in_cubic(0.5));
        // ease_in_cubic — ниже прямой на всём отрезке.
        assert!(ease_in_cubic(0.25) < 0.25);
        assert!(ease_in_cubic(0.75) < 0.75);
    }

    #[test]
    fn tween_reaches_both_ends_of_its_span() {
        let tween = Tween::new(0.0, 1.0, base(), Duration::from_millis(100), ease_out_cubic);

        assert!((tween.value(base()) - 0.0).abs() < 1e-6);
        assert!(!tween.done(base()));
        assert!((tween.value(ms(100)) - 1.0).abs() < 1e-6);
        assert!((tween.value(ms(1000)) - 1.0).abs() < 1e-6);
        assert!(tween.done(ms(100)));
    }

    #[test]
    fn tween_holds_at_both_ends_outside_its_span() {
        let tween = Tween::new(0.2, 0.8, base(), Duration::from_millis(100), ease_in_cubic);

        assert!(
            (tween.value(base()) - 0.2).abs() < 1e-6,
            "до старта не течёт"
        );
        assert!((tween.value(ms(100)) - 0.8).abs() < 1e-6);
        assert!(
            (tween.value(ms(400)) - 0.8).abs() < 1e-6,
            "после конца не течёт"
        );
    }

    #[test]
    fn zero_duration_tween_is_settled_right_away() {
        let tween = Tween::new(0.0, 1.0, base(), Duration::ZERO, ease_out_cubic);

        assert!((tween.value(base()) - 1.0).abs() < 1e-6);
        assert!(tween.done(base()));
    }

    #[test]
    fn tween_time_before_the_start_does_not_underflow() {
        let tween = Tween::new(
            0.0,
            1.0,
            ms(100),
            Duration::from_millis(100),
            ease_out_cubic,
        );

        assert!((tween.value(base()) - 0.0).abs() < 1e-6);
        assert!(!tween.done(base()));
    }

    #[test]
    fn retarget_continues_from_the_current_value() {
        let mut tween = Tween::new(0.0, 1.0, base(), Duration::from_millis(100), ease_out_cubic);
        // Половина пути пройдена, цель разворачивают.
        let half = ms(50);
        let at_half = tween.value(half);
        tween.retarget(half, 0.0);

        assert!(
            (tween.from - at_half).abs() < 1e-6,
            "прыжок в ноль вместо текущего"
        );
        assert!(
            (tween.value(half) - at_half).abs() < 1e-6,
            "в момент retarget значение скачет"
        );
        assert!((tween.to - 0.0).abs() < 1e-6);
    }

    #[test]
    fn retarget_reaches_the_new_target_within_its_own_span() {
        let mut tween = Tween::new(0.0, 1.0, base(), Duration::from_millis(100), ease_out_cubic);
        tween.retarget(ms(50), 0.0);

        assert!((tween.value(ms(150)) - 0.0).abs() < 1e-6);
        assert!(tween.done(ms(150)));
    }

    #[test]
    fn a_hundred_retargets_to_the_same_target_do_not_stall() {
        // `ease_in` ползёт быстрее всего в конце: если бы каждый retarget с той
        // же целью сдвигал отсчёт, движение не сдвинулось бы совсем, а
        // `done()` не наступило бы никогда — так окно могло бы висеть вечно.
        let mut tween = Tween::new(1.0, 0.0, base(), Duration::from_millis(110), ease_in_cubic);
        let mut now = base();
        for _ in 0..100 {
            now += Duration::from_millis(16);
            tween.retarget(now, 0.0);
        }

        assert!(tween.done(now), "повтор той же цели затормозил закрытие");
        assert!((tween.value(now) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn repeating_the_current_target_keeps_the_running_movement() {
        let mut tween = Tween::new(0.0, 1.0, base(), Duration::from_millis(200), ease_out_cubic);
        let midway = tween.value(ms(50));

        tween.retarget(ms(50), 1.0);

        assert_eq!(tween.started, base(), "отсчёт не должен сдвигаться");
        assert!(
            (tween.from - 0.0).abs() < 1e-6,
            "начало не должно трогаться"
        );
        assert!(
            (tween.value(ms(50)) - midway).abs() < 1e-6,
            "значение на месте повтора не должно скакнуть"
        );
        assert!(!tween.done(ms(50)), "движение продолжается");
    }

    #[test]
    fn a_repeated_retarget_still_changes_the_target_when_it_really_changed() {
        // Укрепление не должно заблокировать настоящую смену цели.
        let mut tween = Tween::new(0.0, 1.0, base(), Duration::from_millis(200), ease_out_cubic);
        tween.retarget(ms(50), 1.0);
        tween.retarget(ms(50), 0.0);

        assert!((tween.to - 0.0).abs() < 1e-6);
        assert!(
            (tween.from - tween.value(ms(50))).abs() < 1e-6,
            "смена цели должна стартовать с текущего значения"
        );
    }

    #[test]
    fn a_repeated_set_target_keeps_a_running_value_moving() {
        let mut set = set(base());
        set.set_target("a", 1.0, base());
        set.tick(ms(50));
        let midway = set.value(&"a");

        // Тот же ключ и та же цель на каждом тике — как в цикле окна.
        let mut now = ms(50);
        for _ in 0..100 {
            now += Duration::from_millis(16);
            set.tick(now);
            set.set_target("a", 1.0, now);
        }

        assert!(
            set.value(&"a") >= midway,
            "значение не должно откатываться назад"
        );
        assert!((set.value(&"a") - 1.0).abs() < 1e-6, "цель не достигнута");
        assert!(!set.active());
    }

    fn set(now: Instant) -> AnimSet<&'static str> {
        AnimSet::new(now, Duration::from_millis(100), AnimSettings::default())
    }

    #[test]
    fn anim_set_starts_empty_and_idle() {
        let set = set(base());

        assert!(!set.active());
        assert!((set.value(&"a") - 0.0).abs() < 1e-6);
    }

    #[test]
    fn anim_set_keys_move_independently() {
        let mut set = set(base());
        set.set_target("old", 1.0, base());
        set.tick(ms(100));
        assert!(
            (set.value(&"old") - 1.0).abs() < 1e-6,
            "старый чип не разгорелся"
        );

        // Указатель перескочил на соседний: старый гаснет, новый разгорается.
        // Ключи независимы, поэтому затухание старого не обрывается.
        set.set_target("old", 0.0, ms(100));
        set.set_target("new", 1.0, ms(100));
        set.tick(ms(150));

        let old = set.value(&"old");
        let new = set.value(&"new");

        assert!(
            old > 0.0 && old < 1.0,
            "fade-out старого ключа оборвался: {old}"
        );
        assert!(new > 0.0 && new < 1.0, "новый ключ должен разгораться");
        assert!(set.active(), "оба ключа в работе");

        set.tick(ms(400));
        assert!((set.value(&"old") - 0.0).abs() < 1e-6);
        assert!((set.value(&"new") - 1.0).abs() < 1e-6);
        assert!(!set.active(), "дошедшие записи больше не двигаются");
    }

    #[test]
    fn a_lit_key_keeps_its_value_after_it_settles() {
        // Регрессия: если запись дошедшего до единицы ключа выбрасывается как
        // «больше не нужна», подсветка чипа гаснет сама, без новой цели.
        let mut set = set(base());
        set.set_target("chip", 1.0, base());
        set.tick(ms(500));

        assert!(!set.active(), "анимация дошла до цели");
        assert!(
            (set.value(&"chip") - 1.0).abs() < 1e-6,
            "подсвеченный чип должен остаться подсвеченным"
        );

        // И наоборот: погасший ключ остаётся нулём и без записи.
        set.set_target("chip", 0.0, ms(500));
        set.tick(ms(900));
        assert!((set.value(&"chip") - 0.0).abs() < 1e-6);
    }

    #[test]
    fn anim_set_reaches_target_and_becomes_idle() {
        let mut set = set(base());
        set.set_target("a", 1.0, base());

        assert!(set.active());
        set.tick(ms(50));
        let mid = set.value(&"a");
        assert!(
            mid > 0.0 && mid < 1.0,
            "на середине пути значение промежуточное"
        );
        assert!(set.active(), "в середине пути ещё работает");

        set.tick(ms(200));
        assert!((set.value(&"a") - 1.0).abs() < 1e-6);
        assert!(!set.active(), "по достижении цели набор простаивает");
    }

    #[test]
    fn anim_set_retarget_does_not_jump() {
        let mut set = set(base());
        set.set_target("a", 1.0, base());
        set.tick(ms(60));
        let before = set.value(&"a");

        set.set_target("a", 0.0, ms(60));
        let after = set.value(&"a");

        assert!(
            (before - after).abs() < 1e-6,
            "смена цели дала скачок: {before} → {after}"
        );
        assert!((set.value(&"a") - before).abs() < 1e-6);
    }

    #[test]
    fn anim_set_target_at_current_value_does_nothing() {
        let mut set = set(base());
        set.set_target("a", 0.0, base());

        assert!(!set.active(), "цель достигнута — запись не заводится");
        assert!((set.value(&"a") - 0.0).abs() < 1e-6);
    }

    #[test]
    fn anim_set_normalizes_arbitrary_target_to_zero_or_one() {
        let mut set = set(base());
        // Подсветка либо есть, либо нет: любое значение выше половины — вверх.
        set.set_target("a", 0.6, base());
        set.tick(ms(500));
        assert!((set.value(&"a") - 1.0).abs() < 1e-6, "0.6 — это вверх");

        set.set_target("a", 0.4, ms(500));
        set.tick(ms(1000));
        assert!((set.value(&"a") - 0.0).abs() < 1e-6, "0.4 — это вниз");

        set.set_target("a", -7.0, ms(1000));
        set.tick(ms(1500));
        assert!(
            (set.value(&"a") - 0.0).abs() < 1e-6,
            "отрицательное — это вниз"
        );
    }

    #[test]
    fn disabled_animations_reach_the_target_immediately() {
        let mut set = AnimSet::new(
            base(),
            Duration::from_millis(100),
            AnimSettings {
                enabled: false,
                speed: 1.0,
            },
        );
        set.set_target("a", 1.0, base());

        assert!((set.value(&"a") - 1.0).abs() < 1e-6, "цель не сразу");
        assert!(!set.active(), "выключенная анимация не держит цикл");
        assert!(
            !set.tick(ms(1)),
            "выключенная анимация не будит перерисовку"
        );
    }

    #[test]
    fn anim_settings_scales_duration_by_speed() {
        let duration = Duration::from_millis(200);
        let fast = AnimSettings {
            enabled: true,
            speed: 2.0,
        };
        let slow = AnimSettings {
            enabled: true,
            speed: 0.5,
        };

        assert_eq!(fast.scaled(duration), Duration::from_millis(100));
        assert_eq!(slow.scaled(duration), Duration::from_millis(400));
    }

    #[test]
    fn anim_settings_disabled_gives_zero_duration() {
        let off = AnimSettings {
            enabled: false,
            speed: 1.0,
        };

        assert_eq!(off.scaled(Duration::from_millis(200)), Duration::ZERO);
        assert_eq!(off.scaled(Duration::ZERO), Duration::ZERO);
    }

    #[test]
    fn anim_settings_survives_a_broken_speed() {
        let settings = AnimSettings {
            enabled: true,
            speed: 0.0,
        };

        assert_eq!(
            settings.scaled(Duration::from_millis(200)),
            Duration::from_millis(200),
            "нулевая скорость не должна давать вечную анимацию"
        );
        let nan = AnimSettings {
            enabled: true,
            speed: f32::NAN,
        };
        assert_eq!(
            nan.scaled(Duration::from_millis(200)),
            Duration::from_millis(200)
        );
    }

    #[test]
    fn new_replaces_a_broken_speed_with_the_default_one() {
        // Границы держит парсер settings.json, а `new` страхует от мусора,
        // попавшего в обход него: делить на ноль нельзя.
        assert_eq!(AnimSettings::new(true, 1.5).speed, 1.5);
        assert_eq!(AnimSettings::new(true, 0.0).speed, 1.0);
        assert_eq!(AnimSettings::new(true, -2.0).speed, 1.0);
        assert_eq!(AnimSettings::new(true, f32::NAN).speed, 1.0);
        assert_eq!(AnimSettings::new(true, f32::INFINITY).speed, 1.0);
    }

    #[test]
    fn new_keeps_the_enabled_flag() {
        assert!(AnimSettings::new(true, 1.0).enabled);
        assert!(!AnimSettings::new(false, 1.0).enabled);
    }

    #[test]
    fn switching_animations_off_ends_the_running_movement() {
        // Иначе выключатель ждал бы конца текущей анимации: значение так и
        // доехало бы до единицы самым, уже после выключения.
        let mut set = set(base());
        set.set_target("a", 1.0, base());
        set.tick(ms(40));
        assert!(set.active());

        set.set_settings(AnimSettings::new(false, 1.0));

        assert!(
            (set.value(&"a") - 1.0).abs() < 1e-6,
            "значение не встало на место"
        );
        assert!(!set.active(), "движение продолжается после выключения");
    }

    #[test]
    fn changing_speed_rescales_a_running_movement() {
        let mut set = set(base());
        set.set_target("a", 1.0, base());
        set.tick(ms(20));

        set.set_settings(AnimSettings::new(true, 2.0));
        // 100 мс на скорости 2 — это 50 мс от старта; 60 мс уже за гранью.
        set.tick(ms(60));

        assert!(
            (set.value(&"a") - 1.0).abs() < 1e-6,
            "ускорение не подействовало"
        );
    }

    #[test]
    fn anim_set_honours_the_speed_change_on_the_next_target() {
        let mut set = set(base());
        set.set_settings(AnimSettings {
            enabled: true,
            speed: 2.0,
        });
        set.set_target("a", 1.0, base());

        // Базовая длительность 100 мс на скорости 2 — это 50 мс пути.
        set.tick(ms(50));
        assert!(
            (set.value(&"a") - 1.0).abs() < 1e-6,
            "полпути должно хватить"
        );
        assert!(!set.active());
    }
}

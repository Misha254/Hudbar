//! Скользящая подсветка выбранной строки меню.
//!
//! Состояние живёт рядом с окном, а не в `view`: `view` ничего не помнит и
//! получает готовую позицию параметром. Здесь только твин по вертикали в
//! «строках экрана» и признак уровня, по которому подсветка обязана встать на
//! место вместо хода.
//!
//! Ход идёт только при смене выбора на том же уровне. Вход в подменю, выход,
//! ввод запроса и подгрузка строк меняют уровень — там скольжение было бы
//! враньём: строки на экране другие, и «продолжать путь» неоткуда.

use std::time::{Duration, Instant};

use super::super::anim::{self, AnimSettings, Tween};

/// Длительность хода подсветки. Короче проявления окна: она уже на месте, к
/// моменту первого хода подсветке незачем ехать.
pub const MOVE_DURATION: Duration = Duration::from_millis(90);

/// Признак, по которому подсветка встаёт на место вместо того, чтобы ехать.
///
/// `items` входит в признак не по отдельной нужде: динамический раздел
/// (Bluetooth, Wi-Fi) меняет число строк на месте, и без него подсветка
/// поехала бы к индексу, которого в списке уже нет.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level {
    pub depth: usize,
    pub query: String,
    pub items: usize,
}

pub fn level_of(depth: usize, query: &str, items: usize) -> Level {
    Level {
        depth,
        query: query.to_string(),
        items,
    }
}

/// Позиция выбранной строки в окне списка: смещение от верхней видимой строки.
/// `None` — выбора нет, подсвечивать нечего.
pub fn position(selected: Option<usize>, top: usize) -> Option<f32> {
    selected.map(|selected| (selected as f32 - top as f32).max(0.0))
}

/// Позиция подсветки для отрисовки.
pub fn draw_position(pos: f32) -> f32 {
    pos
}

/// Скользящая подсветка: твин по строкам плюс признак последнего уровня.
#[derive(Debug)]
pub struct Highlight {
    pos: Tween,
    level: Option<Level>,
    visible: bool,
}

impl Highlight {
    /// Пустая подсветка: до первого `update` отрисовывать нечего, поэтому
    /// `row()` отдаёт `None` и `view` рисует как раньше.
    pub fn new(now: Instant) -> Self {
        Self {
            pos: Tween::settled(0.0, now),
            level: None,
            visible: false,
        }
    }

    /// Позиция выбранной строки на текущем кадре. `None` в `target` — выбора
    /// нет: старая полоса не должна оставаться на экране.
    pub fn update(
        &mut self,
        level: Level,
        target: Option<f32>,
        now: Instant,
        settings: AnimSettings,
    ) {
        let same_level = self.level.as_ref() == Some(&level);
        self.level = Some(level);
        let Some(target) = target else {
            self.visible = false;
            self.pos = Tween::settled(0.0, now);
            return;
        };
        let target = target.max(0.0);
        if !self.visible || !same_level {
            // Другой уровень, запрос или число строк: список пересобран, и
            // прежняя позиция не имеет отношения к новой. Скользить некуда.
            self.visible = true;
            self.pos = Tween::settled(target, now);
            return;
        }
        let mut tween = self.pos;
        tween.retarget(now, target);
        if tween.from == tween.to {
            // Ехать некуда: подсветка уже на месте. Длительность обязана быть
            // нулевой, иначе твин с `from == to` числился бы идущим вечно и
            // окно перерисовывалось бы вхолостую.
            tween.duration = Duration::ZERO;
        } else {
            tween.duration = settings.scaled(MOVE_DURATION);
            tween.easing = anim::ease_out_cubic;
        }
        self.pos = tween;
    }

    /// Позиция для `render_window`. `None`, пока подсветка не началась или
    /// пока выбора нет: это тот же путь, который рисует выбранную строку
    /// внутри `row()`.
    pub fn row(&self, now: Instant) -> Option<f32> {
        self.visible.then(|| draw_position(self.pos.value(now)))
    }

    /// Идёт ли ход: пока едет, окно обязано перерисовываться.
    pub fn animating(&self, now: Instant) -> bool {
        self.visible && !self.pos.done(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Instant {
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now)
    }

    fn ms(value: u64) -> Instant {
        base() + Duration::from_millis(value)
    }

    fn level() -> Level {
        level_of(1, "", 12)
    }

    fn started(now: Instant, settings: AnimSettings) -> Highlight {
        let mut highlight = Highlight::new(now);
        highlight.update(level(), Some(3.0), now, settings);
        highlight
    }

    #[test]
    fn position_is_the_offset_from_the_top_row() {
        assert!((position(Some(0), 0).unwrap() - 0.0).abs() < 1e-6);
        assert!((position(Some(4), 0).unwrap() - 4.0).abs() < 1e-6);
        assert!((position(Some(7), 3).unwrap() - 4.0).abs() < 1e-6);
        assert_eq!(position(None, 3), None, "без выбора позиции нет");
    }

    #[test]
    fn position_never_goes_negative() {
        assert!(
            (position(Some(1), 5).unwrap() - 0.0).abs() < 1e-6,
            "выбор выше окна списка не должен уводить подсветку в минус"
        );
    }

    #[test]
    fn the_first_update_lands_on_the_target_right_away() {
        let highlight = started(ms(0), AnimSettings::default());
        assert!((highlight.row(ms(0)).unwrap() - 3.0).abs() < 1e-6);
    }

    /// Пропавший выбор гасит полосу сразу: старая подсветка не должна
    /// остаться висеть на прежней строке.
    #[test]
    fn a_missing_selection_hides_the_band() {
        let settings = AnimSettings::default();
        let mut highlight = started(ms(0), settings);
        assert!(highlight.row(ms(0)).is_some());

        highlight.update(level(), None, ms(10), settings);
        assert!(highlight.row(ms(10)).is_none());
        assert!(highlight.row(ms(500)).is_none());
        assert!(!highlight.animating(ms(10)));
    }

    #[test]
    fn a_change_of_selection_slides() {
        let settings = AnimSettings::default();
        let mut highlight = started(ms(0), settings);
        highlight.update(level(), Some(4.0), ms(10), settings);

        let at_start = highlight.row(ms(10)).unwrap();
        let midway = highlight.row(ms(55)).unwrap();
        let at_end = highlight.row(ms(100)).unwrap();

        assert!(
            (at_start - 3.0).abs() < 1e-6,
            "ход стартует с прежней позиции: {at_start}"
        );
        assert!(midway > 3.0 && midway < 4.0, "ход идёт: {midway}");
        assert!((at_end - 4.0).abs() < 1e-6, "ход доезжает до цели");
    }

    #[test]
    fn a_change_of_level_snaps() {
        let settings = AnimSettings::default();
        let mut highlight = started(ms(0), settings);
        highlight.update(level_of(2, "", 12), Some(6.0), ms(10), settings);

        assert!(
            (highlight.row(ms(10)).unwrap() - 6.0).abs() < 1e-6,
            "вход в подменю не должен ехать от старой позиции"
        );
        assert!(!highlight.animating(ms(10)));
    }

    #[test]
    fn a_typed_query_snaps() {
        let settings = AnimSettings::default();
        let mut highlight = started(ms(0), settings);
        highlight.update(level_of(1, "wif", 12), Some(1.0), ms(10), settings);

        assert!((highlight.row(ms(10)).unwrap() - 1.0).abs() < 1e-6);
        assert!(!highlight.animating(ms(10)));
    }

    #[test]
    fn a_loaded_provider_snaps() {
        // Bluetooth приходит пустым и дозагружается на месте: число строк
        // растёт при том же уровне.
        let settings = AnimSettings::default();
        let mut highlight = Highlight::new(ms(0));
        highlight.update(level_of(1, "", 2), Some(1.0), ms(0), settings);
        highlight.update(level_of(1, "", 9), Some(4.0), ms(200), settings);

        assert!(
            (highlight.row(ms(200)).unwrap() - 4.0).abs() < 1e-6,
            "подгрузка строк не должна гнать подсветку через пустоту"
        );
    }

    #[test]
    fn a_scroll_that_keeps_the_offset_moves_nothing() {
        // `top` уехал вместе с `selected`: строка та же на экране, хода нет.
        let settings = AnimSettings::default();
        let mut highlight = started(ms(0), settings);
        highlight.update(level(), position(Some(5), 2), ms(10), settings);

        assert!(
            (highlight.row(ms(10)).unwrap() - 3.0).abs() < 1e-6,
            "позиция не изменилась — подсветка стоит на месте"
        );
        assert!(!highlight.animating(ms(10)));
    }

    #[test]
    fn a_scroll_that_moves_the_row_takes_the_highlight_along() {
        let settings = AnimSettings::default();
        let mut highlight = started(ms(0), settings);
        // Выбор тот же, окно сдвинулось: строка физически уехала вверх.
        highlight.update(level(), position(Some(5), 3), ms(10), settings);

        let midway = highlight.row(ms(55)).unwrap();
        assert!(
            midway < 3.0 && midway > 2.0,
            "подсветка идёт за строкой: {midway}"
        );
        assert!((highlight.row(ms(100)).unwrap() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn repeated_targets_do_not_restart_the_move() {
        // Удержание стрелки: повторный кадр с той же целью не должен заново
        // отсчитывать ход от текущего значения.
        let settings = AnimSettings::default();
        let mut highlight = started(ms(0), settings);
        highlight.update(level(), Some(8.0), ms(10), settings);

        let mut now = ms(10);
        for _ in 0..100 {
            now += Duration::from_millis(16);
            highlight.update(level(), Some(8.0), now, settings);
        }

        assert!((highlight.row(now).unwrap() - 8.0).abs() < 1e-6);
        assert!(!highlight.animating(now));
    }

    #[test]
    fn a_move_runs_for_the_declared_time() {
        let settings = AnimSettings::default();
        let mut highlight = started(ms(0), settings);
        highlight.update(level(), Some(5.0), ms(10), settings);

        assert!(
            highlight.animating(ms(10)),
            "после смены выбора ход должен идти"
        );
        assert!(
            highlight.animating(ms(10) + MOVE_DURATION - Duration::from_millis(1)),
            "ход не должен кончаться раньше 90 мс"
        );
        assert!(!highlight.animating(ms(10) + MOVE_DURATION));
    }

    #[test]
    fn disabled_animations_jump_to_the_target() {
        let settings = AnimSettings::new(false, 1.0);
        let mut highlight = started(ms(0), settings);
        highlight.update(level(), Some(7.0), ms(10), settings);

        assert!(
            (highlight.row(ms(10)).unwrap() - 7.0).abs() < 1e-6,
            "выключенная анимация должна встать на место сразу"
        );
        assert!(!highlight.animating(ms(10)));
    }

    #[test]
    fn the_speed_setting_stretches_the_move() {
        let fast = AnimSettings::new(true, 2.0).scaled(MOVE_DURATION);
        assert_eq!(fast, Duration::from_millis(45));
        let slow = AnimSettings::new(true, 0.5).scaled(MOVE_DURATION);
        assert_eq!(slow, Duration::from_millis(180));
    }

    #[test]
    fn draw_position_is_a_passthrough() {
        for step in 0..=10 {
            let value = step as f32 / 10.0;
            assert!((draw_position(value) - value).abs() < 1e-6);
        }
    }

    #[test]
    fn nothing_is_drawn_before_the_first_update() {
        let highlight = Highlight::new(ms(0));
        assert!(highlight.row(ms(0)).is_none());
        assert!(!highlight.animating(ms(0)));
    }
}

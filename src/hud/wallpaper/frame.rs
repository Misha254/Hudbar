//! Скользящая рамка выбранной ячейки и появление миниатюр.
//!
//! Состояние живёт в окне, а `view` ничего не помнит и получает готовые
//! позиции параметрами — так же, как подсветка меню.
//!
//! Рамка едет только при смене выбора в том же наборе. Смена папки, фильтра
//! или длины списка пересобирает сетку целиком, и прежняя позиция к новой
//! отношения не имеет: там рамка встаёт на место.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::super::anim::{self, AnimSettings, Tween};

/// Длительность хода рамки между ячейками.
pub const SLIDE_DURATION: Duration = Duration::from_millis(100);
/// Длительность проявления миниатюры, пришедшей после открытия окна.
pub const THUMB_FADE: Duration = Duration::from_millis(160);

/// Признак набора, по которому рамка встаёт на место вместо хода.
///
/// `top_row` входит не по отдельной нужде: прокрутка меняет положение всех
/// ячеек на экране, и без него рамка ехала бы из-под старой строки.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Set {
    pub folder: String,
    pub query: String,
    pub len: usize,
    pub top_row: usize,
}

pub fn set_of(folder: &str, query: &str, len: usize, top_row: usize) -> Set {
    Set {
        folder: folder.to_string(),
        query: query.to_string(),
        len,
        top_row,
    }
}

/// Экранная позиция ячейки: колонка и ряд в ячейках сетки.
pub fn cell_position(sel: usize, cols: usize, top_row: usize) -> (f32, f32) {
    let cols = cols.max(1);
    ((sel % cols) as f32, (sel / cols) as f32 - top_row as f32)
}

/// Позиция рамки для отрисовки.
///
/// В Pixel-теме округляется до целой ячейки: рамка — сплошная обводка без
/// альфы, и на полуячеечном сдвиге её кромка размывается. Делить ход на
/// четыре шага нельзя — `PIXEL_ANIM_STEPS` относится к шкале 0..1, а здесь
/// твин живёт в ячейках, и половина ячейки даёт ту же размытую кромку.
pub fn draw_cell(pos: (f32, f32), pixel: bool) -> (f32, f32) {
    if pixel {
        (pos.0.round(), pos.1.round())
    } else {
        (pos.0.clamp(0.0, f32::MAX), pos.1.clamp(0.0, f32::MAX))
    }
}

/// Скользящая рамка выбранной ячейки.
#[derive(Debug)]
pub struct Frame {
    col: Tween,
    row: Tween,
    set: Option<Set>,
}

impl Frame {
    pub fn new(now: Instant) -> Self {
        Self {
            col: Tween::settled(0.0, now),
            row: Tween::settled(0.0, now),
            set: None,
        }
    }

    /// Позиция выбранной ячейки на текущем кадре.
    pub fn update(&mut self, set: Set, target: (f32, f32), now: Instant, settings: AnimSettings) {
        let same_set = self.set.as_ref() == Some(&set);
        self.set = Some(set);
        if !same_set {
            // Набор пересобран: сетка стала другой, и ехать неоткуда.
            self.col = Tween::settled(target.0, now);
            self.row = Tween::settled(target.1, now);
            return;
        }
        let (col, row) = (&mut self.col, &mut self.row);
        Self::step(col, target.0, now, settings);
        Self::step(row, target.1, now, settings);
    }

    /// Один твин к цели. Нулевая длительность при `from == to` обязательна:
    /// иначе твин без движения числился бы идущим вечно, и окно перерисовывалось
    /// бы вхолостую.
    fn step(tween: &mut Tween, target: f32, now: Instant, settings: AnimSettings) {
        tween.retarget(now, target);
        if tween.from == tween.to {
            tween.duration = Duration::ZERO;
        } else {
            tween.duration = settings.scaled(SLIDE_DURATION);
            tween.easing = anim::ease_out_cubic;
        }
    }
}

/// Позиция рамки для `view`. `None`, пока рамка не началась: это прежний путь,
/// где ячейка рисует рамку сама.
impl Frame {
    pub fn pos(&self, now: Instant, pixel: bool) -> Option<(f32, f32)> {
        self.set
            .as_ref()
            .map(|_| draw_cell((self.col.value(now), self.row.value(now)), pixel))
    }

    /// Идёт ли ход рамки.
    pub fn animating(&self, now: Instant) -> bool {
        !self.col.done(now) || !self.row.done(now)
    }
}

/// Появление миниатюр: карта «путь → момент старта проявления».
///
/// Миниатюры, готовые к моменту открытия окна, в карте не попадают: их показ
/// уже покрыт проявлением окна, и второй раз мигать им незачем.
#[derive(Debug, Default)]
pub struct Fade {
    started: HashMap<PathBuf, Instant>,
    /// Сколько путей держим, пока идёт проявление. Карта растёт только до
    /// этого размера, иначе открытие папки на тысячу файлов съело бы память.
    cap: usize,
}

/// Размер карты появления. Видимых ячеек 16, предзагрузка берёт ±1 ряд, так
/// что потолок с запасом накрывает всё, что загружается без простоя.
pub const FADE_CAPACITY: usize = 128;

impl Fade {
    pub fn new() -> Self {
        Self {
            started: HashMap::new(),
            cap: FADE_CAPACITY,
        }
    }

    /// Смена папки или фильтра: прежние пути в новом наборе не встречаются, а
    /// держать их в карте незачем.
    pub fn clear(&mut self) {
        self.started.clear();
    }

    pub fn len(&self) -> usize {
        self.started.len()
    }

    pub fn is_empty(&self) -> bool {
        self.started.is_empty()
    }

    /// Заметить, что миниатюра пришла после открытия окна.
    pub fn begin(&mut self, path: &Path, now: Instant) {
        if self.started.contains_key(path) {
            return;
        }
        if self.started.len() >= self.cap {
            // Потолок достигнут: самые старые пути уходят, иначе карта растёт
            // вместе с числом файлов в папке.
            let stale: Vec<PathBuf> = self.started.keys().take(self.cap / 4).cloned().collect();
            for path in stale {
                self.started.remove(&path);
            }
        }
        self.started.insert(path.to_path_buf(), now);
    }

    /// Готова ли миниатюра на момент кадра.
    pub fn alpha(&self, path: &Path, now: Instant, settings: AnimSettings) -> f32 {
        let Some(&started) = self.started.get(path) else {
            // Готова к открытию: показывается сразу, без перехода.
            return 1.0;
        };
        let tween = Tween::new(
            0.0,
            1.0,
            started,
            settings.scaled(THUMB_FADE),
            anim::ease_out_cubic,
        );
        if tween.done(now) {
            return 1.0;
        }
        tween.value(now).clamp(0.0, 1.0)
    }

    /// Идёт ли хоть одно проявление: цикл держит короткий тик, пока не кончат.
    pub fn animating(&self, now: Instant, settings: AnimSettings) -> bool {
        self.started.values().any(|started| {
            !Tween::new(
                0.0,
                1.0,
                *started,
                settings.scaled(THUMB_FADE),
                anim::ease_out_cubic,
            )
            .done(now)
        })
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

    fn set() -> Set {
        set_of("anime", "", 40, 0)
    }

    fn started(now: Instant, settings: AnimSettings) -> Frame {
        let mut frame = Frame::new(now);
        frame.update(set(), (0.0, 0.0), now, settings);
        frame
    }

    #[test]
    fn cell_position_splits_the_index_into_a_column_and_a_row() {
        assert_eq!(cell_position(0, 4, 0), (0.0, 0.0));
        assert_eq!(cell_position(3, 4, 0), (3.0, 0.0));
        assert_eq!(cell_position(4, 4, 0), (0.0, 1.0));
    }

    #[test]
    fn cell_position_counts_rows_from_the_window_top() {
        // Индекс 0 при окне, начатом с пятого ряда, стоит на четыре ряда выше
        // верхней границы сетки: рамка обязана уметь уехать за неё.
        assert_eq!(cell_position(0, 4, 5), (0.0, -5.0));
        assert_eq!(cell_position(7, 4, 5), (3.0, -4.0));
        assert_eq!(cell_position(18, 4, 5), (2.0, -1.0));
    }

    #[test]
    fn the_first_update_lands_on_the_target() {
        let frame = started(ms(0), AnimSettings::default());
        assert_eq!(frame.pos(ms(0), false), Some((0.0, 0.0)));
        assert!(!frame.animating(ms(0)));
    }

    #[test]
    fn a_change_of_selection_slides() {
        let settings = AnimSettings::default();
        let mut frame = started(ms(0), settings);
        frame.update(set(), (2.0, 1.0), ms(10), settings);

        assert_eq!(frame.pos(ms(10), false), Some((0.0, 0.0)));
        let midway = frame.pos(ms(60), false).unwrap();
        assert!(
            midway.0 > 0.0 && midway.0 < 2.0,
            "ход по колонке: {midway:?}"
        );
        assert!(midway.1 > 0.0 && midway.1 < 1.0, "ход по ряду: {midway:?}");
        assert_eq!(frame.pos(ms(110), false), Some((2.0, 1.0)));
    }

    #[test]
    fn a_change_of_set_snaps() {
        let settings = AnimSettings::default();
        let mut frame = started(ms(0), settings);
        frame.update(set_of("anime", "", 40, 8), (3.0, 2.0), ms(10), settings);

        assert_eq!(
            frame.pos(ms(10), false),
            Some((3.0, 2.0)),
            "новая папка: рамка встаёт на место"
        );
        assert!(!frame.animating(ms(10)));
    }

    #[test]
    fn a_changed_query_snaps() {
        let settings = AnimSettings::default();
        let mut frame = started(ms(0), settings);
        frame.update(
            set_of("anime", "sunset", 12, 0),
            (1.0, 0.0),
            ms(10),
            settings,
        );

        assert_eq!(frame.pos(ms(10), false), Some((1.0, 0.0)));
        assert!(!frame.animating(ms(10)));
    }

    #[test]
    fn a_changed_length_snaps() {
        let settings = AnimSettings::default();
        let mut frame = started(ms(0), settings);
        frame.update(set_of("anime", "", 7, 0), (2.0, 1.0), ms(10), settings);

        assert_eq!(frame.pos(ms(10), false), Some((2.0, 1.0)));
    }

    #[test]
    fn a_scroll_snaps_because_it_moves_every_cell() {
        // `top_row` входит в признак набора: прокрутка сдвигает сетку целиком,
        // и рамка обязана пересчитаться, а не ехать из старой ячейки.
        let settings = AnimSettings::default();
        let mut frame = started(ms(0), settings);
        frame.update(
            set_of("anime", "", 40, 2),
            cell_position(6, 4, 2),
            ms(10),
            settings,
        );

        assert_eq!(frame.pos(ms(10), false), Some((2.0, 0.0)));
        assert!(!frame.animating(ms(10)));
    }

    #[test]
    fn repeated_targets_do_not_restart_the_slide() {
        let settings = AnimSettings::default();
        let mut frame = started(ms(0), settings);
        frame.update(set(), (3.0, 3.0), ms(10), settings);

        let mut now = ms(10);
        for _ in 0..100 {
            now += Duration::from_millis(16);
            frame.update(set(), (3.0, 3.0), now, settings);
        }

        assert_eq!(frame.pos(now, false), Some((3.0, 3.0)));
        assert!(!frame.animating(now));
    }

    #[test]
    fn the_slide_runs_for_the_declared_time() {
        let settings = AnimSettings::default();
        let mut frame = started(ms(0), settings);
        frame.update(set(), (1.0, 1.0), ms(10), settings);

        assert!(frame.animating(ms(10)));
        assert!(
            frame.animating(ms(10) + SLIDE_DURATION - Duration::from_millis(1)),
            "ход не должен кончаться раньше 100 мс"
        );
        assert!(!frame.animating(ms(10) + SLIDE_DURATION));
    }

    #[test]
    fn disabled_animations_jump_the_frame() {
        let settings = AnimSettings::new(false, 1.0);
        let mut frame = started(ms(0), settings);
        frame.update(set(), (2.0, 1.0), ms(10), settings);

        assert_eq!(frame.pos(ms(10), false), Some((2.0, 1.0)));
        assert!(!frame.animating(ms(10)));
    }

    #[test]
    fn the_speed_setting_stretches_the_slide() {
        assert_eq!(
            AnimSettings::new(true, 2.0).scaled(SLIDE_DURATION),
            Duration::from_millis(50)
        );
        assert_eq!(
            AnimSettings::new(true, 0.5).scaled(SLIDE_DURATION),
            Duration::from_millis(200)
        );
    }

    #[test]
    fn the_pixel_theme_snaps_the_frame_to_whole_cells() {
        let settings = AnimSettings::default();
        let mut frame = started(ms(0), settings);
        frame.update(set(), (3.0, 3.0), ms(10), settings);

        let midway = frame.pos(ms(60), false).unwrap();
        let pixel = frame.pos(ms(60), true).unwrap();

        assert!(
            pixel.0 != midway.0 || pixel.1 != midway.1,
            "округления не было"
        );
        assert!((pixel.0 - pixel.0.round()).abs() < 1e-6);
        assert!((pixel.1 - pixel.1.round()).abs() < 1e-6);
        assert!((0.0..=3.0).contains(&pixel.0));
        assert!((0.0..=3.0).contains(&pixel.1));
    }

    #[test]
    fn draw_cell_is_a_noop_outside_the_pixel_theme() {
        for step in 0..=10 {
            let value = step as f32 / 10.0;
            assert_eq!(draw_cell((value, value), false), (value, value));
        }
        assert_eq!(draw_cell((2.4, 1.6), true), (2.0, 2.0));
    }

    #[test]
    fn nothing_is_drawn_before_the_first_update() {
        let frame = Frame::new(ms(0));
        assert!(frame.pos(ms(0), false).is_none());
        assert!(!frame.animating(ms(0)));
    }

    fn path(name: &str) -> PathBuf {
        PathBuf::from(format!("/wallpapers/{name}"))
    }

    #[test]
    fn a_thumb_ready_at_open_shows_right_away() {
        let fade = Fade::new();
        let settings = AnimSettings::default();

        assert!((fade.alpha(&path("a.jpg"), ms(0), settings) - 1.0).abs() < 1e-6);
        assert!(
            !fade.animating(ms(0), settings),
            "готовые при открытии не должны держать цикл"
        );
    }

    #[test]
    fn a_late_thumb_ramps_from_zero() {
        let settings = AnimSettings::default();
        let mut fade = Fade::new();
        fade.begin(&path("b.jpg"), ms(50));

        assert!((fade.alpha(&path("b.jpg"), ms(50), settings) - 0.0).abs() < 1e-6);
        assert!(fade.animating(ms(50), settings));

        let midway = fade.alpha(&path("b.jpg"), ms(130), settings);
        assert!(midway > 0.0 && midway < 1.0, "проявление идёт: {midway}");
        assert!((fade.alpha(&path("b.jpg"), ms(210), settings) - 1.0).abs() < 1e-6);
        assert!(!fade.animating(ms(210), settings));
    }

    #[test]
    fn the_fade_runs_for_the_declared_time() {
        let settings = AnimSettings::default();
        let mut fade = Fade::new();
        fade.begin(&path("c.jpg"), ms(0));

        assert!(
            fade.animating(ms(0) + THUMB_FADE - Duration::from_millis(1), settings),
            "проявление не должно кончаться раньше 160 мс"
        );
        assert!(!fade.animating(ms(0) + THUMB_FADE, settings));
    }

    #[test]
    fn disabled_animations_show_late_thumbs_right_away() {
        let settings = AnimSettings::new(false, 1.0);
        let mut fade = Fade::new();
        fade.begin(&path("d.jpg"), ms(0));

        assert!((fade.alpha(&path("d.jpg"), ms(0), settings) - 1.0).abs() < 1e-6);
        assert!(!fade.animating(ms(0), settings));
    }

    #[test]
    fn beginning_the_same_path_twice_keeps_the_first_start() {
        let settings = AnimSettings::default();
        let mut fade = Fade::new();
        fade.begin(&path("e.jpg"), ms(0));
        fade.begin(&path("e.jpg"), ms(100));

        assert!(fade.alpha(&path("e.jpg"), ms(100), settings) < 1.0);
        assert!((fade.alpha(&path("e.jpg"), ms(200), settings) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_changed_folder_clears_the_map() {
        let mut fade = Fade::new();
        for index in 0..5 {
            fade.begin(&path(&format!("{index}.jpg")), ms(0));
        }
        assert_eq!(fade.len(), 5);

        fade.clear();

        assert!(fade.is_empty());
        assert!((fade.alpha(&path("0.jpg"), ms(0), AnimSettings::default()) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_map_does_not_grow_past_its_cap() {
        let mut fade = Fade::new();
        for index in 0..(FADE_CAPACITY * 3) {
            fade.begin(&path(&format!("{index}.jpg")), ms(0));
        }

        assert!(
            fade.len() <= FADE_CAPACITY,
            "карта выросла до {} при потолке {FADE_CAPACITY}",
            fade.len()
        );
    }
}

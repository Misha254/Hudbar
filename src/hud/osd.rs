//! OSD громкости, микрофона и яркости: своё окно поверх всего вместо
//! уведомления dunst.
//!
//! Источник изменений один — состояние панели: громкость меняется из панели,
//! из меню, клавишами или вообще сторонним `wpctl` из терминала, яркость —
//! клавишами, ползунком или записью в sysfs, и в панель всё это приходит одним
//! и тем же опросом. Поэтому OSD не подписывается на действия и не требует IPC
//! между процессами: детектор сравнивает снимок значений между кадрами и
//! показывает окно, когда значение отличается от предыдущего. Такой источник
//! честнее подписки на кнопки — окно появляется и при внешнем изменении, а не
//! только при нажатии в панели.
//!
//! Первое наблюдение — базовая линия, а не событие: иначе оверлей мигал бы при
//! каждом старте панели.

use std::time::{Duration, Instant};

/// Что показывает оверлей. Вид важен для решения «показать или скрыть» и для
/// полосы при mute: подписи у полоски нет, и текст на экран не выводится.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsdKind {
    Volume,
    Mic,
    Brightness,
}

/// Значения, которые умеет показывать оверлей, в один момент времени: ровно те,
/// что видит панель.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OsdStamp {
    pub vol: u8,
    pub vol_muted: bool,
    pub mic: u8,
    pub mic_muted: bool,
    /// Значения прочитались хотя бы раз. Пока флаг снят, число — заглушка, а
    /// не состояние: первый успешный опрос не должен выглядеть как нажатие.
    pub vol_known: bool,
    pub mic_known: bool,
    /// `None` — подсветки нет или значение ещё не прочитано.
    pub brightness: Option<u8>,
}

impl OsdStamp {
    /// Уровень и признак «заглушено» для конкретного вида. Яркость не глушат,
    /// поэтому её полоса всегда рисуется акцентом.
    fn level(self, kind: OsdKind) -> (u8, bool) {
        match kind {
            OsdKind::Volume => (self.vol, self.vol_muted),
            OsdKind::Mic => (self.mic, self.mic_muted),
            OsdKind::Brightness => (self.brightness.unwrap_or(0), false),
        }
    }

    /// Какой вид изменился относительно `other`.
    ///
    /// Сравниваются только два прочитанных значения: первый успешный опрос —
    /// не нажатие клавиши. Одновременная смена нескольких — редкое состояние
    /// гонки двух опросов; при нём показывается громкость, чтобы не терять
    /// порядок вызовов, затем микрофон и только потом яркость.
    pub fn changed_from(self, other: Self) -> Option<OsdKind> {
        if self.vol_known
            && other.vol_known
            && (self.vol != other.vol || self.vol_muted != other.vol_muted)
        {
            Some(OsdKind::Volume)
        } else if self.mic_known
            && other.mic_known
            && (self.mic != other.mic || self.mic_muted != other.mic_muted)
        {
            Some(OsdKind::Mic)
        } else if brightness_changed(self.brightness, other.brightness) {
            Some(OsdKind::Brightness)
        } else {
            None
        }
    }
}

/// Сравниваются только два известных значения.
///
/// Появление устройства (`None` → `Some`) и его пропажа — не нажатие клавиши:
/// иначе оверлей всплывал бы при каждом старте панели, пока поток яркости ещё
/// не прочитал sysfs.
fn brightness_changed(current: Option<u8>, other: Option<u8>) -> bool {
    matches!((current, other), (Some(current), Some(other)) if current != other)
}

/// Состояние оверлея для отрисовки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OsdSnapshot {
    pub kind: OsdKind,
    pub value: u8,
    pub muted: bool,
    pub until: Instant,
}

impl OsdSnapshot {
    pub fn visible(&self, now: Instant) -> bool {
        now < self.until
    }
}

/// Превращает поток снимков в решение «показать или скрыть».
///
/// Держит одну базовую линию и один активный снимок: панель опрашивает звук
/// постоянно, поэтому состояние сравнивается здесь, а не запоминается в
/// вызывающем коде.
pub struct OsdDetector {
    base: Option<OsdStamp>,
    shown: Option<OsdSnapshot>,
}

impl OsdDetector {
    pub fn new() -> Self {
        Self {
            base: None,
            shown: None,
        }
    }

    /// Вызывается каждый кадр. Возвращает снимок, который надо нарисовать, либо
    /// `None`, когда оверлей скрыт.
    pub fn poll(
        &mut self,
        stamp: OsdStamp,
        now: Instant,
        duration: Duration,
    ) -> Option<OsdSnapshot> {
        match self.base {
            None => {
                self.base = Some(stamp);
                return None;
            }
            Some(base) => {
                if let Some(kind) = stamp.changed_from(base) {
                    self.base = Some(stamp);
                    let (value, muted) = stamp.level(kind);
                    self.shown = Some(OsdSnapshot {
                        kind,
                        value,
                        muted,
                        until: now + duration,
                    });
                }
            }
        }
        if !self.shown.is_some_and(|shown| shown.visible(now)) {
            self.shown = None;
        }
        self.shown
    }
}

impl Default for OsdDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::settings::Settings;

    fn stamp(vol: u8, vol_muted: bool, mic: u8, mic_muted: bool) -> OsdStamp {
        OsdStamp {
            vol,
            vol_muted,
            mic,
            mic_muted,
            vol_known: true,
            mic_known: true,
            brightness: Some(60),
        }
    }

    /// Тот же снимок, но с другой яркостью: остальные значения на месте.
    fn bright(value: u8) -> OsdStamp {
        OsdStamp {
            brightness: Some(value),
            ..quiet()
        }
    }

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn quiet() -> OsdStamp {
        stamp(40, false, 70, false)
    }

    /// Длительность по умолчанию берётся из настроек, чтобы тест и панель
    /// смотрели на одно и то же число.
    fn default_duration() -> Duration {
        Settings::default().osd_duration()
    }

    #[test]
    fn first_observation_is_a_baseline_and_shows_nothing() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();

        assert_eq!(detector.poll(quiet(), now, default_duration()), None);
    }

    #[test]
    fn unchanged_values_keep_the_overlay_invisible() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());

        for step in 0..5 {
            assert_eq!(
                detector.poll(quiet(), at(now, step * 100), default_duration()),
                None,
                "шаг {step}: без изменений оверлей появляться не должен"
            );
        }
    }

    #[test]
    fn volume_change_shows_the_new_value() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());

        let shown = detector
            .poll(
                stamp(65, false, 70, false),
                at(now, 100),
                default_duration(),
            )
            .expect("изменение громкости должно показать оверлей");

        assert_eq!(shown.kind, OsdKind::Volume);
        assert_eq!(shown.value, 65);
        assert!(!shown.muted);
    }

    #[test]
    fn mute_toggle_counts_as_a_change() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());

        let shown = detector
            .poll(stamp(40, true, 70, false), at(now, 100), default_duration())
            .expect("переключение mute должно показать оверлей");

        assert_eq!(shown.kind, OsdKind::Volume);
        assert!(shown.muted);
    }

    #[test]
    fn mic_change_shows_the_mic_overlay() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());

        let shown = detector
            .poll(stamp(40, false, 12, true), at(now, 100), default_duration())
            .expect("изменение микрофона должно показать оверлей");

        assert_eq!(shown.kind, OsdKind::Mic);
        assert_eq!(shown.value, 12);
        assert!(shown.muted);
    }

    #[test]
    fn simultaneous_change_prefers_volume() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());

        let shown = detector
            .poll(stamp(80, false, 5, false), at(now, 100), default_duration())
            .expect("смена обоих значений всё равно что-то показывает");

        assert_eq!(shown.kind, OsdKind::Volume);
        assert_eq!(shown.value, 80);
    }

    #[test]
    fn overlay_hides_when_the_duration_is_over() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());
        // Дальше значение держим: возврат к прежнему — отдельное изменение.
        let loud = stamp(65, false, 70, false);
        detector.poll(loud, at(now, 100), default_duration());

        assert!(
            detector
                .poll(loud, at(now, 100 + 1599), default_duration())
                .is_some(),
            "до истечения оверлей виден"
        );
        assert_eq!(
            detector.poll(loud, at(now, 100 + 1600), default_duration()),
            None,
            "на границе срока оверлей скрывается"
        );
    }

    #[test]
    fn a_later_change_shows_the_overlay_again() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());
        detector.poll(
            stamp(65, false, 70, false),
            at(now, 100),
            default_duration(),
        );

        let late = at(now, 5000);
        let shown = detector
            .poll(stamp(20, false, 70, false), late, default_duration())
            .expect("новое изменение после паузы снова показывает оверлей");

        assert_eq!(shown.value, 20);
    }

    #[test]
    fn returning_to_the_same_value_shows_the_overlay_again() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());
        detector.poll(
            stamp(65, false, 70, false),
            at(now, 100),
            default_duration(),
        );

        let late = at(now, 5000);
        let shown = detector
            .poll(quiet(), late, default_duration())
            .expect("возврат к прежнему значению — тоже изменение");

        assert_eq!(shown.value, 40);
    }

    #[test]
    fn zero_duration_never_produces_a_visible_overlay() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, Duration::ZERO);

        assert_eq!(
            detector.poll(stamp(65, false, 70, false), at(now, 100), Duration::ZERO),
            None
        );
    }

    #[test]
    fn brightness_change_shows_the_brightness_overlay() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());

        let shown = detector
            .poll(bright(85), at(now, 100), default_duration())
            .expect("изменение яркости должно показать оверлей");

        assert_eq!(shown.kind, OsdKind::Brightness);
        assert_eq!(shown.value, 85);
        assert!(!shown.muted, "яркость не глушат");
    }

    #[test]
    fn the_first_successful_read_is_not_a_key_press() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        let unknown = OsdStamp {
            vol: 0,
            vol_muted: false,
            mic: 0,
            mic_muted: false,
            vol_known: false,
            mic_known: false,
            brightness: Some(60),
        };
        detector.poll(unknown, now, default_duration());

        assert_eq!(
            detector.poll(quiet(), at(now, 100), default_duration()),
            None,
            "нули до первого опроса — не состояние, а отсутствие данных"
        );
    }

    #[test]
    fn the_device_appearing_is_not_a_key_press() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        let unknown = OsdStamp {
            brightness: None,
            ..quiet()
        };
        detector.poll(unknown, now, default_duration());

        for step in 0..3 {
            assert_eq!(
                detector.poll(bright(60), at(now, step * 100), default_duration()),
                None,
                "первое прочтение яркости — не событие"
            );
        }
    }

    #[test]
    fn losing_the_device_is_not_a_key_press() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());

        let gone = OsdStamp {
            brightness: None,
            ..quiet()
        };

        assert_eq!(
            detector.poll(gone, at(now, 100), default_duration()),
            None,
            "пропажа подсветки не должна показывать яркость"
        );
    }

    #[test]
    fn a_volume_change_wins_over_brightness() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());

        let both = OsdStamp {
            vol: 90,
            brightness: Some(90),
            ..quiet()
        };

        let shown = detector
            .poll(both, at(now, 100), default_duration())
            .expect("смена обоих значений всё равно что-то показывает");

        assert_eq!(shown.kind, OsdKind::Volume);
    }

    #[test]
    fn every_kind_is_distinct() {
        assert_ne!(OsdKind::Volume, OsdKind::Mic);
        assert_ne!(OsdKind::Mic, OsdKind::Brightness);
        assert_ne!(OsdKind::Volume, OsdKind::Brightness);
    }
}

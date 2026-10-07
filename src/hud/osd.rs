//! OSD громкости и микрофона: своё окно поверх всего вместо уведомления.
//!
//! Источник изменений один — состояние панели: громкость меняется из панели,
//! из меню, клавишами или вообще сторонним `wpctl` из терминала, и в панель всё
//! это приходит одним и тем же опросом. Поэтому OSD не подписывается на
//! действия и не требует IPC между процессами: детектор сравнивает снимок
//! значений между кадрами и показывает окно, когда значение отличается от
//! предыдущего. Такой источник честнее подписки на кнопки — окно появляется и
//! при внешнем изменении, а не только при нажатии в панели.
//!
//! Первое наблюдение — базовая линия, а не событие: иначе оверлей мигал бы при
//! каждом старте панели.

use std::time::{Duration, Instant};

use super::settings::Language;

/// Что показывает оверлей.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsdKind {
    Volume,
    Mic,
}

impl OsdKind {
    pub fn label(self, language: Language) -> &'static str {
        match (language, self) {
            (Language::Ru, Self::Volume) => "Громкость",
            (Language::Ru, Self::Mic) => "Микрофон",
            (Language::En, Self::Volume) => "Volume",
            (Language::En, Self::Mic) => "Microphone",
        }
    }
}

/// Значения звука в один момент времени: ровно те, что видит панель.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioStamp {
    pub vol: u8,
    pub vol_muted: bool,
    pub mic: u8,
    pub mic_muted: bool,
}

impl AudioStamp {
    fn audio(self, kind: OsdKind) -> (u8, bool) {
        match kind {
            OsdKind::Volume => (self.vol, self.vol_muted),
            OsdKind::Mic => (self.mic, self.mic_muted),
        }
    }

    /// Какой вид звука изменился относительно `other`.
    ///
    /// Одновременная смена обоих — редкое состояние гонки двух опросов; при
    /// нём показывается громкость, чтобы не терять порядок вызовов.
    pub fn changed_from(self, other: Self) -> Option<OsdKind> {
        if self.vol != other.vol || self.vol_muted != other.vol_muted {
            Some(OsdKind::Volume)
        } else if self.mic != other.mic || self.mic_muted != other.mic_muted {
            Some(OsdKind::Mic)
        } else {
            None
        }
    }
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
    base: Option<AudioStamp>,
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
        stamp: AudioStamp,
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
                    let (value, muted) = stamp.audio(kind);
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

    /// Жив ли оверлей прямо сейчас. Главный цикл держит по этому признаку
    /// короткий интервал ожидания, иначе скрытие задерживалось бы на тик.
    pub fn active(&self, now: Instant) -> bool {
        self.shown.is_some_and(|shown| shown.visible(now))
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

    fn stamp(vol: u8, vol_muted: bool, mic: u8, mic_muted: bool) -> AudioStamp {
        AudioStamp {
            vol,
            vol_muted,
            mic,
            mic_muted,
        }
    }

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn quiet() -> AudioStamp {
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
        assert!(!detector.active(now));
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
        assert!(!detector.active(at(now, 100 + 1600)));
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
        assert!(detector.active(late));
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
        assert!(detector.active(late));
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
        assert!(!detector.active(at(now, 100)));
    }

    #[test]
    fn active_follows_the_deadline_without_polling() {
        let mut detector = OsdDetector::new();
        let now = Instant::now();
        detector.poll(quiet(), now, default_duration());
        detector.poll(
            stamp(65, false, 70, false),
            at(now, 100),
            default_duration(),
        );

        assert!(detector.active(at(now, 100)));
        assert!(detector.active(at(now, 1699)));
        assert!(
            !detector.active(at(now, 1700)),
            "признак живого оверлера обязан выключиться на границе"
        );
    }

    #[test]
    fn labels_follow_the_panel_language() {
        assert_eq!(OsdKind::Volume.label(Language::Ru), "Громкость");
        assert_eq!(OsdKind::Mic.label(Language::Ru), "Микрофон");
        assert_eq!(OsdKind::Volume.label(Language::En), "Volume");
        assert_eq!(OsdKind::Mic.label(Language::En), "Microphone");
    }
}

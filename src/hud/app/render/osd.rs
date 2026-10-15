//! Окно громкости и микрофона.
//!
//! Поверхность тянется по всей ширине вывода, а карточка центрируется при
//! отрисовке: так ширина не зависит от того, сколько места занял текст, и
//! окно не прыгает при смене подписи. Слой `Overlay` — OSD обязан быть поверх
//! полноэкранных окон, иначе его не видно там, где он нужен.
//!
//! Решение «показать или скрыть» принимает `osd::OsdDetector`: здесь только
//! поверхность и рисунок.
//!
//! Поверхность живёт, пока оверлей активен. Смена громкости, mute или вида
//! обновляет содержимое существующего окна, а не создаёт слой заново: создание
//! и уничтожение layer-shell на каждое нажатие давало мигание. Слой снимается
//! только один раз — когда доиграла анимация закрытия.

use smithay_client_toolkit::{
    compositor::Region,
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer},
    },
};

use crate::hud::osd::{AudioStamp, OsdSnapshot};

use super::*;

/// Одинаковое ли содержимое у двух снимков: вид, уровень и mute. Срок показа
/// не входит — он обновляется при каждом нажатии, и перерисовывать из-за него
/// нечего.
fn same_content(a: &OsdSnapshot, b: &OsdSnapshot) -> bool {
    a.kind == b.kind && a.value == b.value && a.muted == b.muted
}

/// Нужен ли новый слой: только при первом показе. Всё остальное — смена
/// содержимое, повторное нажатие и закрытие — делается в живом окне.
pub fn needs_surface(current: Option<&OsdSnapshot>, next: Option<&OsdSnapshot>) -> bool {
    current.is_none() && next.is_some()
}

/// Куда идёт проявление карточки: 1 — открыта, 0 — закрыта.
pub fn openness_target(visible: bool) -> f32 {
    if visible { 1.0 } else { 0.0 }
}

/// Надо ли запускать закрытие заново. Пока цель не ноль, закрытие уже идёт:
/// `sync_osd` зовётся каждый тик, и повторный retarget сдвинул бы отсчёт с
/// нуля — карточка ползла бы к нулю бесконечно медленно и `done()` не наступило
/// бы никогда.
pub fn close_needs_start(current_target: f32) -> bool {
    current_target != 0.0
}

/// Что делать с поверхностью по результатам сверки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsdAction {
    /// Ничего: окна нет и показывать нечего.
    Idle,
    /// Создать слой и показать карточку.
    Open,
    /// Обновить содержимое живого окна.
    Update,
    /// Закрытие уже идёт или только начинается.
    Close,
    /// Закрытие доиграло — снять слой.
    Destroy,
}

/// Решение принимается целиком здесь, а `sync_osd` только исполняет его. Так
/// порядок «сначала обновить содержимое, потом гасить» виден в одном месте и
/// проверяется без Wayland.
pub fn decide(
    current: Option<&OsdSnapshot>,
    next: Option<&OsdSnapshot>,
    enabled: bool,
    close_finished: bool,
) -> OsdAction {
    if !enabled {
        // Выключенный OSD не анимируется: слой снимается сразу.
        return if current.is_some() {
            OsdAction::Destroy
        } else {
            OsdAction::Idle
        };
    }
    if needs_surface(current, next) {
        return OsdAction::Open;
    }
    if current.is_none() {
        return OsdAction::Idle;
    }
    match next {
        Some(_) => OsdAction::Update,
        None if close_finished => OsdAction::Destroy,
        None => OsdAction::Close,
    }
}

/// Смещение карточки вверх от её места в открытом состоянии. При `openness` 1
/// карточка на месте, при 0 уехала на весь `OSD_SLIDE_PX`.
pub fn card_offset(openness: f32) -> f32 {
    -OSD_SLIDE_PX * (1.0 - openness.clamp(0.0, 1.0))
}

/// Положение карточки от верхнего края поверхности.
pub fn card_y(openness: f32) -> f32 {
    OSD_CARD_TOP + card_offset(openness)
}

/// Доля заполнения полосы уровня.
pub fn fill_fraction(value: u8) -> f32 {
    (value as f32 / 100.0).clamp(0.0, 1.0)
}

/// Верхний край поверхности OSD. Высоту панели вычитаем, а экранную позицию
/// карточки держим прежней: margin считается от неё, а не наоборот.
pub fn osd_margin_top(bar_height: u32) -> i32 {
    bar_height as i32 + OSD_CARD_SCREEN_OFFSET - OSD_CARD_TOP as i32
}

impl App {
    /// Сверяет состояние звука с прошлым кадром и ведёт окно: показывает,
    /// обновляет содержимое или закрывает.
    pub fn sync_osd(&mut self, qh: &QueueHandle<Self>) {
        let stamp = {
            let g = crate::hud::lock::mutex(&self.shared.sys);
            AudioStamp {
                vol: g.vol,
                vol_muted: g.vol_muted,
                mic: g.mic,
                mic_muted: g.mic_muted,
            }
        };
        // Детектор кормится даже при выключенном OSD: иначе после включения
        // первым изменением окажется значение, накопленное пока оверлея не было.
        let snapshot = self
            .osd_detector
            .poll(stamp, self.now, self.settings.osd_duration());
        let enabled = self.settings.osd;
        let wanted = if enabled { snapshot } else { None };
        let current = self.osd.as_ref().map(|osd| &osd.snapshot);
        let action = decide(
            current,
            wanted.as_ref(),
            enabled,
            self.osd_anim.to == 0.0 && self.osd_anim.done(self.now),
        );

        match action {
            OsdAction::Idle => return,
            OsdAction::Open => {
                if let Some(next) = wanted {
                    self.create_osd(qh, next);
                    self.shared.dirty.store(true, Ordering::Relaxed);
                }
                return;
            }
            OsdAction::Update => {
                let next = wanted.expect("решение Update требует снимка");
                // Окно живо: меняется только содержимое. Пересоздавать слой на
                // каждое нажатие было причиной мигания.
                let changed = current.is_none_or(|current| !same_content(current, &next));
                if let Some(osd) = self.osd.as_mut() {
                    osd.snapshot = next;
                }
                // Звук мог сработать, пока карточка ещё уходила: разворачиваем
                // её от текущего положения, слой остаётся прежним.
                if !self.osd_anim.done(self.now) && self.osd_anim.to == 0.0 {
                    self.start_osd_open();
                }
                self.retarget_osd_fill();
                if changed {
                    self.shared.dirty.store(true, Ordering::Relaxed);
                }
                return;
            }
            OsdAction::Close => self.start_osd_close(),
            OsdAction::Destroy => self.destroy_osd(),
        }
        self.shared.dirty.store(true, Ordering::Relaxed);
    }

    /// Жив ли оверлей прямо сейчас: главный цикл держит по этому признаку
    /// средний интервал ожидания, иначе окно висело бы дольше срока.
    pub fn osd_alive(&self) -> bool {
        self.osd.is_some()
    }

    /// Проявление карточки с учётом темы: в Pixel-теме значение идёт ступенями,
    /// иначе рамка полупрозрачной и не вяжется с остальным рисунком.
    pub fn osd_openness(&self) -> f32 {
        let value = self.osd_anim.value(self.now);
        if self.pixel_mode() {
            return crate::hud::anim::quantize(value, PIXEL_ANIM_STEPS);
        }
        value
    }

    /// Заполнение полосы уровня в текущий момент.
    pub fn osd_fill_value(&self) -> f32 {
        self.osd_fill.value(self.now).clamp(0.0, 1.0)
    }

    /// Появление идёт от текущего значения: если окно ещё закрывалось, оно
    /// разворачивается, а не мигает. Полоса при первом показе встаёт на
    /// актуальное значение сразу — заезд из нуля читался бы как ошибка.
    fn start_osd_open(&mut self) {
        let now = self.now;
        let mut tween = self.osd_anim;
        tween.retarget(now, openness_target(true));
        tween.duration = self.anim.scaled(OSD_OPEN);
        tween.easing = anim::ease_out_cubic;
        self.osd_anim = tween;
    }

    fn start_osd_close(&mut self) {
        if !close_needs_start(self.osd_anim.to) {
            return;
        }
        let now = self.now;
        let mut tween = self.osd_anim;
        tween.retarget(now, openness_target(false));
        tween.duration = self.anim.scaled(OSD_CLOSE);
        tween.easing = anim::ease_in_cubic;
        self.osd_anim = tween;
    }

    /// Догоняет полосу за новым уровнем. Повтор того же значения движение не
    /// запускает — иначе цикл держался бы на коротком тике вхолостую.
    fn retarget_osd_fill(&mut self) {
        let Some(osd) = self.osd.as_ref() else {
            return;
        };
        let target = fill_fraction(osd.snapshot.value);
        let now = self.now;
        if (self.osd_fill.value(now) - target).abs() < f32::EPSILON {
            return;
        }
        let mut tween = self.osd_fill;
        tween.retarget(now, target);
        tween.duration = self.anim.scaled(OSD_FILL);
        tween.easing = anim::ease_out_cubic;
        self.osd_fill = tween;
    }

    pub fn destroy_osd(&mut self) {
        if self.osd.take().is_none() {
            return;
        }
        crate::hud::log::trace("слой уничтожен: OSD");
        self.osd_anim = anim::Tween::settled(0.0, self.now);
        self.osd_fill = anim::Tween::settled(0.0, self.now);
        self.shared.dirty.store(true, Ordering::Relaxed);
    }

    pub fn create_osd(&mut self, qh: &QueueHandle<Self>, snapshot: OsdSnapshot) {
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("hudbar-osd"),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(0, OSD_H as u32);
        layer.set_margin(osd_margin_top(self.settings.height), 0, 0, 0);
        layer.set_exclusive_zone(-1);
        // Пустой input-region: окно не перехватывает мышь под собой.
        if let Ok(region) = Region::new(&self.compositor) {
            layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        layer.commit();
        crate::hud::log::trace("слой создан: OSD");
        self.osd_anim = anim::Tween::settled(0.0, self.now);
        self.osd_fill = anim::Tween::settled(fill_fraction(snapshot.value), self.now);
        self.osd = Some(Osd {
            layer,
            configured: false,
            size: (self.width, OSD_H as u32),
            snapshot,
        });
        self.start_osd_open();
    }

    pub fn draw_osd(&mut self) {
        let Some((configured, size, snapshot)) = self
            .osd
            .as_ref()
            .map(|osd| (osd.configured, osd.size, osd.snapshot))
        else {
            return;
        };
        if !configured || size.0 == 0 || size.1 == 0 {
            return;
        }
        // Альфа и положение читаются до создания буфера: буфер берёт `self`
        // взаймы, и после этого `self` уже нельзя читать.
        let openness = self.osd_openness();
        let filled = self.osd_fill_value();
        let p = self.palette;
        let pw = (size.0 as f32 * SCALE) as u32;
        let ph = (size.1 as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        let bw = 1.0 * SCALE;
        let card_x = ((size.0 as f32 - OSD_CARD_W) / 2.0).max(0.0);
        let cx = card_x * SCALE;
        let cy = card_y(openness) * SCALE;
        let cw = OSD_CARD_W * SCALE;
        let ch = OSD_CARD_H * SCALE;

        let border = p.primary.with_a(0.35 * openness);
        fill_rect(&mut pixmap, cx, cy, cw, ch, p.base.with_a(0.98 * openness));
        fill_rect(&mut pixmap, cx, cy, cw, bw, border);
        fill_rect(&mut pixmap, cx, cy + ch - bw, cw, bw, border);
        fill_rect(&mut pixmap, cx, cy, bw, ch, border);
        fill_rect(&mut pixmap, cx + cw - bw, cy, bw, ch, border);

        let label = snapshot.kind.label(self.settings.language);
        let percent = format!("{}%", snapshot.value);
        let text_y = cy + OSD_PAD * SCALE;
        self.draw_text(
            &mut pixmap,
            label,
            SIZE,
            p.secondary.with_a(openness),
            cx + OSD_PAD * SCALE,
            text_y,
            OSD_TEXT_LINE_H,
        );
        // Процент прижат к правому краю карточки: при mute он остаётся на месте,
        // а полоса ниже показывает, что уровень есть, но звука нет.
        let percent_w = self.text_width(&percent, SIZE);
        self.draw_text(
            &mut pixmap,
            &percent,
            SIZE,
            p.text.with_a(openness),
            cx + cw - OSD_PAD * SCALE - percent_w,
            text_y,
            OSD_TEXT_LINE_H,
        );

        // Полоса уровня.
        let bar_x = cx + OSD_PAD * SCALE;
        let bar_w = cw - OSD_PAD * SCALE * 2.0;
        let bar_h = OSD_BAR_H * SCALE;
        let bar_y = cy + ch - OSD_PAD * SCALE - bar_h;
        fill_rect(
            &mut pixmap,
            bar_x,
            bar_y,
            bar_w,
            bar_h,
            p.base.with_a(0.6 * openness),
        );
        let fill_w = bar_w * filled;
        let fill = if snapshot.muted {
            p.secondary.with_a(0.5 * openness)
        } else {
            p.primary.with_a(openness)
        };
        if fill_w > 0.0 {
            fill_rect(&mut pixmap, bar_x, bar_y, fill_w, bar_h, fill);
        }

        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
        else {
            return;
        };
        let (dst_chunks, _) = canvas.as_chunks_mut::<4>();
        let (src_chunks, _) = pixmap.data().as_chunks::<4>();
        for (dst, src) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
        if let Some(osd) = self.osd.as_ref() {
            let surface = osd.layer.wl_surface();
            surface.set_buffer_scale(SCALE as i32);
            surface.damage_buffer(0, 0, pw as i32, ph as i32);
            let _ = buffer.attach_to(surface);
            osd.layer.commit();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use super::*;
    use crate::hud::anim::Tween;
    use crate::hud::osd::{OsdKind, OsdSnapshot};

    fn base() -> Instant {
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now)
    }

    fn ms(value: u64) -> Instant {
        base() + Duration::from_millis(value)
    }

    fn snapshot(kind: OsdKind, value: u8, muted: bool, in_ms: u64) -> OsdSnapshot {
        OsdSnapshot {
            kind,
            value,
            muted,
            until: ms(in_ms),
        }
    }

    #[test]
    fn the_same_content_does_not_need_a_new_surface() {
        let a = snapshot(OsdKind::Volume, 65, false, 1000);
        let b = snapshot(OsdKind::Volume, 65, false, 1600);

        assert!(
            same_content(&a, &b),
            "срок показа не входит в содержимое: перерисовывать незачем"
        );
        assert!(
            !needs_surface(Some(&a), Some(&b)),
            "повторное нажатие той же клавиши не пересоздаёт окно"
        );
    }

    #[test]
    fn a_new_value_updates_the_live_window_instead_of_recreating_it() {
        let base = snapshot(OsdKind::Volume, 65, false, 1000);
        for next in [
            snapshot(OsdKind::Volume, 40, false, 1000),
            snapshot(OsdKind::Mic, 65, false, 1000),
            snapshot(OsdKind::Volume, 65, true, 1000),
        ] {
            assert!(
                !same_content(&base, &next),
                "содержимое изменилось — обновлять надо"
            );
            assert!(
                !needs_surface(Some(&base), Some(&next)),
                "слой пересоздавать нельзя: это и было причиной мигания"
            );
        }
    }

    #[test]
    fn a_surface_is_only_needed_for_the_first_show() {
        let next = snapshot(OsdKind::Volume, 40, false, 1000);

        assert!(
            needs_surface(None, Some(&next)),
            "первый показ создаёт слой"
        );
        assert!(!needs_surface(None, None), "без показа создавать нечего");
        assert!(
            !needs_surface(Some(&next), None),
            "закрытие идёт в живом окне, а не через новый слой"
        );
    }

    #[test]
    fn openness_target_follows_visibility() {
        assert!((openness_target(true) - 1.0).abs() < 1e-6);
        assert!((openness_target(false) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn the_card_stands_still_when_fully_open() {
        assert!((card_offset(1.0) - 0.0).abs() < 1e-6);
        assert!((card_y(1.0) - OSD_CARD_TOP).abs() < 1e-6);
    }

    #[test]
    fn the_card_travels_exactly_the_slide_distance() {
        assert!((card_offset(0.0) + OSD_SLIDE_PX).abs() < 1e-6);
        assert!(
            (card_y(0.0) - (OSD_CARD_TOP - OSD_SLIDE_PX)).abs() < 1e-6,
            "закрытая карточка должна уехать на весь ход"
        );
        assert!(
            card_y(0.0) > 0.0,
            "закрытая карточка не должна уйти за верхний край поверхности"
        );
        assert!(
            OSD_H >= card_y(0.0) + OSD_CARD_H,
            "поверхности не хватит высоты на уехавшую карточку"
        );
    }

    #[test]
    fn card_offset_clamps_the_progress() {
        assert!((card_offset(1.4) - 0.0).abs() < 1e-6);
        assert!((card_offset(-0.3) + OSD_SLIDE_PX).abs() < 1e-6);
    }

    #[test]
    fn the_open_card_stays_on_the_same_screen_pixel() {
        // Прежний расклад: margin `height + 8`, карточка на 6 пикселей ниже.
        // После добавления запаса под выезд оба числа должны сложиться в то же.
        for height in [24u32, 27, 32, 48] {
            let old_screen_top = height as i32 + 8 + 6;
            let new_screen_top = osd_margin_top(height) + OSD_CARD_TOP as i32;
            assert_eq!(
                new_screen_top, old_screen_top,
                "высота панели {height}: карточка съехала"
            );
        }
    }

    #[test]
    fn the_level_bar_starts_at_its_actual_value() {
        assert!((fill_fraction(0) - 0.0).abs() < 1e-6);
        assert!((fill_fraction(40) - 0.4).abs() < 1e-6);
        assert!((fill_fraction(100) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_repeat_during_closing_turns_back_without_a_jump() {
        // Звук сменился, пока карточка ещё уходила: она разворачивается от
        // текущего положения, а не начинает проявление с нуля.
        let mut closing = Tween::new(0.0, 1.0, base(), OSD_OPEN, anim::ease_out_cubic);
        closing.retarget(ms(140), openness_target(false));
        closing.duration = OSD_CLOSE;
        closing.easing = anim::ease_in_cubic;

        let midway = closing.value(ms(195));
        assert!(midway > 0.0 && midway < 1.0, "закрытие идёт: {midway}");

        closing.retarget(ms(195), openness_target(true));

        assert!(
            (closing.from - midway).abs() < 1e-6,
            "прыжок вместо текущего значения: {} против {midway}",
            closing.from
        );
        assert!(
            (closing.value(ms(195)) - midway).abs() < 1e-6,
            "в момент повторного срабатывания значение не должно скакать"
        );
        assert!((closing.to - 1.0).abs() < 1e-6);
        assert!(
            !closing.done(ms(195)),
            "после разворота окно снова должно доехать до конца"
        );
    }

    #[test]
    fn the_level_bar_retargets_from_the_current_fill() {
        let mut tween = Tween::new(
            0.0,
            fill_fraction(40),
            base(),
            OSD_FILL,
            anim::ease_out_cubic,
        );
        let halfway = tween.value(ms(45));

        tween.retarget(ms(45), fill_fraction(80));

        assert!(
            (tween.from - halfway).abs() < 1e-6,
            "полоса не должна прыгать к новому уровню"
        );
        assert!((tween.value(ms(135)) - fill_fraction(80)).abs() < 1e-6);
    }

    /// Один тик главного цикла: `sync_osd` дёргает закрытие каждый раз, и
    /// повторный вызов на идущей анимации обязан быть no-op.
    fn close_tick(tween: &mut Tween, now: Instant) {
        if close_needs_start(tween.to) {
            tween.retarget(now, openness_target(false));
        }
        tween.duration = OSD_CLOSE;
        tween.easing = anim::ease_in_cubic;
    }

    #[test]
    fn repeated_close_decisions_still_finish_within_the_closing_time() {
        // `sync_osd` зовётся каждый тик и каждый раз решает, что окно надо
        // закрывать. Слой должен сняться в срок, а не висеть до бесконечности.
        // Что именно удерживает отсчёт от сдвига — проверка `Tween::retarget`
        // в `anim.rs`, здесь важно, что решение сходится к концу.
        let start = ms(140);
        let mut closing = Tween::new(0.0, 1.0, start, OSD_OPEN, anim::ease_out_cubic);

        let mut now = start;
        for _ in 0..100 {
            now += Duration::from_millis(16);
            close_tick(&mut closing, now);
        }

        assert!(
            closing.done(now),
            "окно должно сняться, а не висеть после тысячи тиков"
        );
        assert!((closing.value(now) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn the_closing_runs_out_in_its_own_time() {
        let start = ms(140);
        let mut closing = Tween::new(0.0, 1.0, start, OSD_OPEN, anim::ease_out_cubic);
        close_tick(&mut closing, ms(140));
        let began = ms(140);

        assert!(
            !closing.done(began),
            "в момент старта закрытие ещё не кончилось"
        );
        assert!(
            !closing.done(began + OSD_CLOSE - Duration::from_millis(1)),
            "закрытие не должно уходить раньше своих 110 мс"
        );
        assert!(
            closing.done(began + OSD_CLOSE),
            "через 110 мс закрытие закончено"
        );
    }

    #[test]
    fn a_running_close_is_not_started_again() {
        let closing = Tween::new(0.0, 1.0, base(), OSD_OPEN, anim::ease_out_cubic);
        let running = Tween {
            from: 0.4,
            to: 0.0,
            ..closing
        };
        let idle = Tween::settled(0.0, base());

        assert!(
            !close_needs_start(running.to),
            "идущее закрытие не перезапускают"
        );
        assert!(
            !close_needs_start(idle.to),
            "покойное закрытое окно не трогают"
        );
        assert!(
            close_needs_start(closing.to),
            "открытое окно надо закрывать"
        );
    }

    #[test]
    fn the_first_show_opens_and_the_rest_updates_the_live_window() {
        let live = snapshot(OsdKind::Volume, 65, false, 1000);
        let next = snapshot(OsdKind::Volume, 40, false, 2000);

        assert_eq!(
            decide(None, Some(&next), true, false),
            OsdAction::Open,
            "первый показ создаёт слой"
        );
        assert_eq!(
            decide(Some(&live), Some(&next), true, false),
            OsdAction::Update,
            "второй показ идёт в живое окно, а не создаёт новое"
        );
    }

    #[test]
    fn nothing_to_show_means_nothing_to_do() {
        let live = snapshot(OsdKind::Volume, 65, false, 1000);

        assert_eq!(decide(None, None, true, false), OsdAction::Idle);
        assert_eq!(
            decide(Some(&live), None, true, false),
            OsdAction::Close,
            "окно живо и показывать нечего — начинаем закрытие"
        );
    }

    #[test]
    fn the_layer_is_destroyed_only_after_the_closing_is_over() {
        let live = snapshot(OsdKind::Volume, 65, false, 1000);

        assert_eq!(decide(Some(&live), None, true, false), OsdAction::Close);
        assert_eq!(
            decide(Some(&live), None, true, true),
            OsdAction::Destroy,
            "слой снимается после анимации, а не до неё"
        );
    }

    #[test]
    fn a_disabled_osd_drops_the_layer_without_animation() {
        let live = snapshot(OsdKind::Volume, 65, false, 1000);
        let next = snapshot(OsdKind::Volume, 40, false, 2000);

        assert_eq!(
            decide(Some(&live), None, false, false),
            OsdAction::Destroy,
            "выключенный OSD не анимируется"
        );
        assert_eq!(decide(Some(&live), None, false, true), OsdAction::Destroy);
        assert_eq!(
            decide(None, Some(&next), false, false),
            OsdAction::Idle,
            "выключенный OSD не показывает ничего вовсе"
        );
    }
}

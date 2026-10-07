//! Окно громкости и микрофона.
//!
//! Поверхность тянется по всей ширине вывода, а карточка центрируется при
//! отрисовке: так ширина не зависит от того, сколько места занял текст, и
//! окно не прыгает при смене подписи. Слой `Overlay` — OSD обязан быть поверх
//! полноэкранных окон, иначе его не видно там, где он нужен.
//!
//! Решение «показать или скрыть» принимает `osd::OsdDetector`: здесь только
//! поверхность и рисунок. Один и тот же снимок рисуется, пока детектор держит
//! его живым, поэтому пересоздаётся поверхность только при смене содержимого.

use std::time::Instant;

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
/// не входит — он обновляется при каждом нажатии и пересоздавать окно из-за
/// него незачем.
fn same_content(a: &OsdSnapshot, b: &OsdSnapshot) -> bool {
    a.kind == b.kind && a.value == b.value && a.muted == b.muted
}

impl App {
    /// Сверяет состояние звука с прошлым кадром и создаёт или убирает окно.
    pub fn sync_osd(&mut self, qh: &QueueHandle<Self>) {
        let stamp = {
            let g = self.shared.sys.lock().unwrap();
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
            .poll(stamp, Instant::now(), self.settings.osd_duration());
        let wanted = if self.settings.osd { snapshot } else { None };

        // Сравнивается только содержимое: срок показа живёт в детекторе, и
        // повторное нажатие той же клавиши не должно пересоздавать поверхность.
        let same = match (&self.osd, wanted) {
            (Some(current), Some(next)) => same_content(&current.snapshot, &next),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.osd = None;
        if let Some(next) = wanted {
            self.create_osd(qh, next);
        }
        self.shared.dirty.store(true, Ordering::Relaxed);
    }

    /// Жив ли оверлей прямо сейчас: главный цикл держит по этому признаку
    /// короткий интервал ожидания, иначе окно висело бы дольше срока.
    pub fn osd_alive(&self) -> bool {
        self.osd.is_some()
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
        layer.set_margin(self.settings.height as i32 + 8, 0, 0, 0);
        layer.set_exclusive_zone(-1);
        // Пустой input-region: окно не перехватывает мышь под собой.
        if let Ok(region) = Region::new(&self.compositor) {
            layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        layer.commit();
        self.osd = Some(Osd {
            layer,
            configured: false,
            size: (self.width, OSD_H as u32),
            snapshot,
        });
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
        let p = self.palette;
        let pw = (size.0 as f32 * SCALE) as u32;
        let ph = (size.1 as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        let bw = 1.0 * SCALE;
        let card_x = ((size.0 as f32 - OSD_CARD_W) / 2.0).max(0.0);
        let card_y = ((size.1 as f32 - OSD_CARD_H) / 2.0).max(0.0);
        let cx = card_x * SCALE;
        let cy = card_y * SCALE;
        let cw = OSD_CARD_W * SCALE;
        let ch = OSD_CARD_H * SCALE;

        let border = p.primary.with_a(0.35);
        fill_rect(&mut pixmap, cx, cy, cw, ch, p.base.with_a(0.98));
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
            p.secondary,
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
            p.text,
            cx + cw - OSD_PAD * SCALE - percent_w,
            text_y,
            OSD_TEXT_LINE_H,
        );

        // Полоса уровня.
        let bar_x = cx + OSD_PAD * SCALE;
        let bar_w = cw - OSD_PAD * SCALE * 2.0;
        let bar_h = OSD_BAR_H * SCALE;
        let bar_y = cy + ch - OSD_PAD * SCALE - bar_h;
        fill_rect(&mut pixmap, bar_x, bar_y, bar_w, bar_h, p.base.with_a(0.6));
        let filled = bar_w * (snapshot.value as f32 / 100.0);
        let fill = if snapshot.muted {
            p.secondary.with_a(0.5)
        } else {
            p.primary
        };
        if filled > 0.0 {
            fill_rect(&mut pixmap, bar_x, bar_y, filled, bar_h, fill);
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

    use super::*;
    use crate::hud::osd::{OsdKind, OsdSnapshot};

    fn snapshot(kind: OsdKind, value: u8, muted: bool, ms: u64) -> OsdSnapshot {
        OsdSnapshot {
            kind,
            value,
            muted,
            until: Instant::now() + Duration::from_millis(ms),
        }
    }

    #[test]
    fn the_same_content_does_not_need_a_new_surface() {
        let a = snapshot(OsdKind::Volume, 65, false, 1000);
        let b = snapshot(OsdKind::Volume, 65, false, 1600);

        assert!(
            same_content(&a, &b),
            "срок показа не входит в содержимое: поверхность пересоздавать незачем"
        );
    }

    #[test]
    fn a_new_value_or_kind_needs_a_new_surface() {
        let base = snapshot(OsdKind::Volume, 65, false, 1000);

        assert!(!same_content(
            &base,
            &snapshot(OsdKind::Volume, 40, false, 1000)
        ));
        assert!(!same_content(
            &base,
            &snapshot(OsdKind::Mic, 65, false, 1000)
        ));
        assert!(!same_content(
            &base,
            &snapshot(OsdKind::Volume, 65, true, 1000)
        ));
    }
}

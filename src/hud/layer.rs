//! Общий plumbing layer-shell окна: поверхность, пул буферов, масштаб,
//! применение configure без нулевых размеров и доставка кадра в слой.
//!
//! Вынесено из `hud_menu.rs`, чтобы и меню, и wallpaper-picker строили
//! surface/pool/configure одинаково, без копирования.
//!
//! Здесь же живёт проявление окна: `openness` — твин 0→1, который после
//! первого configure ползёт к единице. Фон у окон прозрачный, поэтому картина
//! проявляется сама, без перерисовки содержимого.

use smithay_client_toolkit::{
    compositor::{CompositorState, SurfaceData},
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceData,
        },
    },
    shm::{Shm, slot::SlotPool},
};
use wayland_client::{Dispatch, QueueHandle, protocol::wl_surface};

use super::anim::{self, AnimSettings, PIXEL_ANIM_STEPS, Tween};

/// Длительность проявления окна.
pub const OPEN_DURATION: std::time::Duration = std::time::Duration::from_millis(120);

/// Запас под сдвиг карточки при появлении, в логических пикселях. Задаётся
/// вызывающим: слой у окон размером ровно с карточку, и сдвигать её некуда.
pub const DEFAULT_SLIDE_PX: f32 = 8.0;

/// Умножает все четыре канала уже нарисованной пик-карты на `alpha`.
///
/// Буфер премножающий, поэтому умножается не только альфа: иначе на краях
/// карточки, где цвет смешан с прозрачностью, проступил бы чёрный.
pub fn fade(pixmap: &mut tiny_skia::Pixmap, alpha: f32) {
    let factor = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    if factor >= 255 {
        return;
    }
    let (dst, _) = pixmap.data_mut().as_chunks_mut::<4>();
    for pixel in dst.iter_mut() {
        for channel in pixel.iter_mut() {
            *channel = ((*channel as u32 * factor) / 255) as u8;
        }
    }
}

/// Проявление с учётом темы: в Pixel-теме значение идёт ступенями, полупрозрачной
/// рамки в пиксельной графике быть не должно.
pub fn openness_steps(openness: f32, pixel: bool) -> f32 {
    if pixel {
        anim::quantize(openness, PIXEL_ANIM_STEPS)
    } else {
        openness.clamp(0.0, 1.0)
    }
}

/// Контекст живого слоя: та же поверхность на весь цикл, пул буферов и
/// параметры рендера, которые раньше висели на `MenuApp` по отдельности.
pub struct Context {
    pub layer: LayerSurface,
    pub pool: SlotPool,
    pub width: u32,
    pub height: u32,
    pub configured: bool,
    pub dirty: bool,
    pub scale: u32,
    /// Проявление окна: 0 — только что показалось, 1 — на месте.
    pub openness: Tween,
    /// Настройки анимаций из `settings.json`: окна читают их при старте.
    pub anim: AnimSettings,
}

impl Context {
    /// Запустить проявление. Вызывается на первом `configure`: раньше поверхность
    /// не нарисована и двигать нечего.
    pub fn start_open(&mut self, now: std::time::Instant) {
        let mut tween = self.openness;
        tween.retarget(now, 1.0);
        tween.duration = self.anim.scaled(OPEN_DURATION);
        tween.easing = anim::ease_out_cubic;
        self.openness = tween;
        self.dirty = true;
    }

    /// Текущая степень проявления с учётом темы.
    pub fn openness_now(&self, now: std::time::Instant, pixel: bool) -> f32 {
        openness_steps(self.openness.value(now), pixel)
    }

    /// Идёт ли проявление. Пока идёт, цикл обязан ставить dirty и просыпаться
    /// часто, иначе окно проявится ступенями по событиям ввода.
    pub fn animating(&self, now: std::time::Instant) -> bool {
        !self.openness.done(now)
    }

    /// Применяет configure от композитора. Нули отбрасываются: слой без
    /// якорей не имеет права получить нулевой размер — это ошибка протокола.
    /// Если после фильтра нашли ноль — берём рабочую просьбу модели.
    ///
    /// Первый configure запускает проявление: раньше поверхность ещё не
    /// нарисована и двигать нечего.
    pub fn apply_configure(
        &mut self,
        proposed: (u32, u32),
        fallback: (u32, u32),
        now: std::time::Instant,
    ) {
        let first = !self.configured;
        if proposed.0 > 0 {
            self.width = proposed.0;
        }
        if proposed.1 > 0 {
            self.height = proposed.1;
        }
        if self.width == 0 || self.height == 0 {
            self.width = fallback.0;
            self.height = fallback.1;
        }
        self.configured = true;
        self.dirty = true;
        if first {
            self.start_open(now);
        }
    }

    /// Запрос размера у композитора. Повторный `set_size` с той же величиной
    /// не шлём: лишний commit лишь трёт канал.
    pub fn request_size(&mut self, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        self.layer.set_size(width, height);
        self.layer.commit();
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::anim::{ease_out_cubic, quantize};
    use super::*;
    use crate::anim::PIXEL_ANIM_STEPS;

    fn base() -> std::time::Instant {
        static BASE: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(std::time::Instant::now)
    }

    fn ms(value: u64) -> std::time::Instant {
        base() + std::time::Duration::from_millis(value)
    }

    #[test]
    fn the_normal_theme_keeps_the_whole_range() {
        for step in 0..=10 {
            let openness = step as f32 / 10.0;
            assert!(
                (openness_steps(openness, false) - openness).abs() < 1e-6,
                "обычная тема не должна квантовать"
            );
        }
    }

    #[test]
    fn the_pixel_theme_snaps_to_the_shared_steps() {
        let want = quantize(0.6, PIXEL_ANIM_STEPS);
        assert!((openness_steps(0.6, true) - want).abs() < 1e-6);
        for step in 0..=PIXEL_ANIM_STEPS {
            let value = step as f32 / PIXEL_ANIM_STEPS as f32;
            assert!(
                (openness_steps(value, true) - value).abs() < 1e-6,
                "ступень {step} должна быть точной"
            );
        }
        // Полупрозрачных значений между ступенями не остаётся.
        assert!((openness_steps(0.4, true) - 0.5).abs() < 1e-6);
        assert!((openness_steps(0.1, true) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn openness_steps_clamps_the_progress() {
        assert!((openness_steps(1.5, false) - 1.0).abs() < 1e-6);
        assert!((openness_steps(-0.5, false) - 0.0).abs() < 1e-6);
        assert!((openness_steps(1.5, true) - 1.0).abs() < 1e-6);
        assert!((openness_steps(-0.5, true) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn the_opening_runs_for_the_declared_time() {
        // Твин строится вручную: `Context` без композитора не собрать, а
        // проверяется ровно та формула, что кладёт `start_open`.
        let build = |now: std::time::Instant| Tween {
            from: 0.0,
            to: 1.0,
            started: now,
            duration: AnimSettings::default().scaled(OPEN_DURATION),
            easing: ease_out_cubic,
        };

        let tween = build(base());
        assert!((tween.value(base()) - 0.0).abs() < 1e-6);
        assert!(!tween.done(base()));
        assert!(
            !tween.done(ms(OPEN_DURATION.as_millis() as u64 - 1)),
            "проявление не должно кончаться раньше 120 мс"
        );
        assert!(tween.done(ms(OPEN_DURATION.as_millis() as u64)));
        assert!((tween.value(ms(OPEN_DURATION.as_millis() as u64)) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn disabled_animations_open_the_window_at_once() {
        let now = base();
        let tween = Tween {
            from: 0.0,
            to: 1.0,
            started: now,
            duration: AnimSettings::new(false, 1.0).scaled(OPEN_DURATION),
            easing: ease_out_cubic,
        };

        assert!(tween.done(now), "выключенная анимация не должна тянуться");
        assert!(
            (tween.value(now) - 1.0).abs() < 1e-6,
            "окно открывается сразу"
        );
    }

    #[test]
    fn the_speed_setting_moves_the_opening_time() {
        let fast = AnimSettings::new(true, 2.0).scaled(OPEN_DURATION);
        let slow = AnimSettings::new(true, 0.5).scaled(OPEN_DURATION);

        assert_eq!(fast, std::time::Duration::from_millis(60));
        assert_eq!(slow, std::time::Duration::from_millis(240));
    }

    #[test]
    fn a_repeated_start_open_does_not_restart_the_run() {
        // Старт зовётся на первом configure; повторный не должен продлевать
        // проявление, иначе окно мерцало бы на каждом configure.
        let mut tween = Tween::new(0.0, 1.0, base(), OPEN_DURATION, ease_out_cubic);
        let halfway = tween.value(ms(60));

        tween.retarget(ms(60), 1.0);

        assert_eq!(tween.started, base());
        assert!((tween.value(ms(60)) - halfway).abs() < 1e-6);
        assert!(tween.done(ms(60) + OPEN_DURATION));
    }
}

/// Параметры окна слоя: имя пространства, размер и высота под пул буферов.
/// Собраны в структуру, потому что иначе список аргументов перестаёт читаться.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub namespace: Option<&'static str>,
    pub width: u32,
    pub height: u32,
    /// Потолок высоты: по нему считается пул буферов.
    pub max_height: u32,
    /// Плотность буфера: ровно та, что у `set_buffer_scale`.
    pub scale: u32,
}

/// Создаёт слой: поверхность, `create_layer_surface`, `set_size` до первого
/// commit и пул буферов под `shape.max_height`. Поведение = стартап hud-menu.
pub fn open<D>(
    compositor: &CompositorState,
    layer_shell: &LayerShell,
    shm: &Shm,
    qh: &QueueHandle<D>,
    shape: Shape,
    now: std::time::Instant,
    anim: AnimSettings,
) -> Result<Context, String>
where
    D: Dispatch<wl_surface::WlSurface, SurfaceData<()>>
        + Dispatch<
            smithay_client_toolkit::reexports::protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
            LayerSurfaceData,
        >
        + LayerShellHandler
        + 'static,
{
    let surface = compositor.create_surface(qh);
    let layer = layer_shell.create_layer_surface(
        qh,
        surface.clone(),
        Layer::Overlay,
        shape.namespace,
        None,
    );
    layer.set_anchor(Anchor::empty());
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_exclusive_zone(-1);
    layer.set_margin(0, 0, 0, 0);
    // Размер до первого commit: у слоя без якорей композитор не возьмёт
    // ширину откуда-то ещё.
    layer.set_size(shape.width, shape.height);
    layer.commit();
    let bytes = shape.width as usize
        * shape.max_height as usize
        * 4
        * shape.scale as usize
        * shape.scale as usize;
    let pool = SlotPool::new(bytes * 3 + 4_000_000, shm)
        .map_err(|error| format!("не создать пул буферов: {error}"))?;
    Ok(Context {
        layer,
        pool,
        width: shape.width,
        height: shape.height,
        configured: false,
        dirty: true,
        scale: shape.scale,
        // Проявление ещё не начато: стартует на первом configure.
        openness: Tween::settled(0.0, now),
        anim,
    })
}

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

use super::anim::{self, AnimSettings, Tween};

/// Длительность проявления окна.
pub const OPEN_DURATION: std::time::Duration = std::time::Duration::from_millis(120);

/// Длительность плавного изменения высоты окна. 160 мс хватает, чтобы глаз
/// успел связать подменю с выросшей карточкой, и мало, чтобы не ждать ввода.
pub const RESIZE_DURATION: std::time::Duration = std::time::Duration::from_millis(160);

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
pub fn openness_steps(openness: f32) -> f32 {
    openness.clamp(0.0, 1.0)
}

/// Высота окна под анимацию: цель и текущее значение твина.
///
/// Отдельно от [`Context`], чтобы перецеливание, округление и завершение
/// проверялись без Wayland-композитора.
#[derive(Clone, Copy, Debug)]
pub struct HeightState {
    target: u32,
    tween: Tween,
}

impl HeightState {
    /// Высота, уже стоящая на месте.
    pub fn settled(height: u32, now: std::time::Instant) -> Self {
        let height = height.max(1);
        Self {
            target: height,
            tween: Tween::settled(height as f32, now),
        }
    }

    /// Целевая высота, к которой едет окно.
    pub fn target(&self) -> u32 {
        self.target
    }

    /// Текущая высота кадра, округлённая до целого логического пикселя.
    /// Ноль сюда попасть не может: слой без якорей с нулевой высотой —
    /// ошибка протокола.
    pub fn value(&self, now: std::time::Instant) -> u32 {
        self.tween.value(now).round().max(1.0) as u32
    }

    /// Высота, которую нужно отправить, если она отличается от уже
    /// отправленной `submitted`. Повторный кадр внутри одного пикселя размера
    /// не меняет, поэтому лишний `set_size` не рождается.
    pub fn step(&self, submitted: u32, now: std::time::Instant) -> Option<u32> {
        let height = self.value(now);
        (height != submitted).then_some(height)
    }

    /// Идёт ли движение. Пока идёт, цикл должен просыпаться кадрами и слать
    /// композитору промежуточные размеры.
    pub fn animating(&self, now: std::time::Instant) -> bool {
        !self.tween.done(now)
    }

    /// Новая цель. Движение продолжается с текущего значения, а не с начала
    /// прошлого: быстрый вход и выход из подменю не должен давать скачок.
    /// Направление выбирает кривую: рост мягко садится, сужение быстро уходит.
    pub fn retarget(&mut self, target: u32, now: std::time::Instant, settings: AnimSettings) {
        let target = target.max(1);
        if target == self.target {
            return;
        }
        let current = self.tween.value(now);
        let growing = target as f32 > current;
        self.tween.retarget(now, target as f32);
        self.tween.duration = settings.scaled(RESIZE_DURATION);
        self.tween.easing = if growing {
            anim::ease_out_cubic
        } else {
            anim::ease_in_cubic
        };
        self.target = target;
    }

    /// Поставить высоту без движения: первый configure, внешний размер,
    /// выключенные анимации.
    pub fn settle(&mut self, height: u32) {
        let height = height.max(1);
        self.target = height;
        self.tween.from = height as f32;
        self.tween.to = height as f32;
        self.tween.duration = std::time::Duration::ZERO;
    }
}

/// Размер из configure — внешний, только если у нас нет неподтверждённого
/// `set_size` и не идёт анимация. В остальных случаях композитор отвечает на
/// наши же запросы, и запоздавший ответ не должен откатывать высоту.
fn configure_is_external(pending: bool, resizing: bool) -> bool {
    !pending && !resizing
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
    /// Высота окна: цель и текущее анимированное значение.
    resize: HeightState,
    /// Последний отправленный размер, ещё не подтверждённый configure.
    /// Нужен, чтобы отличить ответ композитора на наш `set_size` от
    /// настоящего внешнего изменения размера.
    pending: Option<(u32, u32)>,
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

    /// Текущая степень проявления.
    pub fn openness_now(&self, now: std::time::Instant) -> f32 {
        openness_steps(self.openness.value(now))
    }

    /// Идёт ли проявление или смена высоты. Пока идёт, цикл обязан ставить
    /// dirty и просыпаться часто, иначе окно менялось бы ступенями по чужим
    /// событиям ввода.
    pub fn animating(&self, now: std::time::Instant) -> bool {
        !self.openness.done(now) || self.resize.animating(now)
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
        // Окна без анимации высоты держат состояние выровненным: configure —
        // единственный источник размера, твин стоит на нём.
        self.resize.settle(self.height);
        self.pending = None;
        self.configured = true;
        self.dirty = true;
        if first {
            self.start_open(now);
        }
    }

    /// Применяет configure к окну, которое само ведёт высоту анимацией.
    ///
    /// Пока есть неподтверждённый `set_size` или идёт движение, размер из
    /// configure — ответ на наши же запросы (в том числе запоздавший): он не
    /// трогает высоту. Когда своих запросов нет, размер считается внешним и
    /// принимается.
    pub fn apply_configure_animated(
        &mut self,
        proposed: (u32, u32),
        fallback: (u32, u32),
        now: std::time::Instant,
    ) {
        if !self.configured {
            let width = if proposed.0 > 0 {
                proposed.0
            } else {
                fallback.0
            };
            let height = if proposed.1 > 0 {
                proposed.1
            } else {
                fallback.1
            };
            self.width = width.max(1);
            self.height = height.max(1);
            self.resize = HeightState::settled(self.height, now);
            self.pending = None;
            self.configured = true;
            self.dirty = true;
            self.start_open(now);
            return;
        }

        // Точное совпадение с последним запросом — подтверждение: запрос
        // закрыт, но размер уже наш, менять нечего.
        if self.pending == Some(proposed) {
            self.pending = None;
        }
        let external = configure_is_external(self.pending.is_some(), self.resize.animating(now));
        if external {
            let width = if proposed.0 > 0 {
                proposed.0
            } else {
                self.width
            };
            let height = if proposed.1 > 0 {
                proposed.1
            } else {
                self.height
            };
            if (width, height) != (self.width, self.height) {
                self.width = width;
                self.height = height;
                self.resize.settle(height);
                self.dirty = true;
            }
        }
        self.configured = true;
        self.dirty = true;
    }

    /// Запрос размера у композитора. Повторный `set_size` с той же величиной
    /// не шлём: лишний commit лишь трёт канал.
    pub fn request_size(&mut self, width: u32, height: u32) {
        self.resize.settle(height.max(1));
        self.submit_size(width.max(1), height.max(1));
    }

    /// Просит новую высоту, а не ставит её сразу. Ширина у меню постоянна и
    /// меняется мгновенно; высота едет твином от последнего кадра.
    pub fn request_height(&mut self, width: u32, height: u32, now: std::time::Instant) {
        let width = width.max(1);
        let height = height.max(1);
        if width != self.width {
            self.submit_size(width, self.height);
        }
        if self.configured {
            self.resize.retarget(height, now, self.anim);
        } else {
            // До первого configure окна ещё нет: первый размер ставится сразу,
            // иначе меню открывалось бы с анимации размера, которой не видно.
            self.resize = HeightState::settled(height, now);
            self.submit_size(width, height);
        }
    }

    /// Отправляет текущий кадр анимации высоты. `true`, если движение ещё
    /// идёт и циклу нужен следующий кадр.
    pub fn tick_resize(&mut self, now: std::time::Instant) -> bool {
        if let Some(height) = self.resize.step(self.height, now) {
            self.submit_size(self.width, height);
        }
        if !self.resize.animating(now) {
            // Округление могло дойти до цели на кадр раньше окончания твина.
            // Досылаем точную цель, чтобы последний `set_size` был именно
            // конечным: иначе запоздавший configure на промежуточную высоту
            // мог бы откатить окно назад.
            let target = self.resize.target();
            if self.height != target {
                self.submit_size(self.width, target);
            }
        }
        self.resize.animating(now)
    }

    /// Целевая высота: по ней можно сверять модель, не дёргая `set_size`.
    pub fn target_height(&self) -> u32 {
        self.resize.target()
    }

    fn submit_size(&mut self, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        self.pending = Some((width, height));
        self.layer.set_size(width, height);
        self.layer.commit();
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::anim::ease_out_cubic;
    use super::*;

    fn base() -> std::time::Instant {
        static BASE: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(std::time::Instant::now)
    }

    fn ms(value: u64) -> std::time::Instant {
        base() + std::time::Duration::from_millis(value)
    }

    #[test]
    fn openness_steps_keeps_the_whole_range() {
        for step in 0..=10 {
            let openness = step as f32 / 10.0;
            assert!(
                (openness_steps(openness) - openness).abs() < 1e-6,
                "прогресс не должен квантоваться"
            );
        }
    }

    #[test]
    fn openness_steps_clamps_the_progress() {
        assert!((openness_steps(1.2) - 1.0).abs() < 1e-6);
        assert!((openness_steps(-0.3) - 0.0).abs() < 1e-6);
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

    fn height(from: u32) -> HeightState {
        HeightState::settled(from, base())
    }

    #[test]
    fn the_height_animation_reaches_its_target_on_time() {
        let mut state = height(300);
        state.retarget(400, base(), AnimSettings::default());

        assert_eq!(state.value(base()), 300, "в начале — прежняя высота");
        assert!(state.animating(base()));
        assert_eq!(
            state.value(ms(RESIZE_DURATION.as_millis() as u64)),
            400,
            "к концу движения высота ровно целевая"
        );
        assert!(
            !state.animating(ms(RESIZE_DURATION.as_millis() as u64)),
            "окончание останавливает кадры"
        );
    }

    #[test]
    fn growth_uses_ease_out_and_shrinking_uses_ease_in() {
        // ease_out обгоняет прямую на старте, ease_in отстаёт. Проверяем
        // четверть пути: на середине обе кривые дают одинаковое отклонение,
        // и по одному числу направление не отличить.
        let mut growing = height(300);
        growing.retarget(400, base(), AnimSettings::default());
        let grow_quarter = growing.value(ms(40));
        assert!(
            grow_quarter > 325,
            "рост должен обгонять прямую: {grow_quarter}"
        );

        let mut shrinking = height(400);
        shrinking.retarget(300, base(), AnimSettings::default());
        let shrink_quarter = shrinking.value(ms(40));
        assert!(
            shrink_quarter > 375,
            "сужение должно отставать от прямой: {shrink_quarter}"
        );
    }

    #[test]
    fn a_retarget_midway_does_not_jump() {
        let mut state = height(300);
        state.retarget(400, base(), AnimSettings::default());
        let half = ms(80);
        let at_half = state.value(half);

        state.retarget(260, half, AnimSettings::default());

        assert_eq!(
            state.value(half),
            at_half,
            "смена цели не должна давать скачок"
        );
        assert!(
            state.value(ms(240)) == 260,
            "новое движение должно доехать до новой цели"
        );
        assert!(!state.animating(ms(240)));
    }

    #[test]
    fn rapid_target_changes_settle_at_the_latest_one() {
        let mut state = height(300);
        for (step, target) in [520u32, 380, 460, 320].into_iter().enumerate() {
            state.retarget(target, ms(step as u64 * 10), AnimSettings::default());
        }

        assert_eq!(state.value(ms(30 + 160)), 320);
        assert_eq!(state.target(), 320);
        assert!(!state.animating(ms(30 + 160)));
    }

    #[test]
    fn equal_rounded_heights_do_not_request_a_new_size() {
        let mut state = height(300);
        state.retarget(400, base(), AnimSettings::default());

        assert_eq!(
            state.step(300, base()),
            None,
            "в начале движения высота ещё прежняя"
        );
        let mid = ms(80);
        let first = state.step(300, mid).expect("на середине высота выросла");
        assert_eq!(
            state.step(first, mid),
            None,
            "тот же округлённый пиксель не шлётся второй раз"
        );
    }

    #[test]
    fn disabled_animations_apply_the_target_height_at_once() {
        let mut state = height(300);
        state.retarget(400, base(), AnimSettings::new(false, 1.0));

        assert_eq!(state.value(base()), 400);
        assert!(
            !state.animating(base()),
            "выключенная анимация не держит кадры"
        );
        assert_eq!(state.step(300, base()), Some(400));
        assert_eq!(state.step(400, base()), None);
    }

    #[test]
    fn the_speed_setting_scales_the_height_animation() {
        let mut fast = height(300);
        fast.retarget(400, base(), AnimSettings::new(true, 2.0));
        assert_eq!(fast.value(ms(80)), 400, "на скорости 2 хватает 80 мс");

        let mut slow = height(300);
        slow.retarget(400, base(), AnimSettings::new(true, 0.5));
        assert!(slow.animating(ms(200)), "на скорости 0.5 движение ещё идёт");
        assert_eq!(slow.value(ms(320)), 400);
    }

    #[test]
    fn our_own_configure_does_not_override_an_active_tween() {
        // Пока идёт движение или не подтверждён `set_size`, размер из
        // configure — эхо наших запросов, а не внешнее изменение.
        assert!(!configure_is_external(true, true));
        assert!(!configure_is_external(true, false));
        assert!(!configure_is_external(false, true));
        assert!(
            configure_is_external(false, false),
            "без своих запросов configure — внешний размер"
        );
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
        // Высота уже задана `set_size` до первого commit: анимации нет.
        resize: HeightState::settled(shape.height.max(1), now),
        pending: None,
        anim,
    })
}

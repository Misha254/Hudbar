//! hud-schemes-rs — окно выбора схемы matugen.
//!
//! Отдельное окно, а не зона в окне обоев: схемы занимали низ карточки и
//! мешали сетке быть крупной. Открывается сразу после применения обоев.
//!
//! Ввод: стрелки по кольцу плиток, Enter — применить и закрыть, Esc — закрыть
//! без изменений.

use std::os::fd::AsFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::poll::{PollFd, PollFlags, poll};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Modifiers, RawModifiers, RepeatInfo},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

#[allow(dead_code)]
#[path = "../hud/bind_data.rs"]
mod bind_data;
#[allow(dead_code)]
#[path = "../hud/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../hud/config_io.rs"]
mod config_io;
#[allow(dead_code)]
#[path = "../hud/dunst.rs"]
mod dunst;
#[allow(dead_code)]
#[path = "../hud/menu/instance.rs"]
mod instance;
#[allow(dead_code)]
#[path = "../hud/layer.rs"]
mod layer;
#[allow(dead_code)]
#[path = "../hud/log.rs"]
mod log;
#[allow(dead_code)]
#[path = "../hud/palette.rs"]
mod palette;
#[allow(dead_code)]
#[path = "../hud/repeat.rs"]
mod repeat;
#[allow(dead_code)]
#[path = "../hud/schemes.rs"]
mod schemes;
#[allow(dead_code)]
#[path = "../hud/settings.rs"]
mod settings;
#[allow(dead_code)]
#[path = "../hud/settings_icons.rs"]
mod settings_icons;
#[allow(dead_code)]
#[path = "../hud/settings_ui.rs"]
mod settings_ui;
#[allow(dead_code)]
#[path = "../hud/settings_view.rs"]
mod settings_view;
#[allow(dead_code)]
#[path = "../hud/settings_widgets.rs"]
mod settings_widgets;
#[allow(dead_code)]
#[path = "../hud/text.rs"]
mod text;
#[allow(dead_code)]
#[path = "../hud/ui_layout.rs"]
mod ui_layout;
#[allow(dead_code)]
#[path = "../hud/ui_tokens.rs"]
mod ui_tokens;
// Модуль целиком, а не только view.rs: view внутри себя адресует
// `super::super::`, то есть рассчитан на вложенность wallpaper::view.
#[allow(dead_code)]
#[path = "../hud/wallpaper.rs"]
mod wallpaper;

use repeat::Repeat;
use schemes::{CARD_W, FOOTER_H, PAD, SCHEMES};
use settings_ui::Rect;
use text::Align;
use text::{SCALE, TextPainter};

const NAMESPACE: &str = "hudschemes";
/// Свой pid-файл: окно схем не должно закрывать окно обоев и наоборот.
const PID_FILE: &str = "hudbar-schemes.pid";
const WIDTH: u32 = CARD_W as u32;
/// Высота карточки из содержимого: заголовок + список схем + подсказки.
fn height() -> u32 {
    (schemes::HEADER_H + SCHEMES.len() as f32 * schemes::ROW_H + FOOTER_H + PAD * 2.0) as u32
}

/// Коды XKB — те же значения, что в окне обоев. Список вертикальный, поэтому
/// влево и вправо не значат ничего и не вынесены.
mod xkb {
    pub const ESCAPE: u32 = 0xff1b;
    pub const RETURN: u32 = 0xff0d;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const UP: u32 = 0xff52;
    pub const DOWN: u32 = 0xff54;
    pub const HOME: u32 = 0xff50;
    pub const END: u32 = 0xff57;
}

/// Что делает окно после действия.
enum Action {
    /// Остаться, перерисовать.
    None,
    /// Применить схему и закрыться.
    Apply(usize),
    /// Закрыться без изменений.
    Close,
}

/// Модель окна: выбранная плитка и текущая схема. Без Wayland.
struct Schemes {
    sel: usize,
    current: Option<String>,
}

impl Schemes {
    fn open() -> Self {
        let current = schemes::current();
        let sel = current
            .as_deref()
            .and_then(|name| SCHEMES.iter().position(|s| *s == name))
            .unwrap_or(0);
        Self { sel, current }
    }

    /// Плитки идут кольцом, как и в старой зоне окна обоев: ←/→ закольцованы,
    /// ↑/↓ держат колонку. Выхода из зоны нет — она тут единственная.
    fn move_dir(&mut self, keysym: u32) {
        let len = SCHEMES.len();
        if len == 0 {
            return;
        }
        // Список зациклен, как и строки меню: вверх с первой — на последнюю.
        self.sel = match keysym {
            xkb::UP => (self.sel + len - 1) % len,
            xkb::DOWN => (self.sel + 1) % len,
            xkb::HOME => 0,
            xkb::END => len - 1,
            _ => self.sel,
        };
    }

    fn activate(&self) -> Action {
        if self.sel < SCHEMES.len() {
            Action::Apply(self.sel)
        } else {
            Action::None
        }
    }

    /// Плитка под курсором. Первый клик выбирает, второй по той же плитке
    /// применяет — так же, как в окне обоев.
    fn click(&mut self, card: Rect, x: f32, y: f32) -> Action {
        let area = schemes::zone(card);
        for index in 0..SCHEMES.len() {
            let Some(line) = schemes::row(area, index) else {
                return Action::None;
            };
            if line.contains(x, y) {
                if self.sel == index {
                    return self.activate();
                }
                self.sel = index;
                return Action::None;
            }
        }
        Action::None
    }
}

struct SchemesApp {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    ctx: layer::Context,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
    /// Автоповтор удержанной стрелки: композитор повторов не шлёт.
    repeat: Repeat,
    painter: TextPainter,
    icons: TextPainter,
    model: Schemes,
    ui: ui_tokens::UiPalette,
    pixel: bool,
    lang: Language,
    exit: Arc<AtomicBool>,
}

impl SchemesApp {
    fn card() -> Rect {
        Rect::new(0.0, 0.0, CARD_W, height() as f32)
    }

    fn handle_key(&mut self, event: KeyEvent) -> Action {
        if self.mods.ctrl || self.mods.logo {
            return Action::None;
        }
        let keysym = event.keysym.into();
        match keysym {
            xkb::ESCAPE => Action::Close,
            xkb::RETURN | xkb::KP_ENTER => self.model.activate(),
            key => {
                self.model.move_dir(key);
                Action::None
            }
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::None => self.ctx.dirty = true,
            Action::Close => self.exit.store(true, Ordering::Relaxed),
            Action::Apply(index) => {
                schemes::apply_detached(SCHEMES[index]);
                self.exit.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Рисует кадр карточкой от нуля и отдаёт буфер слою.
    fn draw(&mut self) {
        // Ждать именно configure, а не ненулевых размеров: `layer::open`
        // заполняет width/height сразу, и буфер, прикреплённый до
        // `ack_configure`, рвёт протокол — композитор убивает клиент, и окно
        // не появляется вовсе.
        if !self.ctx.configured || !self.ctx.dirty {
            return;
        }
        self.ctx.dirty = false;
        let pw = (self.ctx.width as f32 * SCALE) as u32;
        let ph = (self.ctx.height as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        let card = Self::card();

        let mut canvas = wallpaper::view::Canvas::new(
            &mut pixmap,
            &mut self.painter,
            &mut self.icons,
            self.pixel,
            self.ui,
        );
        paint(&mut canvas, card, &self.model, self.pixel, self.lang);
        wallpaper::view::apply_alpha(&mut pixmap, card, wallpaper::view::CARD_ALPHA);

        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) =
            self.ctx
                .pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
        else {
            self.ctx.dirty = true;
            return;
        };
        let (dst, _) = canvas.as_chunks_mut::<4>();
        let (src, _) = pixmap.data().as_chunks::<4>();
        for (out, input) in dst.iter_mut().zip(src.iter()) {
            *out = [input[2], input[1], input[0], input[3]];
        }
        let surface = self.ctx.layer.wl_surface();
        surface.set_buffer_scale(SCALE as i32);
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        let _ = buffer.attach_to(surface);
        self.ctx.layer.commit();
    }
}

use settings_ui::Language;

/// Карточка со схемами: заголовок, плитки с точками и подписью, подсказки.
fn paint(
    canvas: &mut wallpaper::view::Canvas<'_, '_>,
    card: Rect,
    model: &Schemes,
    pixel: bool,
    lang: Language,
) {
    let ts = ui_tokens::type_scale(pixel);
    let area = schemes::zone(card);

    canvas.fill(card, ui_tokens::radii::LG, canvas.ui.panel);
    canvas.outline(card, canvas.ui.border, 1.0);

    let title = match lang {
        Language::Ru => "Схема matugen",
        Language::En => "matugen scheme",
    };
    canvas.label(
        title,
        ts.section_title,
        canvas.ui.text,
        Rect::new(card.x + PAD, card.y, card.w * 0.6, schemes::HEADER_H),
        Align::Start,
    );

    let swatch_map = wallpaper::swatches::load();
    for (index, name) in SCHEMES.iter().enumerate() {
        let Some(line) = schemes::row(area, index) else {
            return;
        };
        let selected = index == model.sel;

        // Выделение держится на рамке и цвете подписи, а не на заливке:
        // заливка акцентом перекрывала и точки-образцы, и название схемы —
        // читать выбранную строку было невозможно.
        let fill = if selected {
            canvas.ui.panel
        } else {
            canvas.ui.idle_panel
        };
        canvas.fill(line, ui_tokens::radii::SM, fill);
        if selected {
            canvas.outline(line, canvas.ui.accent, 2.0);
        }

        // Образцы схемы: четыре точки слева, на нейтральном фоне.
        let colors = schemes::swatches_for(name, swatch_map.as_ref());
        let dots = schemes::row_dots(line);
        let mut x = dots.x;
        let y = dots.y;
        for color in &colors {
            if let Some(rgba) = wallpaper::view::parse_hex(color) {
                canvas.fill(
                    Rect::new(x, y, schemes::SWATCH_D, schemes::SWATCH_D),
                    schemes::SWATCH_D / 2.0,
                    rgba,
                );
            }
            x += schemes::SWATCH_D + schemes::SWATCH_GAP;
        }

        let label = schemes::row_label(line);
        let label_color = if selected {
            canvas.ui.accent
        } else {
            canvas.ui.text
        };
        canvas.label(
            name.trim_start_matches("scheme-"),
            ts.caption,
            label_color,
            label,
            Align::Start,
        );

        // Галочка текущей схемы: она уже применена, и это видно сразу.
        if model.current.as_deref() == Some(*name) {
            canvas.label(
                "✓",
                ts.caption,
                canvas.ui.muted,
                Rect::new(line.right() - schemes::ROW_PAD - 20.0, line.y, 20.0, line.h),
                Align::End,
            );
        }
    }

    let hints: [&str; 3] = match lang {
        Language::Ru => ["↑↓ выбрать", "Enter применить", "Esc закрыть"],
        Language::En => ["↑↓ select", "Enter apply", "Esc close"],
    };
    let mut band = Rect::new(
        card.x + PAD,
        card.y + card.h - FOOTER_H,
        card.w - PAD * 2.0,
        FOOTER_H,
    );
    for hint in hints {
        let w = canvas.text.text_width(hint, ts.caption) + ui_tokens::spacing::XL;
        canvas.label(hint, ts.caption, canvas.ui.muted, band, Align::Start);
        band.x += w;
    }
}

impl CompositorHandler for SchemesApp {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        self.ctx.scale = new_factor as u32;
        self.ctx.dirty = true;
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.ctx.dirty = true;
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for SchemesApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for SchemesApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(pointer) = self.pointer.take()
        {
            pointer.release();
        }
        if capability == Capability::Keyboard
            && let Some(keyboard) = self.keyboard.take()
        {
            keyboard.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl ShmHandler for SchemesApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl PointerHandler for SchemesApp {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if event.surface != *self.ctx.layer.wl_surface() {
                continue;
            }
            let (x, y) = (event.position.0 as f32, event.position.1 as f32);
            if let PointerEventKind::Press { button: 0x110, .. } = event.kind {
                let action = self.model.click(Self::card(), x, y);
                self.apply(action);
            }
        }
    }
}

impl KeyboardHandler for SchemesApp {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[smithay_client_toolkit::seat::keyboard::Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.exit.store(true, Ordering::Relaxed);
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.repeat.press(&event);
        let action = self.handle_key(event);
        self.apply(action);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.repeat.release(&event);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        let action = self.handle_key(event);
        self.apply(action);
    }

    /// Композитор сообщил rate и delay удержания: повторы он не шлёт.
    fn update_repeat_info(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        info: RepeatInfo,
    ) {
        self.repeat.set_info(info);
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.mods = modifiers;
    }
}

impl LayerShellHandler for SchemesApp {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit.store(true, Ordering::Relaxed);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        self.ctx.apply_configure(
            (configure.new_size.0, configure.new_size.1),
            (WIDTH, height()),
        );
    }
}

impl ProvidesRegistryState for SchemesApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(SchemesApp);
smithay_client_toolkit::delegate_dispatch2!(SchemesApp);

/// Один экземпляр. Слой не виден в `niri msg windows`, поэтому единственный
/// способ узнать, открыто ли окно, — pid-файл.
///
/// Это не удобство: каждый слой забирает клавиатуру на себя
/// (`KeyboardInteractivity::Exclusive` в `layer::open`), и второе окно схем
/// оставило бы первое висеть поверх всех остальных, не давая ими пользоваться.
fn main() -> Result<(), String> {
    let pid_file = instance::pid_path_for(PID_FILE);
    if let instance::Startup::CloseRunning(pid) =
        instance::startup_for(PID_FILE, std::path::Path::new("/proc"))
    {
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        )
        .map_err(|error| format!("не закрыть прошлый pid {pid}: {error}"))?;
        return Ok(());
    }
    let pid = std::process::id() as i32;
    if let Err(error) = instance::store(&pid_file, pid) {
        return Err(format!("не записать pid-файл: {error}"));
    }
    let result = run();
    // pid-файл снимается на любом выходе, включая падение: иначе следующий
    // запуск увидит «живой» pid и не откроет окно.
    instance::clear(&pid_file, pid);
    result
}

fn run() -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|error| format!("нет Wayland: {error}"))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|error| format!("нет реестра: {error}"))?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|error| format!("нет wl_compositor: {error}"))?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|_| "нет zwlr_layer_shell".to_string())?;
    let shm = Shm::bind(&globals, &qh).map_err(|error| format!("нет wl_shm: {error}"))?;

    let loaded = settings::load();
    let pixel = loaded.theme.as_deref() == Some("pixel");
    let palette = palette::load();
    let ui = ui_tokens::ui_palette(pixel, palette);

    let ctx = layer::open(
        &compositor,
        &layer_shell,
        &shm,
        &qh,
        layer::Shape {
            namespace: Some(NAMESPACE),
            width: WIDTH,
            height: height(),
            max_height: height(),
            scale: SCALE as u32,
        },
    )?;

    let mut app = SchemesApp {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        ctx,
        pointer: None,
        keyboard: None,
        mods: Modifiers::default(),
        repeat: Repeat::default(),
        painter: TextPainter::new(&settings_view::font_name(pixel)),
        icons: TextPainter::new(settings_icons::FONT),
        model: Schemes::open(),
        ui,
        pixel,
        lang: loaded.language,
        exit: Arc::new(AtomicBool::new(false)),
    };
    app.draw();

    // Порядок как у окна обоев: сначала досылаем накопленное, потом
    // обрабатываем очередь, и только потом ждём сокет. Обработка configure
    // внутри poll-ожидания не срабатывала — окно так и не получало размер.
    while !app.exit.load(Ordering::Relaxed) {
        if event_queue.flush().is_err() {
            break;
        }
        let _ = event_queue.dispatch_pending(&mut app);
        app.draw();
        while let Some(event) = app.repeat.poll() {
            let action = app.handle_key(event);
            app.apply(action);
            app.draw();
        }
        if app.exit.load(Ordering::Relaxed) {
            break;
        }
        if let Some(guard) = event_queue.prepare_read() {
            // С удержанной стрелкой ждать дольше интервала повтора нельзя.
            let idle = 200u16;
            let wait = app
                .repeat
                .wait_hint()
                .map(|left| left.as_millis().clamp(1, u16::MAX as u128) as u16)
                .unwrap_or(idle)
                .min(idle);
            let mut fds = [PollFd::new(conn.as_fd(), PollFlags::POLLIN)];
            let _ = poll(&mut fds, wait);
            let readable = fds[0]
                .revents()
                .is_some_and(|events| events.intersects(PollFlags::POLLIN));
            if readable {
                let _ = guard.read();
            }
        }
    }
    Ok(())
}

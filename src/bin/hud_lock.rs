//! hud-lock-rs — экран блокировки Hudbar на ext-session-lock.
//!
//! Заменяет `swaylock` визуально: рисует ту же карточку, что экран входа, но
//! без кнопки «Войти» и без greetd. Пароль проверяет PAM через
//! `libpam`; сам пароль не попадает ни в argv, ни в окружение, ни в файлы.
//!
//! Запускать только из живой сессии: `ext-session-lock` есть у niri. Если
//! протокола нет или композитор отказал, программа выходит с ошибкой, не
//! оставляя сеанс без блокировки.

use std::os::fd::AsFd;
use std::path::PathBuf;

use chrono::Timelike;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Modifiers, RawModifiers},
    },
    session_lock::{
        SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
        SessionLockSurfaceConfigure,
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_buffer, wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface},
};

#[allow(dead_code)]
#[path = "../hud/anim.rs"]
mod anim;
#[allow(dead_code)]
#[path = "../hud/canvas.rs"]
mod canvas;
#[allow(dead_code)]
#[path = "../hud/card_frame.rs"]
mod card_frame;
#[allow(dead_code)]
#[path = "../hud/geom.rs"]
mod geom;
#[allow(dead_code)]
#[path = "../hud/i18n.rs"]
mod i18n;
#[allow(dead_code)]
#[path = "../hud/log.rs"]
mod log;
/// `log` запускает внешние команды через `data::oneshot`; потоки и DBus
/// панели экрану блокировки не нужны.
#[path = "../hud/data/oneshot.rs"]
pub mod oneshot_data;
pub mod data {
    pub use super::oneshot_data as oneshot;
}
#[allow(dead_code)]
#[path = "../hud/greeter/mod.rs"]
mod greeter;
#[allow(dead_code)]
#[path = "../hud/palette.rs"]
mod palette;
#[allow(dead_code)]
#[path = "../hud/settings.rs"]
mod settings;
#[allow(dead_code)]
#[path = "../hud/settings_icons.rs"]
mod settings_icons;
#[path = "../hud/shell.rs"]
mod shell;
#[allow(dead_code)]
#[path = "../hud/text.rs"]
mod text;
#[allow(dead_code)]
#[path = "../hud/ui_tokens.rs"]
mod ui_tokens;

#[path = "../hud/locker/mod.rs"]
mod locker;

use greeter::config::Config;
use greeter::flow::Secret;
use greeter::image::Background;
use locker::render::{self, View};
use locker::{auth, strings};
use settings::Language;
use text::{SCALE, TextPainter};
use ui_tokens::{UiPalette, ui_palette};

const CLOCK_WAIT_MS: u16 = 200;

mod xkb {
    pub const RETURN: u32 = 0xff0d;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const BACKSPACE: u32 = 0xff08;
    /// `u` в XKB — строчная независимо от Shift.
    pub const U: u32 = 0x75;
}

const USAGE: &str = "\
hud-lock-rs — экран блокировки Hudbar

  hud-lock-rs           заблокировать сессию
  hud-lock-rs --help    эта справка

Пароль проверяет PAM (сервис swaylock, запасной — login). Ни пароль, ни его
длина не пишутся в лог и не передаются в argv.
";

struct LockSurface {
    surface: SessionLockSurface,
    output: wl_output::WlOutput,
    pool: Option<SlotPool>,
    width: u32,
    height: u32,
    configured: bool,
}

impl LockSurface {
    fn new(surface: SessionLockSurface, output: wl_output::WlOutput) -> Self {
        Self {
            surface,
            output,
            pool: None,
            width: 0,
            height: 0,
            configured: false,
        }
    }
}

struct LockApp {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    compositor_state: CompositorState,
    session_lock_state: SessionLockState,
    session_lock: Option<SessionLock>,
    surfaces: Vec<LockSurface>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
    painter: TextPainter,
    ui: UiPalette,
    config: Config,
    background: Option<Background>,
    lang: Language,
    username: String,
    password: Secret,
    message: Option<&'static str>,
    busy: bool,
    dirty: bool,
    exit: bool,
    last_minute: i32,
}

impl LockApp {
    fn lock(&mut self, qh: &QueueHandle<Self>) -> Result<(), String> {
        let lock = self
            .session_lock_state
            .lock(qh)
            .map_err(|error| format!("ext-session-lock недоступен: {error}"))?;
        for output in self.output_state.outputs() {
            let surface = self.compositor_state.create_surface(qh);
            let lock_surface = lock.create_lock_surface(surface, &output, qh);
            self.surfaces.push(LockSurface::new(lock_surface, output));
        }
        if self.surfaces.is_empty() {
            return Err("нет выходов: нечего блокировать".to_string());
        }
        self.session_lock = Some(lock);
        Ok(())
    }

    /// Перерисовка раз в минуту: часы не должны стоять.
    fn tick(&mut self) {
        let minute = chrono::Local::now().hour() as i32 * 60 + chrono::Local::now().minute() as i32;
        if minute != self.last_minute {
            self.last_minute = minute;
            self.dirty = true;
        }
    }

    fn draw(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        for index in 0..self.surfaces.len() {
            self.draw_surface(index);
        }
    }

    fn draw_surface(&mut self, index: usize) {
        let Some(surface) = self.surfaces.get_mut(index) else {
            return;
        };
        if !surface.configured || surface.width == 0 || surface.height == 0 {
            return;
        }
        let pw = (surface.width as f32 * SCALE) as u32;
        let ph = (surface.height as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        pixmap.fill(tiny_skia::Color::TRANSPARENT);

        let view = View {
            lang: self.lang,
            now: chrono::Local::now(),
            username: &self.username,
            password_chars: self.password.chars(),
            caps: self.mods.caps_lock,
            message: self.message.map(|text| (text, true)),
            busy: self.busy,
        };
        render::draw(
            &mut pixmap,
            &mut self.painter,
            &self.ui,
            &self.config,
            self.background.as_ref(),
            &view,
        );

        self.blit(index, &pixmap);
    }

    /// Первый кадр до отрисовки карточки: сплошной `base` вместо красной
    /// подложки композитора. Шрифты и раскладка при этом ещё считаются, и без
    /// этого буфера на экране на секунду остаётся чужой цвет.
    fn fill_surface(&mut self, index: usize) {
        let Some(surface) = self.surfaces.get(index) else {
            return;
        };
        if !surface.configured || surface.width == 0 || surface.height == 0 {
            return;
        }
        let pw = (surface.width as f32 * SCALE) as u32;
        let ph = (surface.height as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        pixmap.fill(tiny_skia::Color::from_rgba8(
            self.ui.base.0,
            self.ui.base.1,
            self.ui.base.2,
            255,
        ));
        self.blit(index, &pixmap);
    }

    fn blit(&mut self, index: usize, pixmap: &tiny_skia::Pixmap) {
        let Some(surface) = self.surfaces.get_mut(index) else {
            return;
        };
        let pw = pixmap.width();
        let ph = pixmap.height();
        let Some(pool) = surface.pool.as_mut() else {
            self.dirty = true;
            return;
        };
        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) =
            pool.create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
        else {
            self.dirty = true;
            return;
        };
        let (dst, _) = canvas.as_chunks_mut::<4>();
        let (src, _) = pixmap.data().as_chunks::<4>();
        for (out, input) in dst.iter_mut().zip(src.iter()) {
            *out = [input[2], input[1], input[0], input[3]];
        }
        let wl_surface = surface.surface.wl_surface();
        wl_surface.set_buffer_scale(SCALE as i32);
        wl_surface.damage_buffer(0, 0, pw as i32, ph as i32);
        let _ = buffer.attach_to(wl_surface);
        wl_surface.commit();
    }

    fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            if !ch.is_control() {
                self.password.push(ch);
            }
        }
        self.message = None;
        self.dirty = true;
    }

    fn submit(&mut self, conn: &Connection) {
        if self.busy || self.password.is_empty() {
            return;
        }
        self.busy = true;
        self.message = None;
        self.dirty = true;
        // «Проверка…» должен успеть уйти в композитор до блокирующего PAM.
        self.draw();
        let _ = conn.flush();

        let result = auth::authenticate(&self.username, self.password.bytes());
        self.password.clear();
        self.busy = false;
        self.dirty = true;
        match result {
            Ok(()) => {
                if let Some(lock) = self.session_lock.take() {
                    lock.unlock();
                    let _ = conn.roundtrip();
                }
                self.exit = true;
            }
            Err(_) => {
                self.message = Some(strings::text(self.lang).wrong);
            }
        }
    }

    fn handle_key(&mut self, conn: &Connection, event: &KeyEvent) {
        if self.busy {
            return;
        }
        let sym: u32 = event.keysym.into();
        if self.mods.ctrl && sym == xkb::U {
            self.password.clear();
            self.message = None;
            self.dirty = true;
            return;
        }
        match sym {
            xkb::RETURN | xkb::KP_ENTER => self.submit(conn),
            xkb::BACKSPACE => {
                self.password.pop();
                self.message = None;
                self.dirty = true;
            }
            _ => {
                if let Some(text) = event.utf8.as_deref() {
                    self.type_text(text);
                }
            }
        }
    }

    fn handle_repeat(&mut self, event: &KeyEvent) {
        let sym: u32 = event.keysym.into();
        if sym == xkb::BACKSPACE && !self.busy {
            self.password.pop();
            self.message = None;
            self.dirty = true;
        }
    }
}

impl SessionLockHandler for LockApp {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        self.dirty = true;
    }

    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        // Композитор отказал или снял блокировку. Выходим, не притворяясь,
        // что экран всё ещё защищён.
        self.session_lock = None;
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _: u32,
    ) {
        let (width, height) = configure.new_size;
        if width == 0 || height == 0 {
            return;
        }
        let Some(index) = self
            .surfaces
            .iter()
            .position(|entry| entry.surface.wl_surface() == surface.wl_surface())
        else {
            return;
        };
        {
            let entry = &mut self.surfaces[index];
            entry.width = width;
            entry.height = height;
            entry.configured = true;
            let bytes = width as usize * height as usize * 4 * SCALE as usize * SCALE as usize;
            entry.pool = SlotPool::new(bytes.saturating_mul(3) + 4096, &self.shm).ok();
        }
        self.fill_surface(index);
        self.dirty = true;
    }
}

impl CompositorHandler for LockApp {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

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

impl OutputHandler for LockApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        // Новый монитор во время блокировки тоже обязан получить шторку,
        // иначе на нём останется рабочий стол.
        let Some(lock) = self.session_lock.clone() else {
            return;
        };
        let surface = self.compositor_state.create_surface(qh);
        let lock_surface = lock.create_lock_surface(surface, &output, qh);
        self.surfaces.push(LockSurface::new(lock_surface, output));
        self.dirty = true;
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.surfaces.retain(|surface| surface.output != output);
        self.dirty = true;
    }
}

impl SeatHandler for LockApp {
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
        if capability == Capability::Keyboard
            && let Some(keyboard) = self.keyboard.take()
        {
            keyboard.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl ShmHandler for LockApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl KeyboardHandler for LockApp {
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
    }

    fn press_key(
        &mut self,
        conn: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.handle_key(conn, &event);
        self.draw();
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.handle_repeat(&event);
        self.draw();
    }

    fn update_repeat_info(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: smithay_client_toolkit::seat::keyboard::RepeatInfo,
    ) {
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
        self.dirty = true;
    }
}

impl ProvidesRegistryState for LockApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(LockApp);
wayland_client::delegate_noop!(LockApp: ignore wl_buffer::WlBuffer);
smithay_client_toolkit::delegate_dispatch2!(LockApp);

fn current_user() -> String {
    nix::unistd::User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|user| user.name)
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_default()
}

fn background() -> Option<Background> {
    let home = std::env::var_os("HOME")?;
    let text = std::fs::read_to_string(PathBuf::from(home).join(".config/wallpaper")).ok()?;
    let path = PathBuf::from(text.trim());
    if !greeter::image::is_png(&path) {
        return None;
    }
    Background::load(&path)
}

fn main() {
    let mut args = std::env::args().skip(1);
    if let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            println!("{USAGE}");
            return;
        }
        eprintln!("неизвестный аргумент: {arg}\nhud-lock-rs --help");
        std::process::exit(2);
    }

    let outcome = run();
    if let Err(error) = outcome {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|error| format!("нет Wayland: {error}"))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|error| format!("нет реестра: {error}"))?;
    let qh = event_queue.handle();
    let compositor_state = CompositorState::bind(&globals, &qh)
        .map_err(|error| format!("нет wl_compositor: {error}"))?;
    let seat_state = SeatState::new(&globals, &qh);
    let shm = Shm::bind(&globals, &qh).map_err(|error| format!("нет wl_shm: {error}"))?;
    let session_lock_state = SessionLockState::new(&globals, &qh);
    let ui = ui_palette(palette::load());
    let lang = settings::load().language;

    let mut app = LockApp {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state,
        shm,
        compositor_state,
        session_lock_state,
        session_lock: None,
        surfaces: Vec::new(),
        keyboard: None,
        mods: Modifiers::default(),
        painter: TextPainter::new(&ui_tokens::font_name()),
        ui,
        config: Config::default(),
        background: background(),
        lang,
        username: current_user(),
        password: Secret::new(),
        message: None,
        busy: false,
        dirty: false,
        exit: false,
        last_minute: chrono::Local::now().hour() as i32 * 60 + chrono::Local::now().minute() as i32,
    };

    event_queue
        .roundtrip(&mut app)
        .map_err(|error| format!("реестр не ответил: {error}"))?;
    app.lock(&qh)?;

    while !app.exit {
        if event_queue.flush().is_err() {
            break;
        }
        let _ = event_queue.dispatch_pending(&mut app);
        app.tick();
        app.draw();
        if app.exit {
            break;
        }
        shell::poll_read(&mut event_queue, conn.as_fd(), CLOCK_WAIT_MS);
    }

    // Успешный `submit` уже снял блокировку. Если цикл закончился по другой
    // причине, объект `SessionLock` обязан быть разблокирован, иначе
    // композитор получит protocol error; `Drop` этого не делает.
    if let Some(lock) = app.session_lock.take() {
        lock.unlock();
        let _ = conn.roundtrip();
    }
    Ok(())
}

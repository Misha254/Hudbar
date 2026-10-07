//! `hud-wallpaper-rs` — выбор обоев поверх всего, мышью и клавиатурой.
//!
//! Окно — слой `zwlr_layer_shell`, как у меню: `Overlay` без якорей (композитор
//! центрирует сам), клавиатура `Exclusive`, потеря фокуса закрывает окно.
//! Размер фиксирован карточкой из `wallpaper::view` (960×600): сетка листается
//! внутри, а не растит окно, поэтому `set_size` шлётся один раз до первого
//! commit.
//!
//! Миниатюры грузит фоновый [`wallpaper::thumb`] пул: первый кадр рисуется
//! сразу с заглушками, рендер диска не касается. Повторный вызов закрывает
//! открытое окно через pid-файл — слой не виден в `niri msg windows`.
//!
//! Запуск:
//!   hud-wallpaper-rs [папка]  — открыть выбор, сразу в папке (`all`, если нет)
//!   hud-wallpaper-rs --snapshot <default|search|schemes|empty> <out.png>

use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
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
        keyboard::{KeyEvent, KeyboardHandler, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{LayerShell, LayerSurface},
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
#[allow(dead_code)]
#[path = "../hud/wallpaper.rs"]
mod wallpaper;

use settings_ui::Rect;
use text::{SCALE, TextPainter};
use wallpaper::input::{WpInput, click as click_at, motion as motion_at};
use wallpaper::state::{Outcome, State};
use wallpaper::thumb::{FsSource, Loader, WORKERS};

/// Пространство слоя: по нему правится размытие в `rules.kdl`.
const NAMESPACE: &str = "hudwallpaper";
/// Окно и есть карточка: размер фиксирован, листание внутри.
const WIDTH: u32 = wallpaper::layout::CARD_W as u32;
const HEIGHT: u32 = wallpaper::layout::CARD_H as u32;
/// Имя pid-файла: свой, чтобы не затирать меню.
const PID_FILE: &str = "hudbar-wallpaper.pid";

const USAGE: &str = "\
hud-wallpaper-rs — выбор обоев HUDbar

  hud-wallpaper-rs [папка]
      Открыть выбор. Папка: имя из колонки (anime, nature, …).
      Нет такой папки — корневой вид «all».
  hud-wallpaper-rs --snapshot <default|search|schemes|empty> <out.png> [--theme normal|pixel]
      Снимок состояния без Wayland.
  -h, --help
      Эта справка.";

/// Живое окно выбора обоев.
struct WpApp {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    ctx: layer::Context,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
    painter: TextPainter,
    icons: TextPainter,
    state: State,
    ui: ui_tokens::UiPalette,
    pixel: bool,
    lang: settings::Language,
    loader: Arc<Loader<FsSource>>,
    scroll_acc: f32,
    exit: Arc<AtomicBool>,
}

impl WpApp {
    /// Прямоугольник карточки: окно и есть карточка от нуля.
    fn card() -> Rect {
        Rect::new(0.0, 0.0, WIDTH as f32, HEIGHT as f32)
    }

    /// Видимые пути для загрузчика: то, что на экране прямо сейчас.
    fn visible_paths(&self) -> Vec<PathBuf> {
        self.state
            .grid
            .visible_cells()
            .iter()
            .filter_map(|index| self.state.filtered.get(*index).cloned())
            .collect()
    }

    /// Соседние ряды для предзагрузки.
    fn prefetch_paths(&self) -> Vec<PathBuf> {
        self.state
            .grid
            .prefetch_cells()
            .iter()
            .filter_map(|index| self.state.filtered.get(*index).cloned())
            .collect()
    }

    /// Слепок набора, от которого зависит заказ миниатюр.
    fn set_key(&self) -> (String, String, usize, usize) {
        (
            self.state.current_folder.clone(),
            self.state.query.clone(),
            self.state.grid.top_row,
            self.state.filtered.len(),
        )
    }

    /// Заказ миниатюр после смены состояния: набор сменился — поколение
    /// вперёд и устаревшие задания в топку; тот же набор — просто дозаказ
    /// недостающего без сброса полёта.
    fn sync_thumbs(&mut self, before: (String, String, usize, usize)) {
        if before != self.set_key() {
            self.loader.bump_generation();
        }
        self.loader
            .request(&self.visible_paths(), &self.prefetch_paths());
        self.ctx.dirty = true;
    }

    /// Команда клавиатуры к модели.
    fn apply(&mut self, input: WpInput) {
        let before = self.set_key();
        let outcome = wallpaper::input::apply(&mut self.state, input);
        self.sync_thumbs(before);
        self.finish(outcome);
    }

    /// Исход действия: применить файл или закрыть окно.
    fn finish(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::None => {}
            Outcome::Close => self.close(),
            Outcome::Apply { path, scheme } => {
                // Окно закрывается сразу: смена занимает секунды, а wall.sh
                // сам уведомляет. Ошибка — в журнал, не молча.
                wallpaper::apply_detached(&path.to_string_lossy(), &scheme);
                self.close();
            }
        }
    }

    fn close(&mut self) {
        debug_log("закрытие по команде");
        self.exit.store(true, Ordering::Relaxed);
    }

    /// Разбирает клавишу и применяет команду. Нажатие и повтор — одним кодом.
    fn handle_key(&mut self, event: KeyEvent) {
        let key = wallpaper::input::event(
            event.keysym.into(),
            event.utf8.as_deref(),
            self.mods.ctrl,
            self.mods.shift,
            self.mods.logo,
        );
        self.apply(wallpaper::input::translate(key, self.state.query.as_str()));
    }

    /// Клик: папка, плитка (второй клик — применить), схема, крестик поиска.
    fn handle_click(&mut self, x: f32, y: f32) {
        let before = self.set_key();
        let outcome = click_at(&mut self.state, Self::card(), x, y);
        self.sync_thumbs(before);
        self.finish(outcome);
    }

    /// Движение мыши: только подсветка.
    fn handle_motion(&mut self, x: f32, y: f32) {
        if motion_at(&mut self.state, Self::card(), x, y) {
            self.ctx.dirty = true;
        }
    }

    /// Колесо: прокрутка рядов. Накопление дробного скролла — как в меню:
    /// тачпад присылает value120 меньше 120.
    fn handle_wheel(&mut self, delta: f32) {
        self.scroll_acc += -delta;
        let steps = self.scroll_acc.trunc();
        self.scroll_acc -= steps;
        if steps != 0.0 {
            let before = self.set_key();
            self.state.scroll_rows(steps as isize);
            self.sync_thumbs(before);
        }
    }

    /// Рисует кадр карточкой от нуля и отдаёт буфер слою.
    fn draw(&mut self) {
        if self.ctx.width == 0 || self.ctx.height == 0 {
            return;
        }
        let pw = (self.ctx.width as f32 * SCALE) as u32;
        let ph = (self.ctx.height as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        let card = Self::card();
        let provider: &dyn wallpaper::thumb::Provider = &*self.loader;
        let frame = wallpaper::view::Frame {
            state: &self.state,
            card,
            pixel: self.pixel,
            ui: self.ui,
            lang: self.lang,
            provider,
        };
        let mut canvas = wallpaper::view::Canvas::new(
            &mut pixmap,
            &mut self.painter,
            &mut self.icons,
            self.pixel,
            self.ui,
        );
        wallpaper::view::draw_card(&mut canvas, &frame);
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

impl KeyboardHandler for WpApp {
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

    /// Потеря клавиатуры закрывает окно: слой с `Exclusive` иначе забрал бы
    /// её себе без способа закрыть безмышно.
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        debug_log("закрытие: клавиатура ушла (keyboard leave)");
        self.close();
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.handle_key(event);
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
        self.handle_key(event);
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

impl PointerHandler for WpApp {
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
            let position = (event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.handle_motion(position.0, position.1);
                }
                PointerEventKind::Leave { .. } => {}
                PointerEventKind::Press { button: 0x110, .. } => {
                    self.handle_click(position.0, position.1);
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let scroll = vertical;
                    let delta = if scroll.value120 != 0 {
                        scroll.value120 as f32 / 120.0_f32
                    } else if scroll.discrete != 0 {
                        scroll.discrete as f32
                    } else if scroll.absolute != 0.0 {
                        (scroll.absolute.signum() * 3.0) as f32
                    } else {
                        continue;
                    };
                    self.handle_wheel(delta);
                }
                _ => {}
            }
        }
    }
}

impl CompositorHandler for WpApp {
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

impl OutputHandler for WpApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for WpApp {
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

impl smithay_client_toolkit::shell::wlr_layer::LayerShellHandler for WpApp {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if layer.wl_surface() == self.ctx.layer.wl_surface() {
            debug_log("закрытие: композитор прислал closed");
            self.close();
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure,
        _: u32,
    ) {
        if layer.wl_surface() != self.ctx.layer.wl_surface() {
            return;
        }
        if configure.new_size.0 > 0 {
            self.ctx.width = configure.new_size.0;
        }
        if configure.new_size.1 > 0 {
            self.ctx.height = configure.new_size.1;
        }
        if self.ctx.width == 0 || self.ctx.height == 0 {
            self.ctx.width = WIDTH;
            self.ctx.height = HEIGHT;
        }
        self.ctx.configured = true;
        self.ctx.dirty = true;
    }
}

impl ShmHandler for WpApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for WpApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(WpApp);
smithay_client_toolkit::delegate_dispatch2!(WpApp);

/// Отладочная строка в stderr: достаточно `RUST_LOG=debug`.
fn debug_log(message: &str) {
    let level = std::env::var("RUST_LOG").unwrap_or_default();
    if level.contains("debug") || level.contains("trace") {
        eprintln!("hud-wallpaper: {message}");
    }
}

/// Флаг остановки для обработчика сигнала: только атомарная запись.
static SIGNAL_EXIT: AtomicBool = AtomicBool::new(false);

/// Обработчик SIGTERM: только атомарная запись, остальное — обычный выход.
extern "C" fn on_sigterm(_signal: i32) {
    SIGNAL_EXIT.store(true, Ordering::Relaxed);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return;
    }
    let outcome = match args.first().map(String::as_str) {
        Some("--snapshot") => snapshot(&args[1..]),
        Some(first) if first.starts_with('-') => {
            eprintln!("неизвестный флаг: {first}\nhud-wallpaper-rs --help");
            std::process::exit(1);
        }
        Some(folder) => run(Some(folder)),
        None => run(None),
    };
    if let Err(error) = outcome {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn snapshot(args: &[String]) -> Result<(), String> {
    // --snapshot <default|search|schemes|empty> <out.png> [--theme ...]
    if args.len() < 2 {
        return Err(
            "hud-wallpaper-rs --snapshot <default|search|schemes|empty> <out.png> [--theme normal|pixel]"
                .to_string(),
        );
    }
    let mode = &args[0];
    let out = &args[1];
    let theme = args
        .iter()
        .position(|a| a == "--theme")
        .and_then(|i| args.get(i + 1));
    let pixel = match theme.map(String::as_str) {
        Some("pixel") => true,
        Some("normal") | None => false,
        Some(other) => return Err(format!("неизвестная тема: {other}")),
    };

    let state = match mode.as_str() {
        "default" => State::open(),
        "search" => {
            let mut state = State::open();
            state.zone = wallpaper::state::Zone::Grid;
            state.query = "search".into();
            // keep only up to 3 results to match "search — 3 результата"
            state.filtered = state.filtered.iter().take(3).cloned().collect();
            state.grid.set_len(state.filtered.len());
            // После смены набора выбран первый результат, а не индекс из
            // прошлого состояния — иначе подсветка уезжает за пределы списка.
            state.grid.select_first();
            state
        }
        "schemes" => {
            let mut state = State::open();
            state.zone = wallpaper::state::Zone::Schemes;
            // keep filtered for grid view, but focus schemes
            state
        }
        "empty" => {
            let mut state = State::open();
            state.query = "zzz_no_match".into();
            state.filtered.clear();
            state.grid.set_len(0);
            state
        }
        other => return Err(format!("неизвестный снапшот-режим: {other}")),
    };

    let palette = palette::load();
    let pixmap = wallpaper::view::render(&state, pixel, palette, settings::Language::Ru);
    write_png(&pixmap, out)
}

/// Запись пикселей в png, имя файла задаётся в out.
fn write_png(pixmap: &tiny_skia::Pixmap, out: &str) -> Result<(), String> {
    let mut encoder = png::Encoder::new(
        std::fs::File::create(Path::new(out)).map_err(|e| format!("no file: {e}"))?,
        pixmap.width(),
        pixmap.height(),
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder
        .write_header()
        .map_err(|e| format!("png header: {e}"))?;
    png_writer
        .write_image_data(pixmap.data())
        .map_err(|e| format!("png data: {e}"))?;
    Ok(())
}

/// Живой цикл: один экземпляр, слой, ввод, перерисовка.
fn run(folder: Option<&str>) -> Result<(), String> {
    // Один экземпляр: слой не виден в `niri msg windows`, поэтому единственный
    // способ узнать, открыто ли окно, — pid-файл.
    let pid_file = instance::pid_path_for(PID_FILE);
    if let instance::Startup::CloseRunning(pid) =
        instance::startup_for(PID_FILE, std::path::Path::new("/proc"))
    {
        debug_log(&format!("окно уже открыто: pid {pid}"));
        // Повторный вызов закрывает: сигнал, а не второй слой.
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
    debug_log(&format!("pid {pid}, файл {}", pid_file.display()));
    let result = event_loop(folder);
    // pid-файл снимается на любом выходе, включая падение: иначе следующий
    // запуск увидит «живой» pid и не откроет окно.
    instance::clear(&pid_file, pid);
    debug_log("pid-файл снят");
    result
}

/// Основной цикл: слой, ввод, перерисовка.
fn event_loop(folder: Option<&str>) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|error| format!("нет Wayland: {error}"))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|error| format!("нет реестра: {error}"))?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|error| format!("нет wl_compositor: {error}"))?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|_| "нет zwlr_layer_shell".to_string())?;
    let shm = Shm::bind(&globals, &qh).map_err(|error| format!("нет wl_shm: {error}"))?;

    // Тема и язык читаются при старте из `settings.json`, как у меню.
    let loaded = settings::load();
    let pixel = loaded.theme.as_deref() == Some("pixel");
    let lang = loaded.language;
    let palette = palette::load();
    let ui = ui_tokens::ui_palette(pixel, palette);

    // Модель заводится до слоя, но размер фиксирован карточкой — первый commit
    // идёт с ним же, нулей протокол не увидит.
    let state = State::open_with(folder);
    let loader = Loader::new(FsSource::new(), WORKERS);

    let ctx = layer::open(
        &compositor,
        &layer_shell,
        &shm,
        &qh,
        layer::Shape {
            namespace: Some(NAMESPACE),
            width: WIDTH,
            height: HEIGHT,
            max_height: HEIGHT,
            scale: SCALE as u32,
        },
    )?;
    debug_log(&format!(
        "слой {NAMESPACE}: set_size {WIDTH}x{HEIGHT} до commit"
    ));

    let mut app = WpApp {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        ctx,
        pointer: None,
        keyboard: None,
        mods: Modifiers::default(),
        painter: TextPainter::new(&settings_view::font_name(pixel)),
        icons: TextPainter::new(settings_icons::FONT),
        state,
        ui,
        pixel,
        lang,
        loader,
        scroll_acc: 0.0,
        exit: Arc::new(AtomicBool::new(false)),
    };
    // Первый заказ миниатюр: видимые и соседи. Кадр рисуется сразу с
    // заглушками — диска первый кадр не ждёт.
    app.loader
        .request(&app.visible_paths(), &app.prefetch_paths());

    unsafe {
        let _ = nix::sys::signal::signal(
            nix::sys::signal::Signal::SIGTERM,
            nix::sys::signal::SigHandler::Handler(on_sigterm),
        );
    }

    while !app.exit.load(Ordering::Relaxed) && !SIGNAL_EXIT.load(Ordering::Relaxed) {
        if event_queue.flush().is_err() {
            break;
        }
        let _ = event_queue.dispatch_pending(&mut app);
        // Готовые миниатюры будят отрисовку: рабочие ставят флаг, цикл его
        // подбирает здесь же — отдельного пробуждения не надо, тик частый.
        if app.loader.take_dirty() {
            app.ctx.dirty = true;
        }
        if app.ctx.configured && app.ctx.dirty {
            app.ctx.dirty = false;
            app.draw();
        }
        if app.exit.load(Ordering::Relaxed) || SIGNAL_EXIT.load(Ordering::Relaxed) {
            break;
        }
        // События ждутся с коротким таймаутом: SIGTERM и готовые миниатюры
        // должны закрывать и дорисовывать без ожидания ввода.
        if let Some(guard) = event_queue.prepare_read() {
            let mut fds = [PollFd::new(conn.as_fd(), PollFlags::POLLIN)];
            // Не держать ввод и готовые миниатюры в очереди дольше кадра.
            let _ = poll(&mut fds, 8u16);
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

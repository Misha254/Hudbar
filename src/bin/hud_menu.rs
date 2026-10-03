//! `hud-menu-rs` — меню HUDbar поверх всего, только с клавиатуры.
//!
//! Окно сделано слоем `zwlr_layer_shell`, а не `xdg_toplevel`: меню должно
//! появляться поверх панели и всех окон, не меняя раскладку рабочих областей
//! и не требуя `niri msg action focus-window`. Слой в слое `Overlay` без якорей —
//! композитор центрирует его сам, а размер равен карточке из `menu::view`.
//! Когда высота карточки меняется (поиск, число строк), слой получает новый
//! `set_size` и новый буфер.
//!
//! Клавиатура — `Exclusive`: меню открыто по бинду, и первое же нажатие должно
//! дойти до него. Плата — клавиатура не вернётся, пока слой жив, поэтому потеря
//! фокуса (`keyboard leave`) закрывает меню, иначе закрыть его без клавиатуры
//! было бы нечем. `OnDemand` тут опаснее: после чужой комбинации niri мог бы не
//! вернуть фокус без явного запроса, и меню осталось бы с мёртвой клавиатурой.
//!
//! Затемнение не рисуется: под меню должны быть видны терминал и обои. Вместо
//! него карточка делается чуть прозрачной — так размытие слоя видно на её
//! границе, а не только вокруг неё.
//!
//! Всё содержимое рисуется теми же функциями `menu::view`, что и снимки: форка
//! кода между окном и `--snapshot` нет.
//!
//! Запуск:
//!   hud-menu-rs [раздел]        — открыть меню, по возможности сразу на разделе
//!   hud-menu-rs --snapshot ...  — снимки без Wayland, см. `menu::snapshot`

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface},
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
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
#[path = "../hud/menu/mod.rs"]
mod menu;
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
#[path = "../hud/settings_snapshot.rs"]
mod settings_snapshot;
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

use menu::state::{Frame, Outcome};
use menu::view::{self, MenuView};
use menu::input::MenuInput;
use menu::state::Menu;
use settings_ui::Rect;
use text::{SCALE, TextPainter};

/// Пространство слоя: по нему правится размытие в `rules.kdl`.
const NAMESPACE: &str = "hudmenu";
/// Ширина окна равна ширине карточки: окно и есть карточка.
const WIDTH: u32 = view::CARD_W as u32;
/// Высота, которой заведомо хватает под любую карточку: нужна только для
/// размера пула буферов.
const MAX_H: u32 = view::HEADER_H as u32
    + (view::ROW_H_SEARCH * view::MAX_ROWS as f32) as u32
    + view::FOOTER_H as u32;
/// Строк на страницу прокрутки: сколько влезает в карточку целиком.
const PAGE: isize = view::MAX_ROWS as isize;

const USAGE: &str = "\
hud-menu-rs — меню HUDbar

  hud-menu-rs [раздел]
      Открыть меню. Раздел: apps, panel, style, notifications, capture,
      keybinds, system, about. Неизвестное имя открывает корень.
  hud-menu-rs --snapshot <root|style|search|empty> <out.png> [--theme normal|pixel]
      Снимок состояния без Wayland.
  hud-menu-rs --all <каталог>
      Все состояния в обеих темах.
  hud-menu-rs --icons <каталог>
      Галерея иконок в обеих темах.
  -h, --help
      Эта справка.";

/// Живое окно меню.
struct MenuApp {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
    painter: TextPainter,
    icons: TextPainter,
    menu: Menu,
    palette: palette::Palette,
    /// Меню открыли сразу на разделе: `Esc` с него закрывает, а не возвращает
    /// на корень, до которого пользователь не доходил.
    direct: bool,
    /// Остаток дробного скролла: тачпад присылает value120 меньше 120, и без
    /// накопления одно деление давало ноль строк.
    scroll_acc: f32,
    width: u32,
    height: u32,
    configured: bool,
    dirty: bool,
    exit: Arc<AtomicBool>,
}

impl MenuApp {
    /// Прямоугольник карточки в окне: окно и есть карточка, поэтому она
    /// начинается в нуле. Размер — тот же расчёт, что у снимков.
    fn card(&self, frame: &Frame) -> Rect {
        let search = view::search_mode(frame);
        let height = view::card_height_for(frame.items.len(), search);
        Rect::new(0.0, 0.0, WIDTH as f32, height)
    }

    /// Высота, которую просит модель. Нужен `set_size` при входе в подменю и
    /// при фильтрации: число строк меняет высоту карточки.
    fn wanted_height(&self) -> u32 {
        let frame = self.menu.frame();
        let height = view::card_height_for(frame.items.len(), view::search_mode(&frame));
        height.round() as u32
    }

    /// Обновляет размер слоя, если карточка изменила высоту. Снапшота слоя
    /// уменьшиться не может, но текст перестаёт попадать в старую рамку.
    fn sync_size(&mut self) {
        let wanted = self.wanted_height();
        if self.configured && wanted == self.height {
            return;
        }
        self.height = wanted;
        self.layer.set_size(WIDTH, wanted);
        self.layer.commit();
        self.dirty = true;
    }

    /// Применяет команду ввода к модели. Модель не знает про окно, поэтому
    /// «закрыть» решается здесь.
    fn apply(&mut self, input: MenuInput) {
        match input {
            MenuInput::Ignore => {}
            MenuInput::Move(delta) => self.menu.move_sel(delta),
            MenuInput::Page(lines) => self.menu.scroll(lines * PAGE),
            MenuInput::Jump(end) => {
                let last = self.menu.current().list.len().saturating_sub(1);
                self.menu.current_mut().list.select(usize::from(end) * last);
            }
            MenuInput::Activate => self.activate(),
            MenuInput::Back => {
                // Прямой вход: Esc с открытого раздела закрывает меню, а не
                // возвращает на корневый уровень, которого не было на экране.
                if self.menu.depth() <= usize::from(self.direct && self.menu.depth() == 1) {
                    self.close();
                } else {
                    self.menu.back();
                }
            }
            MenuInput::Close => self.close(),
            MenuInput::Erase => self.menu.backspace(),
            MenuInput::ClearQuery => self.menu.clear_query(),
            MenuInput::Type(ch) => self.menu.type_char(ch),
        }
        self.dirty = true;
    }

    /// Enter: войти в подменю или применить значение. Пустое подменю и
    /// листовое действие оставляют меню на месте.
    fn activate(&mut self) {
        if self.menu.enter() == Outcome::Changed {
            self.menu.clear_status();
        }
    }

    fn close(&mut self) {
        self.menu.clear_status();
        self.exit.store(true, Ordering::Relaxed);
    }

    /// Клик по строке: выбрать и сразу активировать, как в списках файлов.
    fn click(&mut self, x: f32, y: f32) {
        let frame = self.menu.frame();
        let card = self.card(&frame);
        if let Some(index) = view::row_at(&frame, card, x, y) {
            self.menu.current_mut().list.select(index);
            self.activate();
        }
    }

    /// Разбирает клавишу и применяет команду. Вынесено, чтобы нажатие и
    /// повтор шли одним кодом.
    fn handle_key(&mut self, event: KeyEvent) {
        let key = menu::input::event(
            event.keysym.into(),
            event.utf8.as_deref(),
            self.mods.ctrl,
            self.mods.shift,
            self.mods.logo,
        );
        let input = menu::input::translate(key, self.menu.query());
        self.apply(input);
    }

    /// Рисует кадр и отдаёт буфер слою.
    fn draw(&mut self) {
        if !self.configured || self.width == 0 || self.height == 0 {
            return;
        }
        let pw = (WIDTH as f32 * SCALE) as u32;
        let ph = (self.height as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        // Буфер заливается прозрачным: сквозь него видно рабочий стол, а
        // правило слоя размывает то, что под карточкой.
        pixmap.fill(palette::Rgba(0, 0, 0, 0).to_tiny());
        let frame = self.menu.frame();
        let (ui, scale) = view::theme(false, self.palette);
        let mut window = MenuView {
            pixmap: &mut pixmap,
            painter: &mut self.painter,
            icons: &mut self.icons,
            ui,
            scale,
            pixel: false,
        };
        let card = view::render_window(&mut window, &frame);
        view::apply_card_alpha(&mut pixmap, card, view::CARD_ALPHA);

        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
        else {
            // Буфер занят панелью: попробуем на следующем тике.
            self.dirty = true;
            return;
        };
        let (dst, _) = canvas.as_chunks_mut::<4>();
        let (src, _) = pixmap.data().as_chunks::<4>();
        for (out, input) in dst.iter_mut().zip(src.iter()) {
            *out = [input[2], input[1], input[0], input[3]];
        }
        let surface = self.layer.wl_surface();
        surface.set_buffer_scale(SCALE as i32);
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        let _ = buffer.attach_to(surface);
        self.layer.commit();
    }
}

impl KeyboardHandler for MenuApp {
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

    /// Потеря клавиатуры закрывает меню. Иначе слой с `Exclusive` забрал бы
    /// клавиатуру себе и вернул её только после закрытия, а закрыть его без
    /// клавиатуры нечем.
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
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

    /// Удержание стрелки работает так же, как её нажатие: композитор шлёт
    /// повторы сам, а свой таймер в меню не нужен — список короткий.
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

impl PointerHandler for MenuApp {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if event.surface != *self.layer.wl_surface() {
                continue;
            }
            let position = (event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                // Меню работает с клавиатуры, поэтому указатель только
                // подсвечивает строку, но никогда её не выбирает: иначе
                // случайное движение мыши сдвигало бы выбор.
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {}
                PointerEventKind::Leave { .. } => {}
                PointerEventKind::Press { button: 0x110, .. } => self.click(position.0, position.1),
                PointerEventKind::Axis { vertical, .. } => {
                    let scroll = vertical;
                    // Тачпад шлёт `value120`: одно деление — это 120, а в
                    // единицах строк это меньше одного. Поэтому деления
                    // копятся остатком, иначе одно нажатие колеса молчало бы.
                    let delta = if scroll.value120 != 0 {
                        scroll.value120 as f32 / 120.0_f32
                    } else if scroll.discrete != 0 {
                        scroll.discrete as f32
                    } else if scroll.absolute != 0.0 {
                        (scroll.absolute.signum() * 3.0) as f32
                    } else {
                        return;
                    };
                    self.scroll_acc += -delta;
                    let steps = self.scroll_acc.trunc();
                    self.scroll_acc -= steps;
                    if steps != 0.0 {
                        self.apply(MenuInput::Page(steps as isize));
                    }
                }
                _ => {}
            }
        }
    }
}

impl CompositorHandler for MenuApp {
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
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
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

impl OutputHandler for MenuApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for MenuApp {
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

impl smithay_client_toolkit::shell::wlr_layer::LayerShellHandler for MenuApp {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if layer.wl_surface() == self.layer.wl_surface() {
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
        if layer.wl_surface() != self.layer.wl_surface() {
            return;
        }
        if configure.new_size.0 != 0 {
            self.width = configure.new_size.0;
        }
        if configure.new_size.1 != 0 {
            self.height = configure.new_size.1;
        }
        self.configured = true;
        self.dirty = true;
    }
}

impl ShmHandler for MenuApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for MenuApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(MenuApp);
smithay_client_toolkit::delegate_dispatch2!(MenuApp);

/// Флаг остановки, доступный обработчику сигнала: обработчик не может
/// захватить состояние приложения, поэтому читает общий атомарный флаг.
static SIGNAL_EXIT: AtomicBool = AtomicBool::new(false);

/// Обработчик SIGTERM: только атомарная запись. Всё остальное делает обычный
/// код выхода — так требования `signal-safety` соблюдены.
extern "C" fn on_sigterm(_signal: i32) {
    SIGNAL_EXIT.store(true, Ordering::Relaxed);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return;
    }
    if let Some(mode) = args.first().map(String::as_str) {
        let outcome = match mode {
            "--snapshot" => menu::snapshot::run(&args[1..]),
            "--all" => render_all(args.get(1).map(String::as_str)),
            "--icons" => render_icons(args.get(1).map(String::as_str)),
            _ => run(args.first().map(String::as_str).unwrap_or("")),
        };
        if let Err(error) = outcome {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run("") {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

/// `--all <каталог>`: все состояния в обеих темах.
fn render_all(dir: Option<&str>) -> Result<(), String> {
    let dir = dir.ok_or("--all требует путь к каталогу")?;
    let path = std::path::Path::new(dir);
    std::fs::create_dir_all(path).map_err(|error| format!("не создался каталог {dir}: {error}"))?;
    for file in menu::snapshot::render_all(path)? {
        println!("{}", file.display());
    }
    Ok(())
}

/// `--icons <каталог>`: галерея иконок в обеих темах.
fn render_icons(dir: Option<&str>) -> Result<(), String> {
    let dir = dir.ok_or("--icons требует путь к каталогу")?;
    let path = std::path::Path::new(dir);
    std::fs::create_dir_all(path).map_err(|error| format!("не создался каталог {dir}: {error}"))?;
    for file in menu::snapshot::render_icon_gallery(path)? {
        println!("{}", file.display());
    }
    Ok(())
}

/// Живой цикл меню.
fn run(section: &str) -> Result<(), String> {
    // Один экземпляр: слой не виден в `niri msg windows`, поэтому единственный
    // способ узнать, открыто ли меню, — pid-файл.
    let pid_file = menu::instance::pid_path();
    if let menu::instance::Startup::CloseRunning(pid) =
        menu::instance::startup(std::path::Path::new("/proc"))
    {
        // Повторный вызов закрывает: сигнал, а не второй слой.
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), nix::sys::signal::Signal::SIGTERM)
            .map_err(|error| format!("не закрыть прошлый pid {pid}: {error}"))?;
        return Ok(());
    }
    let pid = std::process::id() as i32;
    if let Err(error) = menu::instance::store(&pid_file, pid) {
        return Err(format!("не записать pid-файл: {error}"));
    }
    // Идентификатор раздела проверяется здесь: неизвестный открывает корень.
    let known = menu::section_title(section).is_some();
    let result = event_loop(section, &pid_file);
    // pid-файл снимается на любом выходе, включая падение: иначе следующий
    // запуск увидит «живой» pid и не откроет меню.
    menu::instance::clear(&pid_file, pid);
    let _ = known;
    result
}

/// Основной цикл: слой, ввод, перерисовка.
fn event_loop(section: &str, pid_file: &std::path::Path) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|error| format!("нет Wayland: {error}"))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|error| format!("нет реестра: {error}"))?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|error| format!("нет wl_compositor: {error}"))?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|_| "нет zwlr_layer_shell".to_string())?;
    let shm = Shm::bind(&globals, &qh).map_err(|error| format!("нет wl_shm: {error}"))?;

    let surface = compositor.create_surface(&qh);
    // Слой `Overlay` без якорей: композитор центрирует его по выходу. Размер
    // равен карточке, поэтому окно не перекрывает рабочую область и не
    // меняет её размеры.
    let layer = layer_shell.create_layer_surface(
        &qh,
        surface.clone(),
        Layer::Overlay,
        Some(NAMESPACE),
        None,
    );
    layer.set_anchor(Anchor::empty());
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_exclusive_zone(-1);
    layer.set_margin(0, 0, 0, 0);
    layer.commit();

    // Буфер под максимальную карточку: 520x(52+10*52+40) — этого хватает для
    // любого состояния, а пересоздавать пул при смене высоты незачем.
    let frame_bytes =
        WIDTH as usize * MAX_H as usize * 4 * (SCALE as usize) * (SCALE as usize);
    let pool = SlotPool::new(frame_bytes * 3 + 4_000_000, &shm)
        .map_err(|error| format!("не создать пул буферов: {error}"))?;

    let mut app = MenuApp {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        pool,
        layer,
        pointer: None,
        keyboard: None,
        mods: Modifiers::default(),
        painter: TextPainter::new(&view::font_name(false)),
        icons: TextPainter::new(settings_icons::FONT),
        menu: menu::demo_menu(),
        palette: palette::load(),
        scroll_acc: 0.0,
        direct: false,
        width: WIDTH,
        height: 0,
        configured: false,
        dirty: true,
        exit: Arc::new(AtomicBool::new(false)),
    };
    // Прямой вход: неизвестный идентификатор молча открывает корень.
    if menu::open_section(&mut app.menu, section) {
        app.direct = true;
    }
    app.sync_size();
    let _ = pid_file;

    unsafe {
        let _ = nix::sys::signal::signal(
            nix::sys::signal::Signal::SIGTERM,
            nix::sys::signal::SigHandler::Handler(on_sigterm),
        );
    }

    while !app.exit.load(Ordering::Relaxed) && !SIGNAL_EXIT.load(Ordering::Relaxed) {
        app.sync_size();
        if event_queue.flush().is_err() {
            break;
        }
        let _ = event_queue.blocking_dispatch(&mut app);
        if app.dirty {
            app.dirty = false;
            app.draw();
        }
    }
    Ok(())
}

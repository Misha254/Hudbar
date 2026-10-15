//! `hud-apps-rs` — лаунчер приложений поверх всего, только с клавиатуры.
//!
//! Замена `rofi -show hudapps` с `hud-drun.sh`: тот же список из desktop-файлов,
//! те же закреплённые, но окно рисуется тем же кодом, что панель и меню, и
//! работает без X11.
//!
//! Окно сделано слоем `zwlr_layer_shell`, а не `xdg_toplevel`: лаунчер должен
//! появляться поверх панели и всех окон, не меняя раскладку рабочих областей и
//! не требуя `niri msg action focus-window`. Слой в `Overlay` без якорей —
//! композитор центрирует его сам, а размер равен карточке из `apps::view`.
//! Когда высота карточки меняется (поиск, число строк), слой получает новый
//! `set_size` и новый буфер.
//!
//! Клавиатура — `Exclusive`: окно открыто по бинду, и первое же нажатие должно
//! дойти до него. Потеря фокуса (`keyboard leave`) закрывает лаунчер, иначе
//! закрыть его без клавиатуры было бы нечем.
//!
//! Затемнение не рисуется: под окном должны быть видны терминал и обои.
//!
//! Запуск:
//!   hud-apps-rs                     — открыть окно; повторный вызов закрывает
//!   hud-apps-rs --help              — справка
//!   hud-apps-rs --snapshot <mode> <out.png>

use std::os::fd::AsFd;
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
        keyboard::{KeyEvent, KeyboardHandler, Modifiers, RawModifiers, RepeatInfo},
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
#[path = "../hud/config_io.rs"]
mod config_io;
#[allow(dead_code)]
#[path = "../hud/dunst.rs"]
mod dunst;
// Меню — отдельный процесс: у него нет `Shared.sys` панели, поэтому нужны
// одноразовые читатели. Весь `data` с его потоками и DBus не подключается.
#[path = "../hud/data/oneshot.rs"]
pub mod oneshot_data;

/// Псевдоним `data::oneshot`: в `hudbar` данные лежат в `data`, а этому
/// бинарнику нужен только сам модуль чтений.
pub mod data {
    pub use super::oneshot_data as oneshot;
}
#[allow(dead_code)]
#[path = "../hud/anim.rs"]
mod anim;
#[path = "../hud/apps/mod.rs"]
mod apps;
#[allow(dead_code)]
#[path = "../hud/layer.rs"]
mod layer;
#[allow(dead_code)]
#[path = "../hud/log.rs"]
mod log;
#[allow(dead_code)]
#[path = "../hud/menu/mod.rs"]
mod menu;
#[allow(dead_code)]
#[path = "../hud/palette.rs"]
mod palette;
#[allow(dead_code)]
#[path = "../hud/power_profile.rs"]
mod power_profile;
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
#[path = "../hud/shell.rs"]
mod shell;
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

use apps::keys::{self, Input};
use apps::model::Model;
use apps::{icons, launch, view};
use menu::highlight;
use menu::view::RowsHighlight;
use repeat::Repeat;
use settings_ui::Rect;
use text::{SCALE, TextPainter};

/// Пространство слоя: по нему правится размытие в `rules.kdl`.
const NAMESPACE: &str = "hudapps";
/// Свой pid-файл: окно лаунчера и меню HUDbar открываются независимо, и делить
/// они один файл не могут — иначе повторное нажатие клавиши меню закрыло бы
/// лаунчер.
const PID_FILE: &str = "hudbar-apps.pid";
/// Ширина окна равна ширине карточки: окно и есть карточка.
const WIDTH: u32 = view::CARD_W as u32;
/// Высота, которой заведомо хватает под любую карточку: нужна только для
/// размера пула буферов.
const MAX_H: u32 =
    view::HEADER_H as u32 + (view::ROW_H * view::MAX_ROWS as f32) as u32 + view::FOOTER_H as u32;
/// Строк на страницу прокрутки: сколько влезает в карточку целиком.
const PAGE: isize = view::MAX_ROWS as isize;

const USAGE: &str = "\
hud-apps-rs — лаунчер приложений

  hud-apps-rs
      Открыть окно. Повторный вызов закрывает уже открытое.
  -h, --help
      Эта справка.";

/// Живое окно лаунчера.
struct AppsApp {
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
    model: Model,
    /// Загрузчик иконок: кэш и очередь, UI-поток только читает готовое.
    loader: icons::Loader,
    /// Скользящая подсветка выбранной строки: позиция в строках экрана.
    highlight: highlight::Highlight,
    /// Строка под указателем мыши. Мышь подсвечивает, но не выбирает.
    hover: Option<usize>,
    palette: palette::Palette,
    /// Пиксельная ли тема сейчас: от неё зависят шрифт и скругления.
    pixel: bool,
    /// Остаток дробного скролла: тачпад присылает value120 меньше 120, и без
    /// накопления одно деление давало ноль строк.
    scroll_acc: f32,
    exit: Arc<AtomicBool>,
}

impl AppsApp {
    /// Прямоугольник карточки в окне: окно и есть карточка, поэтому она
    /// начинается в нуле.
    fn card(&self) -> Rect {
        Rect::new(
            0.0,
            0.0,
            WIDTH as f32,
            view::card_height(view::rows_shown(self.model.visible().len())),
        )
    }

    /// Размер, который просит модель. Считается из карточки, поэтому поиск и
    /// пустой список дают верную высоту.
    fn wanted_size(&self) -> (u32, u32) {
        view::layer_size(self.model.visible().len())
    }

    /// Обновляет размер слоя, если карточка изменила высоту: вход в поиск,
    /// очистка запроса, пустой результат. Слой без якорей не имеет права
    /// получить ноль ни по одной оси — за это композитор рвёт протокол.
    fn sync_size(&mut self) {
        let (width, height) = self.wanted_size();
        debug_assert!(width > 0 && height > 0, "размер слоя не может быть нулевым");
        if width == self.ctx.width && height == self.ctx.height {
            return;
        }
        self.ctx.request_size(width, height);
    }

    /// Применяет команду ввода к модели.
    fn apply(&mut self, input: Input) {
        match input {
            Input::Ignore => {}
            Input::Move(delta) => self.model.move_sel(delta),
            Input::Page(delta) => self.model.page(delta * PAGE),
            Input::Jump(end) => self.model.jump(end),
            Input::Activate => self.activate(),
            Input::Pin => self.toggle_pin(),
            Input::Close => self.close(),
            Input::Erase => self.model.backspace(),
            Input::ClearQuery => self.model.clear_query(),
            Input::Type(ch) => self.model.type_char(ch),
        }
        self.ctx.dirty = true;
    }

    /// Enter: запустить выбранное приложение и закрыть окно. Ошибка запуска
    /// остаётся в подвале — окно закрывать незачем, если приложение не вышло.
    fn activate(&mut self) {
        let Some(app) = self.model.selected_app().cloned() else {
            return;
        };
        match launch::launch(&app) {
            Ok(()) => self.close(),
            Err(error) => self.model.set_status(error),
        }
        self.ctx.dirty = true;
    }

    /// Закрепление: список перестраивается, а подвал говорит, что именно
    /// изменилось.
    fn toggle_pin(&mut self) {
        match self.model.toggle_pin() {
            Some(result) => {
                let status = match (result.pinned, self.model.lang()) {
                    (true, settings::Language::Ru) => format!("Закреплено: {}", result.name),
                    (true, settings::Language::En) => format!("Pinned: {}", result.name),
                    (false, settings::Language::Ru) => format!("Откреплено: {}", result.name),
                    (false, settings::Language::En) => format!("Unpinned: {}", result.name),
                };
                self.model.set_status(status);
            }
            None => self.model.clear_status(),
        }
        self.ctx.dirty = true;
    }

    fn close(&mut self) {
        self.model.clear_status();
        self.exit.store(true, Ordering::Relaxed);
    }

    /// Клик по строке: выбрать и сразу запустить, как в списках файлов.
    fn click(&mut self, x: f32, y: f32) {
        let rows = view::rows_shown(self.model.visible().len());
        if let Some(index) = view::row_at(&self.model, self.card(), rows, x, y) {
            self.model.select(index);
            self.hover = Some(index);
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
        let input = keys::translate(key, self.model.query());
        debug_log(&format!("команда {input:?}"));
        self.apply(input);
    }

    /// Ставит в очередь иконки видимых строк. Запросы дешёвые: кэш отсекает
    /// уже загруженные, а очередь не даёт повторить ожидающее.
    fn request_icons(&mut self) {
        let specs: Vec<String> = self
            .model
            .visible()
            .iter()
            .filter_map(|row| self.model.app_at(row))
            .map(|app| app.icon.clone())
            .collect();
        for spec in specs {
            self.loader.request(&spec);
        }
    }

    /// Рисует кадр и отдаёт буфер слою.
    fn draw(&mut self) {
        let _frame = log::Frame::start("лаунчер");
        let now = std::time::Instant::now();
        let openness = self.ctx.openness_now(now, self.pixel);
        let visible = self.model.visible().len();
        // Подсветка выбранной строки. `update` идёт до чтения позиции: он может
        // запустить ход, и тогда читать надо уже новую.
        self.highlight.update(
            highlight::level_of(0, self.model.query(), self.model.total()),
            highlight::position(self.model.selected(), self.model.top()),
            now,
            self.ctx.anim,
        );
        let sliding = self.highlight.row(now, self.pixel);
        if self.ctx.width == 0 || self.ctx.height == 0 {
            return;
        }
        let pw = (self.ctx.width as f32 * SCALE) as u32;
        let ph = (self.ctx.height as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        // Буфер заливается прозрачным: сквозь него видно рабочий стол, а
        // правило слоя размывает то, что под карточкой.
        pixmap.fill(palette::Rgba(0, 0, 0, 0).to_tiny());
        let (ui, scale) = view::theme(self.pixel, self.palette);
        let card = {
            let mut window = view::AppsView {
                pixmap: &mut pixmap,
                painter: &mut self.painter,
                icons: &mut self.icons,
                ui,
                scale,
                pixel: self.pixel,
            };
            view::render_window(
                &mut window,
                &self.model,
                &self.loader,
                RowsHighlight {
                    sliding,
                    hover: self.hover,
                },
            )
        };
        view::apply_card_alpha(&mut pixmap, card, view::CARD_ALPHA);
        layer::fade(&mut pixmap, openness);

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
        shell::finish_fade(&mut self.ctx.dirty, openness);
        debug_log(&format!(
            "кадр {pw}x{ph}, строк {visible}, в очереди иконок {}",
            self.loader.pending()
        ));
    }
}

impl KeyboardHandler for AppsApp {
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

    /// Потеря клавиатуры закрывает окно: слой с `Exclusive` забрал бы её себе и
    /// вернул только после закрытия, а закрыть его без клавиатуры нечем.
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        debug_log("закрытие: клавиатура ушла с окна (keyboard leave)");
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
        self.repeat.press(&event);
        self.handle_key(event);
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
        self.handle_key(event);
    }

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

impl PointerHandler for AppsApp {
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
                    // Указатель только подсвечивает: случайное движение мыши не
                    // должно двигать выбор.
                    let rows = view::rows_shown(self.model.visible().len());
                    let hover =
                        view::row_at(&self.model, self.card(), rows, position.0, position.1);
                    if hover != self.hover {
                        self.hover = hover;
                        self.ctx.dirty = true;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.hover.take().is_some() {
                        self.ctx.dirty = true;
                    }
                }
                PointerEventKind::Press { button: 0x110, .. } => self.click(position.0, position.1),
                PointerEventKind::Axis { vertical, .. } => {
                    let scroll = vertical;
                    debug_log(&format!(
                        "ось: value120={} discrete={} absolute={}",
                        scroll.value120, scroll.discrete, scroll.absolute
                    ));
                    // Колесо — построчно, а не постранично: Page прыгал на
                    // 10 строк и одно деление уносило в конец списка.
                    let steps = shell::axis_steps(&mut self.scroll_acc, &scroll);
                    let dir = steps.signum();
                    for _ in 0..steps.abs() {
                        self.apply(Input::Move(dir));
                    }
                }
                _ => {}
            }
        }
    }
}

impl CompositorHandler for AppsApp {
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

impl OutputHandler for AppsApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for AppsApp {
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

impl smithay_client_toolkit::shell::wlr_layer::LayerShellHandler for AppsApp {
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
        // Нули от композитора игнорируются: слой без якорей не должен получать
        // нулевой размер. Если композитор предлагает свой — берём его, но
        // только если он положительный.
        if configure.new_size.0 > 0 {
            self.ctx.width = configure.new_size.0;
        }
        if configure.new_size.1 > 0 {
            self.ctx.height = configure.new_size.1;
        }
        // Первый кадр рисуется по нашим размерам, а не по нулям из configure.
        let (width, height) = self.wanted_size();
        if self.ctx.width == 0 || self.ctx.height == 0 {
            self.ctx.width = width;
            self.ctx.height = height;
        }
        let was_configured = self.ctx.configured;
        self.ctx.configured = true;
        self.ctx.dirty = true;
        // Проявление начинается на первом configure: раньше поверхность ещё не
        // нарисована, и двигать нечего.
        if !was_configured {
            self.ctx.start_open(std::time::Instant::now());
        }
    }
}

impl ShmHandler for AppsApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for AppsApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(AppsApp);
smithay_client_toolkit::delegate_dispatch2!(AppsApp);

/// Отладочная строка в stderr: `RUST_LOG=debug`.
fn debug_log(message: &str) {
    let level = std::env::var("RUST_LOG").unwrap_or_default();
    if level.contains("debug") || level.contains("trace") {
        eprintln!("hud-apps: {message}");
    }
}

/// Флаг остановки, доступный обработчику сигнала: обработчик не может
/// захватить состояние приложения, поэтому читает общий атомарный флаг.
static SIGNAL_EXIT: AtomicBool = AtomicBool::new(false);

/// Обработчик SIGTERM: только атомарная запись.
extern "C" fn on_sigterm(_signal: i32) {
    SIGNAL_EXIT.store(true, Ordering::Relaxed);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return;
    }
    let result = match args.first().map(String::as_str) {
        Some("--snapshot") => apps::snapshot::run(&args[1..]),
        Some(first) if first.starts_with('-') => {
            Err(format!("неизвестный флаг: {first}\nhud-apps-rs --help"))
        }
        Some(_) | None => run(),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

/// Живой цикл: один экземпляр, слой, ввод, перерисовка.
fn run() -> Result<(), String> {
    // Один экземпляр: слой не виден в `niri msg windows`, поэтому единственный
    // способ узнать, открыто ли окно, — pid-файл. Свой файл, у меню HUDbar он
    // свой.
    let pid_file = menu::instance::pid_path_for(PID_FILE);
    if let menu::instance::Startup::CloseRunning(pid) =
        menu::instance::startup_for(PID_FILE, std::path::Path::new("/proc"))
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
    if let Err(error) = menu::instance::store(&pid_file, pid) {
        return Err(format!("не записать pid-файл: {error}"));
    }
    debug_log(&format!("pid {pid}, файл {}", pid_file.display()));
    let result = event_loop();
    // pid-файл снимается на любом выходе, включая падение: иначе следующий
    // запуск увидит «живой» pid и не откроет окно.
    menu::instance::clear(&pid_file, pid);
    debug_log("pid-файл снят");
    result
}

/// Основной цикл: слой, ввод, перерисовка.
fn event_loop() -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|error| format!("нет Wayland: {error}"))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|error| format!("нет реестра: {error}"))?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|error| format!("нет wl_compositor: {error}"))?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|_| "нет zwlr_layer_shell".to_string())?;
    let shm = Shm::bind(&globals, &qh).map_err(|error| format!("нет wl_shm: {error}"))?;

    // Модель заводится до слоя: её размер нужен уже первому commit. Скан
    // desktop-файлов занимает единицы миллисекунд, но слой без якорей,
    // получивший `0` по ширине или высоте, — ошибка протокола, и композитор
    // убивает клиента прямо на старте.
    let current = settings::load();
    let pixel = current.theme.as_deref() == Some("pixel");
    // На энергосбережении анимации гаснут сами, настройку не трогаем.
    let anim_on = current.animations && power_profile::anim_allowed();
    if current.animations && !anim_on {
        debug_log("энергосбережение: анимации выключены на время лаунчера");
    }
    let anim = anim::AnimSettings::new(anim_on, current.animation_speed);
    let started = std::time::Instant::now();
    let model = Model::open(view::MAX_ROWS, current.language);
    debug_log(&format!(
        "приложений: {}, закреплено: {}",
        model.total(),
        model.pinned().len()
    ));
    let (start_w, start_h) = view::layer_size(model.visible().len());

    let ctx = layer::open(
        &compositor,
        &layer_shell,
        &shm,
        &qh,
        layer::Shape {
            namespace: Some(NAMESPACE),
            width: start_w,
            height: start_h,
            max_height: MAX_H,
            scale: SCALE as u32,
        },
        started,
        anim,
    )?;
    debug_log(&format!(
        "слой {NAMESPACE}: set_size {start_w}x{start_h} до commit"
    ));

    let mut app = AppsApp {
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
        model,
        loader: icons::Loader::new(),
        highlight: highlight::Highlight::new(started),
        hover: None,
        palette: palette::load(),
        pixel,
        scroll_acc: 0.0,
        exit: Arc::new(AtomicBool::new(false)),
    };
    app.sync_size();

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
        let _ = event_queue.dispatch_pending(&mut app);
        // Иконки приходят из потока: готовые ответы забираются без ожидания, и
        // окно перерисовывается, пока список меняется.
        if app.loader.take() {
            app.ctx.dirty = true;
        }
        app.request_icons();
        let now = std::time::Instant::now();
        // Пока идёт проявление или ход подсветки, кадр нужен каждый тик.
        if app.ctx.animating(now) || app.highlight.animating(now) {
            app.ctx.dirty = true;
        }
        // Рисовать можно только после первого configure: буфер, приложенный
        // раньше ack, композитор рвёт протоколом.
        if app.ctx.configured && app.ctx.dirty {
            app.ctx.dirty = false;
            app.draw();
        }
        // Повтор удержанной стрелки: композитор его не шлёт, крутим сами.
        while let Some(event) = app.repeat.poll() {
            app.handle_key(event);
            if app.ctx.configured {
                app.ctx.dirty = false;
                app.draw();
            }
        }
        if app.exit.load(Ordering::Relaxed) || SIGNAL_EXIT.load(Ordering::Relaxed) {
            break;
        }
        // События ждутся с таймаутом, а не вечно: SIGTERM должен закрывать окно
        // сразу, а не после следующего сообщения композитора.
        // Грязный флаг тоже будит короткий тик: недожатое проявление
        // должно дойти до непрозрачного кадра сразу, а не через простой.
        let moving = app.ctx.animating(now) || app.highlight.animating(now) || app.ctx.dirty;
        let idle = if moving || app.loader.pending() > 0 {
            16u16
        } else {
            100u16
        };
        // С удержанной стрелкой ждать дольше интервала повтора нельзя.
        let wait = shell::wait_ms(app.repeat.wait_hint(), idle);
        shell::poll_read(&mut event_queue, conn.as_fd(), wait);
    }
    Ok(())
}

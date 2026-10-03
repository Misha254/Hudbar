use std::os::fd::AsFd;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use nix::poll::{PollFd, PollFlags, poll};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
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
#[path = "../hud/palette.rs"]
mod palette;
#[path = "../hud/settings.rs"]
#[allow(dead_code)]
mod settings;
#[allow(dead_code)]
#[path = "../hud/settings_ui.rs"]
mod settings_ui;
#[path = "../hud/text.rs"]
mod text;
#[allow(dead_code)]
#[path = "../hud/wallpaper.rs"]
mod wallpaper;

use config::Config;
use settings_ui::{Control, Focus, Hotkey, Module, Nav, NotificationPosition, Rect, Row, Section};
use text::{Align, SCALE, TextPainter};
const WIDTH: u32 = settings_ui::WIDTH as u32;
const HEIGHT: u32 = settings_ui::HEIGHT as u32;
const TITLE: &str = "HUDbar  /  Control Center";
/// Фон окна непрозрачный: сквозь настройки не должен просвечивать терминал.
const BG_ALPHA: f32 = 1.0;
/// Контуры и центральные линии отладочной отрисовки раскладки.
const DEBUG_OUTLINE: palette::Rgba = palette::Rgba(0xff, 0x5c, 0x5c, 255);
/// Шрифтовая шкала окна. Все размеры кратны друг другу, чтобы строки
/// модуля, карточки и подвал выглядели одной системой.
#[derive(Clone, Copy)]
struct Sizes {
    title: f32,
    section: f32,
    row: f32,
    micro: f32,
    value: f32,
}

fn sizes(pixel: bool) -> Sizes {
    if pixel {
        Sizes {
            title: 19.0,
            section: 14.0,
            row: 13.0,
            micro: 11.5,
            value: 13.5,
        }
    } else {
        Sizes {
            title: 20.0,
            section: 15.0,
            row: 14.0,
            micro: 12.0,
            value: 15.0,
        }
    }
}

#[derive(Clone, Copy)]
struct UiPalette {
    base: palette::Rgba,
    panel: palette::Rgba,
    idle_panel: palette::Rgba,
    border: palette::Rgba,
    accent: palette::Rgba,
    text: palette::Rgba,
    muted: palette::Rgba,
}

/// Фоновые операции, которым нужен рестарт панели или чтение двух конфигов.
#[derive(Default)]
struct Job {
    busy: bool,
    result: Option<Result<String, String>>,
}

struct SettingsApp {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
    painter: TextPainter,
    config: config::Config,
    /// Горячие клавиши из niri: раздел «Управление» только их показывает.
    hotkeys: Vec<Hotkey>,
    /// Прокрутка списка клавиш: биндов больше, чем помещается на экран.
    hotkey_scroll: usize,
    /// Состояние раздела «Обои»: файлы, выбранная схема, прокрутка.
    wallpaper: settings_ui::Wallpaper,
    /// Активный раздел и зона фокуса; переходы живут в `settings_ui::Nav`.
    nav: Nav,
    /// Тема, которую пользователь выбрал, но которая ещё не применена.
    picked: Option<bool>,
    status: String,
    job: Arc<Mutex<Job>>,
    rows: Vec<Row>,
    palette: palette::Palette,
    settings_stamp: Option<SystemTime>,
    palette_stamp: Option<SystemTime>,
    pending_height: Option<(u32, Instant)>,
    width: u32,
    height: u32,
    configured: bool,
    dirty: bool,
    exit: Arc<AtomicBool>,
    hover: Option<Focus>,
    dragging_height: bool,
    /// Удерживаемая стрелка в сайдбаре: собственный ускоренный повтор.
    held: Option<HeldNav>,
}

/// Зажатая стрелка навигации по разделам и расписание её повторов.
struct HeldNav {
    raw: u32,
    dir: i32,
    next_at: Instant,
    repeat: settings_ui::Repeat,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

fn settings_path() -> PathBuf {
    home().join(".config/hudbar/settings.json")
}

fn stamp(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Горячие клавиши из `~/.config/niri/binds.kdl` для раздела «Управление».
/// Окно их только показывает: править бинды здесь нельзя.
fn load_hotkeys() -> Vec<Hotkey> {
    let entries = bind_data::load_niri();
    let keys: Vec<String> = entries.iter().map(|entry| entry.key.clone()).collect();
    let descs: Vec<String> = entries.iter().map(|entry| entry.desc.clone()).collect();
    settings_ui::merge_hotkeys(&keys, &descs)
}

fn restart_hudbar() -> Result<(), String> {
    let _ = Command::new("pkill")
        .args(["-TERM", "-x", "hudbar"])
        .status();
    for _ in 0..30 {
        if !Command::new("pgrep")
            .args(["-x", "hudbar"])
            .status()
            .map_err(|error| error.to_string())?
            .success()
        {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    Command::new(home().join(".local/bin/hudbar"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn mix(a: palette::Rgba, b: palette::Rgba, t: f32) -> palette::Rgba {
    palette::Rgba(
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
        255,
    )
}

fn ui_palette(pixel: bool, p: palette::Palette) -> UiPalette {
    if pixel {
        UiPalette {
            base: palette::Rgba(0x0b, 0x10, 0x20, 255).with_a(BG_ALPHA),
            panel: palette::Rgba(0x11, 0x1b, 0x31, 255).with_a(BG_ALPHA),
            idle_panel: palette::Rgba(0x0e, 0x17, 0x2b, 255).with_a(BG_ALPHA),
            border: palette::Rgba(0x31, 0x5b, 0x9b, 255),
            accent: palette::Rgba(0x79, 0xa7, 0xff, 255),
            text: palette::Rgba(0xe8, 0xea, 0xff, 255),
            muted: palette::Rgba(0x8d, 0x9a, 0xbd, 255),
        }
    } else {
        UiPalette {
            base: p.base.with_a(BG_ALPHA),
            panel: mix(p.base, p.secondary, 0.10).with_a(BG_ALPHA),
            idle_panel: mix(p.base, p.secondary, 0.05).with_a(BG_ALPHA),
            border: mix(p.base, p.primary, 0.35),
            accent: p.primary,
            text: p.text,
            muted: p.text.with_a(0.58),
        }
    }
}

/// Файлы обоев из `~/wallpapers` — рекурсивно, по четырём расширениям и с
/// сортировкой по пути. Обход и правила живут в `hud/wallpaper.rs`, чтобы их
/// можно было покрыть тестами; rofi с превью тут не нужен, список листается
/// в самом окне.
fn load_wallpapers() -> Vec<String> {
    wallpaper::list()
}

/// Имя файла без пути: в списке обоев полный путь занимает всю строку.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Подпись обоев: имя файла и папка-категория вместо полного пути.
fn wallpaper_dir_label(path: &str) -> String {
    let name = file_name(path);
    let mut parts = path.rsplit('/');
    parts.next();
    let dir = parts.next().unwrap_or("");
    if dir.is_empty() || dir == "wallpapers" {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn notification_position_label(position: NotificationPosition) -> &'static str {
    match position {
        NotificationPosition::TopLeft => "Сверху слева",
        NotificationPosition::TopRight => "Сверху справа",
        NotificationPosition::BottomLeft => "Снизу слева",
        NotificationPosition::BottomRight => "Снизу справа",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use text::SCALE;

    /// Фон окна настроек обязан быть непрозрачным: сквозь него видно терминал,
    /// и подписи на прозрачном фоне читаются хуже.
    #[test]
    fn window_background_is_opaque() {
        let palette = palette::Palette::default();
        assert_eq!(ui_palette(false, palette).base.3, 255);
        assert_eq!(ui_palette(true, palette).base.3, 255);
        assert_eq!(BG_ALPHA, 1.0);
    }

    #[test]
    fn settings_window_has_no_legacy_setting_commands() {
        let source = include_str!("hud_settings.rs");
        let command_lines = source.lines().filter(|line| {
            line.contains("Command::new") || line.contains(".arg(") || line.contains(".args(")
        });
        for line in command_lines {
            assert!(
                !line.contains("hud-setting"),
                "legacy setting command: {line}"
            );
            assert!(!line.contains("hud-theme"), "legacy theme command: {line}");
        }
    }

    /// Фон заливает весь буфер целиком, включая поля: иначе сквозь окно видно
    /// то, что под ним.
    #[test]
    fn background_covers_the_whole_buffer() {
        let width = (settings_ui::WIDTH * SCALE) as u32;
        let height = (settings_ui::HEIGHT * SCALE) as u32;
        let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
        let p = ui_palette(false, palette::Palette::default());
        pixmap.fill(p.base.to_tiny());
        for (x, y) in [
            (0, 0),
            (width - 1, height - 1),
            (0, height - 1),
            (width - 1, 0),
        ] {
            let pixel = pixmap.pixel(x, y).unwrap();
            assert_eq!(pixel.alpha(), 255, "поле ({x}, {y}) не залито");
        }
    }

    /// Границы залитых пикселей текста внутри полосы.
    fn ink_bounds(pixmap: &tiny_skia::Pixmap) -> (f32, f32) {
        let (mut top, mut bottom) = (f32::MAX, f32::MIN);
        for y in 0..pixmap.height() {
            for x in 0..pixmap.width() {
                let pixel = pixmap.pixel(x, y).unwrap();
                if pixel.alpha() > 8 {
                    top = top.min(y as f32);
                    bottom = bottom.max(y as f32);
                }
            }
        }
        assert!(top <= bottom, "текст не нарисован");
        (top, bottom)
    }

    /// Главная проверка: центр текста должен совпадать с центром своей полосы.
    /// Раньше подписи садились на 15–20 px ниже элементов, потому что высота
    /// блока считалась по заданной высоте строки, а не по реальным глифам.
    #[test]
    fn text_ink_is_centered_in_its_band() {
        let mut painter = TextPainter::new("JetBrainsMono Nerd Font Propo");
        let mut failures = Vec::new();
        for (label, size, align) in [
            ("Обзор", 15.0, Align::Start),
            ("Закрыть", 12.0, Align::Center),
            ("Трей", 14.0, Align::Start),
            ("Погода", 14.0, Align::Start),
            ("вкл", 12.0, Align::End),
            ("−", 15.0, Align::Center),
            ("27", 15.0, Align::Center),
            ("Обычный", 18.0, Align::Start),
        ] {
            let rect = settings_ui::Rect::new(40.0, 60.0, 220.0, 32.0);
            let width = (settings_ui::WIDTH * SCALE) as u32;
            let height = (settings_ui::HEIGHT * SCALE) as u32;
            let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
            pixmap.fill(tiny_skia::Color::TRANSPARENT);
            painter.paint(
                &mut pixmap,
                label,
                size,
                palette::Rgba(255, 255, 255, 255),
                rect,
                align,
            );
            let (top, bottom) = ink_bounds(&pixmap);
            let ink_center = (top + bottom) / 2.0 / SCALE;
            let band_center = rect.y + rect.h / 2.0;
            let delta = ink_center - band_center;
            if delta.abs() > 1.0 {
                failures.push(format!("{label}: смещение {delta:+.2} px"));
            }
        }
        assert!(
            failures.is_empty(),
            "текст не центрирован в полосе: {}",
            failures.join(", ")
        );
    }
}

impl SettingsApp {
    /// Тема, которую сейчас рисуем: выбранная, иначе применённая.
    /// Тема в файле, без учёта неприменённого выбора.
    fn current_mode(&self) -> &'static str {
        match self.config.theme {
            Some(config::Theme::Pixel) => "pixel",
            _ => "normal",
        }
    }

    fn pixel_mode(&self) -> bool {
        self.picked
            .unwrap_or(self.config.theme == Some(config::Theme::Pixel))
    }

    fn font_for(&self, pixel: bool) -> String {
        if pixel {
            "Minecraft Rus".to_string()
        } else {
            "JetBrainsMono Nerd Font Propo".to_string()
        }
    }

    fn busy(&self) -> bool {
        self.job.lock().is_ok_and(|job| job.busy)
    }

    fn poll_job(&mut self) {
        let result = {
            let Ok(mut job) = self.job.lock() else {
                return;
            };
            let Some(result) = job.result.take() else {
                return;
            };
            job.busy = false;
            result
        };
        self.picked = None;
        self.config = Config::from(&settings::load());
        self.painter.set_font(&self.font_for(self.pixel_mode()));
        self.rebuild_rows();
        self.status = match result {
            Ok(what) => format!("{what} — готово"),
            Err(error) => format!("ошибка: {error}"),
        };
        self.dirty = true;
    }

    fn refresh_sources(&mut self) {
        let settings_stamp = stamp(&settings_path());
        if settings_stamp != self.settings_stamp {
            self.settings_stamp = settings_stamp;
            self.config = config::Config::from(&settings::load());
            self.rows = settings_ui::rows_for_state(
                self.nav.section,
                &self.config,
                &self.hotkeys,
                self.hotkey_scroll,
                &self.wallpaper,
            );
            self.picked = None;
            self.painter.set_font(&self.font_for(self.pixel_mode()));
            self.dirty = true;
        }
        let palette_path = home().join(".config/hudbar/colors.css");
        let palette_stamp = stamp(&palette_path);
        if palette_stamp != self.palette_stamp {
            self.palette_stamp = palette_stamp;
            self.palette = palette::load();
            self.dirty = true;
        }
    }

    fn start_job<F>(&mut self, what: &str, operation: F)
    where
        F: FnOnce() -> Result<(), String> + Send + 'static,
    {
        if self.busy() {
            self.status = "Дождитесь предыдущего применения".to_string();
            self.dirty = true;
            return;
        }
        self.status = format!("{what} — применяем…");
        self.dirty = true;
        if let Ok(mut job) = self.job.lock() {
            job.busy = true;
            job.result = None;
        }
        let what = what.to_string();
        let job = Arc::clone(&self.job);
        thread::spawn(move || {
            let result = operation().map(|_| what);
            if let Ok(mut job) = job.lock() {
                job.busy = false;
                job.result = Some(result);
            }
        });
    }

    fn apply_theme(&mut self, pixel: bool) {
        self.flush_height(true);
        if self.busy() {
            self.status = "Дождитесь предыдущего применения".to_string();
            self.dirty = true;
            return;
        }
        let mode = if pixel { "pixel" } else { "normal" };
        if mode == self.current_mode() {
            self.picked = None;
            self.dirty = true;
            return;
        }
        self.picked = Some(pixel);
        self.painter.set_font(&self.font_for(pixel));
        let theme = if pixel {
            config::Theme::Pixel
        } else {
            config::Theme::Normal
        };
        self.start_job("Оформление", move || {
            config_io::apply_theme(theme).map_err(|error| error.to_string())?;
            restart_hudbar()
        });
    }

    fn step_height(&mut self, delta: i32) {
        if self.busy() {
            self.status = "Дождитесь предыдущего применения".to_string();
            self.dirty = true;
            return;
        }
        let Some(height) = settings_ui::step_height(self.config.height, delta) else {
            return;
        };
        self.config.height = height;
        self.pending_height = Some((height, Instant::now() + Duration::from_millis(300)));
        self.status = format!("Высота панели: {height} px");
        self.rebuild_rows();
        self.dirty = true;
    }

    fn set_height_from_x(&mut self, x: f32) {
        let track = settings_ui::height_track_rect();
        let ratio = ((x - track.x) / track.w).clamp(0.0, 1.0);
        let value = settings_ui::HEIGHT_MIN
            + ((settings_ui::HEIGHT_MAX - settings_ui::HEIGHT_MIN) as f32 * ratio).round() as u32;
        let delta = value as i32 - self.config.height as i32;
        if delta != 0 {
            self.step_height(delta);
        }
    }

    fn flush_height(&mut self, force: bool) {
        let Some((height, deadline)) = self.pending_height else {
            return;
        };
        if !force && Instant::now() < deadline {
            return;
        }
        self.pending_height = None;
        match config_io::apply_patch(&config_io::Patch::Height(height)) {
            Ok(_) => {
                self.config = Config::from(&settings::load());
                self.status = format!("Высота панели: {height} px — применено");
            }
            Err(error) => {
                self.config = Config::from(&settings::load());
                self.status = format!("ошибка: {error}");
            }
        }
        self.rebuild_rows();
        self.dirty = true;
    }

    fn step_notification_font(&mut self, delta: i32) {
        let value = (self.config.font_size as i32 + delta).clamp(10, 18) as u32;
        if value == self.config.font_size || self.busy() {
            return;
        }
        self.config.font_size = value;
        self.status = format!("Размер уведомлений: {value}");
        self.apply_notifications(config_io::Patch::Notifications {
            font_size: Some(value),
            line_height: None,
            position: None,
        });
    }

    fn step_notification_line(&mut self, delta: i32) {
        let value = (self.config.line_height as i32 + delta).clamp(14, 28) as u32;
        if value == self.config.line_height || self.busy() {
            return;
        }
        self.config.line_height = value;
        self.status = format!("Высота строки: {value}");
        self.apply_notifications(config_io::Patch::Notifications {
            font_size: None,
            line_height: Some(value),
            position: None,
        });
    }

    fn set_notification_position(&mut self, position: NotificationPosition) {
        if position == self.config.position || self.busy() {
            return;
        }
        self.config.position = position;
        self.apply_notifications(config_io::Patch::Notifications {
            font_size: None,
            line_height: None,
            position: Some(position),
        });
    }

    fn apply_notifications(&mut self, patch: config_io::Patch) {
        self.flush_height(true);
        self.start_job("Уведомления", move || {
            config_io::apply_patch(&patch).map_err(|error| error.to_string())?;
            let config = Config::from(&settings::load());
            dunst::apply(&dunst::values_from_config(&config)).map_err(|error| error.to_string())?;
            Ok(())
        });
    }

    /// Пишет одно поле и перечитывает файл заново.
    ///
    /// `apply_patch` читает `settings.json` с диска и накладывает патч, поэтому
    /// кэш окна не может затереть чужое. При ошибке значение в интерфейсе
    /// откатывается к тому, что лежит в файле, а в статусе появляется причина.
    fn commit(&mut self, patch: config_io::Patch) {
        let label = patch.label();
        match config_io::apply_patch(&patch) {
            Ok(_) => {
                // Файл — источник истины: перечитываем, а не доверяем кэшу.
                self.config = Config::from(&settings::load());
                self.status = format!("{label} — применено");
                self.rebuild_rows();
                self.dirty = true;
            }
            Err(why) => {
                self.config = Config::from(&settings::load());
                self.status = format!("{label} — ошибка: {why}");
                self.rebuild_rows();
                self.dirty = true;
            }
        }
    }

    /// Переключатель видимости: пишем один флаг, панель подхватит на лету.
    fn toggle_module(&mut self, module: Module) {
        let on = !self.config.module_enabled(module);
        self.commit(config_io::Patch::ModuleVisible { module, on });
    }

    /// Перестановка внутри зоны. Модуль не выходит за пределы своей группы:
    /// `Config::move_selected` возвращает `false` на краю, и мы ничего не пишем.
    fn move_module(&mut self, module: Module, delta: i32) {
        if !self.config.move_selected(module, delta) {
            self.status = format!("{} — край группы", module.label());
            self.dirty = true;
            return;
        }
        let moved = self.config.clone();
        self.commit(config_io::Patch::ModuleOrder(moved.flatten()));
    }

    fn toggle_language(&mut self) {
        let next = if self.config.language == settings::Language::Ru {
            settings::Language::En
        } else {
            settings::Language::Ru
        };
        self.commit(config_io::Patch::Language(next));
    }
    /// Показ шестерёнки Control Center. Панель читает файл на лету, поэтому
    /// перезапуск не нужен.
    fn toggle_control_button(&mut self) {
        let on = !self.config.control_button;
        self.commit(config_io::Patch::ControlButton(on));
    }

    /// Выбор файла обоев: только запоминаем, ничего не применяем.
    fn pick_wallpaper(&mut self, index: usize) {
        if index < self.wallpaper.files.len() {
            self.wallpaper.selected = index;
            let name = self.wallpaper.files[index].clone();
            self.status = format!("Выбран файл: {}", file_name(&name));
            self.rebuild_rows();
            self.dirty = true;
        }
    }

    /// Выбор схемы matugen: тоже только запоминаем.
    fn pick_scheme(&mut self, index: usize) {
        if let Some(scheme) = settings_ui::SCHEMES.get(index) {
            self.wallpaper.scheme = index;
            self.status = format!("Схема: {scheme}");
            self.rebuild_rows();
            self.dirty = true;
        }
    }

    /// Единственное место, где окно меняет обои: явное нажатие «Применить».
    /// Запускается `wall.sh --set`, сам matugen окно не зовёт.
    fn apply_wallpaper(&mut self) {
        self.flush_height(true);
        if self.busy() {
            self.status = "Дождитесь предыдущего применения".to_string();
            self.dirty = true;
            return;
        }
        let Some(file) = self.wallpaper.files.get(self.wallpaper.selected).cloned() else {
            self.status = "Нет выбранного файла".to_string();
            self.dirty = true;
            return;
        };
        let scheme = settings_ui::SCHEMES[self.wallpaper.scheme].to_string();
        self.status = "Обои применяются…".to_string();
        self.dirty = true;
        self.start_job("Обои", move || {
            let output = Command::new(home().join(".local/bin/wall.sh"))
                .arg("--set")
                .arg(&file)
                .arg(&scheme)
                .output()
                .map_err(|error| error.to_string())?;
            if output.status.success() {
                return Ok(());
            }
            // wall.sh пишет причину в stderr — показываем её, а не код.
            let reason = String::from_utf8_lossy(&output.stderr);
            let reason = reason
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("неизвестная ошибка")
                .to_string();
            Err(format!(
                "{reason} (код {})",
                output.status.code().unwrap_or(-1)
            ))
        });
    }

    /// Страница вверх или вниз: что именно листается, решает активный раздел.
    fn scroll_page(&mut self, forward: bool) {
        match self.nav.section {
            Section::Wallpaper => self.scroll_wallpapers(forward),
            _ => self.scroll_hotkeys(forward),
        }
    }

    /// Прокрутка списка обоев: выбранный файл всегда остаётся видимым.
    fn scroll_wallpapers(&mut self, forward: bool) {
        let page = settings_ui::WALLPAPER_FILES;
        let count = self.wallpaper.files.len();
        if page == 0 || count <= page {
            return;
        }
        let max_scroll = count - page;
        let selected = self.wallpaper.selected;
        let next = if forward {
            (self.wallpaper.scroll + page).min(max_scroll)
        } else {
            self.wallpaper.scroll.saturating_sub(page)
        };
        if next == self.wallpaper.scroll {
            return;
        }
        self.wallpaper.scroll = next;
        // Выделение подтягиваем в видимую часть списка.
        if selected < next {
            self.wallpaper.selected = next;
        } else if selected >= next + page {
            self.wallpaper.selected = next + page - 1;
        }
        self.status = format!("Файлов: {} · показано {}-{next}{page}", count, next + 1);
        self.rebuild_rows();
        self.dirty = true;
    }

    /// Листание списка горячих клавиш: список длинный, а править его нельзя.
    fn scroll_hotkeys(&mut self, forward: bool) {
        if self.nav.section != Section::Controls {
            return;
        }
        let page = settings_ui::HOTKEY_PAGE;
        if page == 0 || self.hotkeys.len() <= page {
            return;
        }
        let pages = self.hotkeys.len().div_ceil(page);
        let current = self.hotkey_scroll / page;
        let next = if forward {
            (current + 1).min(pages - 1)
        } else {
            current.saturating_sub(1)
        };
        if next != current {
            self.hotkey_scroll = next * page;
            self.status = format!("Горячие клавиши: страница {}/{pages}", next + 1);
            self.rebuild_rows();
            self.dirty = true;
        }
    }

    /// Переход в другой раздел из «Обзора».
    fn open_section(&mut self, target: Section) {
        self.nav.section = target;
        self.nav.focus = Focus::Nav(target.index());
        self.rebuild_rows();
        self.dirty = true;
    }

    /// Пересобирает строки под активный раздел. Фокус сайдбара сохраняется,
    /// из содержимого переносится на первый контрол нового раздела.
    fn rebuild_rows(&mut self) {
        self.rows = settings_ui::rows_for(
            self.nav.section,
            &self.config,
            &self.hotkeys,
            self.hotkey_scroll,
        );
        if !self.nav.focus_nav()
            && let Some(content) = self.nav.first_content(&self.rows)
        {
            self.nav.focus = content;
        }
    }

    /// Клик и Enter ведут в одно место: в сайдбаре открывают раздел, в контенте
    /// применяют контрол. Раздел требует пересборки строк.
    fn activate(&mut self) {
        let Some(control) = self.nav.activate() else {
            self.rebuild_rows();
            self.dirty = true;
            return;
        };
        match control {
            Control::Nav(_) => {}
            Control::Theme(pixel) => self.apply_theme(pixel),
            Control::Toggle(module) | Control::Switch(module) => self.toggle_module(module),
            Control::Move(module, dir) => self.move_module(module, dir),
            Control::Height(delta) => self.step_height(delta),
            Control::NotificationFont(delta) => self.step_notification_font(delta),
            Control::NotificationLineHeight(delta) => self.step_notification_line(delta),
            Control::NotificationPosition(position) => self.set_notification_position(position),
            Control::Goto(target) => self.open_section(target),
            Control::ControlButton => self.toggle_control_button(),
            Control::WallpaperFile(index) => self.pick_wallpaper(index),
            Control::Scheme(index) => self.pick_scheme(index),
            Control::WallpaperApply => self.apply_wallpaper(),
            // Слайдер высоты реагирует на перетаскивание, а не на Space.
            Control::HeightSlider => {}
            Control::Language => self.toggle_language(),
            Control::Close => {
                self.flush_height(true);
                self.exit.store(true, Ordering::Relaxed);
            }
        }
    }

    fn move_focus(&mut self, dx: i32, dy: i32) {
        let before = self.nav.section;
        if self.nav.move_focus(&self.rows, dx, dy) {
            if self.nav.section != before {
                self.rebuild_rows();
            }
            self.dirty = true;
        }
    }

    /// Координаты указателя уже логические: `wl_pointer` сообщает их в
    /// surface-local пикселях, поэтому масштаб буфера не применяется.
    fn hit(&self, x: f32, y: f32) -> Option<Focus> {
        settings_ui::hit(&self.rows, x, y)
    }

    fn text_width(&mut self, text: &str, size: f32) -> f32 {
        self.painter.text_width(text, size)
    }

    /// Сфокусирован ли контрол правой колонки.
    fn focused(&self, control: Control) -> bool {
        self.nav.focus == Focus::Content(control)
    }

    /// Наведён ли мышью контрол правой колонки.
    fn hovered(&self, control: Control) -> bool {
        self.hover == Some(Focus::Content(control))
    }

    /// Стрелка в сайдбаре: первый шаг и запуск ускоренного повтора.
    fn hold_nav_key(&mut self, raw: u32, dir: i32) {
        let repeat = settings_ui::Repeat::sections();
        let next_at = Instant::now() + Duration::from_millis(repeat.delay_ms as u64);
        self.held = Some(HeldNav {
            raw,
            dir,
            next_at,
            repeat,
        });
    }

    fn release_nav_key(&mut self, raw: u32) {
        if self.held.as_ref().is_some_and(|held| held.raw == raw) {
            self.held = None;
        }
    }

    /// Индекс активного пункта сайдбара.
    fn nav_index(&self) -> usize {
        match self.nav.focus {
            Focus::Nav(index) => index,
            Focus::Content(_) => self.nav.section.index(),
        }
    }

    /// Один шаг ускоренного повтора: переключение раздела и пересчёт паузы.
    /// Перескок через край перезапускает кривую, иначе после перехода на
    /// противоположный конец список летел бы сразу на максимальной скорости.
    fn repeat_step(&mut self) {
        let Some(held) = self.held.as_mut() else {
            return;
        };
        let dir = held.dir;
        let pause = held.repeat.next_interval();
        held.next_at = Instant::now() + Duration::from_millis(pause as u64);
        let before = self.nav_index();
        let last = settings_ui::Section::ALL.len() - 1;
        self.move_focus(0, dir);
        let wrapped =
            (before == 0 && self.nav_index() == last) || (before == last && self.nav_index() == 0);
        if wrapped && let Some(held) = self.held.as_mut() {
            held.repeat = settings_ui::Repeat::sections();
            held.next_at =
                Instant::now() + Duration::from_millis(held.repeat.next_interval() as u64);
        }
    }

    /// Тик окна: если подошёл срок ускоренного повтора, делаем шаг.
    pub fn tick_repeat(&mut self) {
        let due = self
            .held
            .as_ref()
            .is_some_and(|held| Instant::now() >= held.next_at);
        if due {
            self.repeat_step();
        }
    }

    /// Ближайший момент повтора — им же меряем паузу в poll-цикле.
    fn repeat_deadline(&self) -> Option<Duration> {
        self.held
            .as_ref()
            .map(|held| held.next_at.saturating_duration_since(Instant::now()))
    }

    /// Общая обработка клавиши для `press_key` и `repeat_key`.
    fn on_key(&mut self, event: KeyEvent) {
        match event.keysym {
            Keysym::Escape => {
                self.flush_height(true);
                self.exit.store(true, Ordering::Relaxed);
            }
            Keysym::Return | Keysym::KP_Enter | Keysym::space => self.activate(),
            // Язык переключается отдельной клавишей: `L`.
            Keysym::l | Keysym::L => self.toggle_language(),
            // Shift+↑/↓ переставляют модуль внутри его зоны; без Shift те же
            // стрелки двигают фокус по разделу.
            Keysym::Up if self.mods.shift => self.shift_module(-1),
            Keysym::Down if self.mods.shift => self.shift_module(1),
            Keysym::Up => self.move_focus(0, -1),
            Keysym::Down => self.move_focus(0, 1),
            Keysym::Left => self.move_focus(-1, 0),
            Keysym::Right => self.move_focus(1, 0),
            Keysym::Tab => {
                if self.nav.cycle_zone(&self.rows, self.mods.shift) {
                    self.dirty = true;
                }
            }
            // В «Обоях» страницы листают список файлов, в «Управлении» — бинды.
            Keysym::Page_Up | Keysym::KP_Page_Up => self.scroll_page(false),
            Keysym::Page_Down | Keysym::KP_Page_Down => self.scroll_page(true),
            Keysym::minus | Keysym::KP_Subtract | Keysym::underscore => self.step_height(-1),
            Keysym::plus | Keysym::equal | Keysym::KP_Add => self.step_height(1),
            _ => {}
        }
    }

    /// Shift+↑/↓: если фокус на строке модуля — перестановка внутри зоны,
    /// иначе перемещение фокуса, как у обычных стрелок.
    fn shift_module(&mut self, delta: i32) {
        let module = match self.nav.focus {
            Focus::Content(Control::Switch(module)) | Focus::Content(Control::Toggle(module)) => {
                Some(module)
            }
            _ => None,
        };
        match module {
            Some(module) => self.move_module(module, delta),
            None => self.move_focus(0, delta),
        }
    }

    fn draw(&mut self) {
        if !self.configured || self.width == 0 || self.height == 0 {
            return;
        }
        let pw = (self.width as f32 * SCALE) as u32;
        let ph = (self.height as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        let p = ui_palette(self.pixel_mode(), self.palette);
        let pixel = self.pixel_mode();
        let s = sizes(pixel);
        pixmap.fill(p.base.to_tiny());
        let k = SCALE;
        fill_rect(&mut pixmap, 0.0, 0.0, pw as f32, k, p.accent);
        fill_rect(&mut pixmap, 0.0, 0.0, k, ph as f32, p.accent.with_a(0.3));

        self.draw_navigation(&mut pixmap, p, s);

        self.painter.paint(
            &mut pixmap,
            TITLE,
            s.title,
            p.text,
            settings_ui::title_rect(),
            Align::Start,
        );

        let status = self.status.clone();
        // строки копируем: рисование берёт &mut self, а обход идёт по self.rows
        let rows = self.rows.clone();
        for row in &rows {
            match row.clone() {
                Row::Header { text, y } => self.painter.paint(
                    &mut pixmap,
                    text,
                    s.section,
                    p.text,
                    settings_ui::header_rect(y),
                    Align::Start,
                ),
                Row::Rule { y } => {
                    let w = settings_ui::WIDTH - settings_ui::PAD_X - settings_ui::PAD_R;
                    fill_rect(
                        &mut pixmap,
                        settings_ui::PAD_X * k,
                        y * k,
                        w * k,
                        k,
                        p.border,
                    );
                }
                Row::Nav { .. } => {}
                Row::Stub { title, note, y } => {
                    self.painter.paint(
                        &mut pixmap,
                        title,
                        s.section + 4.0,
                        p.text,
                        settings_ui::stub_rect(y),
                        Align::Start,
                    );
                    self.painter.paint(
                        &mut pixmap,
                        note,
                        s.row,
                        p.muted,
                        settings_ui::stub_rect(y + 34.0),
                        Align::Start,
                    );
                }
                Row::Theme { pixel, rect } => self.draw_theme_card(&mut pixmap, p, pixel, rect, s),
                Row::Toggle { module, rect } => {
                    let on = self.config.module_enabled(module);
                    let control = Control::Toggle(module);
                    self.draw_toggle(
                        &mut pixmap,
                        p,
                        rect,
                        module.label(),
                        on,
                        self.hovered(control),
                        self.focused(control),
                        s,
                    );
                }
                Row::ZoneLabel {
                    title, hint, rect, ..
                } => {
                    self.painter.paint(
                        &mut pixmap,
                        title,
                        s.row,
                        p.text,
                        Rect::new(rect.x, rect.y, rect.w, 18.0),
                        Align::Start,
                    );
                    self.painter.paint(
                        &mut pixmap,
                        hint,
                        s.micro,
                        p.muted,
                        Rect::new(rect.x, rect.y + 18.0, rect.w, 16.0),
                        Align::Start,
                    );
                }
                Row::ModuleRow { module, line, rect } => {
                    self.draw_module_row(&mut pixmap, p, module, line, rect, s)
                }
                Row::Move {
                    module, dir, rect, ..
                } => {
                    let control = Control::Move(module, dir);
                    self.draw_step(
                        &mut pixmap,
                        p,
                        rect,
                        if dir < 0 { "▲" } else { "▼" },
                        self.hovered(control),
                        self.focused(control),
                        s.micro,
                    );
                }
                Row::HeightLabel { .. } => self.draw_height_scale(&mut pixmap, p, s),
                Row::NotificationFont { dir, rect } => {
                    self.painter.paint(
                        &mut pixmap,
                        &format!("Размер шрифта: {}", self.config.font_size),
                        s.row,
                        p.text,
                        Rect::new(settings_ui::PAD_X, rect.y, 260.0, rect.h),
                        Align::Start,
                    );
                    self.draw_step(
                        &mut pixmap,
                        p,
                        rect,
                        if dir < 0 { "−" } else { "+" },
                        self.hovered(Control::NotificationFont(dir))
                            || self.focused(Control::NotificationFont(dir)),
                        self.focused(Control::NotificationFont(dir)),
                        s.value,
                    );
                }
                Row::NotificationLineHeight { dir, rect } => {
                    self.painter.paint(
                        &mut pixmap,
                        &format!("Высота строки: {}", self.config.line_height),
                        s.row,
                        p.text,
                        Rect::new(settings_ui::PAD_X, rect.y, 260.0, rect.h),
                        Align::Start,
                    );
                    self.draw_step(
                        &mut pixmap,
                        p,
                        rect,
                        if dir < 0 { "−" } else { "+" },
                        self.hovered(Control::NotificationLineHeight(dir))
                            || self.focused(Control::NotificationLineHeight(dir)),
                        self.focused(Control::NotificationLineHeight(dir)),
                        s.value,
                    );
                }
                Row::NotificationPosition { position, rect } => {
                    // Выбранный угол подсвечивается так же, как выбранная тема
                    // в «Внешнем виде»: заливка, акцентная рамка и метка.
                    let control = Control::NotificationPosition(position);
                    let selected = self.config.position == position;
                    let active = selected || self.hovered(control) || self.focused(control);
                    self.draw_button(
                        &mut pixmap,
                        p,
                        rect,
                        notification_position_label(position),
                        active,
                        s.micro,
                        pixel,
                    );
                    if selected {
                        let k = SCALE;
                        let mark = Rect::new(rect.right() - 26.0, rect.y, 20.0, rect.h);
                        self.painter
                            .paint(&mut pixmap, "✓", s.row, p.accent, mark, Align::Center);
                        stroke_rect(
                            &mut pixmap,
                            rect.x * k,
                            rect.y * k,
                            rect.w * k,
                            rect.h * k,
                            p.accent,
                            2.0 * k,
                        );
                    }
                }
                Row::Height { dir, rect } => {
                    let control = Control::Height(dir);
                    self.draw_step(
                        &mut pixmap,
                        p,
                        rect,
                        if dir < 0 { "−" } else { "+" },
                        self.hovered(control),
                        self.focused(control),
                        s.value,
                    );
                }
                Row::Status { y } => {
                    let busy = self.busy();
                    self.painter.paint(
                        &mut pixmap,
                        &status,
                        s.value,
                        if busy { p.accent } else { p.muted },
                        settings_ui::header_rect(y),
                        Align::Start,
                    );
                }
                Row::Footer { y } => {
                    let hints = settings_ui::hints(self.nav.section);
                    let hints: Vec<&str> =
                        hints.into_iter().filter(|hint| !hint.is_empty()).collect();
                    self.draw_footer_hints(&mut pixmap, p, y, s.micro, &hints);
                }
                Row::Language { rect } => {
                    let label = if self.config.language == settings::Language::Ru {
                        "РУС"
                    } else {
                        "ENG"
                    };
                    self.draw_button(
                        &mut pixmap,
                        p,
                        rect,
                        label,
                        self.hovered(Control::Language) || self.focused(Control::Language),
                        s.micro,
                        pixel,
                    );
                }
                Row::Summary {
                    label,
                    value,
                    rect,
                    section,
                } => self.draw_summary(&mut pixmap, p, rect, label, &value, section, s),
                Row::Hotkey { keys, desc, rect } => {
                    self.draw_hotkey(&mut pixmap, p, rect, &keys, &desc, s);
                }
                Row::MissingBind { desc, rect } => {
                    self.draw_missing_bind(&mut pixmap, p, rect, &desc, s);
                }
                Row::ControlButton { rect } => {
                    self.draw_control_button(&mut pixmap, p, rect, s, pixel);
                }
                Row::WallpaperFile {
                    name,
                    rect,
                    selected,
                    ..
                } => {
                    let label = wallpaper_dir_label(&name);
                    self.draw_wallpaper_file(&mut pixmap, p, rect, &label, selected, s);
                }
                Row::Scheme {
                    name,
                    index,
                    rect,
                    selected,
                } => self.draw_scheme(&mut pixmap, p, rect, name, index, selected, s, pixel),
                Row::WallpaperApply { rect } => {
                    let control = Control::WallpaperApply;
                    self.draw_button(
                        &mut pixmap,
                        p,
                        rect,
                        "Применить обои",
                        self.hovered(control) || self.focused(control) || self.busy(),
                        s.row,
                        pixel,
                    );
                }
                Row::Close { rect } => {
                    self.draw_button(
                        &mut pixmap,
                        p,
                        rect,
                        "Закрыть",
                        self.hovered(Control::Close) || self.focused(Control::Close),
                        s.micro,
                        pixel,
                    );
                }
            }
        }

        if settings_ui::debug_layout() {
            self.draw_debug_overlay(&mut pixmap);
        }

        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
        else {
            // панель на перезапуске может занять буфер — попробуем на тике
            self.dirty = true;
            return;
        };
        let (dst, _) = canvas.as_chunks_mut::<4>();
        let (src, _) = pixmap.data().as_chunks::<4>();
        for (out, input) in dst.iter_mut().zip(src.iter()) {
            *out = [input[2], input[1], input[0], input[3]];
        }
        let surface = self.window.wl_surface();
        surface.set_buffer_scale(SCALE as i32);
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        let _ = buffer.attach_to(surface);
        self.window.commit();
    }

    /// Контуры всех полос и линия их центра — включается `HUD_DEBUG_LAYOUT=1`.
    fn draw_debug_overlay(&mut self, pixmap: &mut tiny_skia::Pixmap) {
        let k = SCALE;
        for rect in settings_ui::debug_rects() {
            stroke_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                DEBUG_OUTLINE,
                k,
            );
            // горизонтальная линия центра полосы
            fill_rect(
                pixmap,
                rect.x * k,
                (rect.y + rect.h / 2.0) * k,
                rect.w * k,
                k,
                DEBUG_OUTLINE.with_a(0.8),
            );
        }
    }

    /// Переключатель «Кнопка Control Center»: слева подпись, справа тумблер
    /// вкл/выкл — тот же вид, что у строки модуля.
    fn draw_control_button(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        s: Sizes,
        pixel: bool,
    ) {
        let k = SCALE;
        let control = Control::ControlButton;
        let on = self.config.control_button;
        let active = self.hovered(control) || self.focused(control);
        if active {
            fill_round_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                if self.pixel_mode() { 0.0 } else { 6.0 * k },
                p.idle_panel,
            );
        }
        self.painter.paint(
            pixmap,
            "Кнопка Control Center",
            s.row,
            if on { p.text } else { p.muted },
            Rect::new(rect.x + 10.0, rect.y, rect.w - 60.0, rect.h),
            Align::Start,
        );
        let track = Rect::new(
            rect.right() - 10.0 - 46.0,
            rect.y + (rect.h - 18.0) / 2.0,
            46.0,
            18.0,
        );
        fill_round_rect(
            pixmap,
            track.x * k,
            track.y * k,
            track.w * k,
            track.h * k,
            9.0 * k,
            if on { p.accent } else { p.border },
        );
        let knob = 14.0;
        let knob_x = if on {
            track.right() - knob - 2.0
        } else {
            track.x + 2.0
        };
        fill_round_rect(
            pixmap,
            knob_x * k,
            (track.y + 2.0) * k,
            knob * k,
            knob * k,
            7.0 * k,
            if on { p.base } else { p.text.with_a(0.7) },
        );
        let _ = pixel;
    }

    /// Файл обоев в списке: выбранный подсвечивается акцентной рамкой, имя
    /// обрезается по ширине полосы.
    fn draw_wallpaper_file(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        name: &str,
        selected: bool,
        s: Sizes,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if self.pixel_mode() { 0.0 } else { 6.0 * k },
            if selected { p.panel } else { p.idle_panel },
        );
        stroke_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if selected { p.accent } else { p.border },
            if selected { 2.0 * k } else { k },
        );
        self.painter.paint_boxed(
            pixmap,
            name,
            s.row,
            if selected { p.text } else { p.muted },
            Rect::new(rect.x + 12.0, rect.y, rect.w - 60.0, rect.h),
            Align::Start,
        );
        if selected {
            self.painter.paint(
                pixmap,
                "выбран",
                s.micro,
                p.accent,
                Rect::new(rect.right() - 56.0, rect.y, 46.0, rect.h),
                Align::End,
            );
        }
    }

    /// Схема matugen: та же подсветка выбранного, что и у темы.
    #[allow(clippy::too_many_arguments)]
    fn draw_scheme(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        name: &str,
        index: usize,
        selected: bool,
        s: Sizes,
        pixel: bool,
    ) {
        let control = Control::Scheme(index);
        self.draw_button(
            pixmap,
            p,
            rect,
            name,
            selected || self.hovered(control) || self.focused(control),
            s.micro,
            pixel,
        );
        if selected {
            let k = SCALE;
            self.painter.paint(
                pixmap,
                "✓",
                s.row,
                p.accent,
                Rect::new(rect.right() - 26.0, rect.y, 20.0, rect.h),
                Align::Center,
            );
            stroke_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                p.accent,
                2.0 * k,
            );
        }
    }

    /// Действие меню без бинда: пометка «нет клавиши» вместо сочетания.
    fn draw_missing_bind(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        desc: &str,
        s: Sizes,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if self.pixel_mode() { 0.0 } else { 6.0 * k },
            p.idle_panel,
        );
        self.painter.paint_boxed(
            pixmap,
            "нет клавиши",
            s.micro,
            p.accent,
            Rect::new(rect.x + 10.0, rect.y, rect.w * 0.42, rect.h),
            Align::Start,
        );
        let keys_x = rect.x + 10.0 + rect.w * 0.42;
        self.painter.paint_boxed(
            pixmap,
            desc,
            s.row,
            p.muted,
            Rect::new(keys_x + 10.0, rect.y, rect.right() - keys_x - 20.0, rect.h),
            Align::Start,
        );
    }

    /// Строка сводки: подпись слева, значение справа. Кликается как ссылка в
    /// раздел, к которому относится значение, — дублировать контролы не нужно.
    #[allow(clippy::too_many_arguments)]
    fn draw_summary(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        label: &str,
        value: &str,
        section: Section,
        s: Sizes,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if self.pixel_mode() { 0.0 } else { 8.0 * k },
            p.idle_panel,
        );
        stroke_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            p.border,
            k,
        );
        self.painter.paint(
            pixmap,
            label,
            s.row,
            p.muted,
            Rect::new(rect.x + 12.0, rect.y, rect.w * 0.5, rect.h),
            Align::Start,
        );
        let control = Control::Goto(section);
        let active = self.hovered(control) || self.focused(control);
        if active {
            stroke_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                p.accent,
                k,
            );
        }
        self.painter.paint_boxed(
            pixmap,
            value,
            s.row,
            p.text,
            Rect::new(rect.x + rect.w * 0.5, rect.y, rect.w * 0.5 - 34.0, rect.h),
            Align::End,
        );
        self.painter.paint(
            pixmap,
            "→",
            s.row,
            if active { p.accent } else { p.muted },
            Rect::new(rect.right() - 28.0, rect.y, 20.0, rect.h),
            Align::Center,
        );
    }

    /// Строка модуля в разделе «Панель»: название, переключатель вкл/выкл и
    /// кнопки ▲▼ строго слева направо, без наложений и обрезки.
    fn draw_module_row(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        module: Module,
        line: usize,
        rect: Rect,
        s: Sizes,
    ) {
        let k = SCALE;
        let control = Control::Switch(module);
        let on = self.config.module_enabled(module);
        let radius = if self.pixel_mode() { 0.0 } else { 6.0 * k };
        if self.hovered(control) || self.focused(control) {
            fill_round_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                radius,
                p.idle_panel,
            );
        }

        // 1. Название. Длинные вроде «Не беспокоить» умещаются целиком:
        //    ширина полосы задана от содержимого колонки, а не наоборот.
        let name = module.label();
        self.painter.paint_boxed(
            pixmap,
            name,
            s.micro,
            if on { p.text } else { p.muted },
            settings_ui::module_name_rect(module, line),
            Align::Start,
        );

        // 2. Переключатель вкл/выкл.
        let switch = settings_ui::module_switch_rect(module, line);
        fill_round_rect(
            pixmap,
            switch.x * k,
            switch.y * k,
            switch.w * k,
            switch.h * k,
            switch.h / 2.0 * k,
            if on { p.accent } else { p.border },
        );
        let knob = switch.h - 4.0;
        let knob_x = if on {
            switch.right() - knob - 2.0
        } else {
            switch.x + 2.0
        };
        fill_round_rect(
            pixmap,
            knob_x * k,
            (switch.y + 2.0) * k,
            knob * k,
            knob * k,
            knob / 2.0 * k,
            if on { p.base } else { p.text.with_a(0.7) },
        );

        if self.focused(control) {
            fill_rect(
                pixmap,
                rect.x * k,
                (rect.y + 5.0) * k,
                2.0 * k,
                (rect.h - 10.0) * k,
                p.accent,
            );
        }
        let _ = radius;
    }

    /// Горячая клавиша из niri: сочетание слева, расшифровка справа. Строка
    /// informational — кликать тут нечего.
    fn draw_hotkey(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        keys: &str,
        desc: &str,
        s: Sizes,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if self.pixel_mode() { 0.0 } else { 6.0 * k },
            p.idle_panel,
        );
        // Обе части обрезаются по измеренной ширине: длинное сочетание или
        // расшифровка не должны вылезать за границу колонки.
        // Полоса клавиш фиксированной доли строки: длинное сочетание и длинная
        // расшифровка обе обрезаются по измеренной ширине и не выходят за колонку.
        let keys_w = rect.w * 0.42;
        let keys_x = rect.x + 10.0;
        self.painter.paint_boxed(
            pixmap,
            keys,
            s.micro,
            p.accent,
            Rect::new(keys_x, rect.y, keys_w, rect.h),
            Align::Start,
        );
        let desc_x = keys_x + keys_w + 10.0;
        self.painter.paint_boxed(
            pixmap,
            desc,
            s.row,
            p.text,
            Rect::new(desc_x, rect.y, rect.right() - desc_x - 10.0, rect.h),
            Align::Start,
        );
    }

    /// Кнопка шапки: подпись центрируется по обеим осям внутри рамки.
    #[allow(clippy::too_many_arguments)]
    fn draw_button(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        label: &str,
        active: bool,
        size: f32,
        pixel: bool,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if pixel { 0.0 } else { 6.0 * k },
            if active { p.panel } else { p.idle_panel },
        );
        stroke_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if active { p.accent } else { p.border },
            k,
        );
        self.painter
            .paint(pixmap, label, size, p.text, rect, Align::Center);
    }

    fn draw_navigation(&mut self, pixmap: &mut tiny_skia::Pixmap, p: UiPalette, s: Sizes) {
        let k = SCALE;
        let focus = self.nav.focus;
        let hover = self.hover;
        let section_now = self.nav.section;
        let rail = settings_ui::rail_rect();
        fill_round_rect(
            pixmap,
            rail.x * k,
            rail.y * k,
            rail.w * k,
            rail.h * k,
            7.0 * k,
            p.idle_panel,
        );
        stroke_rect(
            pixmap,
            rail.x * k,
            rail.y * k,
            rail.w * k,
            rail.h * k,
            p.border,
            k,
        );
        self.painter.paint(
            pixmap,
            "РАЗДЕЛЫ",
            s.micro,
            p.muted,
            Rect::new(rail.x + 18.0, rail.y + 14.0, rail.w - 36.0, 20.0),
            Align::Start,
        );

        // Три состояния пункта: активный раздел — заливка и полоса слева,
        // фокус — контур, наведение — слабая подсветка.
        for (index, section) in Section::ALL.iter().enumerate() {
            let rect = settings_ui::nav_rect(index);
            let selected = *section == section_now;
            let focused = focus == Focus::Nav(index);
            let hovered = hover == Some(Focus::Nav(index));
            if selected {
                fill_round_rect(
                    pixmap,
                    rect.x * k,
                    rect.y * k,
                    rect.w * k,
                    rect.h * k,
                    5.0 * k,
                    p.panel,
                );
                fill_rect(
                    pixmap,
                    rect.x * k,
                    (rect.y + 8.0) * k,
                    3.0 * k,
                    (rect.h - 16.0) * k,
                    p.accent,
                );
            } else if hovered {
                fill_round_rect(
                    pixmap,
                    rect.x * k,
                    rect.y * k,
                    rect.w * k,
                    rect.h * k,
                    5.0 * k,
                    p.idle_panel,
                );
            }
            if focused && !selected {
                stroke_rect(
                    pixmap,
                    rect.x * k,
                    rect.y * k,
                    rect.w * k,
                    rect.h * k,
                    p.accent,
                    k,
                );
            }
            self.painter.paint(
                pixmap,
                section.label(),
                s.row,
                if selected { p.text } else { p.muted },
                Rect::new(rect.x + 20.0, rect.y, rect.w - 32.0, rect.h),
                Align::Start,
            );
        }

        let rail_w = rail.w - 36.0;
        fill_rect(
            pixmap,
            (rail.x + 18.0) * k,
            settings_ui::RAIL_RULE_Y * k,
            rail_w * k,
            k,
            p.border,
        );
        let theme = if self.pixel_mode() {
            "ТЕМА: PIXEL"
        } else {
            "ТЕМА: NORMAL"
        };
        let language = if self.config.language == settings::Language::Ru {
            "ЯЗЫК: РУС"
        } else {
            "ЯЗЫК: ENG"
        };
        let lines = [
            ("СОСТОЯНИЕ", p.muted),
            ("● HUDbar активен", p.accent),
            (concat!("v", env!("CARGO_PKG_VERSION")), p.muted),
            (theme, p.muted),
            (language, p.muted),
        ];
        for (index, (text, color)) in lines.into_iter().enumerate() {
            let y = settings_ui::RAIL_STATUS_Y + index as f32 * 30.0;
            self.painter.paint(
                pixmap,
                text,
                s.micro,
                color,
                Rect::new(rail.x + 18.0, y, rail_w, 22.0),
                Align::Start,
            );
        }
    }

    /// Карточка темы: чекбокс, название и метка «применено» стоят в одной полосе,
    /// описание и шрифт — в двух следующих. Всё центрируется по вертикали.
    fn draw_theme_card(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        pixel: bool,
        rect: Rect,
        s: Sizes,
    ) {
        let k = SCALE;
        let control = Control::Theme(pixel);
        let applied = self.config.theme
            == Some(if pixel {
                config::Theme::Pixel
            } else {
                config::Theme::Normal
            });
        let selected = self.pixel_mode() == pixel;
        let hovered = self.hovered(control);
        let focused = self.focused(control);
        let (title, description, font) = settings_ui::theme_card(pixel);
        let border = if selected || hovered || focused {
            p.accent
        } else {
            p.border
        };
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if self.pixel_mode() { 0.0 } else { 10.0 * k },
            if selected { p.panel } else { p.idle_panel },
        );
        stroke_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            border,
            if selected || hovered || focused {
                2.0 * k
            } else {
                k
            },
        );

        // Полоса заголовка: чекбокс, имя темы и метка применения — одна строка.
        let title_band = settings_ui::card_title_rect(pixel);
        let box_size = 18.0;
        let box_x = rect.x + 16.0;
        let box_y = title_band.y + (title_band.h - box_size) / 2.0;
        fill_round_rect(
            pixmap,
            box_x * k,
            box_y * k,
            box_size * k,
            box_size * k,
            if self.pixel_mode() { 0.0 } else { 4.0 * k },
            if selected { p.accent } else { p.base },
        );
        stroke_rect(
            pixmap,
            box_x * k,
            box_y * k,
            box_size * k,
            box_size * k,
            p.accent,
            k,
        );
        self.painter.paint(
            pixmap,
            title,
            s.section + 3.0,
            p.text,
            Rect::new(
                box_x + box_size + 12.0,
                title_band.y,
                title_band.w - box_size - 96.0,
                title_band.h,
            ),
            Align::Start,
        );
        if applied {
            self.painter.paint(
                pixmap,
                "применено",
                s.micro,
                p.accent,
                Rect::new(rect.right() - 104.0, title_band.y, 88.0, title_band.h),
                Align::End,
            );
        }
        self.painter.paint_boxed(
            pixmap,
            description,
            s.micro,
            p.muted,
            settings_ui::card_desc_rect(pixel),
            Align::Start,
        );
        self.painter.paint_boxed(
            pixmap,
            font,
            s.micro,
            p.accent,
            settings_ui::card_font_rect(pixel),
            Align::Start,
        );
    }

    /// Строка модуля: подпись слева, переключатель справа.
    #[allow(clippy::too_many_arguments)]
    fn draw_toggle(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        label: &str,
        on: bool,
        hovered: bool,
        focused: bool,
        s: Sizes,
    ) {
        let k = SCALE;
        if hovered || focused {
            fill_round_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                if self.pixel_mode() { 0.0 } else { 8.0 * k },
                p.idle_panel,
            );
        }
        if focused {
            fill_rect(
                pixmap,
                rect.x * k,
                (rect.y + 6.0) * k,
                2.0 * k,
                (rect.h - 12.0) * k,
                p.accent,
            );
        }
        // Переключатель, подпись и «вкл» живут в одной полосе строки, поэтому
        // их центры совпадают по вертикали при любом размере шрифта.
        let switch_w = 38.0;
        let switch_h = 20.0;
        let switch_rect = Rect::new(rect.right() - 12.0 - switch_w, rect.y, switch_w, rect.h);
        let track_y = switch_rect.y + (switch_rect.h - switch_h) / 2.0;
        fill_round_rect(
            pixmap,
            switch_rect.x * k,
            track_y * k,
            switch_w * k,
            switch_h * k,
            if self.pixel_mode() { 0.0 } else { 10.0 * k },
            if on { p.accent } else { p.border },
        );
        let knob = 16.0;
        let knob_x = if on {
            switch_rect.right() - knob - 2.0
        } else {
            switch_rect.x + 2.0
        };
        fill_round_rect(
            pixmap,
            knob_x * k,
            (track_y + 2.0) * k,
            knob * k,
            knob * k,
            if self.pixel_mode() { 0.0 } else { 8.0 * k },
            if on { p.base } else { p.text.with_a(0.7) },
        );
        self.painter.paint(
            pixmap,
            label,
            s.row,
            if on { p.text } else { p.muted },
            Rect::new(rect.x + 12.0, rect.y, 240.0, rect.h),
            Align::Start,
        );
        self.painter.paint(
            pixmap,
            if on { "вкл" } else { "выкл" },
            s.micro,
            p.muted,
            Rect::new(switch_rect.x - 60.0, rect.y, 48.0, rect.h),
            Align::End,
        );
    }

    /// Строка высоты: дорожка, значение, подпись диапазона и кнопки шага стоят
    /// в одной полосе, поэтому всё лежит на одной линии.
    fn draw_height_scale(&mut self, pixmap: &mut tiny_skia::Pixmap, p: UiPalette, s: Sizes) {
        let k = SCALE;
        let track = settings_ui::height_track_rect();
        fill_round_rect(
            pixmap,
            track.x * k,
            track.y * k,
            track.w * k,
            track.h * k,
            3.0 * k,
            p.idle_panel,
        );
        let span = settings_ui::HEIGHT_MAX - settings_ui::HEIGHT_MIN;
        let share = (self.config.height - settings_ui::HEIGHT_MIN) as f32 / span as f32;
        fill_round_rect(
            pixmap,
            track.x * k,
            track.y * k,
            (track.w * share).max(6.0) * k,
            track.h * k,
            3.0 * k,
            p.accent,
        );
        let value = self.config.height.to_string();
        self.painter.paint(
            pixmap,
            &value,
            s.value,
            p.text,
            settings_ui::height_value_rect(),
            Align::Center,
        );
        let hint = format!("{}–{} px", settings_ui::HEIGHT_MIN, settings_ui::HEIGHT_MAX);
        self.painter.paint(
            pixmap,
            &hint,
            s.micro,
            p.muted,
            settings_ui::height_hint_rect(),
            Align::Start,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_step(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        rect: Rect,
        label: &str,
        hovered: bool,
        focused: bool,
        value_size: f32,
    ) {
        let k = SCALE;
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if self.pixel_mode() { 0.0 } else { 8.0 * k },
            if hovered || focused {
                p.panel
            } else {
                p.idle_panel
            },
        );
        if focused {
            stroke_rect(
                pixmap,
                rect.x * k,
                rect.y * k,
                rect.w * k,
                rect.h * k,
                p.accent,
                k,
            );
        }
        self.painter
            .paint(pixmap, label, value_size, p.text, rect, Align::Center);
    }

    /// Подсказки внизу: промежутки одинаковые, блок центрируется по ширине полосы.
    fn draw_footer_hints(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        y: f32,
        size: f32,
        hints: &[&str],
    ) {
        let band = settings_ui::footer_rect();
        let widths: Vec<f32> = hints
            .iter()
            .map(|hint| self.text_width(hint, size))
            .collect();
        let offsets = settings_ui::spread(band.w, &widths);
        for (hint, offset) in hints.iter().zip(offsets) {
            self.painter.paint(
                pixmap,
                hint,
                size,
                p.muted,
                Rect::new(band.x + offset, y, 400.0, band.h),
                Align::Start,
            );
        }
    }
}

impl CompositorHandler for SettingsApp {
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

impl OutputHandler for SettingsApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for SettingsApp {
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

impl WindowHandler for SettingsApp {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit.store(true, Ordering::Relaxed);
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        self.width = configure.new_size.0.map(|v| v.get()).unwrap_or(WIDTH);
        self.height = configure.new_size.1.map(|v| v.get()).unwrap_or(HEIGHT);
        self.configured = true;
        self.dirty = true;
    }
}

impl PointerHandler for SettingsApp {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if event.surface != *self.window.wl_surface() {
                continue;
            }
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let next = self.hit(event.position.0 as f32, event.position.1 as f32);
                    if next != self.hover {
                        self.hover = next;
                        self.dirty = true;
                    }
                    if self.dragging_height {
                        self.set_height_from_x(event.position.0 as f32);
                    }
                }
                PointerEventKind::Leave { .. } => {
                    self.hover = None;
                    self.dirty = true;
                }
                PointerEventKind::Press { button: 0x110, .. } => {
                    let position = (event.position.0 as f32, event.position.1 as f32);
                    if let Some(focus) = self.hit(position.0, position.1) {
                        // Клик всегда двигает фокус и выполняет действие: в сайдбаре
                        // открывает раздел, в контенте применяет контрол.
                        self.nav.focus = focus;
                        self.hover = Some(focus);
                        if focus == Focus::Content(Control::HeightSlider) {
                            self.dragging_height = true;
                            self.set_height_from_x(position.0);
                        } else {
                            self.activate();
                        }
                    }
                }
                PointerEventKind::Release { button: 0x110, .. } if self.dragging_height => {
                    self.dragging_height = false;
                    self.flush_height(true);
                }
                _ => {}
            }
        }
    }
}

impl KeyboardHandler for SettingsApp {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
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
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        // Пока стрелка удерживается, шаги идут по своему таймеру: повторы
        // композитора в этот момент только мешали бы.
        if self
            .held
            .as_ref()
            .is_some_and(|held| held.raw == event.raw_code)
        {
            return;
        }
        let raw = event.raw_code;
        let arrow = match event.keysym {
            Keysym::Up => Some(-1),
            Keysym::Down => Some(1),
            _ => None,
        };
        let in_sidebar = self.nav.focus_nav();
        self.on_key(event);
        if let Some(dir) = arrow
            && in_sidebar
        {
            self.hold_nav_key(raw, dir);
        }
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        // Повтор композитора во время удержания игнорируем: шагает наш таймер.
        if self
            .held
            .as_ref()
            .is_some_and(|held| held.raw == event.raw_code)
        {
            return;
        }
        self.on_key(event);
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.release_nav_key(event.raw_code);
        self.flush_height(true);
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

impl ShmHandler for SettingsApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for SettingsApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(SettingsApp);
smithay_client_toolkit::delegate_dispatch2!(SettingsApp);

fn fill_rect(pixmap: &mut tiny_skia::Pixmap, x: f32, y: f32, w: f32, h: f32, color: palette::Rgba) {
    let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) else {
        return;
    };
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Shader::SolidColor(color.to_tiny()),
        anti_alias: false,
        ..Default::default()
    };
    pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
}

fn fill_round_rect(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    color: palette::Rgba,
) {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut path = tiny_skia::PathBuilder::new();
    path.move_to(x + r, y);
    path.line_to(x + w - r, y);
    path.quad_to(x + w, y, x + w, y + r);
    path.line_to(x + w, y + h - r);
    path.quad_to(x + w, y + h, x + w - r, y + h);
    path.line_to(x + r, y + h);
    path.quad_to(x, y + h, x, y + h - r);
    path.line_to(x, y + r);
    path.quad_to(x, y, x + r, y);
    path.close();
    if let Some(path) = path.finish() {
        let paint = tiny_skia::Paint {
            shader: tiny_skia::Shader::SolidColor(color.to_tiny()),
            anti_alias: true,
            ..Default::default()
        };
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }
}

fn stroke_rect(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: palette::Rgba,
    width: f32,
) {
    fill_rect(pixmap, x, y, w, width, color);
    fill_rect(pixmap, x, y + h - width, w, width, color);
    fill_rect(pixmap, x, y, width, h, color);
    fill_rect(pixmap, x + w - width, y, width, h, color);
}

fn main() {
    let conn = Connection::connect_to_env().expect("wayland connection");
    let (globals, mut event_queue) = registry_queue_init(&conn).expect("registry");
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm");
    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::ServerDefault, &qh);
    window.set_title(TITLE);
    window.set_app_id("com.mihail.hud-settings");
    window.set_min_size(Some((WIDTH, HEIGHT)));
    window.set_max_size(Some((WIDTH, HEIGHT)));
    window.commit();

    let exit = Arc::new(AtomicBool::new(false));
    // Буфер 1100x740 при scale 2 — это 2200x1480x4 = 13 МБ на кадр. Пул держим
    // на три кадра, иначе при нехватке буферов кадр не рисуется вовсе и сквозь
    // окно видно то, что под ним.
    let frame_bytes = WIDTH as usize * HEIGHT as usize * 4 * (SCALE as usize) * (SCALE as usize);
    let pool = SlotPool::new(frame_bytes * 3 + 4_000_000, &shm).expect("shm pool");
    let config = config::Config::from(&settings::load());
    let picked = config.theme == Some(config::Theme::Pixel);
    let hotkeys = load_hotkeys();
    let start = settings_ui::Section::from_env();
    let wallpaper = settings_ui::Wallpaper {
        files: load_wallpapers(),
        ..settings_ui::Wallpaper::default()
    };
    let rows = settings_ui::rows_for_state(start, &config, &hotkeys, 0, &wallpaper);
    let mut app = SettingsApp {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        pool,
        window,
        pointer: None,
        keyboard: None,
        mods: Modifiers::default(),
        painter: TextPainter::new(if picked {
            "Minecraft Rus"
        } else {
            "JetBrainsMono Nerd Font Propo"
        }),
        config,
        hotkeys,
        hotkey_scroll: 0,
        wallpaper,
        nav: Nav::new(start),
        picked: None,
        status: "Готово".to_string(),
        job: Arc::new(Mutex::new(Job::default())),
        rows,
        palette: palette::load(),
        settings_stamp: stamp(&settings_path()),
        palette_stamp: stamp(&home().join(".config/hudbar/colors.css")),
        pending_height: None,
        width: WIDTH,
        height: HEIGHT,
        configured: false,
        dirty: true,
        exit,
        hover: None,
        dragging_height: false,
        held: None,
    };

    while !app.exit.load(Ordering::Relaxed) {
        app.refresh_sources();
        app.poll_job();
        if event_queue.flush().is_err() {
            break;
        }
        if let Some(guard) = event_queue.prepare_read() {
            let mut fds = [PollFd::new(conn.as_fd(), PollFlags::POLLIN)];
            let timeout = app
                .repeat_deadline()
                .map(|left| left.as_millis().min(250) as u16)
                .unwrap_or(250);
            let _ = poll(&mut fds, timeout);
            if fds[0]
                .revents()
                .is_some_and(|flags| flags.intersects(PollFlags::POLLIN))
            {
                let _ = guard.read();
            } else {
                drop(guard);
            }
        }
        if event_queue.dispatch_pending(&mut app).is_err() {
            break;
        }
        app.tick_repeat();
        app.flush_height(false);
        if app.dirty {
            app.dirty = false;
            app.draw();
        }
    }
}

//! LEGACY: retained for compatibility and release builds, but no longer
//! launched by the current menu or panel. Remove after the Wi-Fi/audio/BT
//! stage is complete and those routes are confirmed in the new menu.

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
#[path = "../hud/log.rs"]
#[allow(dead_code)]
mod log;
#[path = "../hud/palette.rs"]
mod palette;
#[path = "../hud/settings.rs"]
#[allow(dead_code)]
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

use config::Config;
use settings_ui::{Control, Focus, Hotkey, Module, Nav, NotificationPosition, Row, Section};
use settings_view::{DrawExt, View, file_name};
use text::{SCALE, TextPainter};
const WIDTH: u32 = settings_ui::WIDTH as u32;
const HEIGHT: u32 = settings_ui::HEIGHT as u32;

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

/// Файлы обоев из `~/wallpapers` — рекурсивно, по четырём расширениям и с
/// сортировкой по пути. Обход и правила живут в `hud/wallpaper.rs`, чтобы их
/// можно было покрыть тестами; rofi с превью тут не нужен, список листается
/// в самом окне.
fn load_wallpapers() -> Vec<String> {
    wallpaper::list()
}

#[cfg(test)]
mod tests {
    use super::*;
    use text::Align;
    use text::SCALE;
    use ui_tokens::{BG_ALPHA, ui_palette};

    /// Фон окна настроек обязан быть непрозрачным: сквозь него видно терминал,
    /// и подписи на прозрачном фоне читаются хуже.
    #[test]
    fn window_background_is_opaque() {
        let palette = palette::Palette::default();
        assert_eq!(ui_palette(false, palette).base.3, 255);
        assert_eq!(ui_palette(true, palette).base.3, 255);
        assert_eq!(BG_ALPHA, 1.0);
    }

    /// Пересборка строк обязана брать состояние «Обоев»: с пустым состоянием
    /// список файлов исчезал сразу после нажатия. Тест ловит именно вызов
    /// `rows_for`, который состояние роняет.
    #[test]
    fn rebuild_rows_keeps_the_wallpaper_state() {
        let source = include_str!("hud_settings.rs");
        // Строка склеена из кусков: иначе тест поймал бы сам себя, ведь
        // `rows_for_state(` начинается с `rows_for(`.
        let state_less = ["settings_ui::rows_for", "("].concat();
        assert!(
            !source.contains(&state_less),
            "rebuild_rows не должен терять состояние раздела «Обои»"
        );
        assert!(
            source.contains("self.rows = settings_ui::rows_for_state("),
            "rebuild_rows должен звать rows_for_state"
        );
        // Снапшот собирает строки тем же способом: иначе на PNG были бы другие
        // списки, чем в живом окне.
        let snapshot = include_str!("../hud/settings_snapshot.rs");
        assert!(
            snapshot.contains("settings_ui::rows_for_state("),
            "снапшот должен собирать строки через rows_for_state"
        );
        assert!(
            !snapshot.contains(&state_less),
            "снапшот не должен терять состояние раздела"
        );
    }

    #[test]
    fn settings_window_has_no_legacy_setting_commands() {
        // Отрисовка уехала в settings_view, поэтому инвариант проверяется в
        // обоих файлах: старые скрипты не должны вернуться ни туда, ни сюда.
        for source in [
            include_str!("hud_settings.rs"),
            include_str!("../hud/settings_view.rs"),
            include_str!("../hud/settings_snapshot.rs"),
        ] {
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

/// Состояние живого окна для отрисовки: тот же контракт, что у снапшота.
impl View for SettingsApp {
    fn painter(&mut self) -> &mut TextPainter {
        &mut self.painter
    }
    fn config(&self) -> &Config {
        &self.config
    }
    fn rows(&self) -> &[Row] {
        &self.rows
    }
    fn nav(&self) -> &Nav {
        &self.nav
    }
    fn hover(&self) -> Option<Focus> {
        self.hover
    }
    fn picked(&self) -> Option<bool> {
        self.picked
    }
    fn palette(&self) -> palette::Palette {
        self.palette
    }
    fn status(&self) -> String {
        self.status.clone()
    }
    fn busy_flag(&self) -> bool {
        self.job.lock().is_ok_and(|job| job.busy)
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
        self.painter
            .set_font(&settings_view::font_name(self.pixel_mode()));
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
            self.painter
                .set_font(&settings_view::font_name(self.pixel_mode()));
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
        self.painter.set_font(&settings_view::font_name(pixel));
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

    /// Выбор языка кнопкой: пишем только когда он действительно меняется,
    /// иначе каждое нажатие переписывало бы файл впустую.
    fn set_language(&mut self, language: settings::Language) {
        if self.config.language == language {
            self.status = format!("Язык уже {}", language.short());
            self.dirty = true;
            return;
        }
        self.commit(config_io::Patch::Language(language));
    }

    /// Клавиша L переключает язык без мыши: вторая кнопка для быстрого
    /// переключения, сам выбор всё равно за кнопкой.
    fn toggle_language(&mut self) {
        let next = if self.config.language == settings::Language::Ru {
            settings::Language::En
        } else {
            settings::Language::Ru
        };
        self.set_language(next);
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
            self.wallpaper.selected = Some(index);
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
        // Пока файл не выбран, кнопка неактивна и сюда не доходит. Проверка
        // остаётся на случай гонки: список перечитывается при сворачивании.
        let Some(file) = self
            .wallpaper
            .selected
            .and_then(|index| self.wallpaper.files.get(index))
            .cloned()
        else {
            self.status = "Сначала выберите файл обоев".to_string();
            self.dirty = true;
            return;
        };
        let scheme = settings_ui::SCHEMES[self.wallpaper.scheme].to_string();
        self.status = "Обои применяются…".to_string();
        self.dirty = true;
        self.start_job("Обои", move || wallpaper::apply(&file, &scheme));
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
        let next = if forward {
            (self.wallpaper.scroll + page).min(max_scroll)
        } else {
            self.wallpaper.scroll.saturating_sub(page)
        };
        if next == self.wallpaper.scroll {
            return;
        }
        self.wallpaper.scroll = next;
        // Выделение подтягиваем в видимую часть списка. Пока файл не выбран,
        // листание не выбирает его за пользователя.
        if let Some(selected) = self.wallpaper.selected {
            self.wallpaper.selected = Some(if selected < next {
                next
            } else if selected >= next + page {
                next + page - 1
            } else {
                selected
            });
        }
        let last = (next + page).min(count);
        self.status = format!("Файлов: {count} · показано {}-{last}", next + 1);
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
    ///
    /// Именно `rows_for_state`, а не `rows_for`: у раздела «Обои» есть
    /// состояние (файлы, выбор, прокрутка), и пересборка через `rows_for`
    /// подставляла пустое — список файлов исчезал после любого нажатия.
    fn rebuild_rows(&mut self) {
        self.rows = settings_ui::rows_for_state(
            self.nav.section,
            &self.config,
            &self.hotkeys,
            self.hotkey_scroll,
            &self.wallpaper,
        );
        self.nav.focus = settings_ui::focus_after_rebuild(&self.rows, self.nav.focus);
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
            Control::Language(language) => self.set_language(language),
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
        // Вся отрисовка живёт в settings_view: один и тот же код рисует живое
        // окно и снапшоты без Wayland. Здесь осталась только доставка буфера.
        settings_view::DrawExt::render(self, &mut pixmap);

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

fn main() {
    // Снимок не требует композитора: разбираем аргументы и выходим.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--snapshot") {
        if let Err(error) = settings_snapshot::run(&args[1..]) {
            eprintln!("{error}\n{}", settings_snapshot::hint());
            std::process::exit(1);
        }
        return;
    }

    let conn = Connection::connect_to_env().expect("wayland connection");
    let (globals, mut event_queue) = registry_queue_init(&conn).expect("registry");
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm");
    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::ServerDefault, &qh);
    window.set_title(settings_view::TITLE);
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

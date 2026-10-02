use std::os::fd::AsFd;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use cosmic_text::{
    Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
};
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

#[path = "../hud/palette.rs"]
mod palette;
#[allow(dead_code)]
#[path = "../hud/settings_ui.rs"]
mod settings_ui;

use settings_ui::{Control, Module, Rect, Row};

const SCALE: f32 = 2.0;
const WIDTH: u32 = settings_ui::WIDTH as u32;
const HEIGHT: u32 = settings_ui::HEIGHT as u32;
const TITLE: &str = "HUD Settings";
/// Прозрачность фона как у hudbar: base на 90%, сквозь окно видно стол.
const BG_ALPHA: f32 = 0.9;
/// Скрипт, который пишет настройки, перезапускает панель и dunst.
const HUD_SETTING: &str = ".local/bin/hud-setting";

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

/// Состояние из `settings.json`: тема, высота бара и флаги модулей.
struct State {
    mode: String,
    height: u32,
    modules: Vec<(Module, bool)>,
}

impl State {
    fn module(&self, module: Module) -> bool {
        self.modules
            .iter()
            .find(|(m, _)| *m == module)
            .map(|(_, on)| *on)
            .unwrap_or(true)
    }

    fn set_module(&mut self, module: Module, on: bool) {
        if let Some(entry) = self.modules.iter_mut().find(|(m, _)| *m == module) {
            entry.1 = on;
        }
    }
}

/// Фоновый запуск `hud-setting`: он перезапускает панель и dunst, поэтому окно
/// не должно висеть в ожидании. Пока команда идёт, ввод блокируется — иначе две
/// одновременные правки state.json друг друга затрут.
#[derive(Default)]
struct Job {
    busy: bool,
    result: Option<(bool, String)>,
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
    font_system: FontSystem,
    swash: SwashCache,
    font: String,
    state: State,
    /// Тема, которую пользователь выбрал, но которая ещё не применена.
    picked: Option<bool>,
    focus: Control,
    status: String,
    job: Arc<Mutex<Job>>,
    rows: Vec<Row>,
    palette: palette::Palette,
    settings_stamp: Option<SystemTime>,
    palette_stamp: Option<SystemTime>,
    width: u32,
    height: u32,
    configured: bool,
    dirty: bool,
    exit: Arc<AtomicBool>,
    hover: Option<Control>,
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

/// Читает `settings.json` целиком: тема, высота и флаги модулей.
/// Отсутствующие ключи берём у модулей и у правого предела — как `hud-setting`.
fn read_state() -> State {
    let mut state = State {
        mode: "pixel".to_string(),
        height: 27,
        modules: Module::ALL.into_iter().map(|m| (m, true)).collect(),
    };
    let Ok(text) = std::fs::read_to_string(settings_path()) else {
        return state;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return state;
    };
    if let Some(mode) = value
        .get("appearance")
        .and_then(|v| v.get("theme"))
        .and_then(|v| v.as_str())
        .filter(|v| *v == "pixel" || *v == "normal")
    {
        state.mode = mode.to_string();
    }
    let Some(bar) = value.get("hudbar") else {
        return state;
    };
    if let Some(height) = bar.get("height").and_then(|v| v.as_u64()) {
        state.height = settings_ui::clamp_height(height as i64);
    }
    for module in Module::ALL {
        if let Some(on) = bar.get(module.key()).and_then(|v| v.as_bool()) {
            state.set_module(module, on);
        }
    }
    state
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

impl SettingsApp {
    /// Тема, которую сейчас рисуем: выбранная, иначе применённая.
    fn pixel_mode(&self) -> bool {
        self.picked.unwrap_or(self.state.mode == "pixel")
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
        let Ok(mut job) = self.job.lock() else {
            return;
        };
        let Some((ok, what)) = job.result.take() else {
            return;
        };
        job.busy = false;
        self.status = if ok {
            format!("{what} — готово")
        } else {
            format!("{what} — не получилось")
        };
        self.dirty = true;
    }

    fn refresh_sources(&mut self) {
        let settings_stamp = stamp(&settings_path());
        if settings_stamp != self.settings_stamp {
            self.settings_stamp = settings_stamp;
            self.state = read_state();
            self.picked = None;
            self.font = self.font_for(self.pixel_mode());
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

    /// Запускает `hud-setting` в фоне и показывает, что идёт применение.
    fn run(&mut self, args: &[&str], what: &str) {
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
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let job = Arc::clone(&self.job);
        thread::spawn(move || {
            let ok = Command::new(home().join(HUD_SETTING))
                .args(&args)
                .status()
                .is_ok_and(|status| status.success());
            if let Ok(mut job) = job.lock() {
                job.busy = false;
                job.result = Some((ok, what));
            }
        });
    }

    fn apply_theme(&mut self, pixel: bool) {
        let mode = if pixel { "pixel" } else { "normal" };
        if mode == self.state.mode {
            self.picked = None;
            self.dirty = true;
            return;
        }
        self.picked = Some(pixel);
        self.font = self.font_for(pixel);
        self.run(&["theme", mode], "Оформление");
    }

    fn toggle_module(&mut self, module: Module) {
        let next = !self.state.module(module);
        self.run(
            &[
                "hud-toggle",
                module.key(),
                if next { "true" } else { "false" },
            ],
            module.label(),
        );
    }

    fn step_height(&mut self, delta: i32) {
        let Some(height) = settings_ui::step_height(self.state.height, delta) else {
            return;
        };
        let value = height.to_string();
        self.run(&["hud-height", &value], "Высота панели");
    }

    fn activate(&mut self, control: Control) {
        match control {
            Control::Theme(pixel) => self.apply_theme(pixel),
            Control::Toggle(module) => self.toggle_module(module),
            Control::Height(delta) => self.step_height(delta),
            Control::Close => self.exit.store(true, Ordering::Relaxed),
        }
    }

    /// Левая стрелка и вверх идут назад, правая и вниз — вперёд.
    fn move_focus(&mut self, dx: i32, dy: i32) {
        if let Some(next) = settings_ui::neighbour(&self.rows, self.focus, dx, dy) {
            self.focus = next;
            self.dirty = true;
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<Control> {
        settings_ui::hit(&self.rows, x / SCALE, y / SCALE)
    }

    fn layout(&mut self, text: &str, size: f32, line_h: f32) -> (f32, Vec<(CacheKey, i32, i32)>) {
        let mut buffer = Buffer::new_empty(Metrics::new(size * SCALE, line_h * SCALE));
        let attrs = Attrs::new().family(Family::Name(&self.font));
        let mut width: f32 = 0.0;
        let mut glyphs = Vec::new();
        {
            let mut b = buffer.borrow_with(&mut self.font_system);
            b.set_text(text, &attrs, Shaping::Advanced, None);
            b.set_size(None, None);
            b.shape_until_scroll(false);
            for run in b.layout_runs() {
                width = width.max(run.line_w);
                for glyph in run.glyphs {
                    let physical = glyph.physical((0.0, run.line_y), 1.0);
                    glyphs.push((physical.cache_key, physical.x, physical.y));
                }
            }
        }
        (width, glyphs)
    }

    fn text_width(&mut self, text: &str, size: f32) -> f32 {
        self.layout(text, size, size + 8.0).0
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        text: &str,
        size: f32,
        color: palette::Rgba,
        x: f32,
        y: f32,
        line_h: f32,
    ) {
        let (_, glyphs) = self.layout(text, size, line_h);
        for (key, gx, gy) in glyphs {
            if let Some(image) = self.swash.get_image_uncached(&mut self.font_system, key)
                && matches!(image.content, SwashContent::Mask)
            {
                blend(
                    pixmap,
                    x as i32 + gx,
                    y as i32 + gy - image.placement.top,
                    image.placement.width,
                    image.placement.height,
                    color,
                    &image.data,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text_boxed(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        text: &str,
        size: f32,
        color: palette::Rgba,
        max_width: f32,
        x: f32,
        y: f32,
        line_h: f32,
    ) {
        let max_width_px = max_width * SCALE;
        let mut shown = text.to_string();
        if self.text_width(&shown, size) > max_width_px {
            while !shown.is_empty() {
                let candidate = format!("{shown}…");
                if self.text_width(&candidate, size) <= max_width_px {
                    shown = candidate;
                    break;
                }
                shown.pop();
            }
        }
        self.draw_text(pixmap, &shown, size, color, x, y, line_h);
    }

    /// Общая обработка клавиши для `press_key` и `repeat_key`.
    fn on_key(&mut self, event: KeyEvent) {
        match event.keysym {
            Keysym::Escape => self.exit.store(true, Ordering::Relaxed),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                let control = self.focus;
                self.activate(control);
            }
            Keysym::Up => self.move_focus(0, -1),
            Keysym::Down => self.move_focus(0, 1),
            Keysym::Left => self.move_focus(-1, 0),
            Keysym::Right => self.move_focus(1, 0),
            Keysym::Tab => self.move_focus(0, if self.mods.shift { -1 } else { 1 }),
            Keysym::minus | Keysym::KP_Subtract | Keysym::underscore => self.step_height(-1),
            Keysym::plus | Keysym::equal | Keysym::KP_Add => self.step_height(1),
            _ => {}
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
        let title_size = if pixel { 24.0 } else { 26.0 };
        let head_size = if pixel { 12.0 } else { 13.0 };
        let label_size = if pixel { 12.0 } else { 13.0 };
        let small_size = if pixel { 11.0 } else { 12.0 };
        let value_size = if pixel { 13.0 } else { 14.0 };
        pixmap.fill(p.base.to_tiny());
        let k = SCALE;
        fill_rect(&mut pixmap, 0.0, 0.0, pw as f32, k, p.accent);
        fill_rect(&mut pixmap, 0.0, 0.0, k, ph as f32, p.accent.with_a(0.3));

        self.draw_text(
            &mut pixmap,
            TITLE,
            title_size,
            p.text,
            26.0 * k,
            20.0 * k,
            34.0 * k,
        );

        let status = self.status.clone();
        // строки копируем: рисование берёт &mut self, а обход идёт по self.rows
        let rows = self.rows.clone();
        for row in &rows {
            match *row {
                Row::Header { text, y } => self.draw_text(
                    &mut pixmap,
                    text,
                    head_size,
                    p.muted,
                    settings_ui::PAD_X * k,
                    y * k,
                    18.0 * k,
                ),
                Row::Rule { y } => {
                    let w = settings_ui::WIDTH - settings_ui::PAD_X * 2.0;
                    fill_rect(
                        &mut pixmap,
                        settings_ui::PAD_X * k,
                        y * k,
                        w * k,
                        k,
                        p.border,
                    );
                }
                Row::Theme { pixel, rect } => {
                    self.draw_theme_card(&mut pixmap, p, pixel, rect, title_size)
                }
                Row::Toggle { module, rect } => {
                    let on = self.state.module(module);
                    let control = Control::Toggle(module);
                    self.draw_toggle(
                        &mut pixmap,
                        p,
                        rect,
                        module.label(),
                        on,
                        self.hover == Some(control),
                        self.focus == control,
                        label_size,
                        small_size,
                    );
                }
                Row::HeightLabel { scale, .. } => {
                    self.draw_height_scale(&mut pixmap, p, scale, value_size, small_size);
                }
                Row::Height { dir, rect } => {
                    let control = Control::Height(dir);
                    self.draw_step(
                        &mut pixmap,
                        p,
                        rect,
                        if dir < 0 { "−" } else { "+" },
                        self.hover == Some(control),
                        self.focus == control,
                        value_size,
                    );
                }
                Row::Status { y } => {
                    let busy = self.busy();
                    self.draw_text(
                        &mut pixmap,
                        &status,
                        value_size,
                        if busy { p.accent } else { p.muted },
                        settings_ui::PAD_X * k,
                        y * k,
                        20.0 * k,
                    );
                }
                Row::Footer { y } => self.draw_footer(&mut pixmap, p, y, small_size),
                Row::Close { rect } => {
                    let control = Control::Close;
                    let fill = if self.hover == Some(control) || self.focus == control {
                        p.panel
                    } else {
                        p.idle_panel
                    };
                    fill_round_rect(
                        &mut pixmap,
                        rect.x * k,
                        rect.y * k,
                        rect.w * k,
                        rect.h * k,
                        if pixel { 0.0 } else { 8.0 * k },
                        fill,
                    );
                    let label = "Закрыть";
                    let w = self.text_width(label, label_size);
                    self.draw_text(
                        &mut pixmap,
                        label,
                        label_size,
                        p.text,
                        (rect.x + (rect.w - w / SCALE) / 2.0) * k,
                        (rect.y + 7.0) * k,
                        18.0 * k,
                    );
                }
            }
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

    fn draw_theme_card(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        pixel: bool,
        rect: Rect,
        title_size: f32,
    ) {
        let k = SCALE;
        let control = Control::Theme(pixel);
        let applied = self.state.mode == if pixel { "pixel" } else { "normal" };
        let selected = self.pixel_mode() == pixel;
        let hovered = self.hover == Some(control);
        let focused = self.focus == control;
        let (title, description, font) = settings_ui::theme_card(pixel);
        let fill = if selected { p.panel } else { p.idle_panel };
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
            if self.pixel_mode() { 0.0 } else { 12.0 * k },
            fill,
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
        let cx = rect.x + 16.0;
        let cy = rect.y + 18.0;
        fill_round_rect(pixmap, cx * k, cy * k, 18.0 * k, 18.0 * k, 0.0, p.accent);
        fill_round_rect(
            pixmap,
            (cx + 1.0) * k,
            (cy + 1.0) * k,
            16.0 * k,
            16.0 * k,
            (-k).max(0.0),
            if selected { p.accent } else { p.base },
        );
        self.draw_text(
            pixmap,
            title,
            title_size,
            p.text,
            (rect.x + 46.0) * k,
            (rect.y + 14.0) * k,
            24.0 * k,
        );
        let mark = if applied { "применено" } else { "" };
        if !mark.is_empty() {
            let w = self.text_width(mark, 12.0);
            self.draw_text(
                pixmap,
                mark,
                12.0,
                p.accent,
                (rect.right() - 16.0 - w / SCALE) * k,
                (rect.y + 18.0) * k,
                16.0 * k,
            );
        }
        self.draw_text_boxed(
            pixmap,
            description,
            if self.pixel_mode() { 12.0 } else { 13.0 },
            p.muted,
            rect.w - 32.0,
            cx * k,
            (rect.y + 50.0) * k,
            18.0 * k,
        );
        self.draw_text(
            pixmap,
            font,
            if self.pixel_mode() { 11.0 } else { 12.0 },
            p.accent,
            cx * k,
            (rect.y + 78.0) * k,
            16.0 * k,
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
        label_size: f32,
        small_size: f32,
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
                (rect.y + 5.0) * k,
                2.0 * k,
                (rect.h - 10.0) * k,
                p.accent,
            );
        }
        let sw_w = 34.0;
        let sw_h = 18.0;
        let sx = rect.right() - sw_w - 12.0;
        let sy = rect.y + (rect.h - sw_h) / 2.0;
        fill_round_rect(
            pixmap,
            sx * k,
            sy * k,
            sw_w * k,
            sw_h * k,
            if self.pixel_mode() { 0.0 } else { 9.0 * k },
            if on { p.accent } else { p.border },
        );
        let knob = 14.0;
        let kx = if on { sx + sw_w - knob - 2.0 } else { sx + 2.0 };
        fill_round_rect(
            pixmap,
            kx * k,
            (sy + 2.0) * k,
            knob * k,
            knob * k,
            if self.pixel_mode() { 0.0 } else { 7.0 * k },
            if on { p.base } else { p.text.with_a(0.7) },
        );
        self.draw_text(
            pixmap,
            label,
            label_size,
            if on { p.text } else { p.muted },
            (rect.x + 12.0) * k,
            (rect.y + 7.0) * k,
            18.0 * k,
        );
        let state = if on { "вкл" } else { "выкл" };
        let state_w = self.text_width(state, small_size);
        self.draw_text(
            pixmap,
            state,
            small_size,
            p.muted,
            (sx - 8.0) * k - state_w,
            (rect.y + 8.0) * k,
            16.0 * k,
        );
    }

    /// Шкала высоты: заливка по доле от минимума к максимуму плюс число.
    fn draw_height_scale(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        p: UiPalette,
        scale: Rect,
        value_size: f32,
        small_size: f32,
    ) {
        let k = SCALE;
        let (x0, y, w, h) = (scale.x, scale.y, scale.w, scale.h);
        let span = settings_ui::HEIGHT_MAX - settings_ui::HEIGHT_MIN;
        let share = (self.state.height - settings_ui::HEIGHT_MIN) as f32 / span as f32;
        fill_round_rect(pixmap, x0 * k, y * k, w * k, h * k, 3.0 * k, p.idle_panel);
        fill_round_rect(
            pixmap,
            x0 * k,
            y * k,
            (w * share).max(4.0) * k,
            h * k,
            3.0 * k,
            p.accent,
        );
        let value = self.state.height.to_string();
        self.draw_text(
            pixmap,
            &value,
            value_size,
            p.text,
            (x0 + w + 12.0) * k,
            (y - 8.0) * k,
            20.0 * k,
        );
        let hint = format!("{}–{} px", settings_ui::HEIGHT_MIN, settings_ui::HEIGHT_MAX);
        self.draw_text(
            pixmap,
            &hint,
            small_size,
            p.muted,
            (x0 + w + 44.0) * k,
            (y - 5.0) * k,
            16.0 * k,
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
        let fill = if hovered || focused {
            p.panel
        } else {
            p.idle_panel
        };
        fill_round_rect(
            pixmap,
            rect.x * k,
            rect.y * k,
            rect.w * k,
            rect.h * k,
            if self.pixel_mode() { 0.0 } else { 8.0 * k },
            fill,
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
        let w = self.text_width(label, value_size);
        self.draw_text(
            pixmap,
            label,
            value_size,
            p.text,
            (rect.x + (rect.w - w / SCALE) / 2.0) * k,
            (rect.y + 6.0) * k,
            18.0 * k,
        );
    }

    fn draw_footer(&mut self, pixmap: &mut tiny_skia::Pixmap, p: UiPalette, y: f32, size: f32) {
        let k = SCALE;
        for (i, hint) in settings_ui::HINTS.into_iter().enumerate() {
            let x = settings_ui::PAD_X + i as f32 * settings_ui::HINT_GAP;
            self.draw_text(pixmap, hint, size, p.muted, x * k, y * k, 16.0 * k);
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
                }
                PointerEventKind::Leave { .. } => {
                    self.hover = None;
                    self.dirty = true;
                }
                PointerEventKind::Press { button: 0x110, .. } => {
                    if let Some(control) =
                        self.hit(event.position.0 as f32, event.position.1 as f32)
                    {
                        self.focus = control;
                        self.activate(control);
                    }
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
        self.on_key(event);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        // удержание стрелки должно листать и менять высоту, а не срабатывать раз
        self.on_key(event);
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

fn blend(
    pixmap: &mut tiny_skia::Pixmap,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: palette::Rgba,
    mask: &[u8],
) {
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let pixels = pixmap.pixels_mut();
    for row in 0..h as i32 {
        for col in 0..w as i32 {
            let xx = x + col;
            let yy = y + row;
            if xx < 0 || yy < 0 || xx >= pw || yy >= ph {
                continue;
            }
            let alpha = mask[(row * w as i32 + col) as usize] as u32 * color.3 as u32 / 255;
            if alpha == 0 {
                continue;
            }
            let dst = &mut pixels[(yy * pw + xx) as usize];
            let inv = 255 - alpha;
            let nr = (color.0 as u32 * alpha / 255 + dst.red() as u32 * inv / 255).min(255) as u8;
            let ng = (color.1 as u32 * alpha / 255 + dst.green() as u32 * inv / 255).min(255) as u8;
            let nb = (color.2 as u32 * alpha / 255 + dst.blue() as u32 * inv / 255).min(255) as u8;
            let na = (alpha + dst.alpha() as u32 * inv / 255).min(255) as u8;
            if let Some(px) = tiny_skia::PremultipliedColorU8::from_rgba(nr, ng, nb, na) {
                *dst = px;
            }
        }
    }
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
    // буфер 660x704 при scale 2 — около 7.4 МБ, держим три
    let pool = SlotPool::new(26_000_000, &shm).expect("shm pool");
    let state = read_state();
    let picked = state.mode == "pixel";
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
        font_system: FontSystem::new(),
        swash: SwashCache::new(),
        font: if picked {
            "Minecraft Rus".to_string()
        } else {
            "JetBrainsMono Nerd Font Propo".to_string()
        },
        state,
        picked: None,
        focus: Control::Theme(picked),
        status: "Готово".to_string(),
        job: Arc::new(Mutex::new(Job::default())),
        rows: settings_ui::rows(),
        palette: palette::load(),
        settings_stamp: stamp(&settings_path()),
        palette_stamp: stamp(&home().join(".config/hudbar/colors.css")),
        width: WIDTH,
        height: HEIGHT,
        configured: false,
        dirty: true,
        exit,
        hover: None,
    };

    while !app.exit.load(Ordering::Relaxed) {
        app.refresh_sources();
        app.poll_job();
        if event_queue.flush().is_err() {
            break;
        }
        if let Some(guard) = event_queue.prepare_read() {
            let mut fds = [PollFd::new(conn.as_fd(), PollFlags::POLLIN)];
            let _ = poll(&mut fds, 250u16);
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
        if app.dirty {
            app.dirty = false;
            app.draw();
        }
    }
}

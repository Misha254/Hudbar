//! hud-greeter-rs — экран входа Hudbar для greetd.
//!
//! Окно занимает весь экран слоем `Overlay` с якорями на все четыре стороны и
//! забирает клавиатуру (`KeyboardInteractivity::Exclusive`): рабочих столов
//! ещё нет, и делить ввод не с кем.
//!
//! Режимы:
//!
//! * обычный — протокол greetd через `$GREETD_SOCK`;
//! * `--preview` — фейковый вход без greetd (пароль `test`), чтобы смотреть
//!   экран из живой сессии niri;
//! * `--screenshot <path> [--variant <вид>] [--lang ru|en]` — кадр в PNG без
//!   Wayland;
//! * `--sync [--dir <каталог>]` — перенос темы пользователя в каталог гритера.
//!
//! Пароль не попадает ни в лог, ни в argv, ни в файлы: он живёт только в
//! `flow::Secret` и затирается после отправки.

use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::Command;
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
#[path = "../hud/layer.rs"]
mod layer;
#[allow(dead_code)]
#[path = "../hud/log.rs"]
mod log;
/// `log` запускает внешние команды через `data::oneshot`; потоки и DBus
/// панели гритеру не нужны.
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
#[path = "../hud/power_profile.rs"]
mod power_profile;
#[allow(dead_code)]
#[path = "../hud/repeat.rs"]
mod repeat;
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

use greeter::config::{self, Config};
use greeter::flow::{LineKind, Login, Stage, Submit};
use greeter::image::Background;
use greeter::ipc::{self, GreetdClient, Reply, Socket};
use greeter::layout;
use greeter::panel;
use greeter::render::{self, Field, View};
use greeter::sessions::{self, Session};
use greeter::state;
use greeter::strings;
use greeter::sync;
use repeat::Repeat;
use settings::Language;
use text::{SCALE, TextPainter};

const NAMESPACE: &str = "hudgreeter";
/// Размер, который просим до первого configure. Полноэкранный слой получает
/// настоящий размер от композитора; это значение — запасное, если configure
/// придёт с нулями.
const FALLBACK_W: u32 = 1920;
const FALLBACK_H: u32 = 1080;
/// Предел имени пользователя: длиннее не бывает, а поле всё равно обрежет.
const USER_MAX: usize = 64;

const USAGE: &str = "\
hud-greeter-rs — экран входа Hudbar

  hud-greeter-rs
      Обычный запуск из greetd: вход через $GREETD_SOCK.

  hud-greeter-rs --preview [--data <каталог>] [--state <файл>] [--lang ru|en]
      Показать экран из живой сессии. Пароль — test, greetd не нужен.
      После успешного входа окно закрывается, но niri не завершается.

  hud-greeter-rs --screenshot <out.png> [--variant <вид>] [--lang ru|en]
                 [--data <каталог>]
      Кадр в PNG без Wayland. Виды: known, unknown, error, caps, power.

  hud-greeter-rs --sync [--dir <каталог>]
      Перенести colors.css, обои и язык в каталог гритера.
      По умолчанию /var/lib/hudbar-greeter. Запускать от своего пользователя.

  -h, --help
";

/// Коды XKB, которые нужны экрану входа. Всё остальное приходит текстом.
mod xkb {
    pub const ESCAPE: u32 = 0xff1b;
    pub const TAB: u32 = 0xff09;
    pub const RETURN: u32 = 0xff0d;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const BACKSPACE: u32 = 0xff08;
    pub const F2: u32 = 0xffbf;
    pub const F12: u32 = 0xffc9;
    /// `u` в XKB — строчная независимо от Shift.
    pub const U: u32 = 0x75;
}

/// Вид снимка. Варианты — те состояния, которые просили проверить глазами:
/// известный пользователь, пустое имя, ошибка, Caps Lock и режим питания.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Variant {
    #[default]
    Known,
    Unknown,
    Error,
    Caps,
    Power,
}

impl Variant {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "known" => Some(Self::Known),
            "unknown" => Some(Self::Unknown),
            "error" => Some(Self::Error),
            "caps" => Some(Self::Caps),
            "power" => Some(Self::Power),
            _ => None,
        }
    }
}

/// Что делать после разбора аргументов.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    Greetd,
    Preview,
    Screenshot(PathBuf),
    Sync,
    Help,
}

#[derive(Clone, Debug)]
struct Cli {
    mode: Mode,
    data: Option<PathBuf>,
    dir: Option<PathBuf>,
    state: Option<PathBuf>,
    lang: Option<Language>,
    variant: Variant,
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    let mut cli = Cli {
        mode: Mode::Greetd,
        data: None,
        dir: None,
        state: None,
        lang: None,
        variant: Variant::default(),
    };
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("{name} требует значение"))
        };
        match arg {
            "-h" | "--help" => cli.mode = Mode::Help,
            "--preview" => cli.mode = Mode::Preview,
            "--sync" => cli.mode = Mode::Sync,
            "--screenshot" => cli.mode = Mode::Screenshot(PathBuf::from(value("--screenshot")?)),
            "--data" => cli.data = Some(PathBuf::from(value("--data")?)),
            "--dir" => cli.dir = Some(PathBuf::from(value("--dir")?)),
            "--state" => cli.state = Some(PathBuf::from(value("--state")?)),
            "--lang" => {
                let text = value("--lang")?;
                cli.lang = Some(settings::Language::from_key(&text));
                if text != "ru" && text != "en" {
                    return Err(format!("неизвестный язык: {text}. Доступны: ru, en"));
                }
            }
            "--variant" => {
                let text = value("--variant")?;
                cli.variant = Variant::parse(&text).ok_or_else(|| {
                    format!("неизвестный вид: {text}. Доступны: known, unknown, error, caps, power")
                })?;
            }
            other if other.starts_with('-') => return Err(format!("неизвестный флаг: {other}")),
            other => return Err(format!("лишний аргумент: {other}")),
        }
        index += 1;
    }
    Ok(cli)
}

/// Клиент, которого нет: сокет не задан или не открылся. Все запросы
/// возвращают причину — гритер покажет её и останется на экране.
struct NoGreetd(String);

impl GreetdClient for NoGreetd {
    fn create_session(&mut self, _username: &str) -> Result<Reply, String> {
        Err(self.0.clone())
    }

    fn post_auth_message_response(&mut self, _response: Option<&str>) -> Result<Reply, String> {
        Err(self.0.clone())
    }

    fn start_session(&mut self, _cmd: &[String], _env: &[String]) -> Result<Reply, String> {
        Err(self.0.clone())
    }

    fn cancel_session(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Фейковый greetd для `--preview`: один пароль `test`, никакой PAM.
struct PreviewClient;

impl GreetdClient for PreviewClient {
    fn create_session(&mut self, _username: &str) -> Result<Reply, String> {
        Ok(Reply::AuthMessage {
            kind: ipc::PromptKind::Secret,
            message: "Password: ".into(),
        })
    }

    fn post_auth_message_response(&mut self, response: Option<&str>) -> Result<Reply, String> {
        if response == Some("test") {
            Ok(Reply::Success)
        } else {
            Ok(Reply::Error {
                error_type: "auth_error".into(),
                description: String::new(),
            })
        }
    }

    fn start_session(&mut self, _cmd: &[String], _env: &[String]) -> Result<Reply, String> {
        Ok(Reply::Success)
    }

    fn cancel_session(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// Окно входа.
struct GreeterApp {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    ctx: layer::Context,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
    repeat: Repeat,
    painter: TextPainter,
    ui: ui_tokens::UiPalette,
    config: Config,
    background: Option<Background>,
    lang: Language,
    sessions: Vec<Session>,
    session_index: usize,
    login: Login<Box<dyn GreetdClient>>,
    username: String,
    focus: Field,
    power: bool,
    battery: Option<panel::Battery>,
    /// Сообщение, которого нет у автомата: нет сеансов, сломанный systemctl.
    notice: Option<String>,
    /// Последняя показанная минута: по её смене кадр перерисовывается.
    last_clock: String,
    state_path: PathBuf,
    preview: bool,
    exit: Arc<AtomicBool>,
}

impl GreeterApp {
    fn session(&self) -> Option<&Session> {
        self.sessions.get(self.session_index)
    }

    fn session_name(&self) -> String {
        self.session()
            .map(|session| session.name.clone())
            .unwrap_or_else(|| strings::text(self.lang).session_unknown.to_string())
    }

    /// Перерисовка раз в минуту, чтобы часы не стояли. Батарея читается на
    /// каждом тике: это два `read_to_string` из sysfs, дешевле лишнего кадра.
    fn tick(&mut self) {
        let now = chrono::Local::now();
        let clock = panel::big_clock(now);
        if clock != self.last_clock {
            self.last_clock = clock;
            self.ctx.dirty = true;
        }
        let battery = panel::battery();
        if battery != self.battery {
            self.battery = battery;
            self.ctx.dirty = true;
        }
    }

    fn cycle_session(&mut self) {
        if self.sessions.is_empty() {
            self.notice = Some(strings::text(self.lang).no_sessions.to_string());
            return;
        }
        self.session_index = sessions::next_index(self.sessions.len(), self.session_index);
        self.notice = None;
        self.ctx.dirty = true;
    }

    fn backspace(&mut self) {
        if self.power {
            return;
        }
        match self.focus {
            Field::User => {
                self.username.pop();
            }
            Field::Password => self.login.backspace(),
        }
        self.notice = None;
        self.ctx.dirty = true;
    }

    /// Ввод символа в активное поле. Пароль идёт в `Secret`, имя — в строку.
    fn type_text(&mut self, text: &str) {
        if self.power {
            return;
        }
        for ch in text.chars() {
            if ch.is_control() {
                continue;
            }
            match self.focus {
                Field::User => {
                    if self.username.chars().count() < USER_MAX {
                        self.username.push(ch);
                    }
                }
                Field::Password => self.login.type_char(ch),
            }
        }
        self.notice = None;
        self.ctx.dirty = true;
    }

    /// Enter: в поле имени — переход к паролю, в пароле — вход.
    fn submit(&mut self) {
        if self.power {
            return;
        }
        if self.focus == Field::User && self.login.password_chars() == 0 {
            if !self.username.trim().is_empty() {
                self.focus = Field::Password;
            }
            self.ctx.dirty = true;
            return;
        }
        if self.login.password_chars() == 0 {
            return;
        }
        let Some(session) = self.session().cloned() else {
            self.notice = Some(strings::text(self.lang).no_sessions.to_string());
            self.ctx.dirty = true;
            return;
        };
        self.login.set_username(self.username.trim());
        let cmd = session.argv();
        if cmd.is_empty() {
            self.notice = Some(strings::text(self.lang).session_failed.to_string());
            self.ctx.dirty = true;
            return;
        }
        let env = session_env(&session.name);
        let outcome = self.login.submit(&cmd, &env);
        self.ctx.dirty = true;
        if outcome == Submit::Done {
            self.finish_login(&session);
        }
    }

    /// Сеанс запущен. В greetd-режиме гритер обязан выйти: тогда greetd
    /// стартует сессию. Композитор гритера (niri) надо погасить — иначе два
    /// niri подерутся за DRM.
    fn finish_login(&mut self, session: &Session) {
        if self.preview {
            println!(
                "hud-greeter-rs: preview, сеанс «{}» не запускается",
                session.name
            );
            self.exit.store(true, Ordering::Relaxed);
            return;
        }
        let last = state::Last::with_username(self.username.trim()).with_session(&session.name);
        if let Err(error) = state::save(&self.state_path, &last) {
            // Не фатально: вход важнее удобства следующего раза.
            log::warn(format!("состояние не сохранено: {error}"));
        }
        // argv-вызов без shell: строка не проходит через интерпретатор.
        let _ = Command::new("niri")
            .args(["msg", "action", "quit", "--skip-confirmation"])
            .status();
        self.exit.store(true, Ordering::Relaxed);
    }

    /// F12–1/2/3. `systemctl` вызывается только здесь, только по явному
    /// нажатию и только argv-списком.
    fn power_action(&mut self, action: &str) {
        let unit = match action {
            "1" => "poweroff",
            "2" => "reboot",
            "3" => "suspend",
            _ => return,
        };
        self.power = false;
        match Command::new("systemctl").arg(unit).status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                self.notice = Some(format!("systemctl {unit}: код {status}"));
            }
            Err(error) => {
                self.notice = Some(format!("systemctl {unit}: {error}"));
            }
        }
        self.ctx.dirty = true;
    }

    fn handle_key(&mut self, event: &KeyEvent) {
        let sym: u32 = event.keysym.into();
        let ctrl = self.mods.ctrl;
        let logo = self.mods.logo;

        if ctrl && !logo {
            // Ctrl+U чистит активное поле. Остальные Ctrl-сочетания не наши.
            if sym == xkb::U {
                match self.focus {
                    Field::User => self.username.clear(),
                    Field::Password => self.login.clear_password(),
                }
                self.notice = None;
                self.ctx.dirty = true;
            }
            return;
        }
        if logo {
            return;
        }

        match sym {
            xkb::ESCAPE => {
                if self.power {
                    self.power = false;
                } else {
                    self.login.cancel();
                    self.notice = None;
                }
                self.ctx.dirty = true;
            }
            xkb::TAB => {
                if !self.power {
                    self.focus = match self.focus {
                        Field::User => Field::Password,
                        Field::Password => Field::User,
                    };
                    self.ctx.dirty = true;
                }
            }
            xkb::F2 => {
                if !self.power {
                    self.cycle_session();
                }
            }
            xkb::F12 => {
                self.power = !self.power;
                self.notice = None;
                self.ctx.dirty = true;
            }
            xkb::RETURN | xkb::KP_ENTER => {
                self.submit();
            }
            xkb::BACKSPACE => self.backspace(),
            _ => {
                if self.power {
                    if let Some(text) = event.utf8.as_deref() {
                        for ch in text.chars() {
                            if ('1'..='3').contains(&ch) {
                                self.power_action(&ch.to_string());
                                break;
                            }
                        }
                    }
                    return;
                }
                if let Some(text) = event.utf8.as_deref() {
                    self.type_text(text);
                }
            }
        }
    }

    /// Повторы: только Backspace. Удерживать Enter на экране входа опасно —
    /// каждая попытка создаёт сеанс greetd.
    fn handle_repeat(&mut self, event: &KeyEvent) {
        let sym: u32 = event.keysym.into();
        if sym == xkb::BACKSPACE {
            self.backspace();
        }
    }

    fn click(&mut self, x: f32, y: f32) {
        if self.power {
            return;
        }
        let card = layout::card((self.ctx.width as f32, self.ctx.height as f32));
        let rows = layout::of(card);
        if rows.user_box.contains(x, y) {
            self.focus = Field::User;
        } else if rows.password_box.contains(x, y) {
            self.focus = Field::Password;
        } else if rows.button.contains(x, y) {
            self.submit();
            return;
        } else {
            return;
        }
        self.ctx.dirty = true;
    }

    /// Кадр. Тот же `render::draw`, что у `--screenshot`.
    fn draw(&mut self) {
        let _frame = log::Frame::start("вход");
        if !self.ctx.configured || !self.ctx.dirty {
            return;
        }
        let now = std::time::Instant::now();
        let openness = self.ctx.openness_now(now);
        self.ctx.dirty = false;
        let pw = (self.ctx.width as f32 * SCALE) as u32;
        let ph = (self.ctx.height as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        pixmap.fill(tiny_skia::Color::TRANSPARENT);

        let message = self
            .notice
            .clone()
            .map(|text| (text, LineKind::Error))
            .or_else(|| {
                self.login
                    .message()
                    .map(|line| (line.text.clone(), line.kind))
            });
        let session = self.session_name();
        let view = View {
            lang: self.lang,
            now: chrono::Local::now(),
            username: &self.username,
            password_chars: self.login.password_chars(),
            focus: self.focus,
            caps: self.mods.caps_lock,
            session: &session,
            message: message.as_ref().map(|(text, kind)| (text.as_str(), *kind)),
            stage: self.login.stage(),
            power: self.power,
            battery: self.battery,
        };
        render::draw(
            &mut pixmap,
            &mut self.painter,
            &self.ui,
            &self.config,
            self.background.as_ref(),
            &view,
        );
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
    }
}

/// Окружение сеанса. Только минимум: HOME и USER выставит PAM после входа, и
/// перекрывать их окружением гритера нельзя.
fn session_env(desktop: &str) -> Vec<String> {
    let mut env = vec![
        "XDG_SESSION_TYPE=wayland".to_string(),
        format!("XDG_CURRENT_DESKTOP={desktop}"),
        format!("XDG_SESSION_DESKTOP={desktop}"),
    ];
    if let Ok(lang) = std::env::var("LANG")
        && !lang.is_empty()
    {
        env.push(format!("LANG={lang}"));
    }
    env
}

/// Снимок без Wayland: тот же кадр, что в окне.
fn screenshot(cli: &Cli, out: &Path) -> Result<(), String> {
    let dir = cli.data.clone().unwrap_or_else(config::data_dir);
    let mut config = config::load(&dir);
    if let Some(lang) = cli.lang {
        config.language = lang;
    }
    let lang = config.language;
    let ui = ui_tokens::ui_palette(config::palette_of(&dir));
    let background = Background::load(&config::background_path(&dir));
    let t = strings::text(lang);

    let (username, password_chars, focus, caps, power, message): (
        &str,
        usize,
        Field,
        bool,
        bool,
        Option<(&str, LineKind)>,
    ) = match cli.variant {
        Variant::Known => ("mihail", 4, Field::Password, false, false, None),
        Variant::Unknown => ("", 0, Field::User, false, false, None),
        Variant::Error => (
            "mihail",
            0,
            Field::Password,
            false,
            false,
            Some((t.auth_failed, LineKind::Error)),
        ),
        Variant::Caps => ("mihail", 4, Field::Password, true, false, None),
        Variant::Power => ("mihail", 4, Field::Password, false, true, None),
    };
    let sessions = sessions::list();
    let session_name = sessions
        .iter()
        .find(|session| session.name.to_lowercase().contains("niri"))
        .map(|session| session.name.clone())
        .unwrap_or_else(|| t.session_unknown.to_string());

    let view = View {
        lang,
        now: chrono::Local::now(),
        username,
        password_chars,
        focus,
        caps,
        session: &session_name,
        message,
        stage: Stage::Idle,
        power,
        battery: panel::battery(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(FALLBACK_W * SCALE as u32, FALLBACK_H * SCALE as u32)
        .ok_or_else(|| "не выделить буфер снимка".to_string())?;
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    let mut painter = TextPainter::new(&config.font);
    render::draw(
        &mut pixmap,
        &mut painter,
        &ui,
        &config,
        background.as_ref(),
        &view,
    );
    canvas::write_png(&pixmap, out)?;
    println!(
        "{} {}x{} → {}",
        out.display(),
        pixmap.width(),
        pixmap.height(),
        out.display()
    );
    Ok(())
}

fn run_sync(cli: &Cli) -> Result<(), String> {
    let home = sync::home_from_env()?;
    let dir = cli.dir.clone().unwrap_or_else(config::data_dir);
    let report = sync::sync(&dir, &home)?;
    println!("каталог: {}", report.dir.display());
    if report.colors {
        println!("colors.css: скопирован");
    } else {
        println!("colors.css: не найден в ~/.config/hudbar, оставлен прежний");
    }
    match report.background {
        Some(tool) => println!("background.png: обновлён ({tool})"),
        None => println!("background.png: обоев нет, оставлен прежний"),
    }
    println!("язык: {:?}", report.language);
    Ok(())
}

/// Выбор клиента: preview, живой сокет или заглушка с причиной.
fn backend(preview: bool) -> Result<Box<dyn GreetdClient>, String> {
    if preview {
        return Ok(Box::new(PreviewClient));
    }
    let Some(path) = ipc::socket_path() else {
        return Ok(Box::new(NoGreetd(
            "переменная GREETD_SOCK не задана: гритер запущен не из greetd".to_string(),
        )));
    };
    match Socket::connect(&path) {
        Ok(socket) => Ok(Box::new(socket)),
        Err(error) => Ok(Box::new(NoGreetd(error))),
    }
}

fn run(cli: &Cli, preview: bool) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|error| format!("нет Wayland: {error}"))?;
    let (globals, mut event_queue) =
        registry_queue_init(&conn).map_err(|error| format!("нет реестра: {error}"))?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|error| format!("нет wl_compositor: {error}"))?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).map_err(|_| "нет zwlr_layer_shell".to_string())?;
    let shm = Shm::bind(&globals, &qh).map_err(|error| format!("нет wl_shm: {error}"))?;

    // Гритер не видит settings.json пользователя, поэтому анимации берутся
    // из его собственного окружения: там обычно нет файла, и это нормальные
    // значения по умолчанию.
    let loaded = settings::load();
    let anim = anim::AnimSettings::new(
        loaded.animations && power_profile::anim_allowed(),
        loaded.animation_speed,
    );
    let dir = cli.data.clone().unwrap_or_else(config::data_dir);
    let mut config = config::load(&dir);
    if let Some(lang) = cli.lang {
        config.language = lang;
    }
    let lang = config.language;
    let ui = ui_tokens::ui_palette(config::palette_of(&dir));
    let background = Background::load(&config::background_path(&dir));
    let sessions = sessions::list();
    let state_path = cli.state.clone().unwrap_or_else(state::path);
    let last = state::load(&state_path);

    let mut session_index = sessions::default_index(&sessions).unwrap_or(0);
    if let Some(saved) = last.session.as_deref()
        && let Some(index) = sessions.iter().position(|session| session.name == saved)
    {
        session_index = index;
    }
    let username = last.username.clone().unwrap_or_default();
    let focus = if username.is_empty() {
        Field::User
    } else {
        Field::Password
    };

    // Причина «нет сокета» видна сразу, а не после ввода пароля: на экране
    // входа пользователь не должен гадать, почему Enter ничего не делает.
    let notice = if !preview && ipc::socket_path().is_none() {
        Some(strings::text(lang).no_greetd.to_string())
    } else {
        None
    };

    let client = backend(preview)?;
    let login = Login::new(client, lang, username.clone());

    let started = std::time::Instant::now();
    let ctx = layer::open_fullscreen(
        &compositor,
        &layer_shell,
        &shm,
        &qh,
        layer::Shape {
            namespace: Some(NAMESPACE),
            width: FALLBACK_W,
            height: FALLBACK_H,
            max_height: FALLBACK_H,
            scale: SCALE as u32,
        },
        started,
        anim,
    )?;

    let mut app = GreeterApp {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        ctx,
        pointer: None,
        keyboard: None,
        mods: Modifiers::default(),
        repeat: Repeat::default(),
        painter: TextPainter::new(&config.font),
        ui,
        config,
        background,
        lang,
        sessions,
        session_index,
        login,
        username,
        focus,
        power: false,
        battery: panel::battery(),
        notice,
        last_clock: panel::big_clock(chrono::Local::now()),
        state_path,
        preview,
        exit: Arc::new(AtomicBool::new(false)),
    };
    app.draw();

    // Порядок как у остальных окон: досылаем накопленное, обрабатываем
    // очередь, рисуем и только потом ждём сокет. Configure внутри
    // poll-ожидания не срабатывает — окно осталось бы без размера.
    while !app.exit.load(Ordering::Relaxed) {
        if event_queue.flush().is_err() {
            break;
        }
        let _ = event_queue.dispatch_pending(&mut app);
        let now = std::time::Instant::now();
        if app.ctx.animating(now) {
            app.ctx.dirty = true;
        }
        app.tick();
        app.draw();
        while let Some(event) = app.repeat.poll() {
            app.handle_repeat(&event);
            app.draw();
        }
        if app.exit.load(Ordering::Relaxed) {
            break;
        }
        let idle = if app.ctx.animating(std::time::Instant::now()) {
            16u16
        } else {
            200u16
        };
        let wait = shell::wait_ms(app.repeat.wait_hint(), idle);
        shell::poll_read(&mut event_queue, conn.as_fd(), wait);
    }
    Ok(())
}

impl CompositorHandler for GreeterApp {
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

impl OutputHandler for GreeterApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for GreeterApp {
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
        _seat: wl_seat::WlSeat,
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

impl ShmHandler for GreeterApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl PointerHandler for GreeterApp {
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
            if let PointerEventKind::Press { button: 0x110, .. } = event.kind {
                let (x, y) = (event.position.0 as f32, event.position.1 as f32);
                self.click(x, y);
            }
        }
    }
}

impl KeyboardHandler for GreeterApp {
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

    /// Уход клавиатуры не закрывает гритер: слой уже забрал ввод
    /// (`KeyboardInteractivity::Exclusive`), а выход по leave оставил бы
    /// экран входа без единой клавиши.
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
        self.repeat.press(&event);
        self.handle_key(&event);
        self.ctx.dirty = true;
        self.draw();
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
        self.handle_repeat(&event);
        self.draw();
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
        self.ctx.dirty = true;
    }
}

impl LayerShellHandler for GreeterApp {
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
            (FALLBACK_W, FALLBACK_H),
            std::time::Instant::now(),
        );
    }
}

impl ProvidesRegistryState for GreeterApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(GreeterApp);
smithay_client_toolkit::delegate_dispatch2!(GreeterApp);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_args(&args) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("{error}\nhud-greeter-rs --help");
            std::process::exit(1);
        }
    };
    let outcome = match cli.mode {
        Mode::Help => {
            println!("{USAGE}");
            Ok(())
        }
        Mode::Sync => run_sync(&cli),
        Mode::Screenshot(ref out) => screenshot(&cli, out),
        Mode::Preview => run(&cli, true),
        Mode::Greetd => run(&cli, false),
    };
    if let Err(error) = outcome {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

//! `hud-telegram-rs` — мост Telegram ↔ opencode, только с телефона.
//!
//!   hud-telegram-rs                 — запустить мост
//!   hud-telegram-rs --check         — проверить токен, чат и сервер, не запуская
//!   hud-telegram-rs --notify <текст> — отправить тестовое сообщение в чат
//!
//! Конфиг — `~/.config/hudbar/telegram.json` (токен бота и чат), сервер
//! opencode — `opencode serve --port 4096` отдельным процессом. Мост пишет
//! только в журнал (`log::warn`) и в systemd-журнал через stderr при `--check`.

#[allow(dead_code)]
#[path = "../hud/log.rs"]
mod log;
#[allow(dead_code)]
#[path = "../hud/telegram/mod.rs"]
mod telegram;

use std::process::ExitCode;

use telegram::bridge::Bridge;
use telegram::config;

const USAGE: &str = "\
hud-telegram-rs — мост Telegram ↔ opencode

  hud-telegram-rs                 запустить мост
  hud-telegram-rs --check         проверить токен, чат и сервер
  hud-telegram-rs --notify TEXT   отправить тестовое сообщение в чат
  hud-telegram-rs --panel-token   создать секрет панели Mini App и напечатать ссылку
";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => run(),
        Some("--check") => check(),
        Some("--panel-token") => panel_token(),
        Some("--notify") => {
            let text: Vec<String> = args.collect();
            notify(&text.join(" "))
        }
        Some("--help" | "-h") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("неизвестный аргумент: {other}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Создаёт секрет панели Mini App и печатает ссылку для Telegram.
///
/// Токен берётся из `/dev/urandom`, а не из времени или pid: иначе его можно
/// было бы угадать по времени запуска. Файл создаётся с правами `600` и
/// перезаписывается только если его уже нет — случайный повторный вызов не
/// оставил бы человека со ссылкой, которая не работает.
fn panel_token() -> ExitCode {
    use std::io::Read;
    let path = telegram::panel::path();
    if let Some(parent) = path.parent()
        && let Err(why) = std::fs::create_dir_all(parent)
    {
        eprintln!("hud-telegram: каталог не создан ({why})");
        return ExitCode::FAILURE;
    }
    // Токен берётся из готового секрета, если он уже есть: адрес дописывается
    // в тот же файл, иначе пришлось бы генерировать новый токен и рвать
    // ссылку, которая уже работает.
    let existing = telegram::panel::load_from(&path);
    let token = match existing.token.clone() {
        Some(token) => token,
        None => {
            let mut bytes = [0u8; 24];
            let random = std::fs::File::open("/dev/urandom")
                .and_then(|mut source| source.read_exact(&mut bytes));
            if let Err(why) = random {
                eprintln!("hud-telegram: случайность не получена ({why})");
                return ExitCode::FAILURE;
            }
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        }
    };
    // Адрес панели: из окружения, из аргумента или из уже сохранённого
    // секрета. Пустой адрес оставляем в файле, чтобы `tailscale serve` можно
    // было поднять позже и не пересоздавать токен.
    let base = std::env::args()
        .skip(1)
        .find(|argument| argument.starts_with("https://"))
        .or_else(|| std::env::var("HUDBAR_PANEL_URL").ok())
        .map(|url| url.trim().to_string());
    match &base {
        Some(url) if !url.trim().is_empty() => {
            let text = format!("{{\n  \"token\": \"{token}\",\n  \"url\": \"{url}\"\n}}\n");
            let _ = std::fs::write(&path, text);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            println!("секрет: {}", path.display());
            println!("ссылка: {}", telegram::panel::link(url, &token));
        }
        _ => {
            println!("секрет: {}", path.display());
            println!("токен создан, адрес не задан.");
            println!("Потом: hud-telegram-rs --panel-token https://<узел>.<tailnet>.ts.net");
        }
    }
    ExitCode::SUCCESS
}

/// Запуск моста: конфиг, готовность сервера, два потока.
fn run() -> ExitCode {
    let config = match config::load_ready() {
        Ok(config) => config,
        Err(why) => {
            eprintln!("hud-telegram: {why}");
            return ExitCode::FAILURE;
        }
    };
    let client = match telegram::opencode::Client::new(&config.opencode) {
        Ok(mut client) => {
            client.set_directory(&config.directory);
            client
        }
        Err(why) => {
            eprintln!("hud-telegram: {why}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(why) = client.wait_ready(12, std::time::Duration::from_secs(5)) {
        eprintln!("hud-telegram: сервер не поднялся: {why}");
        return ExitCode::FAILURE;
    }
    match Bridge::new(config) {
        Ok(bridge) => {
            bridge.run();
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("hud-telegram: {why}");
            ExitCode::FAILURE
        }
    }
}

/// Проверка без запуска: токен, чат, сервер и папка. Каждый пункт печатает
/// своё «ок» или причину, в конце — общий итог.
fn check() -> ExitCode {
    let mut ok = true;
    let config = config::load();
    let api = telegram::api::Api::new(&config.bot_token);
    match api.whoami() {
        Ok(username) => println!("токен: ок (@{username})"),
        Err(why) => {
            println!("токен: {why}");
            ok = false;
        }
    }
    match config.chat_id {
        Some(chat) => println!("чат: ок ({chat})"),
        None => {
            println!("чат: не задан — напиши боту /start");
            ok = false;
        }
    }
    let client = match telegram::opencode::Client::new(&config.opencode) {
        Ok(mut client) => {
            client.set_directory(&config.directory);
            Some(client)
        }
        Err(why) => {
            println!("сервер: {why}");
            ok = false;
            None
        }
    };
    if let Some(client) = client {
        match client.list_sessions(1) {
            Ok(_) => println!("сервер: ок ({})", config.opencode),
            Err(why) => {
                println!("сервер: {why}");
                ok = false;
            }
        }
    }
    if std::path::Path::new(&config.directory).is_dir() {
        println!("папка: ок ({})", config.directory);
    } else {
        println!("папка: нет такой ({})", config.directory);
        ok = false;
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Тестовое сообщение в чат: проверяет весь путь «мост → Telegram».
fn notify(text: &str) -> ExitCode {
    if text.trim().is_empty() {
        eprintln!("hud-telegram: пустое сообщение");
        return ExitCode::FAILURE;
    }
    let config = config::load();
    let Some(chat) = config.chat_id else {
        eprintln!("hud-telegram: не задан chat_id");
        return ExitCode::FAILURE;
    };
    let api = telegram::api::Api::new(&config.bot_token);
    match api.send(chat, &telegram::api::escape_html(text)) {
        Ok(_) => {
            println!("отправлено");
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("hud-telegram: {why}");
            ExitCode::FAILURE
        }
    }
}

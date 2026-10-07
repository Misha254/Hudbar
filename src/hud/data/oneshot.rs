//! Одноразовые читатели состояния: без фоновых потоков и без общего кэша.
//!
//! Панель держит состояние в `Shared.sys` и обновляет его потоками. Меню —
//! отдельный процесс с общим кэшем панели, поэтому читать ему приходится
//! разово: спросили у dunst, wpctl, nmcli или sysfs и поставили значение в
//! строку. Каждый запрос с таймаутом — иначе зависший демон повесит меню.
//!
//! Функции `dnd_paused`, `wpctl_volume` и `power_state` живут здесь, а панель
//! берёт их отсюда же: одна реализация вместо двух. Иначе чтение DND в меню и
//! в панели разошлось бы при первом же изменении команды.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Сколько ждём разовую команду. `dunstctl` и `wpctl` отвечают за миллисекунды;
/// секунда — это уже зависший демон.
pub const TIMEOUT: Duration = Duration::from_secs(1);

/// Запуск команды с таймаутом. При превышении времени завершает процесс и
/// его дочерние процессы, иначе они могли бы удерживать stdout/stderr открытыми.
/// Возвращает `(код, stdout, stderr)`.
pub fn run_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<(Option<i32>, String, String), String> {
    run_with_timeout_stdin(program, args, None, timeout)
}

/// Как [`run_with_timeout`], но с байтами на stdin команды. Механизм для
/// случая, когда секрет не должен попадать в `argv` вовсе: pipe не виден в
/// `ps` и `/proc/<pid>/cmdline`. Готов и покрыт тестами. В W5.2b пароль Wi-Fi
/// уходит через `CmdArg::Secret`, то есть всё-таки в `argv` — перевести его
/// сюда мешает `nmcli`, который пароль из stdin не читает. Аргументы здесь
/// по-прежнему передаются без shell, напрямую в `Command`.
pub fn run_with_timeout_stdin(
    program: &str,
    args: &[&str],
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<(Option<i32>, String, String), String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("{program} не запустился: {error}"))?;
    // Секрет уходит в отдельном потоке: если команда не читает stdin, запись
    // не должна вешать ожидание; pipe закроется вместе с потоком.
    if let Some(input) = stdin.map(|input| input.to_vec())
        && let Some(mut stdin) = child.stdin.take()
    {
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(&input);
        });
    }
    // Вывод читаем в потоках: если не читать, а процесс много пишет, он
    // заблокируется на записи и не завершится до таймаута.
    let mut out = child.stdout.take().expect("stdout");
    let mut err = child.stderr.take().expect("stderr");
    let out_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = out.read_to_string(&mut buf);
        buf
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = err.read_to_string(&mut buf);
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(5).min(timeout - started.elapsed()));
            }
            Ok(None) => {
                stop_child(&mut child);
                let _ = child.wait();
                let _ = out_handle.join();
                let _ = err_handle.join();
                return Err(format!(
                    "{program} не ответил за {} мс",
                    timeout.as_millis()
                ));
            }
            Err(error) => {
                stop_child(&mut child);
                let _ = child.wait();
                let _ = out_handle.join();
                let _ = err_handle.join();
                return Err(format!("ошибка ожидания {program}: {error}"));
            }
        }
    };
    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    Ok((status.code(), stdout, stderr))
}

fn stop_child(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = -(child.id() as i32);
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(process_group),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    let _ = child.kill();
}

/// Есть ли программа в `PATH` или в `~/.local/bin`. По этому признаку меню
/// прячет пункты, чей скрипт не установлен.
pub fn program_exists(program: &str) -> bool {
    if program.contains('/') {
        let direct = home().join(program);
        return direct.exists();
    }
    if let Ok(paths) = std::env::var("PATH") {
        for dir in paths.split(':').filter(|dir| !dir.is_empty()) {
            if std::path::Path::new(dir).join(program).exists() {
                return true;
            }
        }
    }
    home().join(".local/bin").join(program).exists()
}

fn home() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// Есть ли программа в том `PATH`, каким niri запускает бинды через
/// `spawn-sh`. Меню скрывает пункт, если его программы нет в окружении
/// niri — потому что сам бинд `kitty --class calcurse zsh -c 'calcurse'`
/// видит именно это окружение, а не `PATH` из интерактивного zsh.
pub fn program_for_spawn_sh(program: &str) -> bool {
    if program.contains('/') {
        return home().join(program).exists();
    }
    for dir in spawn_sh_dirs().unwrap_or_else(|| {
        std::env::var("PATH")
            .unwrap_or_default()
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(std::path::PathBuf::from)
            .collect()
    }) {
        if dir.join(program).exists() {
            return true;
        }
    }
    home().join(".local/bin").join(program).exists()
}

fn spawn_sh_dirs() -> Option<Vec<std::path::PathBuf>> {
    for entry in std::fs::read_dir("/proc").ok()?.filter_map(Result::ok) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        if comm.trim_end() != "niri" {
            continue;
        }
        let Ok(environ) = std::fs::read(entry.path().join("environ")) else {
            return None;
        };
        for pair in environ.split(|byte| *byte == 0) {
            if let Some(path) = pair.strip_prefix(b"PATH=") {
                let text = String::from_utf8_lossy(path);
                return Some(
                    text.split(':')
                        .filter(|dir| !dir.is_empty())
                        .map(std::path::PathBuf::from)
                        .collect(),
                );
            }
        }
        return None;
    }
    None
}

/// Запущен ли dunst в режиме «не беспокоить».
pub fn dnd_paused() -> bool {
    run_with_timeout("dunstctl", &["is-paused"], TIMEOUT)
        .map(|(code, stdout, _)| code == Some(0) && stdout.trim() == "true")
        .unwrap_or(false)
}

/// Разбор строки terse-вывода nmcli (`-t`): поля разделены `\:`, поэтому
/// двоеточие внутри SSID или имени профиля приходит экранированным. Правило
/// одно и то же для панели и для меню, версия панели — в `data::network`.
pub fn split_nmcli_terse(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            field.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == ':' {
            fields.push(std::mem::take(&mut field));
        } else {
            field.push(ch);
        }
    }
    if escaped {
        field.push('\\');
    }
    fields.push(field);
    fields
}

/// Громкость и mute для указанного узла wpctl.
pub fn wpctl_volume(node: &str) -> Option<(u8, bool)> {
    let (_, stdout, _) = run_with_timeout("wpctl", &["get-volume", node], TIMEOUT).ok()?;
    parse_wpctl_volume(&stdout)
}

/// Разбор строки `wpctl get-volume`: `Volume: 0.42 [MUTED]`.
pub fn parse_wpctl_volume(text: &str) -> Option<(u8, bool)> {
    let value = text
        .split_whitespace()
        .find_map(|part| part.parse::<f32>().ok())?;
    Some((
        ((value.clamp(0.0, 1.0) * 100.0).round() as u8).min(100),
        text.contains("[MUTED]"),
    ))
}

/// Питание: `(зарядка, сеть)`. Порядок и условия ровно как у панели, иначе
/// меню показало бы «заряжается» там, где панель показывает «от сети».
pub fn power_state() -> Option<(bool, bool)> {
    let mut found = false;
    let mut charging = false;
    let mut ac = false;
    let Ok(read_dir) = std::fs::read_dir("/sys/class/power_supply") else {
        return None;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        match read_trim(&path.join("type")).as_str() {
            "Battery" => {
                found = true;
                let status = read_trim(&path.join("status"));
                charging |= status == "Charging";
            }
            "Mains" => {
                found = true;
                ac |= read_trim(&path.join("online")) == "1";
            }
            _ => {}
        }
    }
    found.then_some((charging, ac))
}

fn read_trim(path: &std::path::Path) -> String {
    std::fs::read_to_string(path)
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

/// Включён ли кофе-мод: `coffee-toggle.sh` хранит состояние в `hud-coffee`
/// в runtime-каталоге пользователя. Меню читает тот же файл, чтобы значение не
/// зависело от живого списка процессов.
pub fn coffee_on() -> bool {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| {
            std::path::PathBuf::from(format!("/run/user/{}", nix::unistd::getuid().as_raw()))
        });
    coffee_state_present(&dir.join("hud-coffee"))
}

fn coffee_state_present(path: &std::path::Path) -> bool {
    path.is_file()
}

/// Идёт ли запись экрана: `record.sh` работает, пока жив `wf-recorder`.
/// Сам скрипт тоже попадает под шаблон, поэтому запись видна уже с момента
/// открытого выбора режима, а не только старта кодирования.
pub fn record_on() -> bool {
    pgrep_found("[w]f-recorder")
}

/// Есть ли процесс по шаблону `pgrep -f`. Квадратные скобки в шаблоне —
/// чтобы сам `pgrep` не находил себя в списке процессов.
fn pgrep_found(pattern: &str) -> bool {
    run_with_timeout("pgrep", &["-f", pattern], TIMEOUT)
        .map(|(code, _, _)| code == Some(0))
        .unwrap_or(false)
}

/// Индекс схемы matugen по файлу `~/.config/hudbar/scheme`. Тот же файл
/// читает панель, поэтому источник один.
pub fn scheme() -> Option<String> {
    let text = std::fs::read_to_string(home().join(".config/hudbar/scheme")).ok()?;
    let name = text.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Активная сеть: имя или `Нет`.
pub fn network_name() -> String {
    match run_with_timeout(
        "nmcli",
        &["-t", "-f", "NAME,TYPE", "device", "status"],
        TIMEOUT,
    ) {
        Ok((_, stdout, _)) => stdout
            .lines()
            .filter_map(|line| line.split(':').next())
            .find(|kind| *kind == "wifi" || *kind == "ethernet")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "Нет сети".to_string()),
        Err(_) => "Нет сети".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nmcli_terse_keeps_escaped_colons_inside_a_field() {
        assert_eq!(
            split_nmcli_terse(r"*:My\:Net:78:WPA2"),
            ["*", "My:Net", "78", "WPA2"]
        );
        assert_eq!(
            split_nmcli_terse("*:MyNet:78:WPA2"),
            ["*", "MyNet", "78", "WPA2"]
        );
        assert_eq!(
            split_nmcli_terse(r" :Дом \: принтер:54:WPA2"),
            [" ", "Дом : принтер", "54", "WPA2"]
        );
        assert_eq!(split_nmcli_terse(r"a\\b:c"), ["a\\b", "c"]);
        assert_eq!(split_nmcli_terse(""), [""]);
    }

    #[test]
    fn volume_parsing_covers_plain_and_muted() {
        assert_eq!(parse_wpctl_volume("Volume: 0.42"), Some((42, false)));
        assert_eq!(parse_wpctl_volume("Volume: 1.00"), Some((100, false)));
        assert_eq!(parse_wpctl_volume("Volume: 0.00"), Some((0, false)));
        assert_eq!(parse_wpctl_volume("Volume: 0.00 [MUTED]"), Some((0, true)));
        assert_eq!(parse_wpctl_volume("Volume: 0.40 [MUTED]"), Some((40, true)));
        assert_eq!(parse_wpctl_volume("mutado"), None);
    }

    #[test]
    fn volume_parsing_survives_odd_spacing_and_junk() {
        assert_eq!(parse_wpctl_volume("Volume:   0.55"), Some((55, false)));
        assert_eq!(parse_wpctl_volume("  Volume: 0.05\n"), Some((5, false)));
        assert_eq!(parse_wpctl_volume(""), None);
        assert_eq!(parse_wpctl_volume("Volume:"), None);
        assert_eq!(parse_wpctl_volume("Volume: громко"), None);
    }

    #[test]
    fn volume_is_clamped_to_a_hundred() {
        assert_eq!(parse_wpctl_volume("Volume: 1.40"), Some((100, false)));
    }

    #[test]
    fn timeouts_are_short_enough_to_not_freeze_the_menu() {
        assert!(TIMEOUT <= Duration::from_secs(2), "меню не должно висеть");
    }

    #[test]
    fn missing_program_is_an_error_not_a_panic() {
        let result = run_with_timeout("hud-bar-nonexistent", &[], TIMEOUT);
        assert!(result.is_err(), "нет программы — это ошибка, а не паника");
    }

    #[test]
    fn shell_command_runs_and_returns_output() {
        let (code, stdout, _) = run_with_timeout("printf", &["ok"], TIMEOUT).expect("printf есть");
        assert_eq!(code, Some(0));
        assert_eq!(stdout.trim(), "ok");
    }

    #[test]
    fn failing_command_returns_its_code() {
        let (code, _, _) = run_with_timeout("false", &[], TIMEOUT).expect("false есть");
        assert_ne!(code, Some(0));
    }

    #[test]
    fn timeout_kills_the_command_and_reports_the_requested_limit() {
        let started = Instant::now();
        let error = run_with_timeout("sh", &["-c", "sleep 5"], Duration::from_millis(50))
            .expect_err("sleep должен быть прерван");

        assert!(error.contains("50 мс"), "не указан таймаут: {error}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "процесс не завершён вовремя"
        );
    }

    #[test]
    fn stdin_reaches_the_command_without_shell() {
        let (code, stdout, _) =
            run_with_timeout_stdin("cat", &[], Some(b"quiet secret"), Duration::from_secs(2))
                .expect("cat есть");
        assert_eq!(code, Some(0));
        assert_eq!(stdout, "quiet secret");
    }

    #[test]
    fn coffee_state_uses_runtime_file() {
        let root = std::env::temp_dir().join(format!(
            "hud-coffee-test-{}-{}",
            std::process::id(),
            "present"
        ));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("hud-coffee");

        assert!(!coffee_state_present(&file));
        std::fs::write(&file, []).unwrap();
        assert!(coffee_state_present(&file));
        let missing_parent = root.join("absent").join("hud-coffee");
        assert!(!coffee_state_present(&missing_parent));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn program_detection_finds_shells() {
        assert!(program_exists("sh"), "оболочка точно есть");
        assert!(!program_exists("hud-bar-nonexistent"));
        assert!(!program_exists("niri/definitely/not/here"));
    }

    #[test]
    fn program_detection_finds_absolute_paths() {
        assert!(program_exists("/bin/sh"), "абсолютный путь тоже ищется");
    }
}

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Output;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use chrono::Local;

use super::data::oneshot;

/// Сколько ждём внешнюю команду, прочитанную через `output`/`status`.
///
/// Предел обязателен: `log::output` зовётся с главного потока панели, и
/// зависший демон (`niri`, `nmcli`, `bluetoothctl`) иначе встал бы колом —
/// ни перерисовки, ни ввода, до самого SIGTERM.
pub const TIMEOUT: Duration = Duration::from_secs(2);

fn path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        });
    base.join("hudbar.log")
}

pub fn warn(message: impl AsRef<str>) {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock().ok();
    let path = path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            file,
            "{} WARN {}",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            message.as_ref()
        );
    }
}

fn command_line(program: &str, args: &[&str]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(program.to_string());
    for (index, part) in args.iter().enumerate() {
        let redacted = index > 0 && args[index - 1] == "password";
        let part = if redacted { "<redacted>" } else { part };
        parts.push(if part.contains(char::is_whitespace) {
            format!("{part:?}")
        } else {
            part.to_string()
        });
    }
    parts.join(" ")
}

pub fn output(program: &str, args: &[&str]) -> Option<Output> {
    let command = command_line(program, args);
    match oneshot::output_with_timeout(program, args, TIMEOUT) {
        Ok(output) if output.status.success() => Some(output),
        Ok(output) => {
            warn(format!(
                "command failed ({}) exit={:?} stderr={}",
                command,
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            None
        }
        // Сюда попадают и «не запустился», и «не ответил за 2000 мс»: оба
        // случая для панели выглядят одинаково — значения просто нет.
        Err(error) => {
            warn(format!("command failed ({command}): {error}"));
            None
        }
    }
}

pub fn status(program: &str, args: &[&str]) -> bool {
    output(program, args).is_some()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::{TIMEOUT, command_line, output, status};

    #[test]
    fn redacts_password_arguments() {
        let line = command_line(
            "nmcli",
            &["dev", "wifi", "connect", "Cafe", "password", "secret"],
        );

        assert!(line.contains("<redacted>"));
        assert!(!line.contains("secret"));
    }

    /// Главный поток панели ждёт внешнюю команду синхронно: без предела
    /// зависший `niri` или `nmcli` встал бы колом до самого SIGTERM.
    #[test]
    fn a_sleeping_command_is_cut_off_instead_of_being_waited_out() {
        let started = Instant::now();
        let result = output("sh", &["-c", "sleep 30"]);
        let elapsed = started.elapsed();

        assert!(result.is_none(), "зависшая команда — это None, а не вывод");
        assert!(
            elapsed < TIMEOUT + std::time::Duration::from_secs(2),
            "ждали {elapsed:?} при пределе {TIMEOUT:?}: команда отпущена не была"
        );
    }

    #[test]
    fn status_is_false_when_the_command_outlives_the_timeout() {
        assert!(!status("sh", &["-c", "sleep 30"]));
    }

    #[test]
    fn a_normal_command_still_returns_its_output() {
        let result = output("printf", &["ok"]).expect("printf есть");

        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "ok");
        assert!(result.status.success());
    }

    #[test]
    fn a_missing_program_is_none_not_a_panic() {
        assert!(output("hud-bar-nonexistent", &[]).is_none());
        assert!(!status("hud-bar-nonexistent", &[]));
    }
}

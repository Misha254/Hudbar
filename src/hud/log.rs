use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

use chrono::Local;

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
    match Command::new(program).args(args).output() {
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
        Err(error) => {
            warn(format!("command unavailable ({command}): {error}"));
            None
        }
    }
}

pub fn status(program: &str, args: &[&str]) -> bool {
    output(program, args).is_some()
}

#[cfg(test)]
mod tests {
    use super::command_line;

    #[test]
    fn redacts_password_arguments() {
        let line = command_line(
            "nmcli",
            &["dev", "wifi", "connect", "Cafe", "password", "secret"],
        );

        assert!(line.contains("<redacted>"));
        assert!(!line.contains("secret"));
    }
}

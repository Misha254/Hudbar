//! Запуск внешних программ из меню.
//!
//! Меню — отдельный процесс, у него нет `Shared` панели, поэтому функции из
//! `hud::actions` ему недоступны: они требуют общего состояния. Здесь то же
//! самое, но без него — запуск приложения и перезапуск панели. Обе операции
//! немедленно возвращают управление: меню закрывается, а человек переключается
//! на то, что запустилось.
//!
//! Ошибки не молчат: команда пишется в журнал `hudbar.log`, и если запуск не
//! удался, меню показывает это в подвале.

use std::process::{Command, Stdio};

/// Запуск приложения без ожидания: kitty, Steam, flatpak. Отдельная функция
/// вместо `actions::spawn`, потому что здесь нужен запуск с аргументами.
pub fn spawn_detached(program: &str, args: &[&str]) -> Result<(), String> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("{program} не запустился: {error}"))
}

/// Перезапуск панели: ровно как это делает окно настроек — сначала TERM
/// старому процессу, потом запуск нового.
pub fn restart_hudbar() -> Result<(), String> {
    let _ = Command::new("pkill")
        .args(["-TERM", "-x", "hudbar"])
        .status();
    for _ in 0..30 {
        let alive = Command::new("pgrep")
            .args(["-x", "hudbar"])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !alive {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    home()
        .join(".local/bin/hudbar")
        .to_str()
        .ok_or_else(|| "в HOME нет пути".to_string())
        .and_then(|bin| {
            Command::new(bin)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|_| ())
                .map_err(|error| format!("панель не запустилась: {error}"))
        })
}

fn home() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_program_reports_an_error_instead_of_panicking() {
        let error = spawn_detached("hud-bar-nonexistent", &[]).unwrap_err();
        assert!(error.contains("не запустился"), "неясная ошибка: {error}");
    }

    /// Запуск настоящей программы без ожидания: результат тут же, процесс
    /// живёт сам по себе.
    #[test]
    fn detached_spawn_reports_success() {
        assert!(spawn_detached("true", &[]).is_ok());
    }

    #[test]
    fn home_is_absolute() {
        assert!(home().is_absolute());
    }
}

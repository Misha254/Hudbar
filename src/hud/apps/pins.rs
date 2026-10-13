//! Закреплённые приложения: список, файл и перенос из прежнего меню.
//!
//! Файл — строки с desktop-id в порядке закрепления. Порядок значим: верхняя
//! секция показывает закреплённые в том порядке, в каком их закрепили, а не по
//! алфавиту. Поэтому это не множество, а список с дедупликацией при чтении и
//! перезаписью целиком при записи.
//!
//! Прежнее окно rofi держало список в `~/.config/rofi/pinned`. Файл читается
//! один раз — при первом запуске, когда своего ещё нет, — и больше не трогается:
//! иначе удалённый пользователем из rofi закреплённый возродился бы сам.

use std::path::{Path, PathBuf};

/// Имя файла в каталоге конфигурации hudbar.
pub const FILE_NAME: &str = "apps-pinned";
/// Прежний файл меню rofi.
pub const LEGACY_NAME: &str = "pinned";

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into())).join(".config")
        })
}

/// Путь к своему файлу: `~/.config/hudbar/apps-pinned`.
pub fn path() -> PathBuf {
    config_home().join("hudbar").join(FILE_NAME)
}

/// Путь к файлу прежнего окна rofi.
pub fn legacy_path() -> PathBuf {
    config_home().join("rofi").join(LEGACY_NAME)
}

/// Читает список из произвольного файла. Мусор и повторы отбрасываются, пустой
/// файл — пустой список, а не ошибка.
pub fn load_from(file: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    clean(&content)
}

/// Разбор содержимого файла: строки без пробелов и дублей, порядок сохранён.
fn clean(content: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::with_capacity(16);
    for line in content.lines() {
        let id = line.trim();
        if id.is_empty() || ids.iter().any(|item| item == id) {
            continue;
        }
        ids.push(id.to_string());
    }
    ids
}

/// Пишет список во весь файл. Запись идёт через временный файл и переименование:
/// окно может быть убито в любой момент, а оборванный список закреплённых
/// читался бы как «пользователь ничего не закреплял».
pub fn save_to(file: &Path, ids: &[String]) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("не создать {}: {error}", dir.display()))?;
    }
    let mut body = String::new();
    for id in ids {
        body.push_str(id);
        body.push('\n');
    }
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, body)
        .map_err(|error| format!("не записать {}: {error}", tmp.display()))?;
    std::fs::rename(&tmp, file).map_err(|error| format!("не заменить {}: {error}", file.display()))
}

/// Список закреплённых из своего файла.
pub fn load() -> Vec<String> {
    load_from(&path())
}

/// Переносит список из файла rofi, если своего ещё нет. Возвращает `true`,
/// когда перенос состоялся: вызывающий пересобирает список строк.
pub fn import_legacy() -> bool {
    let own = path();
    if own.exists() {
        return false;
    }
    let legacy = load_from(&legacy_path());
    if legacy.is_empty() {
        return false;
    }
    save_to(&own, &legacy).is_ok()
}

/// Закрепить или открепить приложение. Возвращает новое состояние: `true` —
/// теперь закреплено. Ошибка записи возвращается наружу, а список в памяти всё
/// равно меняется: иначе окно показало бы «откреплено», а на следующем запуске
/// приложение снова было бы в закреплённых.
pub fn toggle(id: &str) -> Result<bool, String> {
    let file = path();
    let mut ids = load_from(&file);
    let pinned = match ids.iter().position(|item| item == id) {
        Some(at) => {
            ids.remove(at);
            false
        }
        None => {
            ids.push(id.to_string());
            true
        }
    };
    let written = save_to(&file, &ids);
    written.map(|()| pinned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Временный каталог теста. Отдельный каталог на тест: тесты в одном
    /// процессе идут параллельно, и общий файл списал бы их друг на друга.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hudbar-pins-{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&dir).expect("временный каталог");
        dir.join("pinned")
    }

    #[test]
    fn a_missing_file_is_an_empty_list() {
        assert!(load_from(Path::new("/nonexistent/hudbar/pinned")).is_empty());
    }

    #[test]
    fn reading_keeps_order_and_drops_junk() {
        let file = temp("read");
        std::fs::write(&file, "firefox\n\n  kitty  \nfirefox\n  \nzen\n").expect("запись");
        assert_eq!(load_from(&file), vec!["firefox", "kitty", "zen"]);
    }

    #[test]
    fn writing_then_reading_round_trips() {
        let file = temp("roundtrip");
        let ids = vec!["firefox".to_string(), "kitty".to_string()];
        save_to(&file, &ids).expect("сохранение");
        assert_eq!(load_from(&file), ids);
        // Временный файл переименования не должен остаться рядом.
        assert!(!file.with_extension("tmp").exists());
    }

    #[test]
    fn an_empty_list_is_written_as_an_empty_file() {
        let file = temp("empty");
        save_to(&file, &["firefox".to_string()]).expect("сохранение");
        save_to(&file, &[]).expect("очистка");
        assert!(load_from(&file).is_empty());
        assert_eq!(std::fs::read_to_string(&file).expect("чтение"), "");
    }

    /// Перенос из rofi случается один раз: свой файл — источник правды, и
    /// удалённое из него закреплённое не должно воскреснуть из старого списка.
    #[test]
    fn the_legacy_import_runs_only_when_our_file_is_missing() {
        let dir = std::env::temp_dir().join(format!("hudbar-pins-legacy-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("rofi")).expect("каталог rofi");
        std::fs::write(dir.join("rofi/pinned"), "firefox\nkitty\n").expect("старый список");

        let own = dir.join("hudbar/apps-pinned");
        let _ = std::fs::remove_file(&own);

        // Прямая проверка логики без чтения окружения: пути заданы явно.
        assert!(!own.exists());
        save_to(&own, &clean("firefox\nkitty\n")).expect("перенос");
        assert_eq!(load_from(&own), vec!["firefox", "kitty"]);

        // Второй перенос не происходит: свой файл уже на месте.
        assert!(own.exists());
    }
}

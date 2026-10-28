//! Что гритер помнит между запусками: последний пользователь и сеанс.
//!
//! Файл маленький и необязательный: битый, чужой или нечитаемый даёт значения
//! по умолчанию, а не отказ входа. Ошибка записи тоже не фатальна — вход
//! важнее удобства следующего раза.
//!
//! Путь по умолчанию — `/var/cache/hudbar-greeter/state.json`: это состояние
//! машины, а не пользователя (пользователь ещё не вошёл), и `/var/cache`
//! переживает перезагрузку, но не считается ценными данными.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Путь по умолчанию. Переопределяется аргументом `--state` и переменной
/// `HUDBAR_GREETER_STATE` — так тесты не трогают системный файл.
pub const DEFAULT_PATH: &str = "/var/cache/hudbar-greeter/state.json";

/// Запомненное состояние.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Last {
    pub username: Option<String>,
    pub session: Option<String>,
}

impl Last {
    /// Подстановка пользователя: пустая строка — это «неизвестен».
    pub fn with_username(username: &str) -> Self {
        Self {
            username: non_empty(username),
            session: None,
        }
    }

    pub fn with_session(mut self, session: &str) -> Self {
        self.session = non_empty(session);
        self
    }
}

fn non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Путь к файлу состояния с учётом окружения.
pub fn path() -> PathBuf {
    std::env::var_os("HUDBAR_GREETER_STATE")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PATH))
}

/// Чтение состояния. Любая беда — значения по умолчанию.
pub fn load(path: &Path) -> Last {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Last::default();
    };
    parse(&text)
}

/// Разбор JSON: отдельно от диска, чтобы битый файл проверялся без записи.
pub fn parse(text: &str) -> Last {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Last::default();
    };
    Last {
        username: value
            .get("username")
            .and_then(Value::as_str)
            .and_then(non_empty),
        session: value
            .get("session")
            .and_then(Value::as_str)
            .and_then(non_empty),
    }
}

/// Запись состояния: временный файл рядом и `rename`, чтобы читатель никогда
/// не увидел половину JSON.
pub fn save(path: &Path, last: &Last) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("нет каталога {}: {error}", parent.display()))?;
    }
    let payload = json!({
        "username": last.username.clone().unwrap_or_default(),
        "session": last.session.clone().unwrap_or_default(),
    });
    let text = serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("состояние не сериализуется: {error}"))?;
    let tmp = temp_path(path);
    std::fs::write(&tmp, text.as_bytes())
        .map_err(|error| format!("нет записи {}: {error}", tmp.display()))?;
    if let Err(error) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("нет переименования {}: {error}", path.display()));
    }
    Ok(())
}

/// Временный файл рядом с целевым: `rename` работает только в пределах одной
/// файловой системы.
fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "state.json".to_string());
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hudbar-greeter-state-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_saved_state_reads_back() {
        let dir = dir("roundtrip");
        let path = dir.join("state.json");
        let last = Last::with_username("mihail").with_session("niri");

        save(&path, &last).expect("запись");
        assert_eq!(load(&path), last);
        // Временных файлов рядом не осталось.
        assert_eq!(std::fs::read_dir(&dir).expect("каталог").count(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_gives_defaults() {
        let path = dir("missing").join("state.json");
        assert_eq!(load(&path), Last::default());
    }

    /// Битый JSON — не повод падать и не повод не пускать: значения по
    /// умолчанию, поля по отдельности.
    #[test]
    fn broken_json_gives_defaults() {
        assert_eq!(parse("{не json"), Last::default());
        assert_eq!(parse("[]"), Last::default());
        assert_eq!(parse(""), Last::default());
        assert_eq!(
            parse(r#"{"username": null, "session": 42}"#),
            Last::default()
        );
        assert_eq!(
            parse(r#"{"username": "  ", "session": "niri"}"#),
            Last {
                username: None,
                session: Some("niri".into())
            },
            "пробелы — тоже пусто"
        );
        assert_eq!(
            parse(r#"{"username": "mihail", "session": "niri", "extra": 1}"#),
            Last {
                username: Some("mihail".into()),
                session: Some("niri".into())
            },
            "лишние поля не мешают"
        );
    }

    /// Запись в недоступное место возвращает ошибку, а не панику: вызывающий
    /// покажет её один раз и продолжит вход.
    #[test]
    fn an_unwritable_path_is_an_error() {
        let dir = dir("unwritable");
        std::fs::create_dir_all(&dir).expect("каталог");
        // Родитель — обычный файл: создать каталог внутри нельзя.
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, b"x").expect("файл");
        let path = blocker.join("state.json");

        assert!(save(&path, &Last::default()).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

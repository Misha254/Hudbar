//! Секрет панели Mini App: отдельный файл, отдельные права.
//!
//! Почему не в `telegram.json`: там лежит токен бота, а этот файл нужен ещё и
//! для чтения человеком (чтобы вставить токен в ссылку). Держать их в разных
//! файлах значит, что и права настраиваются независимо.
//!
//! Почему вообще отдельный файл, а не поле в state: state-файл мост
//! перезаписывает часто и он не секретный — в него попадают id сессий и
//! номера сообщений. Токен в нём оказался бы доступен всем, кто читает
//! состояние, и уехал бы в бэкапы.
//!
//! Файл читается, но никогда не переписывается: единственный способ сменить
//! токен — удалить файл и создать заново. Это защита от тихих подмен.
//!
//! Права файла — `600`, и проверяются при чтении: если они шире, мост
//! предупреждает в журнал и работает дальше. Отказываться работать из-за прав
//! значило бы оставить человека без панели и без объяснения.

use std::path::{Path, PathBuf};

use super::config::read_json;

/// Путь файла секрета панели.
pub fn path() -> PathBuf {
    home().join(".config/hudbar/miniapp.json")
}

/// Секрет панели: токен доступа, порт и внешний адрес.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    /// Токен доступа. `None` — панель выключена.
    pub token: Option<String>,
    /// Порт на localhost. По умолчанию 8787.
    pub port: u16,
    /// Адрес, который Tailscale отдал панели: `https://узел.tailnet.ts.net`.
    /// Пустой — значит `tailscale serve` ещё не поднят.
    pub url: Option<String>,
}

impl Panel {
    /// Панель выключена: токена нет.
    pub fn off() -> Self {
        Self {
            token: None,
            port: default_port(),
            url: None,
        }
    }
}

/// Порт по умолчанию: 8787 не занят на этой машине и не конфликтует с
/// opencode на 4096.
pub const DEFAULT_PORT: u16 = 8787;

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// Читает секрет панели. Отсутствующий или битый файл — выключенная панель, а
/// не падение: мост должен работать и без веб-панели.
pub fn load() -> Panel {
    load_from(&path())
}

/// Чтение по указанному пути: тесты не должны трогать живой файл.
pub fn load_from(file: &Path) -> Panel {
    let Ok(Some(value)) = read_json(file) else {
        return Panel::off();
    };
    let token = value
        .get("token")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    let port = value
        .get("port")
        .and_then(serde_json::Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .unwrap_or_else(default_port);
    let url = value
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|url| url.starts_with("https://"))
        .map(str::to_string);
    Panel { token, port, url }
}

/// Права файла: 600 у секрета. Windows прав не имеет, поэтому проверка там
/// всегда «нормально», а не ошибка.
pub fn permissions_are_secret(file: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(file).is_ok_and(|meta| meta.permissions().mode() & 0o077 == 0)
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        true
    }
}

/// Ссылка для Telegram: адрес панели с токеном во фрагменте. Фрагмент
/// браузер серверу не отправляет, поэтому токен не осядет в логах туннеля.
pub fn link(base: &str, token: &str) -> String {
    format!(
        "{}/#{}={}",
        base.trim_end_matches('/'),
        super::mini::TOKEN_FRAGMENT,
        token
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hud-mini-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        dir.join("miniapp.json")
    }

    #[test]
    fn token_and_port_are_read_from_the_file() {
        let file = temp_file("read");
        std::fs::write(&file, r#"{"token":" abc ","port": 9999}"#).expect("запись");
        let panel = load_from(&file);
        assert_eq!(panel.token.as_deref(), Some("abc"), "пробелы обрезаны");
        assert_eq!(panel.port, 9999);
        let _ = std::fs::remove_dir_all(file.parent().expect("родитель"));
    }

    #[test]
    fn missing_or_broken_file_means_a_disabled_panel() {
        let file = temp_file("missing");
        assert_eq!(load_from(&file).token, None, "файла нет — панель выключена");
        std::fs::write(&file, "{ не json").expect("запись");
        assert_eq!(
            load_from(&file).token,
            None,
            "битый файл — панель выключена"
        );
        std::fs::write(&file, r#"{"token":"   "}"#).expect("запись");
        assert_eq!(load_from(&file).token, None, "пустой токен — не токен");
        let _ = std::fs::remove_dir_all(file.parent().expect("родитель"));
    }

    #[test]
    fn broken_port_falls_back_to_the_default() {
        let file = temp_file("port");
        std::fs::write(&file, r#"{"token":"t","port":"не число"}"#).expect("запись");
        assert_eq!(load_from(&file).port, DEFAULT_PORT);
        std::fs::write(&file, r#"{"token":"t","port":0}"#).expect("запись");
        assert_eq!(load_from(&file).port, DEFAULT_PORT, "нулевой порт не годен");
        std::fs::write(&file, r#"{"token":"t","port":99999}"#).expect("запись");
        assert_eq!(load_from(&file).port, DEFAULT_PORT, "порт не влезает в u16");
        let _ = std::fs::remove_dir_all(file.parent().expect("родитель"));
    }

    /// Ссылка несёт токен во фрагменте: сервер его не видит, поэтому в логах
    /// туннеля токена не будет. Хвостовой слеш убран, чтобы не было `//`.
    #[test]
    fn link_puts_the_token_into_the_fragment() {
        assert_eq!(
            link("https://box.tail1234.ts.net/", "секрет"),
            "https://box.tail1234.ts.net/#t=секрет".to_string()
        );
        assert!(link("https://x", "t").contains("/#t="));
    }

    #[cfg(unix)]
    #[test]
    fn secret_permissions_are_checked() {
        use std::os::unix::fs::PermissionsExt;
        let file = temp_file("perm");
        std::fs::write(&file, r#"{"token":"t"}"#).expect("запись");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("права");
        assert!(permissions_are_secret(&file), "600 — это секрет");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("права");
        assert!(!permissions_are_secret(&file), "644 — читают все");
        let _ = std::fs::remove_dir_all(file.parent().expect("родитель"));
    }
}

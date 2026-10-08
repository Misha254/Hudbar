//! Конфиг моста: `~/.config/hudbar/telegram.json`.
//!
//! Отдельный файл от `settings.json`: мост запускается отдельным бинарником и
//! не должен тащить за собой панель. Обязательны два поля — `bot_token` и
//! `chat_id`; остальные можно не писать, тогда берутся значения из
//! [`Config::defaults`].
//!
//! `chat_id` — единственный источник команд: апдейты от других чатов и от
//! групп отбрасываются, поэтому подписи команд проверять не нужно. Токен в
//! файле и не попадает ни в журнал, ни в `Debug`: в `Debug` на его месте
//! строка `<redacted>`, иначе `Debug` конфига ушёл бы в сообщение об ошибке.
//!
//! Значений по умолчанию для токена и чата нет: это личные данные конкретной
//! машины, в публичный репозиторий им нельзя. `bot_token` ещё и перекрывается
//! переменной окружения `TELEGRAM_BOT_TOKEN` — удобно для проверки, не трогая
//! файл.

use std::path::{Path, PathBuf};

///
/// Домашняя папка. Своя копия вместо `config_io::home`: тот модуль тянет за
/// собой настройки панели, а мосту нужен только путь.
fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// Атомарная запись: временный файл рядом с целью и переименование. Прав
/// не меняет — их выставляет [`restrict`] отдельно.
fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("у {} нет родительского каталога", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|why| why.to_string())?;
    let temporary = parent.join(format!(
        ".{}.tmp.{}",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        std::process::id()
    ));
    std::fs::write(&temporary, text).map_err(|why| why.to_string())?;
    std::fs::rename(&temporary, path).map_err(|why| why.to_string())?;
    Ok(())
}

/// Переменная окружения с токеном: перекрывает значение из файла.
pub const TOKEN_ENV: &str = "TELEGRAM_BOT_TOKEN";

/// Адрес opencode по умолчанию: локальный сервер на 4096.
pub const DEFAULT_OPENCODE: &str = "http://127.0.0.1:4096";

/// Конфиг моста.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Токен бота от BotFather.
    pub bot_token: String,
    /// Чат, из которого принимаются команды: обычно личка владельца.
    pub chat_id: Option<i64>,
    /// Адрес сервера opencode.
    pub opencode: String,
    /// Папка сессии: команды из телефона работают в ней.
    pub directory: String,
    /// Активная сессия. `None` — мост создаст её при первом промпте.
    pub session: Option<String>,
    /// Дублировать события в dunst через `notify-send`.
    pub dunst: bool,
}

impl Config {
    /// Значения по умолчанию: локальный opencode, папка `~/code/hudbar`,
    /// зеркало в dunst. Токена и чата нет — их заполняет человек.
    pub fn defaults() -> Self {
        Self {
            bot_token: String::new(),
            chat_id: None,
            opencode: DEFAULT_OPENCODE.to_string(),
            directory: home().join("code/hudbar").display().to_string(),
            session: None,
            dunst: true,
        }
    }

    /// Путь файла конфига.
    pub fn path() -> PathBuf {
        home().join(".config/hudbar/telegram.json")
    }

    /// Конфиг в виде строки для журнала: токен заменён заглушкой, чат —
    /// последними цифрами, чтобы было видно, что он вообще задан.
    pub fn redacted_debug(&self) -> String {
        format!(
            "Config {{ bot_token: <redacted>, chat_id: {}, opencode: {}, directory: {}, session: {}, dunst: {} }}",
            self.chat_id
                .map(|id| format!("…{id}"))
                .unwrap_or("нет".to_string()),
            self.opencode,
            self.directory,
            self.session.as_deref().unwrap_or("нет"),
            self.dunst,
        )
    }

    /// Задан ли чат: без него мост не знает, кому отвечать, и командами не
    /// управляет.
    pub fn ready(&self) -> bool {
        !self.bot_token.trim().is_empty() && self.chat_id.is_some()
    }
}

/// Читает конфиг, дополняя пробелы значениями по умолчанию. Отсутствующий
/// файл — не ошибка: возвращаются умолчания. Переменная окружения
/// перекрывает токен из файла.
pub fn load() -> Config {
    let mut config = load_from(&Config::path()).unwrap_or_else(|why| {
        super::log::warn(format!("мост: конфиг не прочитан ({why}), беру умолчания"));
        Config::defaults()
    });
    if let Ok(token) = std::env::var(TOKEN_ENV)
        && !token.trim().is_empty()
    {
        config.bot_token = token.trim().to_string();
    }
    config
}

/// Конфиг, пригодный для запуска: токен и чат обязательны, без них мост не
/// знает, куда писать и от кого принимать команды.
pub fn load_ready() -> Result<Config, String> {
    let config = load();
    ready_check(&config)
}

/// Проверка готовности на готовом конфиге. Отделена от [`load_ready`], чтобы
/// правило проверялось тестом без файла и переменных окружения.
pub fn ready_check(config: &Config) -> Result<Config, String> {
    if config.bot_token.trim().is_empty() {
        return Err(format!(
            "не задан bot_token: заполни его в {} или переменной {TOKEN_ENV}",
            Config::path().display()
        ));
    }
    if config.chat_id.is_none() {
        return Err(
            "не задан chat_id: напиши боту /start в Телеграме, чтобы он увидел твой чат"
                .to_string(),
        );
    }
    Ok(config.clone())
}

/// Читает конфиг из указанного пути.
pub fn load_from(path: &Path) -> Result<Config, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Config::defaults());
        }
        Err(why) => return Err(format!("{}: {why}", path.display())),
    };
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|why| format!("{}: {why}", path.display()))?;
    Ok(from_value(&value))
}

/// Собирает конфиг из JSON: отсутствующие поля берутся из умолчаний, пустая
/// строка в поле означает «не задано» и тоже заменяется умолчанием.
pub fn from_value(value: &serde_json::Value) -> Config {
    let mut config = Config::defaults();
    if let Some(token) = value.get("bot_token").and_then(serde_json::Value::as_str) {
        config.bot_token = token.trim().to_string();
    }
    if let Some(chat) = value.get("chat_id").and_then(serde_json::Value::as_i64) {
        config.chat_id = Some(chat);
    }
    if let Some(opencode) = value.get("opencode").and_then(serde_json::Value::as_str) {
        config.opencode = opencode.trim().trim_end_matches('/').to_string();
    }
    if let Some(directory) = value.get("directory").and_then(serde_json::Value::as_str) {
        config.directory = directory.trim().to_string();
    }
    if let Some(session) = value.get("session").and_then(serde_json::Value::as_str) {
        let session = session.trim();
        if !session.is_empty() {
            config.session = Some(session.to_string());
        }
    }
    if let Some(dunst) = value.get("dunst").and_then(serde_json::Value::as_bool) {
        config.dunst = dunst;
    }
    config
}

/// JSON для записи. `chat_id` и `session` пишутся, только когда заданы: пустая
/// строка читалась бы как «взять умолчание», а не как «нет значения».
pub fn to_value(config: &Config) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    object.insert(
        "bot_token".to_string(),
        serde_json::Value::String(config.bot_token.clone()),
    );
    if let Some(chat) = config.chat_id {
        object.insert("chat_id".to_string(), serde_json::Value::from(chat));
    }
    object.insert(
        "opencode".to_string(),
        serde_json::Value::String(config.opencode.clone()),
    );
    object.insert(
        "directory".to_string(),
        serde_json::Value::String(config.directory.clone()),
    );
    if let Some(session) = &config.session {
        object.insert(
            "session".to_string(),
            serde_json::Value::String(session.clone()),
        );
    }
    object.insert("dunst".to_string(), serde_json::Value::Bool(config.dunst));
    serde_json::Value::Object(object)
}

/// Записывает конфиг в файл по умолчанию с правами `600`: токен не должен быть
/// виден другим пользователям машины.
pub fn save(config: &Config) -> Result<(), String> {
    let path = Config::path();
    let text = serde_json::to_string_pretty(&to_value(config))
        .map_err(|why| format!("конфиг не собран: {why}"))?;
    atomic_write(&path, &format!("{text}\n"))?;
    restrict(&path);
    Ok(())
}

/// Права `600` на конфиг: иначе токен читает любой процесс пользователя.
pub fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_local_and_have_no_token_or_chat() {
        let config = Config::defaults();
        assert!(config.bot_token.is_empty());
        assert!(config.chat_id.is_none());
        assert_eq!(config.opencode, "http://127.0.0.1:4096");
        assert!(config.directory.ends_with("code/hudbar"));
        assert!(config.dunst);
    }

    #[test]
    fn values_from_json_override_defaults_and_keep_the_rest() {
        let value = serde_json::json!({
            "bot_token": "  123:abc  ",
            "chat_id": -1001234,
            "opencode": "http://127.0.0.1:5000/",
            "session": "ses_1",
            "dunst": false,
        });
        let config = from_value(&value);
        assert_eq!(config.bot_token, "123:abc");
        assert_eq!(config.chat_id, Some(-1001234));
        assert_eq!(config.opencode, "http://127.0.0.1:5000", "слэш срезан");
        assert_eq!(config.session.as_deref(), Some("ses_1"));
        assert!(!config.dunst);
        assert_eq!(config.directory, Config::defaults().directory);
    }

    /// Пустая строка — это «не задано», а не значение: иначе нельзя было бы
    /// стереть токен, не выписывая его заново.
    #[test]
    fn empty_strings_fall_back_to_defaults() {
        let value = serde_json::json!({ "bot_token": "  ", "session": "" });
        let config = from_value(&value);
        assert!(config.bot_token.is_empty());
        assert!(config.session.is_none());
    }

    /// Круг по JSON: записали → прочитали → то же самое, включая `chat_id` и
    /// `session`, которые пишутся только когда заданы.
    #[test]
    fn round_trip_through_json_keeps_the_config() {
        let mut config = Config::defaults();
        config.bot_token = "123:abc".to_string();
        config.chat_id = Some(42);
        config.session = Some("ses_abc".to_string());
        let value = to_value(&config);
        assert_eq!(from_value(&value), config);
    }

    #[test]
    fn token_never_appears_in_debug() {
        let mut config = Config::defaults();
        config.bot_token = "123456:AAEtoken".to_string();
        config.chat_id = Some(987654321);
        let debug = config.redacted_debug();
        assert!(!debug.contains("AAEtoken"), "токен в Debug: {debug}");
        assert!(!debug.contains("123456"), "префикс токена в Debug: {debug}");
        assert!(debug.contains("987654321"), "хвост чата виден: {debug}");
    }

    /// Без токена или чата мост не стартует: сказать об этом дешевле, чем
    /// молча подписываться в пустоту.
    #[test]
    fn ready_refuses_incomplete_configs() {
        let mut config = Config::defaults();
        assert!(ready_check(&config).is_err(), "без токена и чата");

        config.bot_token = "123:abc".to_string();
        assert!(ready_check(&config).is_err(), "без чата");

        config.chat_id = Some(1);
        assert!(ready_check(&config).is_ok(), "токен и чат на месте");
    }

    #[test]
    fn missing_file_yields_defaults() {
        let path = std::env::temp_dir().join(format!("hud-tg-missing-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(load_from(&path).expect("умолчания"), Config::defaults());
    }

    #[test]
    fn broken_json_is_an_error_not_defaults() {
        let path = std::env::temp_dir().join(format!("hud-tg-broken-{}.json", std::process::id()));
        std::fs::write(&path, "{ это не json").expect("запись");
        assert!(load_from(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }
}

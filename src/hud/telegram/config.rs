//! Конфиг моста: секреты и состояние в разных файлах.
//!
//! Секреты (`bot_token`, `chat_id`) живут в `~/.config/hudbar/telegram.json`
//! с правами `600`. Код этот файл только читает: записи туда нет ни одной,
//! поэтому токен не может утечь через запись.
//!
//! Всё остальное — адрес сервера, папка, активная сессия, зеркало в dunst —
//! лежит в `~/.local/state/hudbar/telegram-state.json` и перезаписывается при
//! каждом изменении сессии. Старый совмещённый файл (всё в `telegram.json`)
//! мигрирует сам при первом старте: значения переезжают в state-файл, а файл
//! секретов не трогается.
//!
//! Обязательны только токен и чат; остальное можно не писать, тогда берутся
//! значения из [`Config::defaults`].
//!
//! `chat_id` — единственный источник команд: апдейты от других чатов и от
//! групп отбрасываются, поэтому подписи команд проверять не нужно. Токен не
//! попадает ни в журнал, ни в `Debug`: в `Debug` на его месте строка
//! `<redacted>`, иначе `Debug` конфига ушёл бы в сообщение об ошибке.
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

/// Каталог состояния: `XDG_STATE_HOME` или `~/.local/state`. Тот же выбор,
/// что в `hud::log`: состояние и журнал лежат рядом.
fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"))
}

/// Атомарная запись: временный файл рядом с целью и переименование. Читатель
/// видит либо старый файл целиком, либо новый целиком, но никогда половину.
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

/// Читает JSON-файл: `Ok(None)` — файла нет, `Err` — он есть, но не читается.
/// Различие важно для миграции: отсутствующий state-файл означает «первый
/// старт», а битый — «не трогать молча, а сказать».
fn read_json(path: &Path) -> Result<Option<serde_json::Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|why| format!("{}: {why}", path.display())),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(why) => Err(format!("{}: {why}", path.display())),
    }
}

/// Переменная окружения с токеном: перекрывает значение из файла.
pub const TOKEN_ENV: &str = "TELEGRAM_BOT_TOKEN";

/// Адрес opencode по умолчанию: локальный сервер на 4096.
pub const DEFAULT_OPENCODE: &str = "http://127.0.0.1:4096";

/// Конфиг моста: секреты из файла секретов плюс исполняемые значения из
/// state-файла.
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
    /// Удалять команды пользователя после ответа. Выключено по умолчанию.
    pub delete_commands: bool,
}

/// Запись о живой карточке: сессия, чат и сообщение, чьи кнопки ещё на
/// экране. Хранится в state-файле, чтобы переживать перезапуск: при старте
/// мост правит карточку в «перезапущен» и снимает кнопки, а не оставляет их
/// висеть.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardRef {
    /// Сессия карточки.
    pub session: String,
    /// Чат сообщения.
    pub chat: i64,
    /// `message_id` в Telegram.
    pub message: i64,
}

impl CardRef {
    /// Разбор записи: без чисел это не запись, а мусор старого формата.
    pub fn from_value(value: &serde_json::Value) -> Option<Self> {
        Some(Self {
            session: value
                .get("session")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            chat: value.get("chat")?.as_i64()?,
            message: value.get("message")?.as_i64()?,
        })
    }

    /// JSON для записи.
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::json!({
            "session": self.session,
            "chat": self.chat,
            "message": self.message,
        })
    }
}

/// Запись о сообщении с запросом прав: тот же формат, что у карточки, плюс
/// id запроса, чтобы отличать свои кнопки от чужих.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermRef {
    /// Id запроса из события `permission.asked`.
    pub request: String,
    /// Чат сообщения.
    pub chat: i64,
    /// `message_id` в Telegram.
    pub message: i64,
}

impl PermRef {
    /// Разбор записи: без id запроса и чисел это не запись.
    pub fn from_value(value: &serde_json::Value) -> Option<Self> {
        Some(Self {
            request: value
                .get("request")
                .and_then(serde_json::Value::as_str)
                .filter(|request| !request.is_empty())
                .map(str::to_string)?,
            chat: value.get("chat")?.as_i64()?,
            message: value.get("message")?.as_i64()?,
        })
    }

    /// JSON для записи.
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::json!({
            "request": self.request,
            "chat": self.chat,
            "message": self.message,
        })
    }
}

/// Исполняемая часть конфига: то, что лежит в state-файле. Отдельный тип,
/// чтобы запись состояния не могла задеть секреты даже случайно: у неё их
/// просто нет в полях.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runtime {
    /// Адрес сервера opencode.
    pub opencode: String,
    /// Папка сессии.
    pub directory: String,
    /// Активная сессия.
    pub session: Option<String>,
    /// Зеркало в dunst.
    pub dunst: bool,
    /// Удалять команды пользователя после ответа. Выключено по умолчанию:
    /// история команд — тоже история.
    pub delete_commands: bool,
    /// Живые карточки: чьи кнопки ещё на экране.
    pub cards: Vec<CardRef>,
    /// Сообщения с запросами прав: чьи кнопки ещё на экране.
    pub permissions: Vec<PermRef>,
}

impl Runtime {
    /// Значения по умолчанию: локальный сервер, папка `~/code/hudbar`,
    /// зеркало включено, сессии нет, удаления команд нет, записей нет.
    pub fn defaults() -> Self {
        Self {
            opencode: DEFAULT_OPENCODE.to_string(),
            directory: home().join("code/hudbar").display().to_string(),
            session: None,
            dunst: true,
            delete_commands: false,
            cards: Vec::new(),
            permissions: Vec::new(),
        }
    }

    /// Собирает исполняемые значения из JSON: отсутствующие поля берутся из
    /// умолчаний, пустая строка — «не задано». Старые файлы без новых ключей
    /// читаются как прежде: записей нет, удаление выключено.
    pub fn from_value(value: &serde_json::Value) -> Self {
        let mut runtime = Self::defaults();
        if let Some(opencode) = value.get("opencode").and_then(serde_json::Value::as_str) {
            runtime.opencode = opencode.trim().trim_end_matches('/').to_string();
        }
        if let Some(directory) = value.get("directory").and_then(serde_json::Value::as_str) {
            runtime.directory = directory.trim().to_string();
        }
        if let Some(session) = value.get("session").and_then(serde_json::Value::as_str) {
            let session = session.trim();
            if !session.is_empty() {
                runtime.session = Some(session.to_string());
            }
        }
        if let Some(dunst) = value.get("dunst").and_then(serde_json::Value::as_bool) {
            runtime.dunst = dunst;
        }
        if let Some(delete) = value
            .get("delete_commands")
            .and_then(serde_json::Value::as_bool)
        {
            runtime.delete_commands = delete;
        }
        if let Some(cards) = value.get("cards").and_then(serde_json::Value::as_array) {
            runtime.cards = cards.iter().filter_map(CardRef::from_value).collect();
        }
        if let Some(permissions) = value
            .get("permissions")
            .and_then(serde_json::Value::as_array)
        {
            runtime.permissions = permissions.iter().filter_map(PermRef::from_value).collect();
        }
        runtime
    }

    /// JSON для записи. `session` пишется, только когда задана: пустая строка
    /// читалась бы как «взять умолчание», а не как «сессии нет».
    pub fn to_value(&self) -> serde_json::Value {
        let mut object = serde_json::Map::new();
        object.insert(
            "opencode".to_string(),
            serde_json::Value::String(self.opencode.clone()),
        );
        object.insert(
            "directory".to_string(),
            serde_json::Value::String(self.directory.clone()),
        );
        if let Some(session) = &self.session {
            object.insert(
                "session".to_string(),
                serde_json::Value::String(session.clone()),
            );
        }
        object.insert("dunst".to_string(), serde_json::Value::Bool(self.dunst));
        object.insert(
            "delete_commands".to_string(),
            serde_json::Value::Bool(self.delete_commands),
        );
        object.insert(
            "cards".to_string(),
            serde_json::Value::Array(self.cards.iter().map(CardRef::to_value).collect()),
        );
        object.insert(
            "permissions".to_string(),
            serde_json::Value::Array(self.permissions.iter().map(PermRef::to_value).collect()),
        );
        serde_json::Value::Object(object)
    }
}

impl Config {
    /// Значения по умолчанию: пустые секреты и исполняемые умолчания.
    /// Токена и чата нет — их заполняет человек.
    pub fn defaults() -> Self {
        let runtime = Runtime::defaults();
        Self {
            bot_token: String::new(),
            chat_id: None,
            opencode: runtime.opencode,
            directory: runtime.directory,
            session: runtime.session,
            dunst: runtime.dunst,
            delete_commands: runtime.delete_commands,
        }
    }

    /// Путь файла секретов.
    pub fn path() -> PathBuf {
        home().join(".config/hudbar/telegram.json")
    }

    /// Путь файла состояния.
    pub fn state_path() -> PathBuf {
        state_dir().join("hudbar/telegram-state.json")
    }

    /// Конфиг в виде строки для журнала: секреты заменены заглушками, видно
    /// только, что они заданы. Цифр чата здесь нет: `chat_id` — тоже секрет.
    pub fn redacted_debug(&self) -> String {
        format!(
            "Config {{ bot_token: <redacted>, chat_id: {}, opencode: {}, directory: {}, session: {}, dunst: {} }}",
            if self.chat_id.is_some() {
                "задан"
            } else {
                "нет"
            },
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

/// Секреты из JSON файла секретов: только токен и чат. Остальные ключи здесь
/// не читаются даже для миграции: миграция — забота [`load_runtime`].
fn secrets_from_value(value: &serde_json::Value) -> (String, Option<i64>) {
    let token = value
        .get("bot_token")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let chat = value.get("chat_id").and_then(serde_json::Value::as_i64);
    (token, chat)
}

/// Читает конфиг: секреты из файла секретов, исполняемое — из state-файла с
/// миграцией старого совмещённого файла. Отсутствующие файлы — не ошибка:
/// возвращаются умолчания. Переменная окружения перекрывает токен из файла.
pub fn load() -> Config {
    let secrets_value = match read_json(&Config::path()) {
        Ok(value) => value,
        Err(why) => {
            super::log::warn(format!("мост: файл секретов не прочитан ({why})"));
            None
        }
    };
    let (mut token, chat) = secrets_value
        .as_ref()
        .map(secrets_from_value)
        .unwrap_or_default();
    if let Ok(from_env) = std::env::var(TOKEN_ENV)
        && !from_env.trim().is_empty()
    {
        token = from_env.trim().to_string();
    }
    let runtime = load_runtime(secrets_value.as_ref());
    let mut config = Config::defaults();
    config.bot_token = token;
    config.chat_id = chat;
    config.opencode = runtime.opencode;
    config.directory = runtime.directory;
    config.session = runtime.session;
    config.dunst = runtime.dunst;
    config.delete_commands = runtime.delete_commands;
    config
}

/// Исполняемые значения: state-файл, а при его отсутствии — миграция старых
/// ключей из файла секретов. Файл секретов при этом не пишется: миграция
/// только читает. Неудачная запись state-файла — предупреждение, а не смерть:
/// мост продолжит со значениями в памяти и попробует снова при смене сессии.
fn load_runtime(secrets: Option<&serde_json::Value>) -> Runtime {
    match read_json(&Config::state_path()) {
        Ok(Some(value)) => Runtime::from_value(&value),
        Ok(None) => {
            let runtime = secrets
                .map(Runtime::from_value)
                .unwrap_or_else(Runtime::defaults);
            let snapshot = Snapshot::from_runtime(&runtime);
            if let Err(why) = save_state_to(&Config::state_path(), &snapshot) {
                super::log::warn(format!("мост: миграция состояния ({why})"));
            }
            runtime
        }
        Err(why) => {
            super::log::warn(format!(
                "мост: state-файл не прочитан ({why}), беру умолчания"
            ));
            Runtime::defaults()
        }
    }
}

/// Снимок исполняемых значений для записи. Отдельный тип, а не `Config`:
/// сохранить секреты в state-файл должно быть невыразимо.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Адрес сервера opencode.
    pub opencode: String,
    /// Папка сессии.
    pub directory: String,
    /// Активная сессия.
    pub session: Option<String>,
    /// Зеркало в dunst.
    pub dunst: bool,
    /// Удалять команды пользователя после ответа.
    pub delete_commands: bool,
    /// Живые карточки: чьи кнопки ещё на экране.
    pub cards: Vec<CardRef>,
    /// Сообщения с запросами прав: чьи кнопки ещё на экране.
    pub permissions: Vec<PermRef>,
}

impl Snapshot {
    /// Снимок из исполняемых значений.
    pub fn from_runtime(runtime: &Runtime) -> Self {
        Self {
            opencode: runtime.opencode.clone(),
            directory: runtime.directory.clone(),
            session: runtime.session.clone(),
            dunst: runtime.dunst,
            delete_commands: runtime.delete_commands,
            cards: runtime.cards.clone(),
            permissions: runtime.permissions.clone(),
        }
    }

    /// Снимок из полного конфига: секреты не копируются, у снимка их нет.
    /// Записи о живых сообщениях сюда не попадают: их добавляет мост
    /// отдельно, а конфиг про них не знает.
    pub fn from_config(config: &Config) -> Self {
        Self {
            opencode: config.opencode.clone(),
            directory: config.directory.clone(),
            session: config.session.clone(),
            dunst: config.dunst,
            delete_commands: config.delete_commands,
            cards: Vec::new(),
            permissions: Vec::new(),
        }
    }

    /// JSON для записи.
    pub fn to_value(&self) -> serde_json::Value {
        Runtime {
            opencode: self.opencode.clone(),
            directory: self.directory.clone(),
            session: self.session.clone(),
            dunst: self.dunst,
            delete_commands: self.delete_commands,
            cards: self.cards.clone(),
            permissions: self.permissions.clone(),
        }
        .to_value()
    }
}

/// Записывает снимок в state-файл по умолчанию. Атомарно: временный файл и
/// переименование. Файл секретов не трогается никогда.
pub fn save_state(snapshot: &Snapshot) -> Result<(), String> {
    save_state_to(&Config::state_path(), snapshot)
}

/// Записывает снимок по указанному пути. Отдельная функция ради тестов: путь
/// по умолчанию трогать из тестов нельзя.
pub fn save_state_to(path: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&snapshot.to_value())
        .map_err(|why| format!("состояние не собрано: {why}"))?;
    atomic_write(path, &format!("{text}\n"))
}

/// Живые записи из state-файла: карточки и сообщения с правами. Нужно
/// стартовой чистке: она правит пережившие перезапуск сообщения до запуска
/// потоков. Путь параметром, чтобы тесты читали временный файл, а не живой.
pub fn load_refs_from(path: &Path) -> (Vec<CardRef>, Vec<PermRef>) {
    match read_json(path) {
        Ok(Some(value)) => {
            let runtime = Runtime::from_value(&value);
            (runtime.cards, runtime.permissions)
        }
        _ => (Vec::new(), Vec::new()),
    }
}

/// Живые записи из state-файла по умолчанию.
pub fn load_refs() -> (Vec<CardRef>, Vec<PermRef>) {
    load_refs_from(&Config::state_path())
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
    fn runtime_values_override_defaults_and_keep_the_rest() {
        let value = serde_json::json!({
            "opencode": "http://127.0.0.1:5000/",
            "directory": "/tmp/работа",
            "session": "ses_1",
            "dunst": false,
            "delete_commands": true,
        });
        let runtime = Runtime::from_value(&value);
        assert_eq!(runtime.opencode, "http://127.0.0.1:5000", "слэш срезан");
        assert_eq!(runtime.directory, "/tmp/работа");
        assert_eq!(runtime.session.as_deref(), Some("ses_1"));
        assert!(!runtime.dunst);
        assert!(runtime.delete_commands);
        assert!(runtime.cards.is_empty() && runtime.permissions.is_empty());
    }

    /// Старый файл без новых ключей читается как прежде: удаление выключено,
    /// записей нет.
    #[test]
    fn old_files_without_new_keys_read_as_before() {
        let runtime = Runtime::from_value(&serde_json::json!({ "session": "ses_1" }));
        assert!(!runtime.delete_commands);
        assert!(runtime.cards.is_empty());
        assert!(runtime.permissions.is_empty());
    }

    /// Битые записи отбрасываются, а не роняют разбор: мост переживёт ручную
    /// правку файла.
    #[test]
    fn broken_refs_are_dropped_not_fatal() {
        let runtime = Runtime::from_value(&serde_json::json!({
            "cards": [{ "session": "ses_1" }, { "session": "s", "chat": 1, "message": 2 }],
            "permissions": [{ "request": "", "chat": 1, "message": 2 }],
        }));
        assert_eq!(runtime.cards.len(), 1);
        assert_eq!(runtime.cards[0].message, 2);
        assert!(runtime.permissions.is_empty());
    }

    /// Пустая строка — это «не задано», а не значение: иначе нельзя было бы
    /// сбросить сессию, не выписывая её заново.
    #[test]
    fn empty_session_falls_back_to_none() {
        let runtime = Runtime::from_value(&serde_json::json!({ "session": "  " }));
        assert!(runtime.session.is_none());
    }

    /// Секреты читаются отдельно: лишние ключи игнорируются, а не тянутся в
    /// исполняемые значения.
    #[test]
    fn secrets_take_only_token_and_chat() {
        let (token, chat) = secrets_from_value(&serde_json::json!({
            "bot_token": "  123:abc  ",
            "chat_id": -1001234,
            "session": "ses_1",
            "directory": "/tmp",
        }));
        assert_eq!(token, "123:abc");
        assert_eq!(chat, Some(-1001234));
    }

    /// Круг state-файла: записали снимок → прочитали исполняемые значения →
    /// то же самое.
    #[test]
    fn state_round_trip_keeps_runtime() {
        let dir = std::env::temp_dir().join(format!("hud-tg-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("telegram-state.json");
        let snapshot = Snapshot {
            opencode: "http://127.0.0.1:5000".to_string(),
            directory: "/tmp/работа".to_string(),
            session: Some("ses_abc".to_string()),
            dunst: false,
            delete_commands: true,
            cards: vec![CardRef {
                session: "ses_abc".to_string(),
                chat: 1,
                message: 7,
            }],
            permissions: vec![PermRef {
                request: "perm_1".to_string(),
                chat: 1,
                message: 8,
            }],
        };
        save_state_to(&path, &snapshot).expect("запись");
        let value = read_json(&path).expect("чтение").expect("значение");
        assert_eq!(
            Runtime::from_value(&value),
            Runtime {
                opencode: snapshot.opencode.clone(),
                directory: snapshot.directory.clone(),
                session: snapshot.session.clone(),
                dunst: snapshot.dunst,
                delete_commands: snapshot.delete_commands,
                cards: snapshot.cards.clone(),
                permissions: snapshot.permissions.clone(),
            }
        );
        // В каталоге нет временных файлов: запись атомарна и убирает за собой.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("каталог")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "остатки записи: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Миграция: state-файла нет, а в файле секретов лежат старые ключи —
    /// сессия и папка переезжают, токен и чат остаются только в секретах.
    #[test]
    fn legacy_keys_migrate_from_secrets_without_touching_them() {
        let secrets = serde_json::json!({
            "bot_token": "123:abc",
            "chat_id": 42,
            "session": "ses_old",
            "directory": "/tmp/старая",
            "dunst": false,
        });
        // Миграция читает исполняемые ключи из старого файла…
        let runtime = Runtime::from_value(&secrets);
        assert_eq!(runtime.session.as_deref(), Some("ses_old"));
        assert_eq!(runtime.directory, "/tmp/старая");
        assert!(!runtime.dunst);
        // …а секреты — только свои два ключа.
        let (token, chat) = secrets_from_value(&secrets);
        assert_eq!((token.as_str(), chat), ("123:abc", Some(42)));
        // Снимок для state-файла не содержит секретов даже полями.
        let snapshot = Snapshot::from_runtime(&runtime);
        let text = serde_json::to_string(&snapshot.to_value()).expect("json");
        assert!(!text.contains("123:abc"), "токен в state-файле");
        assert!(!text.contains("bot_token"), "поле токена в state-файле");
    }

    #[test]
    fn token_never_appears_in_debug() {
        let mut config = Config::defaults();
        config.bot_token = "123456:AAEtoken".to_string();
        config.chat_id = Some(987654321);
        let debug = config.redacted_debug();
        assert!(!debug.contains("AAEtoken"), "токен в Debug: {debug}");
        assert!(!debug.contains("123456"), "префикс токена в Debug: {debug}");
        assert!(!debug.contains("987654321"), "чат в Debug: {debug}");
        assert!(debug.contains("задан"), "видно, что чат задан: {debug}");
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
    fn missing_file_yields_nothing() {
        let path = std::env::temp_dir().join(format!("hud-tg-missing-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(read_json(&path).expect("отсутствие — не ошибка").is_none());
    }

    #[test]
    fn broken_json_is_an_error_not_defaults() {
        let path = std::env::temp_dir().join(format!("hud-tg-broken-{}.json", std::process::id()));
        std::fs::write(&path, "{ это не json").expect("запись");
        assert!(read_json(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }
}

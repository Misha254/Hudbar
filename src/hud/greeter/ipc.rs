//! Протокол greetd: сокет `$GREETD_SOCK`, сообщение — длина `u32` в native
//! endian и JSON.
//!
//! Крейта `greetd_ipc` здесь нет намеренно: протокол — четыре запроса и три
//! ответа, и держать его своим позволяет и mock в тестах, и `--preview` без
//! настоящего демона. Разбор ответа идёт по `serde_json::Value`, без derive:
//! типы описаны рядом с вызовами, а не в отдельной схеме.
//!
//! Здесь нет ни одного `unwrap`: любая ошибка сокета, длины или разбора
//! возвращается `Result` — гритер обязан показать причину, а не упасть на
//! экране входа.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

/// Предел длины сообщения от greetd. Реальные ответы — сотни байт; всё, что
/// больше мегабайта, — не протокол, а мусор в сокете, и читать его в память
/// нельзя.
pub const MAX_MESSAGE: usize = 1 << 20;

/// Ожидание ответа. При входе нет ничего хуже вечного «проверяю»: PAM может
/// думать над токеном, но не десять секунд молча.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// Вид запроса, который прислал PAM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    /// Обычный текст: имя, код, ответ на вопрос.
    Visible,
    /// Секрет: вводится точками и не показывается.
    Secret,
    /// Пояснение, отвечать нечем.
    Info,
    /// Ошибка от PAM, отвечать нечем.
    Error,
}

impl PromptKind {
    /// Разбор `auth_message_type`. Неизвестный вид считается пояснением:
    /// показать текст безобиднее, чем сорвать вход.
    pub fn parse(text: &str) -> Self {
        match text {
            "secret" => Self::Secret,
            "visible" => Self::Visible,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }

    /// Нужен ли ответ от пользователя.
    pub fn asks(self) -> bool {
        matches!(self, Self::Visible | Self::Secret)
    }
}

/// Ответ greetd.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Запрос выполнен.
    Success,
    /// Отказ: `auth_error` — неверный пароль, `error` — поломка.
    Error {
        error_type: String,
        description: String,
    },
    /// PAM просит ввод (или присылает пояснение).
    AuthMessage { kind: PromptKind, message: String },
}

/// Что гритер умеет сказать greetd. Реализация — сокет, mock — в тестах.
pub trait GreetdClient {
    fn create_session(&mut self, username: &str) -> Result<Reply, String>;
    fn post_auth_message_response(&mut self, response: Option<&str>) -> Result<Reply, String>;
    fn start_session(&mut self, cmd: &[String], env: &[String]) -> Result<Reply, String>;
    fn cancel_session(&mut self) -> Result<(), String>;
}

/// Путь к сокету: его задаёт greetd перед запуском гритера.
pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os("GREETD_SOCK")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// Разбор ответа. Отдельно от чтения: тесты гоняют его на строках, а не на
/// сокете.
pub fn parse_reply(value: &Value) -> Result<Reply, String> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "ответ greetd без поля type".to_string())?;
    match kind {
        "success" => Ok(Reply::Success),
        "error" => Ok(Reply::Error {
            error_type: string_field(value, "error_type"),
            description: string_field(value, "description"),
        }),
        "auth_message" => Ok(Reply::AuthMessage {
            kind: PromptKind::parse(&string_field(value, "auth_message_type")),
            message: string_field(value, "auth_message"),
        }),
        other => Err(format!("неизвестный ответ greetd: {other}")),
    }
}

/// Строковое поле ответа; отсутствующее — пустая строка, а не отказ разбора:
/// PAM вправе прислать `null` в описании.
fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Разбор JSON-байт ответа — то, что пришло из сокета.
pub fn parse_reply_bytes(bytes: &[u8]) -> Result<Reply, String> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| format!("ответ greetd не JSON: {error}"))?;
    parse_reply(&value)
}

/// Тело запроса. Отдельно от отправки: mock и тесты сверяют ровно его.
pub fn request_create_session(username: &str) -> Value {
    json!({"type": "create_session", "username": username})
}

pub fn request_auth_response(response: Option<&str>) -> Value {
    match response {
        Some(text) => json!({"type": "post_auth_message_response", "response": text}),
        None => json!({"type": "post_auth_message_response"}),
    }
}

pub fn request_start_session(cmd: &[String], env: &[String]) -> Value {
    json!({"type": "start_session", "cmd": cmd, "env": env})
}

pub fn request_cancel_session() -> Value {
    json!({"type": "cancel_session"})
}

/// Кодирование сообщения: длина в native endian, потом JSON.
pub fn encode_message(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    out.extend_from_slice(payload);
    out
}

/// Живой сокет greetd.
pub struct Socket {
    stream: UnixStream,
}

impl Socket {
    /// Подключение к сокету с таймаутами на чтение и запись.
    pub fn connect(path: &Path) -> Result<Self, String> {
        let stream = UnixStream::connect(path)
            .map_err(|error| format!("нет сокета greetd {}: {error}", path.display()))?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .map_err(|error| format!("таймаут чтения greetd: {error}"))?;
        stream
            .set_write_timeout(Some(TIMEOUT))
            .map_err(|error| format!("таймаут записи greetd: {error}"))?;
        Ok(Self { stream })
    }

    /// Один запрос и один ответ.
    pub fn call(&mut self, request: &Value) -> Result<Reply, String> {
        let payload = serde_json::to_vec(request)
            .map_err(|error| format!("запрос greetd не собрался: {error}"))?;
        self.stream
            .write_all(&encode_message(&payload))
            .and_then(|()| self.stream.flush())
            .map_err(|error| format!("greetd не принял запрос: {error}"))?;
        let reply = read_message(&mut self.stream)?;
        parse_reply_bytes(&reply)
    }
}

/// Чтение одного сообщения: длина, проверка предела, тело.
fn read_message(stream: &mut UnixStream) -> Result<Vec<u8>, String> {
    let mut head = [0u8; 4];
    stream
        .read_exact(&mut head)
        .map_err(|error| format!("greetd закрыл сокет: {error}"))?;
    let len = u32::from_ne_bytes(head) as usize;
    if len > MAX_MESSAGE {
        return Err(format!("ответ greetd длиннее предела: {len}"));
    }
    let mut body = vec![0u8; len];
    stream
        .read_exact(&mut body)
        .map_err(|error| format!("ответ greetd оборвался: {error}"))?;
    Ok(body)
}

impl GreetdClient for Socket {
    fn create_session(&mut self, username: &str) -> Result<Reply, String> {
        self.call(&request_create_session(username))
    }

    fn post_auth_message_response(&mut self, response: Option<&str>) -> Result<Reply, String> {
        self.call(&request_auth_response(response))
    }

    fn start_session(&mut self, cmd: &[String], env: &[String]) -> Result<Reply, String> {
        self.call(&request_start_session(cmd, env))
    }

    fn cancel_session(&mut self) -> Result<(), String> {
        self.call(&request_cancel_session()).map(|_| ())
    }
}

/// Клиент за `Box<dyn GreetdClient>`: гритер выбирает реализацию в рантайме
/// (сокет, preview, заглушка), а автомат входа не должен знать, какая из них
/// внутри.
impl<T: GreetdClient + ?Sized> GreetdClient for Box<T> {
    fn create_session(&mut self, username: &str) -> Result<Reply, String> {
        (**self).create_session(username)
    }

    fn post_auth_message_response(&mut self, response: Option<&str>) -> Result<Reply, String> {
        (**self).post_auth_message_response(response)
    }

    fn start_session(&mut self, cmd: &[String], env: &[String]) -> Result<Reply, String> {
        (**self).start_session(cmd, env)
    }

    fn cancel_session(&mut self) -> Result<(), String> {
        (**self).cancel_session()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_a_native_length_then_json() {
        let encoded = encode_message(b"{}");
        assert_eq!(encoded.len(), 6);
        assert_eq!(&encoded[..4], &2u32.to_ne_bytes());
        assert_eq!(&encoded[4..], b"{}");
    }

    #[test]
    fn replies_are_parsed_by_type() {
        assert_eq!(
            parse_reply(&json!({"type": "success"})).unwrap(),
            Reply::Success
        );
        assert_eq!(
            parse_reply(
                &json!({"type": "auth_message", "auth_message_type": "secret",
                                "auth_message": "Password: "})
            )
            .unwrap(),
            Reply::AuthMessage {
                kind: PromptKind::Secret,
                message: "Password: ".into()
            }
        );
        assert_eq!(
            parse_reply(&json!({"type": "error", "error_type": "auth_error",
                                "description": "Authentication failure"}))
            .unwrap(),
            Reply::Error {
                error_type: "auth_error".into(),
                description: "Authentication failure".into()
            }
        );
    }

    /// Отсутствующие поля — пустые строки, а не отказ: PAM может прислать
    /// ответ без описания, и вход из-за этого срываться не должен.
    #[test]
    fn missing_fields_become_empty_strings() {
        let reply = parse_reply(&json!({"type": "error"})).unwrap();
        assert_eq!(
            reply,
            Reply::Error {
                error_type: String::new(),
                description: String::new()
            }
        );
    }

    #[test]
    fn unknown_replies_are_errors_not_panics() {
        assert!(parse_reply(&json!({"type": "nonsense"})).is_err());
        assert!(parse_reply(&json!({"nothing": true})).is_err());
        assert!(parse_reply_bytes(b"not json").is_err());
        assert!(parse_reply_bytes(&[]).is_err());
    }

    /// Неизвестный вид запроса не рвёт вход: он показывается как пояснение.
    #[test]
    fn an_unknown_prompt_kind_is_shown_as_info() {
        assert_eq!(PromptKind::parse("hidden-secret"), PromptKind::Info);
        assert_eq!(PromptKind::parse("visible"), PromptKind::Visible);
        assert!(PromptKind::Secret.asks());
        assert!(!PromptKind::Error.asks());
    }

    /// Запросы собираются ровно так, как ждёт greetd.
    #[test]
    fn requests_carry_the_documented_fields() {
        assert_eq!(
            request_create_session("mihail"),
            json!({"type": "create_session", "username": "mihail"})
        );
        assert_eq!(
            request_auth_response(Some("secret")),
            json!({"type": "post_auth_message_response", "response": "secret"})
        );
        assert_eq!(
            request_start_session(&["niri-session".into()], &["LANG=ru_RU.UTF-8".into()]),
            json!({"type": "start_session", "cmd": ["niri-session"], "env": ["LANG=ru_RU.UTF-8"]})
        );
        assert_eq!(request_cancel_session(), json!({"type": "cancel_session"}));
    }

    /// Сокет не открывается — ошибка с путём, без паники.
    #[test]
    fn a_missing_socket_is_an_error() {
        let path = std::env::temp_dir().join(format!(
            "hudbar-greeter-no-such-socket-{}",
            std::process::id()
        ));
        let error = Socket::connect(&path).err().expect("сокета нет");
        assert!(error.contains("greetd"), "{error}");
    }
}

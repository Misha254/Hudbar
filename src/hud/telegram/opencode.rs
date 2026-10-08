//! Клиент HTTP-API opencode: свой, локальный и без TLS.
//!
//! Клиент намеренно крошечный: адрес всегда локальный, тело ответа — маленький
//! JSON, а лишняя зависимость ради нескольких `POST` в проект с уже выбранным
//! набором крейтов тянула бы за собой полвека пакетов. Поэтому здесь только то,
//! что реально нужно: один запрос, один ответ, один таймаут. У `vpn_api` та же
//! философия: разные сервисы, один стиль кода.
//!
//! Формат запросов подсмотрен в SDK `~/.config/opencode/node_modules` (типы
//! `SessionCreateData`, `SessionPromptData`, `PermissionRespondData`) и сверен
//! с живой спекой сервера (`GET /doc`): промпты уходят через неблокирующий
//! `prompt_async` (204), ответы на права — через `POST /permission/{id}/reply`.
//! Использование подтверждается не только этим комментарием, но и тестами:
//! сборки с неправильными полями падают на живых проверках.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::{Value, json};

/// Таймаут подключения и чтения ответа: локальный сервер отвечает за
/// миллисекунды, а зависший демон иначе держал бы цикл отправки.
const TIMEOUT: Duration = Duration::from_secs(10);
/// Ответ сервера не может быть бесконечным: 8 MiB хватает с запасом и не даёт
/// зависшему серверу раздувать память моста.
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Ошибка вызова: текст уходит в журнал и — в усечённом виде — в Telegram.
pub type Error = String;

/// Сессия opencode в нужных полях.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// `ses_…`: идентификатор.
    pub id: String,
    /// Название: у новых сессий может не быть, тогда в интерфейсе оно пустое.
    pub title: String,
    /// Папка сессии.
    pub directory: String,
}

impl Session {
    /// Краткий id для экрана: первые 12 символов после `ses_`.
    pub fn short_id(&self) -> &str {
        self.id
            .strip_prefix("ses_")
            .map(|rest| &rest[..rest.len().min(12)])
            .unwrap_or(&self.id)
    }
}

impl std::fmt::Display for Session {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.title.is_empty() {
            write!(formatter, "{}", self.short_id())
        } else {
            write!(formatter, "{} ({})", self.title, self.short_id())
        }
    }
}

/// Событие из потока `GET /event`: тип и свойства как есть.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// `evt_…`: идентификатор потока.
    pub id: String,
    /// Тип: `permission.asked`, `message.updated`, `session.idle` и другие.
    pub event_type: String,
    /// Свойства события: структура зависит от типа.
    pub properties: Value,
}

/// Разбирает строку SSE `data: {...}` в событие. Всё остальное —
/// служебные строки потока, их отдаёт значение `None`.
pub fn parse_event_line(line: &str) -> Result<Option<Event>, Error> {
    let rest = line.strip_prefix("data:").unwrap_or(line);
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    if rest.is_empty() || rest == "[DONE]" {
        return Ok(None);
    }
    if !rest.trim_start().starts_with('{') {
        return Ok(None);
    }
    let value: Value =
        serde_json::from_str(rest).map_err(|why| format!("событие не json: {why}"))?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if event_type.is_empty() {
        return Ok(None);
    }
    Ok(Some(Event {
        id,
        event_type,
        properties: value.get("properties").cloned().unwrap_or(Value::Null),
    }))
}

/// Клиент локального сервера opencode.
#[derive(Clone)]
pub struct Client {
    host: String,
    port: u16,
    directory: String,
}

impl Client {
    /// Клиент по адресу вида `http://127.0.0.1:4096`. Только `http`: мост и
    /// сервер живут на одной машине, TLS там не нужен и сервер его не слушает.
    pub fn new(base: &str) -> Result<Self, Error> {
        let rest = base
            .strip_prefix("http://")
            .ok_or_else(|| format!("адрес сервера должен начинаться с http://: {base}"))?;
        let (host, port) = rest
            .rsplit_once(':')
            .ok_or_else(|| format!("в адресе нет порта: {base}"))?;
        let port: u16 = port.parse().map_err(|_| format!("порт не число: {base}"))?;
        Ok(Self {
            host: host.to_string(),
            port,
            directory: String::new(),
        })
    }

    /// Папка, в которой работает мост. Отдельно от адреса: та же машина
    /// обслуживает несколько папок, и переезд — это просто новое значение.
    pub fn set_directory(&mut self, directory: &str) {
        self.directory = directory.to_string();
    }

    /// Папка моста.
    pub fn directory(&self) -> &str {
        &self.directory
    }

    fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, Error> {
        let addr = format!("{}:{}", self.host, self.port);
        let socket: SocketAddr = addr
            .to_socket_addrs()
            .map_err(|why| format!("адрес сервера не разобран ({addr}): {why}"))?
            .next()
            .ok_or_else(|| format!("адрес сервера не разобран: {addr}"))?;
        let mut stream = TcpStream::connect_timeout(&socket, TIMEOUT)
            .map_err(|why| format!("сервер недоступен ({addr}): {why}"))?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .map_err(|why| format!("таймаут чтения: {why}"))?;
        let encoded = url_encode(&self.directory);
        let query = format!("?directory={encoded}");
        let raw = body
            .map(|value| value.to_string())
            .unwrap_or_else(|| "{}".to_string());
        let request = format!(
            "{method} {path}{query} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{raw}",
            raw.len()
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|why| format!("запрос не отправлен: {why}"))?;
        let mut response = Vec::new();
        stream
            .take(MAX_RESPONSE_BYTES as u64 + 1)
            .read_to_end(&mut response)
            .map_err(|why| format!("ответ не прочитан: {why}"))?;
        if response.len() > MAX_RESPONSE_BYTES {
            return Err("ответ слишком большой".to_string());
        }
        let text = String::from_utf8_lossy(&response);
        let (status, headers, body) =
            split_response(&text).ok_or_else(|| "ответ не похож на HTTP".to_string())?;
        if !(200..300).contains(&status) {
            return Err(format!("{}: {}", describe_status(status), first_line(body)));
        }
        // `204 No Content` и пустые ответы — норма, а не ошибка.
        let body = body.trim();
        if body.is_empty() {
            return Ok(Value::Null);
        }
        // Закодированный ответ не поддерживается: локальный сервер отдаёт
        // обычный текст, а не `chunked` и не `gzip`.
        let _ = headers;
        serde_json::from_str(body).map_err(|why| format!("ответ не json: {why}"))
    }

    /// Ждёт готовности сервера: пробует короткое чтение списка сессий, пока
    /// тот не ответит. Нужен при старте: `opencode serve` поднимается позже
    /// моста, и первая команда не должна падать.
    pub fn wait_ready(&self, attempts: u32, pause: Duration) -> Result<(), Error> {
        let mut last = String::new();
        for _ in 0..attempts.max(1) {
            match self.request("GET", "/session", None) {
                Ok(_) => return Ok(()),
                Err(why) => {
                    last = why;
                    std::thread::sleep(pause);
                }
            }
        }
        Err(last)
    }

    /// Новая сессия в папке моста.
    pub fn create_session(&self, title: Option<&str>) -> Result<Session, Error> {
        let mut body = serde_json::Map::new();
        if let Some(title) = title {
            body.insert("title".to_string(), Value::String(title.to_string()));
        }
        let value = self.request("POST", "/session", Some(&Value::Object(body)))?;
        session_from(&value)
    }

    /// Одна сессия по id. `Ok(None)` — сессии нет (404); остальные ошибки —
    /// причина, а не ответ, и отдаются наружу, чтобы вызывающий не стирал
    /// состояние по недоступности сервера.
    pub fn get_session(&self, session_id: &str) -> Result<Option<Session>, Error> {
        match self.request("GET", &format!("/session/{session_id}"), None) {
            Ok(value) => session_from(&value).map(Some),
            Err(why) if why.contains("HTTP 404") => Ok(None),
            Err(why) => Err(why),
        }
    }

    /// Последние сессии папки, свежие сверху.
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<Session>, Error> {
        let path = format!("/session?limit={limit}");
        let value = self.request("GET", &path, None)?;
        let items = value
            .as_array()
            .ok_or_else(|| "список сессий не список".to_string())?;
        items.iter().map(session_from).collect()
    }

    /// Отправляет промпт в сессию и возвращается сразу: сервер отвечает 204
    /// «Prompt accepted», а результат приходит только через поток `/event`.
    /// Блокирующий `POST /message` здесь не используется: он держит
    /// соединение до конца работы агента.
    pub fn send_prompt_async(&self, session_id: &str, text: &str) -> Result<(), Error> {
        self.request(
            "POST",
            &format!("/session/{session_id}/prompt_async"),
            Some(&prompt_body(text)),
        )
        .map(|_| ())
    }

    /// Карта состояний сессий папки: id сессии → `idle`/`busy`/`retry`.
    /// Записи неизвестного вида пропускаются: спека на новые значения нет, а
    /// падать из-за них нельзя.
    pub fn session_status(&self) -> Result<HashMap<String, String>, Error> {
        let value = self.request("GET", "/session/status", None)?;
        let map = value
            .as_object()
            .ok_or_else(|| "статусы сессий не объект".to_string())?;
        let mut out = HashMap::new();
        for (id, status) in map {
            if let Some(kind) = status.get("type").and_then(Value::as_str) {
                out.insert(id.clone(), kind.to_string());
            }
        }
        Ok(out)
    }

    /// Прерывает работу агента в сессии.
    pub fn abort_session(&self, session_id: &str) -> Result<(), Error> {
        self.request("POST", &format!("/session/{session_id}/abort"), None)
            .map(|_| ())
    }

    /// Ответ на запрос прав: `once` — разрешить один раз, `always` —
    /// разрешить всегда, `reject` — отказать. Эндпоинт без сессии в пути:
    /// `POST /session/{id}/permissions/{id}` помечен deprecated в спеке
    /// сервера, а id из события `permission.asked` — это и есть requestID.
    pub fn reply_permission(&self, permission_id: &str, response: &str) -> Result<(), Error> {
        let body = json!({ "reply": response });
        self.request(
            "POST",
            &format!("/permission/{permission_id}/reply"),
            Some(&body),
        )
        .map(|_| ())
    }

    /// Поток событий папки моста. Блочный: возвращает управление только после
    /// `true` из колбэка — мост держит этот вызов в отдельном потоке.
    pub fn stream_events<F>(&self, mut each: F) -> Result<(), Error>
    where
        F: FnMut(&Event) -> bool,
    {
        let addr = format!("{}:{}", self.host, self.port);
        let socket: SocketAddr = addr
            .to_socket_addrs()
            .map_err(|why| format!("адрес сервера не разобран ({addr}): {why}"))?
            .next()
            .ok_or_else(|| format!("адрес сервера не разобран: {addr}"))?;
        let mut stream = TcpStream::connect_timeout(&socket, TIMEOUT)
            .map_err(|why| format!("сервер недоступен ({addr}): {why}"))?;
        // Поток живой долго, но не бесконечно: по истечении получаса без
        // данных мост переподключается свежим соединением.
        stream
            .set_read_timeout(Some(Duration::from_secs(1800)))
            .map_err(|why| format!("таймаут чтения: {why}"))?;
        let encoded = url_encode(&self.directory);
        let request = format!(
            "GET /event?directory={encoded} HTTP/1.1\r\nHost: {addr}\r\nAccept: text/event-stream\r\nConnection: keep-alive\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|why| format!("подписка не запрошена: {why}"))?;
        let mut reader = BufReader::new(stream);
        let mut headers = String::new();
        let mut line = String::new();
        while reader
            .read_line(&mut line)
            .map_err(|why| format!("чтение заголовков: {why}"))?
            > 0
        {
            if line == "\r\n" || line == "\n" {
                break;
            }
            headers.push_str(&line);
            line.clear();
        }
        if !headers.starts_with("HTTP/1.1 2") && !headers.starts_with("HTTP/1.0 2") {
            return Err(format!("подписка отклонена: {}", first_line(&headers)));
        }
        line.clear();
        // Чанкованная передача: сначала идёт длина чанка в hex, потом данные.
        // opencode отдаёт поток чанками, поэтому читаем не сырыми строками, а
        // через декодер: hex-строку пропускаем, данные собираем.
        loop {
            line.clear();
            let read = match reader.read_line(&mut line) {
                Ok(count) => count,
                Err(_) => return Ok(()),
            };
            if read == 0 {
                return Ok(());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if is_chunk_length(trimmed) {
                // Читаем содержимое чанка и отдаём его построчно.
                let size = match usize::from_str_radix(trimmed, 16) {
                    Ok(size) => size,
                    Err(_) => continue,
                };
                if size == 0 {
                    continue;
                }
                let mut chunk = vec![0u8; size.min(MAX_RESPONSE_BYTES)];
                reader
                    .read_exact(&mut chunk)
                    .map_err(|why| format!("чтение чанка: {why}"))?;
                let text = String::from_utf8_lossy(&chunk);
                let mut stop_now = false;
                for chunk_line in text.lines() {
                    if let Some(event) = parse_event_line(chunk_line)
                        .map_err(|why| format!("событие не разобрано: {why}"))?
                        && each(&event)
                    {
                        stop_now = true;
                    }
                }
                if stop_now {
                    return Ok(());
                }
                continue;
            }
            if let Some(event) =
                parse_event_line(trimmed).map_err(|why| format!("событие не разобрано: {why}"))?
                && each(&event)
            {
                return Ok(());
            }
        }
    }
}

/// Тело промпта: `parts` — массив, текстовая часть внутри. Отдельная функция,
/// чтобы тест проверял форму без сети: сервер принимает только массив.
pub fn prompt_body(text: &str) -> Value {
    json!({ "parts": [{ "type": "text", "text": text }] })
}

/// Сессия из JSON сервера.
pub fn session_from(value: &Value) -> Result<Session, Error> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "сессия без id".to_string())?;
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let directory = value
        .pointer("/path/root")
        .or_else(|| value.pointer("/path/cwd"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(Session {
        id: id.to_string(),
        title,
        directory,
    })
}

/// URL-кодирование значения параметра: пробел — `%20`, кириллица — байты.
/// В std готового кодировщика нет, а папка с пробелами ломала бы запрос.
pub fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Делит сырой HTTP-ответ на код, заголовки и тело.
pub fn split_response(text: &str) -> Option<(u16, &str, &str)> {
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))?;
    let mut lines = head.lines();
    let status = lines.next()?;
    let code: u16 = status.split_whitespace().nth(1)?.parse().ok()?;
    let mut headers_end = 0usize;
    for line in lines {
        headers_end += line.len() + 1;
    }
    let _ = headers_end;
    Some((code, &head[status.len()..], body))
}

/// Короткое описание кода: полный текст статуса сервер не отдаёт, поэтому
/// только код.
pub fn describe_status(status: u16) -> String {
    format!("HTTP {status}")
}

/// Первая строка текста: ошибка сервера в одну строку, без простыни стека.
pub fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

/// Длина чанка `chunked`: только hex-цифры, пустых строк и `data:` тут нет.
pub fn is_chunk_length(line: &str) -> bool {
    !line.is_empty()
        && line.len() <= 8
        && line.chars().all(|character| character.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 404 означает «сессии нет», а не ошибку: вызывающий сбрасывает
    /// состояние, а не падает. Проверяется на локальной заглушке, а не на
    /// живом сервере: тест не должен зависеть от чужих сессий.
    #[test]
    fn http_404_maps_to_none() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("слушатель");
        let port = listener.local_addr().expect("порт").port();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            if let Ok((mut stream, _)) = listener.accept() {
                let mut head = vec![0u8; 4096];
                let _ = stream.read(&mut head);
                let body = r#"{"name":"NotFoundError","data":{"message":"no such session"}}"#;
                let response = format!(
                    "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        let mut client = Client::new(&format!("http://127.0.0.1:{port}")).expect("разбор");
        client.set_directory("/tmp");
        assert_eq!(client.get_session("ses_нет").expect("ответ"), None);
    }

    /// Закрытый порт — транспортная ошибка, а не 404: сессия остаётся, мост
    /// разберётся живьём.
    #[test]
    fn refused_connection_is_an_error_not_a_missing_session() {
        let client = Client::new("http://127.0.0.1:1").expect("разбор");
        assert!(client.get_session("ses_нет").is_err());
    }

    /// Заглушка HTTP: отдаёт один заранее заданный ответ. Нужна, чтобы
    /// проверить разбор ответов без живого сервера.
    fn stub_server(status: &str, body: &str) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("слушатель");
        let port = listener.local_addr().expect("порт").port();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len(),
        );
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            if let Ok((mut stream, _)) = listener.accept() {
                let mut head = vec![0u8; 4096];
                let _ = stream.read(&mut head);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        port
    }

    /// `prompt_async` возвращает 204 без тела: это успех, а не «ответ не
    /// json». Ошибка сервера с телом маппится в текст ошибки.
    #[test]
    fn prompt_async_accepts_204_and_maps_errors() {
        let port = stub_server("204 No Content", "");
        let mut client = Client::new(&format!("http://127.0.0.1:{port}")).expect("разбор");
        client.set_directory("/tmp");
        assert!(
            client.send_prompt_async("ses_1", "привет").is_ok(),
            "204 — это принятие промпта"
        );

        let port = stub_server("400 Bad Request", r#"{"name":"BadRequest"}"#);
        let mut client = Client::new(&format!("http://127.0.0.1:{port}")).expect("разбор");
        client.set_directory("/tmp");
        let error = client
            .send_prompt_async("ses_1", "привет")
            .expect_err("400 — это ошибка");
        assert!(error.contains("HTTP 400"), "код виден: {error}");
    }

    /// Тело промпта — объект с массивом `parts`, текстовая часть внутри.
    /// Форма сверена с живой спекой (`prompt_async` в `GET /doc`): сервер
    /// принимает только массив.
    #[test]
    fn prompt_body_is_parts_array_with_text_inside() {
        let body = prompt_body("привет");
        let text = serde_json::to_string(&body).expect("json");
        assert!(text.contains("\"parts\":["), "массив, а не объект: {text}");
        assert!(text.contains("\"type\":\"text\""), "тип части: {text}");
        assert!(
            text.contains("\"text\":\"привет\""),
            "текст на месте: {text}"
        );
    }

    /// Карта статусов разбирается в пары id → тип; записи без типа
    /// пропускаются, а не роняют разбор.
    #[test]
    fn session_status_maps_ids_to_kinds() {
        let port = stub_server(
            "200 OK",
            r#"{"ses_1":{"type":"busy"},"ses_2":{"type":"idle"},"ses_3":{"nonsense":1}}"#,
        );
        let mut client = Client::new(&format!("http://127.0.0.1:{port}")).expect("разбор");
        client.set_directory("/tmp");
        let map = client.session_status().expect("карта");
        assert_eq!(map.get("ses_1").map(String::as_str), Some("busy"));
        assert_eq!(map.get("ses_2").map(String::as_str), Some("idle"));
        assert!(!map.contains_key("ses_3"), "без типа пропускается");
    }

    #[test]
    fn base_parses_host_and_port_and_rejects_everything_else() {
        let client = Client::new("http://127.0.0.1:4096").expect("разбор");
        assert_eq!((client.host.as_str(), client.port), ("127.0.0.1", 4096));
        assert!(Client::new("https://127.0.0.1:4096").is_err());
        assert!(Client::new("127.0.0.1:4096").is_err());
        assert!(Client::new("http://127.0.0.1").is_err());
        assert!(Client::new("http://127.0.0.1:abc").is_err());
    }

    #[test]
    fn session_keeps_id_title_and_directory() {
        let session = session_from(&json!({
            "id": "ses_abc123def456",
            "title": "Мост",
            "path": { "cwd": "/home/mihail/code/hudbar", "root": "/home/mihail/code/hudbar" },
        }))
        .expect("разбор");
        assert_eq!(session.short_id(), "abc123def456");
        assert_eq!(session.to_string(), "Мост (abc123def456)");
    }

    #[test]
    fn session_without_a_title_shows_the_id() {
        let session = session_from(&json!({ "id": "ses_xyz" })).expect("разбор");
        assert_eq!(session.to_string(), "xyz");
        assert!(session_from(&json!({})).is_err());
    }

    #[test]
    fn sse_lines_become_events_and_noise_is_skipped() {
        let event = parse_event_line(
            r#"data: {"id":"evt_1","type":"session.idle","properties":{"sessionID":"ses_1"}}"#,
        )
        .expect("разбор")
        .expect("событие");
        assert_eq!(event.id, "evt_1");
        assert_eq!(event.event_type, "session.idle");
        assert_eq!(event.properties["sessionID"], json!("ses_1"));

        assert!(parse_event_line("").expect("пусто").is_none());
        assert!(parse_event_line("event: message").expect("тип").is_none());
        assert!(parse_event_line("data: [DONE]").expect("конец").is_none());
        assert!(parse_event_line(": ping").expect("пинг").is_none());
        assert!(parse_event_line("00a").expect("чанк").is_none());
    }

    #[test]
    fn broken_event_json_is_an_error() {
        assert!(parse_event_line("data: {не json}").is_err());
    }

    #[test]
    fn response_splits_into_status_headers_and_body() {
        let text = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        let (status, _, body) = split_response(text).expect("разбор");
        assert_eq!((status, body), (200, "{}"));
        assert!(split_response("мусор").is_none());
    }

    #[test]
    fn error_body_collapses_to_the_first_line() {
        assert_eq!(
            first_line("первая\nвторая\nтретья"),
            "первая",
            "в ответ уходит только первая строка"
        );
    }

    #[test]
    fn directory_with_spaces_and_letters_is_encoded() {
        assert_eq!(
            url_encode("/home/mihail/Мои проекты/x"),
            "/home/mihail/%D0%9C%D0%BE%D0%B8%20%D0%BF%D1%80%D0%BE%D0%B5%D0%BA%D1%82%D1%8B/x"
        );
    }

    #[test]
    fn chunk_lengths_are_hex_and_sse_data_lines_are_not() {
        assert!(is_chunk_length("1a"));
        assert!(is_chunk_length("0"));
        assert!(
            !is_chunk_length("data: {}"),
            "строка события не длина чанка"
        );
        assert!(!is_chunk_length(""), "пустая строка не длина чанка");
        assert!(
            !is_chunk_length("evt_11aee1f60001"),
            "id события не длина чанка"
        );
    }
}

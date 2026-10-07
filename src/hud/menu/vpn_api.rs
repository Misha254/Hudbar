//! HTTP-клиент к external controller Mihomo.
//!
//! Клиент намеренно крошечный: адрес всегда `127.0.0.1`, тело ответа — маленький
//! JSON, а лишняя зависимость ради одного `GET` в проект с уже выбранным
//! набором крейтов тянула бы за собой полвека пакетов. Поэтому здесь только то,
//! что реально нужно: один запрос, один ответ, один таймаут.
//!
//! Секрет контроллера не попадает ни в журнал, ни в `Debug`, ни в снапшот: в
//! заголовке `Authorization` он нужен целиком, но строка для логов его не
//! содержит. Значение приходит из файла или переменной окружения и живёт только
//! в структуре клиента.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Значение по умолчанию: адрес из конфига Mihomo на этой машине.
pub const DEFAULT_ADDR: &str = "127.0.0.1:9090";
/// Таймаут по умолчанию: локальный контроллер отвечает за миллисекунды, а
/// зависший демон иначе держал бы окно открытым.
pub const DEFAULT_TIMEOUT_MS: u32 = 1500;
/// Mihomo возвращает несколько сотен имён узлов; 4 MiB хватает с запасом и
/// не позволяет ошибочному контроллеру бесконечно раздувать память клиента.
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Ошибка может попасть прямо в подпись HUD: не отдавать туда целиком тело
/// ответа контроллера или его HTML-страницу ошибки.
const MAX_ERROR_CHARS: usize = 240;

/// Клиент контроллера.
#[derive(Clone)]
pub struct Controller {
    addr: String,
    timeout: Duration,
    secret: Option<String>,
}

impl std::fmt::Debug for Controller {
    /// Секрет в `Debug` не печатается: адрес виден, значение — нет.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Controller")
            .field("addr", &self.addr)
            .field("timeout", &self.timeout)
            .field("secret", &self.secret.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Ошибка соединения или разбора ответа: текст идёт в подвал окна, поэтому он
/// должен быть понятен человеку, а не программисту.
pub type Error = String;

impl Controller {
    /// Клиент по умолчанию: локальный контроллер без секрета.
    pub fn local() -> Self {
        Controller {
            addr: DEFAULT_ADDR.to_string(),
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS as u64),
            secret: None,
        }
    }

    /// Клиент по адресу и таймауту.
    pub fn with_addr(addr: &str, timeout: Duration) -> Self {
        Controller {
            addr: addr.to_string(),
            timeout,
            secret: None,
        }
    }

    /// Задать секрет контроллера.
    pub fn with_secret(mut self, secret: Option<String>) -> Self {
        self.secret =
            secret.filter(|value| !value.trim().is_empty() && !value.chars().any(char::is_control));
        self
    }

    /// Адрес для журнала: без секрета.
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// Есть ли секрет: нужно для сообщения об ошибке до первого запроса.
    pub fn has_secret(&self) -> bool {
        self.secret.is_some()
    }

    /// `GET` с телом ответа строкой.
    pub fn get(&self, path: &str) -> Result<String, Error> {
        let (status, body) = self.request("GET", path, None)?;
        check_status(status, &body)?;
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    /// `PUT` с JSON-телом. Ответ не читается: контроллер отвечает пустым.
    pub fn put(&self, path: &str, body: &str) -> Result<(), Error> {
        let (status, answer) = self.request("PUT", path, Some(body.to_string()))?;
        check_status(status, &answer)?;
        Ok(())
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<String>,
    ) -> Result<(u16, Vec<u8>), Error> {
        validate_path(path)?;
        let stream = self.connect()?;
        let mut stream = stream;
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\nConnection: close\r\n{auth}{extra}",
            host = self.addr,
            auth = match &self.secret {
                Some(secret) => format!("Authorization: Bearer {secret}\r\n"),
                None => String::new(),
            },
            extra = match &body {
                Some(body) => format!(
                    "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                ),
                None => "\r\n".to_string(),
            },
        );
        // Заголовки и тело пишутся одним буфером: два вызова давали бы два
        // пакета, и сервер на другой машине мог бы ждать тело отдельно.
        let mut request = head.into_bytes();
        if let Some(body) = &body {
            request.extend_from_slice(body.as_bytes());
        }
        stream
            .write_all(&request)
            .map_err(|error| format!("запись запроса: {error}"))?;
        let mut raw = Vec::new();
        stream
            .take((MAX_RESPONSE_BYTES + 1) as u64)
            .read_to_end(&mut raw)
            .map_err(|error| format!("чтение ответа: {error}"))?;
        if raw.len() > MAX_RESPONSE_BYTES {
            return Err(format!("ответ больше {MAX_RESPONSE_BYTES} байт"));
        }
        parse_response(&raw)
    }

    fn connect(&self) -> Result<TcpStream, Error> {
        let addrs: Vec<_> = self
            .addr
            .to_socket_addrs()
            .map_err(|error| format!("адрес не разобран: {error}"))?
            .collect();
        if addrs.is_empty() {
            return Err("адрес не разобран: нет адресов".to_string());
        }
        // Контроллер локальный, а заголовок Authorization содержит токен.
        // Не отправлять его в сеть из-за ошибочной или подменённой настройки.
        if addrs.iter().any(|addr| !addr.ip().is_loopback()) {
            return Err("адрес контроллера должен быть loopback".to_string());
        }
        let mut last = String::from("адрес не отвечает");
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, self.timeout) {
                Ok(stream) => {
                    let _ = stream.set_read_timeout(Some(self.timeout));
                    let _ = stream.set_write_timeout(Some(self.timeout));
                    return Ok(stream);
                }
                Err(error) => {
                    last = if error.kind() == std::io::ErrorKind::TimedOut {
                        "таймаут".to_string()
                    } else {
                        error.to_string()
                    };
                }
            }
        }
        Err(format!("контроллер не отвечает: {last}"))
    }
}

/// Код ответа вне 2xx — ошибка с текстом тела: у Mihomo он короткий и полезный.
fn check_status(status: u16, body: &[u8]) -> Result<(), Error> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    let text = String::from_utf8_lossy(body);
    let text = text.trim();
    if text.is_empty() {
        return Err(format!("HTTP {status}"));
    }
    let mut concise: String = text.chars().take(MAX_ERROR_CHARS).collect();
    if text.chars().count() > MAX_ERROR_CHARS {
        concise.push('…');
    }
    Err(format!("HTTP {status}: {concise}"))
}

/// Разбирает сырой ответ: код, заголовки и тело.
pub fn parse_response(raw: &[u8]) -> Result<(u16, Vec<u8>), Error> {
    if raw.len() > MAX_RESPONSE_BYTES {
        return Err(format!("ответ больше {MAX_RESPONSE_BYTES} байт"));
    }
    let split = find(raw, b"\r\n\r\n").ok_or_else(|| "ответ без заголовков".to_string())?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let rest = &raw[split + 4..];
    let status = parse_status(&head)?;
    if header_value(&head, "transfer-encoding").is_some_and(|value| {
        value
            .split(',')
            .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
    }) {
        return Ok((status, decode_chunked(rest)?));
    }
    match header_value(&head, "content-length") {
        Some(value) => {
            let length = value
                .parse::<usize>()
                .map_err(|error| format!("Content-Length {value:?}: {error}"))?;
            if rest.len() < length {
                return Err(format!(
                    "ответ короче Content-Length: получено {}, ожидалось {length}",
                    rest.len()
                ));
            }
            Ok((status, rest[..length].to_vec()))
        }
        None => Ok((status, rest.to_vec())),
    }
}

/// Путь контроллера должен быть origin-form: только локальный путь, без
/// внедрения новой строки в HTTP-запрос или абсолютного URI.
fn validate_path(path: &str) -> Result<(), Error> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("некорректный путь контроллера".to_string());
    }
    Ok(())
}

/// Код из строки состояния: `HTTP/1.1 204 No Content`.
pub fn parse_status(head: &str) -> Result<u16, Error> {
    let first = head.lines().next().unwrap_or_default();
    let code = first
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| format!("не строка состояния: {first:?}"))?;
    Ok(code)
}

/// Значение заголовка по имени в любом регистре.
pub fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim().eq_ignore_ascii_case(name)).then_some(value.trim())
    })
}

/// Тело по `chunked`-кодировке. Ошибки усечения важны: молчаливый обрыв дал бы
/// окну «успех» на неполном JSON.
pub fn decode_chunked(raw: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let mut rest = raw;
    loop {
        let line_end = find(rest, b"\r\n").ok_or_else(|| "chunk: нет размера".to_string())?;
        let size_text = String::from_utf8_lossy(&rest[..line_end]).into_owned();
        let size_text = size_text.split(';').next().unwrap_or("").trim().to_string();
        let size = usize::from_str_radix(&size_text, 16)
            .map_err(|error| format!("chunk: размер {size_text:?}: {error}"))?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            if rest.starts_with(b"\r\n") {
                return Ok(out);
            }
            if find(rest, b"\r\n\r\n").is_some() {
                return Ok(out);
            }
            return Err("chunk: не завершены trailer-заголовки".to_string());
        }
        if rest.len() < size.saturating_add(2) {
            return Err("chunk: тело короче заявленного".to_string());
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size..];
        if !rest.starts_with(b"\r\n") {
            return Err("chunk: нет CRLF после данных".to_string());
        }
        rest = &rest[2..];
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Экранирует имя для пути: пробелы и не-ASCII не должны ломать запрос.
pub fn escape_path(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Путь к селектору контроллера.
pub fn selector_path(name: &str) -> String {
    format!("/proxies/{}", escape_path(name))
}

/// Тело `PUT` для смены выбора селектора.
pub fn select_body(value: &str) -> String {
    serde_json::json!({ "name": value }).to_string()
}

/// Секрет из окружения или файла: переменная `HUD_VPN_SECRET` важнее файла,
/// чтобы можно было передать её из терминала без правки конфига.
pub fn secret_from_env_or_file(path: &std::path::Path) -> Option<String> {
    if let Ok(value) = std::env::var("HUD_VPN_SECRET") {
        let value = value.trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    let text = std::fs::read_to_string(path).ok()?;
    let value = text.trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::super::vpn::DIRECT;
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parses_a_response_with_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";

        let (status, body) = parse_response(raw).unwrap();

        assert_eq!(status, 200);
        assert_eq!(body, b"{}");
    }

    #[test]
    fn parses_a_response_without_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"now\":\"A\"}";

        let (status, body) = parse_response(raw).unwrap();

        assert_eq!(status, 200);
        assert_eq!(String::from_utf8_lossy(&body), "{\"now\":\"A\"}");
    }

    #[test]
    fn rejects_a_body_shorter_than_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n{}";

        let error = parse_response(raw).unwrap_err();

        assert!(error.contains("короче Content-Length"), "{error}");
    }

    #[test]
    fn rejects_a_malformed_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: many\r\n\r\n{}";

        assert!(parse_response(raw).unwrap_err().contains("Content-Length"));
    }

    #[test]
    fn rejects_an_oversized_response_before_parsing_it() {
        let raw = vec![b'x'; MAX_RESPONSE_BYTES + 1];

        assert!(parse_response(&raw).unwrap_err().contains("больше"));
    }

    #[test]
    fn decodes_a_chunked_body() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";

        let (status, body) = parse_response(raw).unwrap();

        assert_eq!(status, 200);
        assert_eq!(String::from_utf8_lossy(&body), "hello world");
    }

    #[test]
    fn a_truncated_chunk_is_an_error_not_a_short_answer() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhel";

        let error = parse_response(raw).unwrap_err();

        assert!(error.contains("короче"), "{error}");
    }

    #[test]
    fn a_chunk_without_its_crlf_is_rejected() {
        assert!(
            decode_chunked(b"5\r\nhelloX\r\n0\r\n\r\n")
                .unwrap_err()
                .contains("CRLF")
        );
    }

    #[test]
    fn chunked_trailers_must_be_terminated() {
        assert!(
            decode_chunked(b"0\r\nX-Info: value\r\n")
                .unwrap_err()
                .contains("trailer")
        );
        assert_eq!(decode_chunked(b"0\r\nX-Info: value\r\n\r\n").unwrap(), b"");
    }

    #[test]
    fn a_broken_chunk_size_is_an_error() {
        assert!(decode_chunked(b"zz\r\nhi\r\n").is_err());
    }

    #[test]
    fn a_response_without_headers_is_an_error() {
        assert!(parse_response("просто текст".as_bytes()).is_err());
    }

    #[test]
    fn reads_the_status_code() {
        assert_eq!(parse_status("HTTP/1.1 204 No Content\r\n").unwrap(), 204);
        assert_eq!(
            parse_status("HTTP/1.1 500 Internal Server Error\r\n").unwrap(),
            500
        );
    }

    #[test]
    fn a_head_without_a_code_is_an_error() {
        assert!(parse_status("HTTP\r\n").is_err());
    }

    #[test]
    fn header_lookup_ignores_case() {
        let head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n";

        assert_eq!(header_value(head, "content-type"), Some("application/json"));
        assert_eq!(header_value(head, "CONTENT-TYPE"), Some("application/json"));
        assert_eq!(header_value(head, "content-length"), None);
    }

    #[test]
    fn non_2xx_becomes_an_error_with_the_body() {
        let error = check_status(401, b"{\"message\":\"Unauthorized\"}").unwrap_err();

        assert!(error.contains("401"), "{error}");
        assert!(error.contains("Unauthorized"), "{error}");
    }

    #[test]
    fn non_2xx_without_a_body_stays_readable() {
        assert_eq!(check_status(503, b"  ").unwrap_err(), "HTTP 503");
    }

    #[test]
    fn non_2xx_error_bodies_are_limited_for_the_hud() {
        let body = "x".repeat(MAX_ERROR_CHARS + 500);

        let error = check_status(500, body.as_bytes()).unwrap_err();

        assert!(error.ends_with('…'));
        assert!(error.chars().count() <= MAX_ERROR_CHARS + "HTTP 500: ".len() + 1);
    }

    #[test]
    fn escapes_unsafe_characters_in_a_path() {
        assert_eq!(escape_path("MODE-RU"), "MODE-RU");
        assert_eq!(
            escape_path("ПРОКСИ"),
            "%D0%9F%D0%A0%D0%9E%D0%9A%D0%A1%D0%98"
        );
        assert_eq!(
            escape_path("🇫🇷 x"),
            "%F0%9F%87%AB%F0%9F%87%B7%20x",
            "пробел должен экранироваться, а не разделять путь"
        );
    }

    #[test]
    fn builds_the_selector_path_and_body() {
        assert_eq!(selector_path("MODE-REST"), "/proxies/MODE-REST");
        assert_eq!(select_body("DIRECT"), "{\"name\":\"DIRECT\"}");
    }

    #[test]
    fn the_debug_of_a_client_hides_the_secret() {
        let client = Controller::local().with_secret(Some("hunter2".to_string()));
        let text = format!("{client:?}");

        assert!(!text.contains("hunter2"), "{text}");
        assert!(text.contains("<redacted>"), "{text}");
    }

    #[test]
    fn a_blank_secret_is_treated_as_absent() {
        let client = Controller::local().with_secret(Some("   ".to_string()));

        assert!(!client.has_secret());
    }

    #[test]
    fn a_secret_with_header_controls_is_rejected() {
        let client = Controller::local().with_secret(Some("token\r\nX-Evil: yes".to_string()));

        assert!(!client.has_secret());
    }

    #[test]
    fn non_loopback_controller_addresses_are_rejected_before_connecting() {
        let client = Controller::with_addr("192.0.2.10:9090", Duration::from_millis(200));

        let error = client.get("/version").unwrap_err();

        assert!(error.contains("loopback"), "{error}");
    }

    #[test]
    fn request_paths_cannot_inject_headers_or_absolute_uris() {
        let client = Controller::local();

        for path in [
            "/version\r\nAuthorization: Bearer leaked",
            "//example.test/path",
        ] {
            assert!(client.get(path).unwrap_err().contains("путь"), "{path:?}");
        }
    }

    #[test]
    fn a_dead_port_is_reported_as_an_unreachable_controller() {
        // Порт из `/dev/null`: соединение отклоняется сразу, без ожидания.
        let client = Controller::with_addr("127.0.0.1:1", Duration::from_millis(200));

        let error = client.get("/version").unwrap_err();

        assert!(error.contains("не отвечает"), "{error}");
    }

    #[test]
    fn talks_to_a_real_local_server() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("порт");
        let addr = listener.local_addr().expect("адрес");
        let answer = b"HTTP/1.1 200 OK\r\nContent-Length: 33\r\n\r\n{\"meta\":true,\"version\":\"v1.19.31\"}";
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("клиент");
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            stream.write_all(answer).expect("ответ");
        });
        let client = Controller::with_addr(&addr.to_string(), Duration::from_millis(1000))
            .with_secret(Some("s3cret".to_string()));

        let text = client.get("/version").expect("ответ API");

        assert!(text.contains("v1.19.31"), "{text}");
        handle.join().ok();
    }

    #[test]
    fn put_sends_the_body_and_accepts_an_empty_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("порт");
        let addr = listener.local_addr().expect("адрес");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("клиент");
            let mut buffer = [0u8; 2048];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            assert!(request.starts_with("PUT /proxies/MODE-RU"), "{request}");
            assert!(request.contains("{\"name\":\"DIRECT\"}"), "{request}");
            let expected = select_body(DIRECT).len();
            assert!(
                request.contains(&format!("Content-Length: {expected}")),
                "{request}"
            );
            stream.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").ok();
        });
        let client = Controller::with_addr(&addr.to_string(), Duration::from_millis(1000));

        client
            .put("/proxies/MODE-RU", &select_body(DIRECT))
            .expect("смена выбора");

        handle.join().ok();
    }
}

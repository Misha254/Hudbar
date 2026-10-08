//! Mini App: маленькая панель opencode, которая открывается прямо в Telegram.
//!
//! Зачем при боте с кнопками: список моделей у этой машины на 42 строки, и
//! набирать номер пальцем по кнопкам — мучение. Веб-панель даёт нормальный
//! список с поиском и поле ввода промпта без `/`-команд.
//!
//! ## Почему нужен туннель
//!
//! Mini App грузится только по валидному HTTPS, а opencode слушает
//! `127.0.0.1` без TLS: с телефона до него напрямую не достучаться. Вход —
//! `tailscale serve`, адрес вида `https://<узел>.<tailnet>.ts.net` с сертификатом,
//! который выпускает сам Tailscale. Слушает мост при этом только localhost:
//! наружу выставлен туннель, и второй путь к агенту не появляется.
//!
//! ## Чем защищено
//!
//! Адрес и так доступен только устройствам tailnet, но токен остаётся: в
//! tailnet может случайно оказаться чужое устройство, а панель даёт промпты
//! агенту. Токен проверяется в каждом запросе кроме самой страницы, сравнением
//! за постоянное время, — чтобы по задержке ответа нельзя было подбирать его
//! по байтам. Страница отдаётся без токена и бесполезна: все данные за `/api`.
//!
//! Токен живёт во фрагменте адреса (`#t=…`), а фрагмент браузер серверу не
//! отправляет — в логах туннеля его нет, в отличие от query-параметра.
//!
//! ## Чего сервер не умеет
//!
//! Никакого доступа к файлам и никаких произвольных команд: промпт уходит в
//! сессию тем же путём, что и с телефона в боте. Перебор папок, чтение
//! `/etc` и запуск шелл-команд через панель невозможны — эндпоинтов для этого
//! просто нет.

use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::opencode::{self, Client};

/// Заголовок с токеном. Имя своё, а не `Authorization`, чтобы случайный
/// прокси не перехватил его своей схемой Basic/Bearer.
pub const TOKEN_HEADER: &str = "x-hudbar-token";

/// Имя параметра во фрагменте адреса: страница достаёт токен сама и кладёт в
/// заголовок.
pub const TOKEN_FRAGMENT: &str = "t";

/// Таймаут чтения запроса и записи ответа. Запросы мелкие, а висящий клиент
/// не должен держать поток.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Максимальный размер тела запроса: промпт с телефонной клавиатуры короткий,
/// а мегабайтный POST — это уже не человек.
const MAX_BODY: usize = 64 * 1024;

/// Потолок ответа: страница и списки моделей умещаются в сотни килобайт.
const MAX_RESPONSE: usize = 4 * 1024 * 1024;

/// Что ответил сервер.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    /// Код ответа.
    pub status: u16,
    /// Заголовки: имя → значение.
    pub headers: Vec<(String, String)>,
    /// Тело: страница или JSON.
    pub body: Vec<u8>,
    /// Отправлять ли тело частями по событиям (SSE).
    pub stream: bool,
}

impl Reply {
    /// JSON-ответ с кодом.
    pub fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            headers: vec![(
                "Content-Type".to_string(),
                "application/json; charset=utf-8".to_string(),
            )],
            body: value.to_string().into_bytes(),
            stream: false,
        }
    }

    /// Ошибка с коротким текстом.
    pub fn error(status: u16, text: &str) -> Self {
        Self::json(status, &json!({ "error": text }))
    }

    /// HTML-страница.
    pub fn html(body: &str) -> Self {
        Self {
            status: 200,
            headers: vec![(
                "Content-Type".to_string(),
                "text/html; charset=utf-8".to_string(),
            )],
            body: body.as_bytes().to_vec(),
            stream: false,
        }
    }

    /// Поток событий: страница держит соединение открытым и читает `data:`.
    pub fn events() -> Self {
        Self {
            status: 200,
            headers: vec![
                (
                    "Content-Type".to_string(),
                    "text/event-stream; charset=utf-8".to_string(),
                ),
                ("Cache-Control".to_string(), "no-store".to_string()),
            ],
            body: Vec::new(),
            stream: true,
        }
    }
}

/// Разобранный запрос: путь, метод, заголовки, тело.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    /// Метод: `GET`, `POST`.
    pub method: String,
    /// Путь без query-строки.
    pub path: String,
    /// Значение query-строки.
    pub query: String,
    /// Заголовки в нижнем регистре имён.
    pub headers: Vec<(String, String)>,
    /// Тело запроса.
    pub body: String,
}

impl Request {
    /// Заголовок по имени: регистр не важен.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Тело как JSON: мусор — ошибка, а не пустое тело.
    pub fn json(&self) -> Result<Value, String> {
        if self.body.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&self.body).map_err(|why| format!("тело не json: {why}"))
    }

    /// Поле строки из тела: пустое и отсутствующее дают `None`.
    pub fn field(&self, name: &str) -> Option<String> {
        self.json()
            .ok()?
            .get(name)
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    /// Поле числа из тела: `0` и мусор дают `None`.
    pub fn number(&self, name: &str) -> Option<usize> {
        self.json()
            .ok()?
            .get(name)
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
    }

    /// Построитель для тестов: запрос без сети.
    pub fn test_get(path: &str, token: Option<&str>) -> Self {
        Self {
            method: "GET".to_string(),
            path: path.to_string(),
            query: String::new(),
            headers: token
                .map(|token| vec![(TOKEN_HEADER.to_string(), token.to_string())])
                .unwrap_or_default(),
            body: String::new(),
        }
    }

    /// Построитель для тестов: POST с JSON-телом.
    pub fn test_post(path: &str, token: Option<&str>, body: &str) -> Self {
        Self {
            method: "POST".to_string(),
            path: path.to_string(),
            query: String::new(),
            headers: token
                .map(|token| vec![(TOKEN_HEADER.to_string(), token.to_string())])
                .unwrap_or_default(),
            body: body.to_string(),
        }
    }
}

/// Сравнение строк за постоянное время. Ранний выход по первому различию
/// превращает проверку токена в оракул по времени ответа: замеряя, как долго
/// отвечает сервер, можно подбирать токен по байту за байтом. Здесь сравнение
/// идёт по всей длине всегда.
pub fn tokens_match(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    // Разные длины тоже надо сравнить целиком: ранний выход по длине —
    // такой же оракул. Поэтому сравниваем максимум и учитываем разницу.
    let length = left.len().max(right.len());
    let mut difference = left.len() ^ right.len();
    for index in 0..length {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(a ^ b);
    }
    difference == 0
}

/// Разбор сырого HTTP-запроса из сокета. Возвращает `None`, если это не
/// запрос вовсе: на публичном адресе любой может прислать мусор.
pub fn parse_request(raw: &str) -> Option<Request> {
    let mut lines = raw.split("\r\n").flat_map(|line| line.split('\n'));
    let start = lines.next()?;
    let mut parts = start.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?;
    if method.is_empty() || !target.starts_with('/') {
        return None;
    }
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (target.to_string(), String::new()),
    };
    let mut headers = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_lowercase(), value.trim().to_string()));
        }
    }
    Some(Request {
        method,
        path,
        query,
        headers,
        body: String::new(),
    })
}

/// Читает запрос из сокета целиком: заголовки плюс тело по `Content-Length`.
pub fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|why| format!("таймаут не задался: {why}"))?;
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|why| format!("сокет не клонирован: {why}"))?,
    );
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        let read = read_line(&mut reader, &mut line)?;
        if read == 0 {
            return Err("запрос оборван".to_string());
        }
        let done = line == b"\r\n" || line == b"\n";
        head.extend_from_slice(&line);
        if done {
            break;
        }
        if head.len() > 16 * 1024 {
            return Err("заголовки длиннее 16 КиБ".to_string());
        }
    }
    let mut request = parse_request(&String::from_utf8_lossy(&head))
        .ok_or_else(|| "это не HTTP-запрос".to_string())?;
    let length: usize = request
        .header("content-length")
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return Err(format!("тело длиннее {MAX_BODY} байт"));
    }
    if length > 0 {
        let mut body = vec![0u8; length];
        reader
            .read_exact(&mut body)
            .map_err(|why| format!("тело не прочитано: {why}"))?;
        request.body = String::from_utf8_lossy(&body).to_string();
    }
    Ok(request)
}

/// Строка из BufReader вместе с `\n`, как `read_line`, но с потолком: ответ
/// мусора не должен съесть память.
fn read_line(reader: &mut BufReader<TcpStream>, out: &mut Vec<u8>) -> Result<usize, String> {
    let mut total = 0usize;
    loop {
        let mut byte = [0u8; 1];
        match reader.read(&mut byte) {
            Ok(0) => return Ok(total),
            Ok(_) => {
                total += 1;
                out.push(byte[0]);
                if byte[0] == b'\n' {
                    return Ok(total);
                }
                if out.len() > MAX_RESPONSE {
                    return Err("строка запроса слишком длинная".to_string());
                }
            }
            Err(why) => return Err(format!("чтение: {why}")),
        }
    }
}

/// Заголовки ответа: минимальный набор плюс длина. Без `Connection: close`
/// браузер телефона будет ждать конца потока.
pub fn response_head(reply: &Reply, keep_alive: bool) -> String {
    let mut head = format!("HTTP/1.1 {} {}\r\n", reply.status, reason(reply.status));
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    // У потока событий длины нет: он не кончается, и `Content-Length: 0`
    // заставил бы клиента считать ответ завершённым на заголовках.
    if !reply.stream {
        head.push_str(&format!("Content-Length: {}\r\n", reply.body.len()));
    }
    head.push_str(&format!(
        "Connection: {}\r\n",
        if keep_alive { "keep-alive" } else { "close" }
    ));
    // Никакого кеша: страница меняется вместе с состоянием, а токен в
    // заголовке не должен осесть в кеше браузера телефона.
    head.push_str("Cache-Control: no-store\r\n");
    head.push_str("X-Content-Type-Options: nosniff\r\n");
    head.push_str("Referrer-Policy: no-referrer\r\n");
    head.push_str("\r\n");
    head
}

/// Текст кода ответа: клиенту незачем знать больше.
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

/// Тело API в JSON: `{ok: bool, ...}` либо `{error: строка}`.
pub fn ok_json(value: Value) -> Reply {
    let mut object = json!({ "ok": true });
    if let Some(map) = value.as_object() {
        for (key, item) in map {
            object[key] = item.clone();
        }
    }
    Reply::json(200, &object)
}

/// Ошибка API: тот же JSON с `error`, чтобы страница читала одно поле.
pub fn fail_json(status: u16, text: &str) -> Reply {
    Reply::json(status, &json!({ "ok": false, "error": text }))
}

/// Сессия и её заголовок для панели.
pub fn session_json(session: &opencode::Session) -> Value {
    json!({
        "id": session.id,
        "title": session.title,
        "directory": session.directory,
    })
}

/// Модель для панели: id, имя и провайдер, чтобы страница знала, что показать
/// в подписи и что положить в запрос переключения.
pub fn model_json(model: &opencode::Model) -> Value {
    json!({
        "id": model.reference.id,
        "provider": model.reference.provider,
        "name": model.name,
        "label": model.label(),
        "full": model.reference.full_id(),
    })
}

/// Пустая сессия-заглушка: нужна ответам, где сессии нет вовсе.
pub fn no_session() -> Value {
    json!({ "id": Value::Null, "title": "", "directory": "" })
}

/// Текст промпта из запроса: обрезанный и с отброшенным хвостом мусора.
/// Пустой промпт выполнять нечего, поэтому это ошибка, а не тишина.
pub fn prompt_from(request: &Request) -> Result<String, String> {
    let text = request
        .field("prompt")
        .ok_or_else(|| "пустой промпт".to_string())?;
    Ok(text)
}

/// Состояние моста под мьютексом: то же, чем пользуется бот.
pub type StateShared = std::sync::Arc<std::sync::Mutex<super::commands::State>>;

/// Клиент opencode под мьютексом: то же, чем пользуется бот.
pub type ClientShared = std::sync::Arc<std::sync::Mutex<Client>>;

/// Слушающий сокет на localhost: наружу его выставляет туннель, и сам мост
/// в интернет не выходит ни одной строкой.
pub fn bind_local(port: u16) -> Result<TcpListener, String> {
    let address = format!("127.0.0.1:{port}");
    TcpListener::bind(&address).map_err(|why| format!("слушатель {address}: {why}"))
}

pub fn serve(port: u16, token: Option<String>, client: ClientShared, state: StateShared) {
    let listener = match bind_local(port) {
        Ok(listener) => listener,
        Err(why) => {
            super::log::warn(format!("панель: сервер не поднялся: {why}"));
            return;
        }
    };
    super::log::warn(format!("панель: слушаю http://127.0.0.1:{port}"));
    for incoming in listener.incoming() {
        let Ok(mut stream) = incoming else {
            continue;
        };
        let request = match read_request(&mut stream) {
            Ok(request) => request,
            Err(_) => {
                // Мусор или оборванное соединение: отвечаем коротко и идём
                // дальше, иначе один негодяй заблокирует панель.
                let _ = stream.write_all(
                    response_head(&Reply::error(400, "плохой запрос"), false).as_bytes(),
                );
                continue;
            }
        };
        let deps = super::mini_server::Deps {
            client: &client,
            state: &state,
            token: token.as_deref(),
        };
        let reply = super::mini_server::route(&deps, &request);
        if reply.stream {
            stream_events(&mut stream, &deps);
            continue;
        }
        let head = response_head(&reply, false);
        if stream.write_all(head.as_bytes()).is_err() {
            continue;
        }
        let _ = stream.write_all(&reply.body);
        let _ = stream.flush();
    }
}

/// Поток событий: раз в несколько секунд шлёт состояние сессии, пока соединение
/// живо. Клиент читает `data:` и обновляет шапку — так часы не требуют опроса.
fn stream_events(stream: &mut TcpStream, deps: &super::mini_server::Deps<'_>) {
    let head = response_head(&Reply::events(), false);
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    loop {
        let payload = match deps.client.lock() {
            Ok(client) => {
                let state = super::mini_server::state_payload(deps, &client);
                format!("data: {state}\n\n")
            }
            Err(_) => "data: {\"ok\":false}\n\n".to_string(),
        };
        if stream.write_all(payload.as_bytes()).is_err() {
            return;
        }
        if stream.flush().is_err() {
            return;
        }
        thread::sleep(Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_parsed_from_raw_http() {
        let raw = "GET /api/sessions?x=1 HTTP/1.1\r\nHost: h\r\nX-Hudbar-Token: секрет\r\n\r\n";
        let request = parse_request(raw).expect("разбор");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/sessions");
        assert_eq!(request.query, "x=1");
        assert_eq!(request.header(TOKEN_HEADER), Some("секрет"));
        assert_eq!(
            request.header("x-hudbar-token"),
            Some("секрет"),
            "регистр не важен"
        );
    }

    #[test]
    fn header_names_are_lowercased() {
        let raw = "GET / HTTP/1.1\r\nCONTENT-TYPE: text/plain\r\n\r\n";
        let request = parse_request(raw).expect("разбор");
        assert_eq!(request.header("Content-Type"), Some("text/plain"));
    }

    #[test]
    fn junk_is_not_a_request() {
        assert_eq!(parse_request("привет"), None);
        assert_eq!(parse_request("GET\r\nHost: h\r\n\r\n"), None, "без пути");
        assert_eq!(
            parse_request("GET ftp://x HTTP/1.1\r\n\r\n"),
            None,
            "не путь"
        );
        assert_eq!(parse_request(""), None);
    }

    /// Токен проверяется целиком: и по содержимому, и по длине. Ранний выход
    /// по первому различию или по длине — оракул по времени ответа.
    #[test]
    fn token_comparison_is_exact() {
        assert!(tokens_match("секрет", "секрет"), "тот же токен");
        assert!(!tokens_match("секрет", "секрет2"), "длиннее");
        assert!(!tokens_match("секрет2", "секрет"), "короче");
        assert!(!tokens_match("", "секрет"), "пустой не подходит");
        assert!(!tokens_match("секрет", ""), "и наоборот");
        assert!(
            tokens_match("", ""),
            "два пустых совпадают, но это не токен"
        );
        assert!(!tokens_match("abc", "abd"), "различие в последнем байте");
    }

    #[test]
    fn json_fields_are_read_leniently_but_reject_empty() {
        let request = Request::test_post("/api/prompt", None, r#"{"prompt":"  привет  "}"#);
        assert_eq!(prompt_from(&request).as_deref(), Ok("привет"));
        let request = Request::test_post("/api/prompt", None, r#"{"prompt":"   "}"#);
        assert!(prompt_from(&request).is_err(), "пустой промпт — ошибка");
        let request = Request::test_post("/api/prompt", None, "{ не json");
        assert!(
            prompt_from(&request).is_err(),
            "мусор — ошибка, а не пустой промпт"
        );
        let request = Request::test_post("/api/prompt", None, "");
        assert!(prompt_from(&request).is_err(), "пустое тело");
    }

    #[test]
    fn numbers_reject_zero_and_junk() {
        let request = Request::test_post("/api/model", None, r#"{"index":3}"#);
        assert_eq!(request.number("index"), Some(3));
        let request = Request::test_post("/api/model", None, r#"{"index":0}"#);
        assert_eq!(request.number("index"), None, "нулевого списка нет");
        let request = Request::test_post("/api/model", None, r#"{"index":"3"}"#);
        assert_eq!(request.number("index"), None, "строка вместо числа");
        assert_eq!(request.number("нет"), None);
    }

    #[test]
    fn response_head_carries_length_and_no_cache() {
        let reply = Reply::html("<html></html>");
        let head = response_head(&reply, false);
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{head}");
        assert!(head.contains("Content-Type: text/html"), "{head}");
        assert!(
            head.contains(&format!("Content-Length: {}", reply.body.len())),
            "{head}"
        );
        assert!(head.contains("Connection: close"), "{head}");
        assert!(
            head.contains("Cache-Control: no-store"),
            "кеш выключен: {head}"
        );
        assert!(head.contains("X-Content-Type-Options: nosniff"), "{head}");
        assert!(head.ends_with("\r\n\r\n"), "пустая строка в конце: {head}");
    }

    #[test]
    fn errors_carry_one_field_for_the_page() {
        let reply = fail_json(401, "нет токена");
        assert_eq!(reply.status, 401);
        let value: Value = serde_json::from_slice(&reply.body).expect("json");
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"], "нет токена");
    }

    #[test]
    fn ok_json_keeps_ok_true() {
        let value =
            serde_json::from_slice::<Value>(&ok_json(json!({ "busy": true })).body).expect("json");
        assert_eq!(value["ok"], true);
        assert_eq!(value["busy"], true);
    }

    #[test]
    fn events_reply_has_no_length_and_is_streamed() {
        let reply = Reply::events();
        assert!(reply.stream);
        assert!(reply.body.is_empty(), "тело потока не в заголовке");
        let head = response_head(&reply, false);
        assert!(head.contains("text/event-stream"), "{head}");
        assert!(
            !head.contains("Content-Length"),
            "у потока событий длины нет: {head}"
        );
    }

    #[test]
    fn listener_binds_localhost_only() {
        // Порт выбирает система, поэтому проверяем именно адрес: сервер не
        // должен слушать наружу даже случайно.
        let listener = bind_local(0).expect("слушатель");
        let address = listener.local_addr().expect("адрес");
        assert!(
            address.ip().is_loopback(),
            "слушаем только localhost: {address}"
        );
    }
}

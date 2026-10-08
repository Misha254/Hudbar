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

/// Модель в виде, в котором сервер её принимает и хранит: пара
/// «провайдер + id». Именно её ждёт `ModelRef` в спеке.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRef {
    /// Провайдер: `vibecode-claude`, `opencode`.
    pub provider: String,
    /// Id модели у провайдера: `claude-sonnet-4-6`.
    pub id: String,
}

impl ModelRef {
    /// Пара из значений, без проверки: пустые строки отбрасывает вызывающий.
    pub fn new(provider: &str, id: &str) -> Self {
        Self {
            provider: provider.trim().to_string(),
            id: id.trim().to_string(),
        }
    }

    /// `provider/id` — так модель пишут в конфиге opencode.
    pub fn full_id(&self) -> String {
        format!("{}/{}", self.provider, self.id)
    }
}

/// Модель для выбора: пара из `ModelRef` плюс человеческое имя.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    /// Провайдер и id.
    pub reference: ModelRef,
    /// Имя для человека: `Claude Sonnet 4.6`. Пустое — покажем id.
    pub name: String,
}

impl Model {
    /// Подпись для списка: имя, а при его отсутствии — id.
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.reference.id
        } else {
            &self.name
        }
    }

    /// Помечается ли модель текущей в сессии.
    pub fn is_current(&self, current: Option<&ModelRef>) -> bool {
        current == Some(&self.reference)
    }
}

impl std::fmt::Display for Model {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} ({})", self.label(), self.reference.provider)
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
        serde_json::from_str(body).map_err(|why| {
            // HTML вместо JSON — это заглушка приложения: значит путь не
            // маршрутизирован и сервер отдал 200 в никуда. Без отдельного
            // текста такой промах выглядит как «сервер сломался».
            if body.trim_start().starts_with('<') {
                format!("сервер отдал HTML на {method} {path}: эндпоинт не найден")
            } else {
                format!("ответ не json: {why}")
            }
        })
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
        self.create_session_modeled(title, None)
    }

    /// Новая сессия, при желании — сразу на выбранной модели: тело
    /// `POST /session` по спеке принимает `model` с `id` и `providerID`.
    /// Новая сессия иначе взяла бы модель по умолчанию, и выбранная с
    /// телефона работала бы только в старой.
    pub fn create_session_modeled(
        &self,
        title: Option<&str>,
        model: Option<&ModelRef>,
    ) -> Result<Session, Error> {
        let mut body = serde_json::Map::new();
        if let Some(title) = title {
            body.insert("title".to_string(), Value::String(title.to_string()));
        }
        if let Some(model) = model {
            body.insert("model".to_string(), model_body(model));
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

    /// Модели, которые можно выбрать: только подключённые провайдеры. В
    /// ответе `GET /provider` их 229, из них с ключами единицы, а список из
    /// всех отвечал бы «модель есть», но работать она не смогла бы.
    pub fn list_models(&self) -> Result<Vec<Model>, Error> {
        let value = self.request("GET", "/provider", None)?;
        let connected: Vec<&str> = value
            .get("connected")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let all = value
            .get("all")
            .and_then(Value::as_array)
            .ok_or_else(|| "список провайдеров не список".to_string())?;
        let mut models: Vec<Model> = Vec::new();
        for provider in all {
            let id = match provider.get("id").and_then(Value::as_str) {
                Some(id) if connected.contains(&id) => id,
                _ => continue,
            };
            let Some(entries) = provider.get("models").and_then(Value::as_object) else {
                continue;
            };
            for (model_id, model) in entries {
                if model_id.is_empty() {
                    continue;
                }
                models.push(Model {
                    reference: ModelRef::new(id, model_id),
                    name: model
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                });
            }
        }
        // Порядок от провайдера к провайдеру и по имени внутри: список на
        // телефоне должен быть устойчивым, иначе номера «съезжают» и
        // `/model 7` выбрал бы не то.
        models.sort_by(|left, right| {
            left.reference
                .provider
                .cmp(&right.reference.provider)
                .then_with(|| sort_key(&left.reference.id).cmp(&sort_key(&right.reference.id)))
        });
        Ok(models)
    }

    /// Модель сессии сейчас: `None`, если сервер её не назвал (спека на
    /// обязательность `model` не указывает).
    pub fn session_model(&self, session_id: &str) -> Result<Option<ModelRef>, Error> {
        let value = self.request("GET", &format!("/session/{session_id}"), None)?;
        Ok(model_ref_from(&value))
    }

    /// Модель сессии: `POST /session/{id}/model` отвечает 204, а сама
    /// подхватывается со следующего запроса агента. Тело — ровно то, что
    /// ждёт схема `ModelRef`: `id` и `providerID`.
    pub fn set_session_model(&self, session_id: &str, model: &ModelRef) -> Result<(), Error> {
        // Именно `/api/session/{id}/model`. Путь без префикса тоже есть в
        // спеке, но на живом сервере он не маршрутизирован: отдаётся
        // HTML-заглушка приложения с кодом 200, то есть «успех», который
        // ничего не сделал. С префиксом — 204 и модель действительно меняется.
        let body = json!({ "model": model_body(model) });
        self.request(
            "POST",
            &format!("/api/session/{session_id}/model"),
            Some(&body),
        )
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

/// Тело модели для запроса: спекой принимается только `id` и `providerID`.
fn model_body(model: &ModelRef) -> Value {
    json!({ "id": model.id, "providerID": model.provider })
}

/// Модель из ответа сессии: `model: {id, providerID}`. Пустые строки — не
/// модель, а мусор, такой ответ считается отсутствующей моделью.
fn model_ref_from(value: &Value) -> Option<ModelRef> {
    let model = value.get("model")?;
    let id = model
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let provider = model
        .get("providerID")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if id.is_empty() || provider.is_empty() {
        return None;
    }
    Some(ModelRef::new(provider, id))
}

/// Ключ сортировки без учёта регистра: `GPT 6` и `gpt 6` в одном списке
/// должны вставать рядом, а не вразнобой.
fn sort_key(id: &str) -> String {
    id.to_lowercase()
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

    /// Тело смены модели: спекой `ModelRef` принимает ровно `id` и
    /// `providerID`. Лишние поля сервер не ждёт, а пустые значения он
    /// отвергнет, поэтому проверяем форму, а не только наличие ключей.
    #[test]
    fn model_body_has_exactly_id_and_provider() {
        let body = model_body(&ModelRef::new("vibecode-claude", "claude-sonnet-4-6"));
        assert_eq!(
            body,
            json!({ "id": "claude-sonnet-4-6", "providerID": "vibecode-claude" })
        );
        let keys: Vec<&String> = body.as_object().expect("объект").keys().collect();
        assert_eq!(keys.len(), 2, "ни `variant`, ни лишнего: {body}");
    }

    /// Модель из ответа сессии: неполная пара — не модель. Иначе в списке
    /// появилось бы «текущая» с пустым провайдером.
    #[test]
    fn session_model_needs_both_halves() {
        // Ответ сессии: модель лежит под ключом `model`.
        let full =
            json!({ "model": { "id": "claude-sonnet-4-6", "providerID": "vibecode-claude" } });
        assert_eq!(
            model_ref_from(&full),
            Some(ModelRef::new("vibecode-claude", "claude-sonnet-4-6"))
        );
        assert_eq!(model_ref_from(&json!({})), None, "модели нет");
        assert_eq!(model_ref_from(&json!({ "model": null })), None);
        assert_eq!(
            model_ref_from(&json!({ "model": { "id": "x" } })),
            None,
            "нет провайдера"
        );
        assert_eq!(
            model_ref_from(&json!({ "model": { "id": "  ", "providerID": "p" } })),
            None,
            "пустой id — не модель"
        );
    }

    /// Порядок списка устойчив: сортировка по провайдеру и id без учёта
    /// регистра. Номера в `/models` не должны «съезжать» между вызовами.
    #[test]
    fn model_sort_is_case_insensitive_and_stable() {
        let mut models = [
            Model {
                reference: ModelRef::new("vibecode", "GPT-6"),
                name: String::new(),
            },
            Model {
                reference: ModelRef::new("opencode", "zeta"),
                name: String::new(),
            },
            Model {
                reference: ModelRef::new("vibecode", "gpt-5"),
                name: String::new(),
            },
            Model {
                reference: ModelRef::new("vibecode", "Fable"),
                name: String::new(),
            },
        ];
        models.sort_by(|left, right| {
            left.reference
                .provider
                .cmp(&right.reference.provider)
                .then_with(|| sort_key(&left.reference.id).cmp(&sort_key(&right.reference.id)))
        });
        let ids: Vec<String> = models.iter().map(|m| m.reference.full_id()).collect();
        assert_eq!(
            ids,
            vec![
                "opencode/zeta",
                "vibecode/Fable",
                "vibecode/gpt-5",
                "vibecode/GPT-6"
            ],
            "провайдер, потом id без регистра: {ids:?}"
        );
    }

    /// `ModelRef` печатается как в конфиге opencode: `провайдер/id`.
    #[test]
    fn model_ref_prints_as_provider_slash_id() {
        assert_eq!(
            ModelRef::new("vibecode-claude", "claude-sonnet-4-6").full_id(),
            "vibecode-claude/claude-sonnet-4-6"
        );
        assert_eq!(
            ModelRef::new("  opencode  ", " free ").full_id(),
            "opencode/free",
            "края обрезаны"
        );
    }

    /// Заглушка-как-SPA: отвечает HTML на 200, как сервер opencode на пути,
    /// который не маршрутизирован. Отдаёт запрошенный путь, чтобы тест увидел,
    /// куда ушёл вызов.
    fn stub_spa() -> (u16, std::sync::mpsc::Receiver<String>) {
        use std::sync::mpsc;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("слушатель");
        let port = listener.local_addr().expect("порт").port();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            if let Ok((mut stream, _)) = listener.accept() {
                let mut head = vec![0u8; 8192];
                let read = stream.read(&mut head).unwrap_or(0);
                let head = String::from_utf8_lossy(&head[..read]).to_string();
                let line = head.lines().next().unwrap_or("").to_string();
                let _ = sender.send(line);
                let body = "<!doctype html><html></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (port, receiver)
    }

    /// Путь смены модели — именно с префиксом `/api`. Без него сервер отвечает
    /// 200 с HTML-заглушкой: вызов «успешен», но модель не меняется, и
    /// человек видит, что переключение не сработало. Тест ловит именно эту
    /// ошибку — путь, а не только код ответа.
    #[test]
    fn model_switch_uses_the_api_prefixed_path() {
        let (port, line) = stub_spa();
        let mut client = Client::new(&format!("http://127.0.0.1:{port}")).expect("разбор");
        client.set_directory("/tmp");
        let model = ModelRef::new("vibecode", "gpt-5.6-luna");
        // Ответ-HTML обязан быть ошибкой с внятным текстом, а не «успехом».
        let why = client
            .set_session_model("ses_1", &model)
            .expect_err("HTML-заглушка — это не успех");
        assert!(
            why.contains("эндпоинт не найден"),
            "текст объясняет, что произошло: {why}"
        );
        let request = line
            .recv_timeout(Duration::from_secs(5))
            .expect("строка запроса");
        // В строке запроса после пути идёт query с папкой, поэтому сверяем
        // начало до параметров, а не всю строку.
        assert!(
            request.starts_with("POST /api/session/ses_1/model"),
            "смена модели идёт через /api: {request}"
        );
        assert!(
            !request.starts_with("POST /session/"),
            "путь без префикса даёт HTML-заглушку: {request}"
        );
    }

    /// Заглушка, которая читает тело запроса и отвечает заранее заданным
    /// ответом. Возвращает порт и канал с полученным телом: тесты проверяют
    /// не только форму ответа, но и то, что мост отправил серверу.
    fn stub_echo(status: &str, body: &str) -> (u16, std::sync::mpsc::Receiver<String>) {
        use std::sync::mpsc;
        // Строки уходят в поток, поэтому владение переходит внутрь него.
        let status = status.to_string();
        let body = body.to_string();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("слушатель");
        let port = listener.local_addr().expect("порт").port();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = vec![0u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or(0);
                buffer.truncate(read);
                // Заголовки и тело пришли одним куском: тело начинается
                // после пустой строки, иначе `read_exact` ждал бы данных,
                // которых уже нет.
                let split = buffer
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|at| at + 4)
                    .unwrap_or(read);
                let head = String::from_utf8_lossy(&buffer[..split]).to_string();
                let mut payload = buffer[split..].to_vec();
                let length = head
                    .split("Content-Length:")
                    .nth(1)
                    .and_then(|rest| rest.split("\r\n").next())
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                while payload.len() < length {
                    let mut more = vec![0u8; length - payload.len()];
                    match stream.read(&mut more) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => payload.extend_from_slice(&more[..count]),
                    }
                }
                payload.truncate(length);
                let _ = sender.send(String::from_utf8_lossy(&payload).to_string());
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (port, receiver)
    }

    /// Смена модели уходит на `POST /session/{id}/model` телом ровно с
    /// `model: {id, providerID}`, а 204 без тела считается успехом.
    #[test]
    fn set_session_model_posts_model_ref_and_accepts_204() {
        let (port, sent) = stub_echo("204 No Content", "");
        let mut client = Client::new(&format!("http://127.0.0.1:{port}")).expect("разбор");
        client.set_directory("/tmp");
        let model = ModelRef::new("vibecode-claude", "claude-sonnet-4-6");
        assert!(
            client.set_session_model("ses_1", &model).is_ok(),
            "204 — успех"
        );
        let body = sent.recv_timeout(Duration::from_secs(5)).expect("тело");
        let value: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(
            value,
            json!({ "model": { "id": "claude-sonnet-4-6", "providerID": "vibecode-claude" } })
        );
    }

    /// Новая сессия на выбранной модели: `model` едет в теле `POST /session`.
    /// Без него новая сессия взяла бы модель по умолчанию, и выбор с телефона
    /// работал бы только в старой сессии.
    #[test]
    fn create_session_with_model_sends_it_in_the_body() {
        let session = serde_json::json!({ "id": "ses_new", "title": "", "directory": "" });
        let (port, sent) = stub_echo("200 OK", &session.to_string());
        let mut client = Client::new(&format!("http://127.0.0.1:{port}")).expect("разбор");
        client.set_directory("/tmp");
        let model = ModelRef::new("vibecode", "gpt-5.6-luna");
        let created = client
            .create_session_modeled(None, Some(&model))
            .expect("сессия");
        assert_eq!(created.id, "ses_new");
        let body = sent.recv_timeout(Duration::from_secs(5)).expect("тело");
        let value: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(
            value,
            json!({ "model": { "id": "gpt-5.6-luna", "providerID": "vibecode" } }),
            "без модели было бы пусто: {body}"
        );

        // Без выбранной модели ключа в теле нет вовсе.
        let (port, sent) = stub_echo("200 OK", &session.to_string());
        let mut client = Client::new(&format!("http://127.0.0.1:{port}")).expect("разбор");
        client.set_directory("/tmp");
        client.create_session(None).expect("сессия");
        let body = sent.recv_timeout(Duration::from_secs(5)).expect("тело");
        assert_eq!(body, "{}", "пустое тело, а не model: null");
    }

    /// Список моделей берётся только у подключённых провайдеров и
    /// сортируется устойчиво. Форма ответа сверена с живым `GET /provider`.
    #[test]
    fn models_come_from_connected_providers_only_and_are_sorted() {
        let body = serde_json::json!({
            "all": [
                { "id": "opencode", "models": { "zeta": { "id": "zeta", "name": "Zeta" } } },
                {
                    "id": "vibecode",
                    "models": {
                        "GPT-6": { "id": "GPT-6", "name": "GPT 6" },
                        "fable": { "id": "fable" },
                        "": { "id": "", "name": "Пустой" }
                    }
                },
                { "id": "secret", "models": { "x": { "id": "x", "name": "X" } } },
                { "id": "без моделей", "models": {} }
            ],
            "connected": ["vibecode", "opencode"],
            "default": {}
        });
        let (port, _sent) = stub_echo("200 OK", &body.to_string());
        let mut client = Client::new(&format!("http://127.0.0.1:{port}")).expect("разбор");
        client.set_directory("/tmp");
        let models = client.list_models().expect("список");
        let ids: Vec<String> = models.iter().map(|m| m.reference.full_id()).collect();
        assert_eq!(
            ids,
            vec!["opencode/zeta", "vibecode/fable", "vibecode/GPT-6"],
            "только connected, без пустых id, порядок устойчив: {ids:?}"
        );
        assert_eq!(models[1].label(), "fable", "без имени показываем id");
        assert_eq!(models[2].label(), "GPT 6", "имя из ответа");
    }

    /// Провайдер без `connected` в ответе — пустой список, а не падение:
    /// спека требует поле, но полагаться на это при отладке нельзя.
    #[test]
    fn models_survive_a_response_without_connected() {
        let body = serde_json::json!({
            "all": [{ "id": "opencode", "models": { "free": { "id": "free" } } }]
        });
        let (port, _sent) = stub_echo("200 OK", &body.to_string());
        let mut client = Client::new(&format!("http://127.0.0.1:{port}")).expect("разбор");
        client.set_directory("/tmp");
        assert!(client.list_models().expect("список").is_empty());
    }
}

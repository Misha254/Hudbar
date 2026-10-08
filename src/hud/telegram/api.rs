//! Клиент Bot API Telegram поверх `curl`.
//!
//! TLS в std нет, а проект сознательно обходится без лишних крейтов, поэтому
//! сетевой вызов делает `curl`. Токен в `argv` не попадает никогда: URL с
//! токеном и поля уходят в конфиг на stdin (`curl --config -`), а в командной
//! строке только флаги без значений. В `ps` процесс виден, а секретов в нём
//! нет.
//!
//! Два вызова требуют внимания к деталям, на которых ломается молча:
//!
//! - `getUpdates` с `timeout=30` — long polling. Он возвращает пустой список
//!   по истечении таймаута, и это не ошибка: пустой список означает «новых
//!   сообщений нет», а не «сервер недоступен»;
//! - `chat_id` в ответе может быть отрицательным (группы) и длиннее, чем помещается
//!   в `i32`: тип `i64`, никакого приведения.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// Ошибка вызова: текст уходит в журнал и, если команда с телефона,
/// в ответ на неё.
pub type Error = String;

/// Приоритет сообщения: чем выше, тем заметнее push на телефоне.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    /// Тихая: «готово», зеркала без срочности.
    Low,
    /// Обычная: ответы агента.
    Default,
    /// Заметная: вопросы агента.
    High,
    /// Максимум: запросы прав и ошибки.
    Max,
}

impl Priority {
    /// Ключ, который понимает Bot API в поле приоритета.
    fn key(&self) -> &'static str {
        match self {
            Priority::Low => "1",
            Priority::Default => "3",
            Priority::High => "4",
            Priority::Max => "5",
        }
    }
}

/// Предел Telegram на одно сообщение.
pub const MESSAGE_LIMIT: usize = 4096;

/// Кнопка под сообщением.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    /// Подпись кнопки.
    pub text: String,
    /// Что вернёт нажатие: id запроса и решение.
    pub callback_data: String,
}

impl Button {
    /// Кнопка с подписью и полезной нагрузкой.
    pub fn new(text: &str, callback_data: &str) -> Self {
        Self {
            text: text.to_string(),
            callback_data: callback_data.to_string(),
        }
    }
}

/// Клиент Bot API.
#[derive(Clone)]
pub struct Api {
    token: String,
    /// Программа для сетевого вызова. Поле, а не константа, чтобы тест мог
    /// подставить заглушку.
    program: String,
}

impl std::fmt::Debug for Api {
    /// Токен в `Debug` не печатается: адрес API с токеном уходит в ошибку
    /// `curl`, а `Debug` конфига — в сообщение об ошибке.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Api")
            .field("token", &"<redacted>")
            .field("program", &self.program)
            .finish()
    }
}

impl Api {
    /// Клиент с токеном бота.
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_string(),
            program: "curl".to_string(),
        }
    }

    /// Токен в `Debug` не печатается: адрес с токеном уходит в ошибку `curl`,
    /// а `Debug` конфига — в сообщение об ошибке.
    fn endpoint(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.token, method)
    }

    /// POST с полями формы. Возвращает поле `result` ответа.
    fn call(&self, method: &str, fields: &[(&str, String)]) -> Result<Value, Error> {
        let args = self.argv();
        let mut child = Command::new(&args[0])
            .args(&args[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|why| format!("{} не запустился: {why}", self.program))?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(self.curl_config(method, fields).as_bytes())
                .map_err(|why| format!("конфиг curl не записан: {why}"))?;
        }
        // stdin закрыт, иначе curl ждал бы конца потока вечно.
        drop(child.stdin.take());
        let output = child
            .wait_with_output()
            .map_err(|why| format!("{} не ответил: {why}", self.program))?;
        let text = String::from_utf8_lossy(&output.stdout);
        match serde_json::from_str(text.trim()) {
            Ok(value) => Self::unwrap_result(method, value),
            Err(_) if output.status.success() => {
                Err(format!("{method}: ответ не json: {}", first_line(&text)))
            }
            Err(_) => {
                // Транспортная ошибка: stderr чистим от токена, потому что
                // curl в некоторых ошибках печатает URL, а URL его содержит.
                let stderr = redact(&String::from_utf8_lossy(&output.stderr), &self.token);
                Err(format!("{method}: {}", first_line(&stderr)))
            }
        }
    }

    /// Поле `result` ответа или ошибка из него.
    fn unwrap_result(method: &str, value: Value) -> Result<Value, Error> {
        if value.get("ok").and_then(Value::as_bool) != Some(true) {
            let description = value
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("без описания");
            return Err(format!("{method}: {description}"));
        }
        Ok(value.get("result").cloned().unwrap_or(Value::Null))
    }

    /// Аргументы curl без секретов: URL с токеном в argv не попадает, он
    /// уходит в конфиг на stdin (`--config -`). Чистая функция ради теста:
    /// проверяется, что токен нигде не встречается.
    fn argv(&self) -> Vec<String> {
        vec![
            self.program.clone(),
            "--disable".to_string(),
            "--config".to_string(),
            "-".to_string(),
        ]
    }

    /// Конфиг curl на stdin: URL и поля. Сырые значения в argv не появляются,
    /// поэтому процесс не виден в `ps` целиком.
    fn curl_config(&self, method: &str, fields: &[(&str, String)]) -> String {
        let mut config = String::from("silent\nshow-error\nfail-with-body\n");
        config.push_str(&format!("url = {}\n", quote(&self.endpoint(method))));
        for (name, value) in fields {
            // Квотируется вся пара `имя=значение` целиком. По отдельности
            // curl читает только имя и теряет значение: `="v"` он считает
            // отдельной директивой и молча выкидывает, а Telegram получает
            // пустой `text` и отвечает «message text is empty».
            config.push_str(&format!(
                "data-urlencode = {}\n",
                quote(&format!("{name}={value}"))
            ));
        }
        config
    }

    /// Отправка сообщения. Возвращает `message_id` — он нужен, чтобы потом
    /// править сообщение на месте вместо нового.
    pub fn send(&self, chat_id: i64, text: &str) -> Result<i64, Error> {
        self.send_inner(chat_id, text, None)
    }

    /// Отправка сообщения с кнопками под ним.
    pub fn send_with_keyboard(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: Vec<Vec<Button>>,
    ) -> Result<i64, Error> {
        self.send_inner(chat_id, text, Some(keyboard))
    }

    fn send_inner(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: Option<Vec<Vec<Button>>>,
    ) -> Result<i64, Error> {
        let mut fields: Vec<(&str, String)> = vec![
            ("chat_id", chat_id.to_string()),
            ("text", text.to_string()),
            ("parse_mode", "HTML".to_string()),
            ("disable_web_page_preview", "true".to_string()),
        ];
        if let Some(keyboard) = keyboard {
            fields.push(("reply_markup", keyboard_json(&keyboard)));
        }
        let result = self.call("sendMessage", &fields)?;
        result
            .get("message_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| "sendMessage без message_id".to_string())
    }

    /// Правка сообщения с кнопками. Ошибку «message is not modified» считаем
    /// успехом: она означает, что текст не изменился, а это не повод слать
    /// ответ заново.
    pub fn edit_with_keyboard(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Vec<Vec<Button>>,
    ) -> Result<(), Error> {
        let fields: Vec<(&str, String)> = vec![
            ("chat_id", chat_id.to_string()),
            ("message_id", message_id.to_string()),
            ("text", text.to_string()),
            ("parse_mode", "HTML".to_string()),
            ("disable_web_page_preview", "true".to_string()),
            ("reply_markup", keyboard_json(&keyboard)),
        ];
        match self.call("editMessageText", &fields) {
            Ok(_) => Ok(()),
            Err(why) if why.contains("message is not modified") => Ok(()),
            Err(why) => Err(why),
        }
    }

    /// Правка клавиатуры без трогания текста: пустой список кнопок снимает
    /// клавиатуру с сообщения. Так убираются несвежие кнопки, не переписывая
    /// историю переписки.
    pub fn edit_reply_markup(
        &self,
        chat_id: i64,
        message_id: i64,
        keyboard: Vec<Vec<Button>>,
    ) -> Result<(), Error> {
        let result = self.call(
            "editMessageReplyMarkup",
            &[
                ("chat_id", chat_id.to_string()),
                ("message_id", message_id.to_string()),
                ("reply_markup", keyboard_json(&keyboard)),
            ],
        )?;
        let _ = result;
        Ok(())
    }

    /// Правка текста со снятием клавиатуры: финальные состояния карточки
    /// больше не нуждаются в кнопках.
    pub fn edit_drop_keyboard(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
    ) -> Result<(), Error> {
        let fields: Vec<(&str, String)> = vec![
            ("chat_id", chat_id.to_string()),
            ("message_id", message_id.to_string()),
            ("text", text.to_string()),
            ("parse_mode", "HTML".to_string()),
            ("disable_web_page_preview", "true".to_string()),
            ("reply_markup", keyboard_json(&[])),
        ];
        match self.call("editMessageText", &fields) {
            Ok(_) => Ok(()),
            Err(why) if why.contains("message is not modified") => Ok(()),
            Err(why) => Err(why),
        }
    }

    /// Удаление сообщения. Ошибки (протухший id, чужая история старше 48 ч)
    /// отдаются наружу, а игнорирует их вызывающий: удаление — всегда
    /// best-effort, молчание вокруг него дороже.
    pub fn delete_message(&self, chat_id: i64, message_id: i64) -> Result<(), Error> {
        self.call(
            "deleteMessage",
            &[
                ("chat_id", chat_id.to_string()),
                ("message_id", message_id.to_string()),
            ],
        )
        .map(|_| ())
    }

    /// Ответ на нажатие кнопки: короткая надпись во всплывающем окне.
    pub fn answer_callback(&self, callback_id: &str, text: &str) -> Result<(), Error> {
        self.call(
            "answerCallbackQuery",
            &[
                ("callback_query_id", callback_id.to_string()),
                ("text", text.to_string()),
            ],
        )
        .map(|_| ())
    }

    /// Long polling: ждёт до `timeout` секунд и возвращает накопленные
    /// апдейты. Пустой список — обычное дело, а не ошибка.
    pub fn poll(&self, offset: Option<i64>, timeout: u32) -> Result<Vec<Value>, Error> {
        let mut fields: Vec<(&str, String)> = vec![
            ("timeout", timeout.to_string()),
            (
                "allowed_updates",
                json!(["message", "callback_query"]).to_string(),
            ),
        ];
        if let Some(offset) = offset {
            fields.push(("offset", offset.to_string()));
        }
        let result = self.call("getUpdates", &fields)?;
        let items = result
            .as_array()
            .ok_or_else(|| "getUpdates вернул не список".to_string())?;
        Ok(items.clone())
    }

    /// Проверка живости: `getMe` отвечает, только если токен верный.
    pub fn whoami(&self) -> Result<String, Error> {
        let result = self.call("getMe", &[])?;
        result
            .get("username")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "getMe без username".to_string())
    }

    /// Снимает webhook, если его ставили раньше: при нём `getUpdates` не
    /// работает и мост молчал бы, не получая команд.
    pub fn drop_webhook(&self) -> Result<(), Error> {
        self.call(
            "deleteWebhook",
            &[("drop_pending_updates", "false".to_string())],
        )
        .map(|_| ())
    }
}

/// Клавиатура в JSON, который понимает Bot API.
pub fn keyboard_json(rows: &[Vec<Button>]) -> String {
    let rows: Vec<Value> = rows
        .iter()
        .map(|row| {
            Value::Array(
                row.iter()
                    .map(|button| {
                        json!({ "text": button.text, "callback_data": button.callback_data })
                    })
                    .collect(),
            )
        })
        .collect();
    json!({ "inline_keyboard": rows }).to_string()
}

/// Одна строка конфига в кавычках. curl понимает внутри кавычек `\\`, `\"`,
/// `\t`, `\n`, `\r`: всё остальное идёт как есть, включая юникод. Сырой
/// перевод строки здесь невозможен: конфиг построчный, и он разорвал бы
/// директиву на две.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(character),
        }
    }
    out.push('"');
    out
}

/// Токен из текста: URL с токеном никогда не должен попасть в журнал.
fn redact(text: &str, token: &str) -> String {
    if token.is_empty() {
        text.to_string()
    } else {
        text.replace(token, "<redacted>")
    }
}

/// Первая строка текста: ошибка curl в одну строку, без простыни.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

/// Секунды из описания флуд-контроля Telegram: «Too Many Requests: retry
/// after 3». Возвращает `None`, если число не разобралось, — тогда вызывающий
/// ждёт значение по умолчанию, а не падает.
pub fn parse_retry_after(description: &str) -> Option<u64> {
    let (_, tail) = description.split_once("retry after")?;
    tail.split_whitespace()
        .next()?
        .trim_end_matches(|character: char| !character.is_ascii_digit())
        .parse::<u64>()
        .ok()
}

/// Экранирование текста для `parse_mode=HTML`. Без этого один `<` в ответе
/// агента ломает разметку всего сообщения, а `&` превращается в символ.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(character),
        }
    }
    out
}

/// Делит текст на части не длиннее `limit`. По возможности режет по строкам:
/// разорванный посреди слова абзац читается хуже, чем два коротких абзаца.
pub fn chunk_text(text: &str, limit: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    if text.chars().count() <= limit {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for line in text.split('\n') {
        let line = line.trim_end();
        if line.chars().count() > limit {
            // Строка длиннее лимита — режем её сами, по словам.
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            for piece in split_long(line, limit) {
                chunks.push(piece);
            }
            continue;
        }
        let candidate = if current.is_empty() {
            line.to_string()
        } else {
            format!("{current}\n{line}")
        };
        if candidate.chars().count() > limit {
            chunks.push(std::mem::replace(&mut current, line.to_string()));
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Режет слишком длинную строку: по словам, а слово длиннее лимита — по
/// символам. Всё, что выходит из функции, не длиннее лимита: длинный хвост не
/// откладывается «на потом», потому что потом — конец строки.
fn split_long(line: &str, limit: usize) -> Vec<String> {
    fn hard(text: &str, limit: usize, out: &mut Vec<String>) {
        let chars: Vec<char> = text.chars().collect();
        if chars.len() <= limit {
            out.push(text.to_string());
            return;
        }
        for chunk in chars.chunks(limit) {
            out.push(chunk.iter().collect());
        }
    }
    let mut pieces = Vec::new();
    let mut current = String::new();
    for word in line.split(' ') {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if candidate.chars().count() > limit && !current.is_empty() {
            hard(&current, limit, &mut pieces);
            current = word.to_string();
        } else if candidate.chars().count() > limit {
            hard(word, limit, &mut pieces);
            current.clear();
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        hard(&current, limit, &mut pieces);
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_keeps_the_token_out_of_debug() {
        let api = Api::new("123:secret");
        let debug = format!("{api:?}");
        assert!(!debug.contains("secret"), "токен в Debug: {debug}");
    }

    /// Токен не должен попасть в `argv`: процесс виден в `ps`, и командная
    /// строка — не место для секретов. URL уходит в конфиг на stdin.
    #[test]
    fn argv_has_no_token_and_no_url() {
        let api = Api::new("123456:AAEtoken-value");
        let args = api.argv();
        assert_eq!(args.first().map(String::as_str), Some("curl"));
        assert!(
            args.iter().any(|arg| arg == "--config"),
            "конфиг идёт через stdin: {args:?}"
        );
        assert!(
            !args
                .iter()
                .any(|arg| arg == "--url" || arg == "--data-urlencode"),
            "значения ушли в конфиг, а не в argv: {args:?}"
        );
        for arg in &args {
            assert!(!arg.contains("AAEtoken-value"), "токен в argv: {arg:?}");
        }
    }

    /// Конфиг несёт URL и поля, а спецсимволы в нём экранированы: сырой
    /// перевод строки разорвал бы конфиг на две директивы.
    #[test]
    fn curl_config_quotes_values() {
        let api = Api::new("123:tok");
        let config = api.curl_config("sendMessage", &[("text", "a\nb \"c\" \\ d".to_string())]);
        assert!(config.contains("silent\n"), "флаги на месте");
        assert!(config.contains("url = \"https://api.telegram.org/bot123:tok/sendMessage\""));
        assert!(config.contains("data-urlencode = \"text=a\\nb \\\"c\\\" \\\\ d\""));
        assert_eq!(
            config.lines().count(),
            5,
            "каждая директива на своей строке"
        );
        // Ровно одна пара в кавычках на директиву: раздельно заквотированные
        // имя и значение (`"text"="v"`) curl читает как имя без значения, и
        // Telegram получает пустой `text`.
        for line in config
            .lines()
            .filter(|line| line.starts_with("data-urlencode"))
        {
            let value = line.trim_start_matches("data-urlencode = ");
            assert!(value.starts_with('"') && value.ends_with('"'), "{line:?}");
            assert!(
                !value.contains("\"=\""),
                "пара name=value в одних кавычках: {line:?}"
            );
            let inner = &value[1..value.len() - 1];
            assert!(inner.contains('='), "внутри есть имя=значение: {line:?}");
            assert!(!inner.starts_with('='), "имя не пустое: {line:?}");
        }
    }

    /// `redact` вычищает токен из текста ошибки, а пустой токен ничего не
    /// трогает: иначе замена по пустой строке раздвинула бы весь текст.
    #[test]
    fn stderr_is_redacted_before_it_reaches_the_log() {
        let dirty = "curl: (6) Could not resolve host: https://api.telegram.org/bot123:tok/x";
        assert_eq!(
            redact(dirty, "123:tok"),
            "curl: (6) Could not resolve host: https://api.telegram.org/bot<redacted>/x"
        );
        assert_eq!(redact("чистый текст", ""), "чистый текст");
    }

    #[test]
    fn html_special_characters_are_escaped() {
        assert_eq!(escape_html("a & b < c > d"), "a &amp; b &lt; c &gt; d");
    }

    /// Экранирование применяется к «сырому» тексту ровно один раз. Правка
    /// сообщения тоже экранирует, но текст приходит из ответа агента, а не из
    /// уже собранного сообщения, поэтому двойного `&amp;amp;` не бывает.
    #[test]
    fn escaping_is_applied_to_raw_text_once() {
        let raw = "x = a & b < c";
        let once = escape_html(raw);
        assert_eq!(once, "x = a &amp; b &lt; c");
        assert!(!once.contains("&amp;amp;"));
    }

    /// Пустая клавиатура — это тоже клавиатура: `{"inline_keyboard":[]}`
    /// снимает кнопки, не трогая текст.
    #[test]
    fn empty_keyboard_drops_buttons() {
        assert_eq!(keyboard_json(&[]), r#"{"inline_keyboard":[]}"#);
    }

    #[test]
    fn retry_after_parses_seconds() {
        assert_eq!(
            parse_retry_after("Too Many Requests: retry after 3"),
            Some(3)
        );
        assert_eq!(
            parse_retry_after("editMessageText: Too Many Requests: retry after 27"),
            Some(27)
        );
        assert_eq!(parse_retry_after("просто ошибка"), None);
        assert_eq!(parse_retry_after("retry after"), None);
    }

    #[test]
    fn keyboard_json_has_rows_and_callback_data() {
        let keyboard = vec![
            vec![
                Button::new("Разрешить", "perm:abc:once"),
                Button::new("Всегда", "perm:abc:always"),
            ],
            vec![Button::new("Отмена", "perm:abc:reject")],
        ];
        let json = keyboard_json(&keyboard);
        assert!(json.contains("\"inline_keyboard\""));
        assert!(json.contains("\"callback_data\":\"perm:abc:once\""));
        assert!(json.contains("\"Отмена\""));
    }

    #[test]
    fn short_text_is_one_chunk() {
        assert_eq!(chunk_text("привет", 100), vec!["привет"]);
        assert!(chunk_text("   ", 100).is_empty());
    }

    #[test]
    fn long_text_splits_on_line_boundaries() {
        let text = (0..10)
            .map(|index| format!("строка номер {index} с текстом"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = chunk_text(&text, 60);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(
                chunk.chars().count() <= 60,
                "кусок длиннее лимита: {chunk:?}"
            );
        }
        // Куски — абзацы: внутри могут быть переносы, но склейка даёт исходник.
        assert_eq!(chunks.join("\n"), text.trim());
    }

    #[test]
    fn one_huge_line_splits_by_words_and_characters() {
        let text = "слово ".repeat(40) + &"я".repeat(120);
        for chunk in chunk_text(&text, 50) {
            assert!(
                chunk.chars().count() <= 50,
                "кусок длиннее лимита: {chunk:?}"
            );
        }
    }
}

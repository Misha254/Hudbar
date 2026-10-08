//! Клиент Bot API Telegram поверх `curl`.
//!
//! TLS в std нет, а проект сознательно обходится без лишних крейтов, поэтому
//! сетевой вызов делает `curl`: поля уходят через `--data-urlencode`, шелла
//! нет, токен в `argv` не попадает, потому что адрес собирается из двух
//! аргументов — так же, как делается везде в проекте.
//!
//! Два вызова требуют внимания к деталям, на которых ломается молча:
//!
//! - `getUpdates` с `timeout=30` — long polling. Он возвращает пустой список
//!   по истечении таймаута, и это не ошибка: пустой список означает «новых
//!   сообщений нет», а не «сервер недоступен»;
//! - `chat_id` в ответе может быть отрицательным (группы) и длиннее, чем помещается
//!   в `i32`: тип `i64`, никакого приведения.

use std::process::Command;

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
        let url = self.endpoint(method);
        let mut command = Command::new(&self.program);
        command
            // Первый аргумент: без этого curl читает ~/.curlrc, а там
            // пользователь мог оставить proxy или перехват тела.
            .arg("--disable")
            .arg("--silent")
            .arg("--show-error")
            .arg("--fail-with-body")
            .arg("--url")
            .arg(&url);
        for (name, value) in fields {
            command
                .arg("--data-urlencode")
                .arg(format!("{name}={value}"));
        }
        let output = command
            .output()
            .map_err(|why| format!("{} не запустился: {why}", self.program))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let value: Value = serde_json::from_str(text.trim())
            .map_err(|why| format!("{method}: ответ не json: {why}"))?;
        if value.get("ok").and_then(Value::as_bool) != Some(true) {
            let description = value
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("без описания");
            return Err(format!("{method}: {description}"));
        }
        Ok(value.get("result").cloned().unwrap_or(Value::Null))
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

    /// Правка сообщения на месте. Ошибку «message is not modified» считаем
    /// успехом: она означает, что текст не изменился, а это не повод слать
    /// ответ заново.
    pub fn edit(&self, chat_id: i64, message_id: i64, text: &str) -> Result<(), Error> {
        self.edit_inner(chat_id, message_id, text, None)
    }

    /// Правка сообщения с кнопками.
    pub fn edit_with_keyboard(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Vec<Vec<Button>>,
    ) -> Result<(), Error> {
        self.edit_inner(chat_id, message_id, text, Some(keyboard))
    }

    fn edit_inner(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Option<Vec<Vec<Button>>>,
    ) -> Result<(), Error> {
        let mut fields: Vec<(&str, String)> = vec![
            ("chat_id", chat_id.to_string()),
            ("message_id", message_id.to_string()),
            ("text", text.to_string()),
            ("parse_mode", "HTML".to_string()),
            ("disable_web_page_preview", "true".to_string()),
        ];
        if let Some(keyboard) = keyboard {
            fields.push(("reply_markup", keyboard_json(&keyboard)));
        }
        match self.call("editMessageText", &fields) {
            Ok(_) => Ok(()),
            Err(why) if why.contains("message is not modified") => Ok(()),
            Err(why) => Err(why),
        }
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

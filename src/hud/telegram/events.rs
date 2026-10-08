//! События opencode → сообщения телефона и зеркало в dunst.
//!
//! Мост видит только свою сессию: чужие сессии на том же сервере (человек уже
//! открыл `opencode` в этой же папке) молчат, иначе телефон гудел бы от каждой
//! строки рабочего терминала. Фильтр по `sessionID` стоит здесь, а не в потоке:
//! поток отдаёт события всех сессий, и это честно — у потока нет задачи знать,
//! какая сессия сейчас активна.
//!
//! Ответ агента приходит одним сообщением и правится на месте: чат не растёт,
//! как лента лога, а `message_id` первого отправленного куска хранит мост.
//! Длинный ответ режется на части по [`MESSAGE_LIMIT`], а правка правит только
//! первый кусок — последующие отправляются новыми сообщениями. Так лента
//! оказывается короче: одно длинное сообщение правится, а хвост дописывается.
//!
//! Решение осознанно простое: переоценить длину заранее нельзя, потому что
//! текст растёт по ходу генерации. Хвост, который переехал в отдельное
//! сообщение, не правится — он висит как дополнение к первому куску.

use serde_json::Value;

use super::api::{Button, Priority, chunk_text, escape_html};
use super::commands::{PendingPermission, PermissionReply};

/// Предел Telegram на одно сообщение — в этих единицах режутся длинные ответы.
pub use super::api::MESSAGE_LIMIT;

/// Что отправить человеку: текст, приоритет, кнопки и канал.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    /// Текст с HTML-разметкой моста: агентский текст уже экранирован.
    pub text: String,
    /// Важность push.
    pub priority: Priority,
    /// Эмодзи-теги сообщения: один, чтобы не шуметь.
    pub tag: &'static str,
    /// Кнопки под сообщением. У запросов прав — три выбора ответа.
    pub buttons: Vec<Vec<Button>>,
    /// Зеркалировать ли событие в dunst на ноутбуке.
    pub dunst: bool,
    /// Заголовок зеркала для `notify-send`.
    pub subject: String,
}

/// Запрос прав из события `permission.asked`: название показывает, что хочет
/// агент, а id уходит в кнопки и в `pending`.
pub fn permission_notice(
    session: &str,
    id: &str,
    permission: &str,
    patterns: &[String],
    metadata: &Value,
) -> (Notice, PendingPermission) {
    let title = describe_permission(permission, patterns, metadata);
    let buttons = vec![vec![
        Button::new(
            PermissionReply::Once.label(),
            &format!("perm:{id}:{}", PermissionReply::Once.key()),
        ),
        Button::new(
            PermissionReply::Always.label(),
            &format!("perm:{id}:{}", PermissionReply::Always.key()),
        ),
        Button::new(
            PermissionReply::Reject.label(),
            &format!("perm:{id}:{}", PermissionReply::Reject.key()),
        ),
    ]];
    (
        Notice {
            text: format!("⚠️ <b>Нужно разрешение</b>\n\n{}", escape_html(&title)),
            priority: Priority::Max,
            tag: "warning",
            buttons,
            dunst: true,
            subject: "opencode: нужно разрешение".to_string(),
        },
        PendingPermission {
            id: id.to_string(),
            session: session.to_string(),
            title,
        },
    )
}

/// Название запроса: команда из `metadata.command`, путь из `metadata.path`,
/// иначе — сырое имя права. Сырое имя лучше, чем пустота: «edit» говорит
/// больше, чем «Запрос» без подробностей.
fn describe_permission(permission: &str, patterns: &[String], metadata: &Value) -> String {
    if let Some(command) = metadata
        .get("command")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
    {
        let mut title = format!("Выполнить: {command}");
        if let Some(path) = metadata
            .get("path")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
        {
            title.push_str(&format!("\nВ папке: {path}"));
        }
        return title;
    }
    if let Some(path) = metadata
        .get("path")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
    {
        return format!("{permission}: {path}");
    }
    if !patterns.is_empty() {
        return format!("{permission}: {}", patterns.join(", "));
    }
    permission.to_string()
}

/// Ответ агента: текст одного сообщения с разметкой. Пустой текст — причина не
/// отправлять ничего: пустое сообщение в чате не несёт смысла.
pub fn answer_notice(text: &str) -> Option<Notice> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(Notice {
        text: escape_html(text),
        priority: Priority::Default,
        tag: "speech_balloon",
        buttons: Vec::new(),
        dunst: true,
        subject: "opencode: ответ".to_string(),
    })
}

/// Ошибка сессии: название и, если есть, текст ошибки — но без простыни. У
/// `session.error` `error` необязателен, тогда только название.
pub fn error_notice(name: Option<&str>, text: Option<&str>) -> Notice {
    let name = name.unwrap_or("ошибка");
    let body = text
        .map(|value| value.lines().next().unwrap_or("").trim().to_string())
        .filter(|value| !value.is_empty());
    let text = match body {
        Some(body) => format!("❌ <b>Ошибка</b> ({name})\n\n{}", escape_html(&body)),
        None => format!("❌ <b>Ошибка</b>: {name}"),
    };
    Notice {
        text,
        priority: Priority::Max,
        tag: "x",
        buttons: Vec::new(),
        dunst: true,
        subject: "opencode: ошибка".to_string(),
    }
}

/// «Готово»: сессия перешла в idle. Тихая важность, потому что ответ уже
/// прилетел отдельным сообщением, и это лишь отметка в ленте.
pub fn idle_notice() -> Notice {
    Notice {
        text: "✅ Готово".to_string(),
        priority: Priority::Low,
        tag: "white_check_mark",
        buttons: Vec::new(),
        dunst: true,
        subject: "opencode: готово".to_string(),
    }
}

/// Накопитель текста ответа: агентский текст приходит кусками (`time.end` у
/// части ещё нет), а уходить должен целиком. Хранит последний текст
/// сообщения и отдаёт его, когда сообщение завершается.
#[derive(Clone, Debug, Default)]
pub struct Answer {
    /// `messageID` последнего куска и его текст.
    last: Option<(String, String)>,
}

impl Answer {
    /// Новая порция текста от агента: запоминает, не отправляет.
    pub fn feed(&mut self, message_id: &str, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        match &mut self.last {
            Some((id, saved)) if id == message_id => *saved = text.to_string(),
            _ => self.last = Some((message_id.to_string(), text.to_string())),
        }
    }

    /// Текущий текст ответа без отдачи: нужен правкам на месте. Отдаёт пару
    /// «id сообщения, текст», чтобы правка точно правила свой текст.
    pub fn current(&self, message_id: &str) -> Option<(String, String)> {
        match &self.last {
            Some((id, text)) if id == message_id => Some((id.clone(), text.clone())),
            _ => None,
        }
    }

    /// Сообщение завершено: отдать накопленный текст и забыть. `message_id`
    /// обязан совпасть — иначе это хвост от следующего сообщения, и хранить
    /// его дальше незачем.
    pub fn finish(&mut self, message_id: &str) -> Option<String> {
        match self.last.take() {
            Some((id, text)) if id == message_id => Some(text),
            _ => None,
        }
    }
}

/// Вопрос агента из `ask`: заголовок, текст и варианты — текстом, без кнопок:
/// вопрос требует ответа словом, а не нажатия. Кнопок нет осознанно:
/// варианты — подсказка, а ответ человек печатает сам.
pub fn question_notice(questions: &[Value]) -> Option<Notice> {
    let first = questions.first()?;
    let header = first
        .get("header")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("Вопрос");
    let question = first
        .get("question")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let mut text = format!("❓ <b>{}</b>", escape_html(header));
    if !question.is_empty() {
        text.push_str(&format!("\n\n{}", escape_html(question)));
    }
    let labels: Vec<String> = first
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(|option| option.get("label"))
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if !labels.is_empty() {
        text.push_str(&format!(
            "\n\nВарианты: {}",
            escape_html(&labels.join(" · "))
        ));
    }
    Some(Notice {
        text,
        priority: Priority::High,
        tag: "question",
        buttons: Vec::new(),
        dunst: true,
        subject: "opencode: вопрос".to_string(),
    })
}

/// Режет текст на куски для отправки. Первый кусок правится на месте, остальные
/// уходят новыми сообщениями — см. модуль выше.
pub fn chunks(text: &str) -> Vec<String> {
    chunk_text(text, MESSAGE_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn permission_title_comes_from_metadata_and_patterns() {
        let (_, pending) = permission_notice("ses_1", "perm_1", "bash", &[], &json!({}));
        assert_eq!(pending.title, "bash");
        assert_eq!(pending.id, "perm_1");
        assert_eq!(pending.session, "ses_1");

        let (notice, _) = permission_notice(
            "ses_1",
            "perm_2",
            "bash",
            &[],
            &json!({ "command": "rm -rf /tmp/x", "path": "/tmp" }),
        );
        assert!(notice.text.contains("rm -rf /tmp/x"));
        assert!(notice.text.contains("/tmp"));
        assert_eq!(notice.buttons.len(), 1, "один ряд кнопок");
        assert_eq!(notice.buttons[0].len(), 3, "три выбора ответа");
        assert_eq!(notice.priority, Priority::Max);
    }

    #[test]
    fn empty_answer_is_not_a_notice() {
        assert!(answer_notice("   \n ").is_none());
        let notice = answer_notice("готово <вот так>").expect("заметка");
        assert!(notice.text.contains("&lt;вот так&gt;"), "текст экранирован");
    }

    #[test]
    fn error_without_details_still_makes_sense() {
        let notice = error_notice(None, None);
        assert!(notice.text.contains("Ошибка"));
        let notice = error_notice(Some("ApiError"), Some("лимит\nвторая строка"));
        assert!(notice.text.contains("ApiError"));
        assert!(notice.text.contains("лимит"));
        assert!(
            !notice.text.contains("вторая строка"),
            "только первая строка"
        );
    }

    #[test]
    fn answer_accumulates_text_and_gives_it_once() {
        let mut answer = Answer::default();
        answer.feed("msg_1", "первая");
        answer.feed("msg_1", "первая вторая");
        assert_eq!(answer.finish("msg_1").as_deref(), Some("первая вторая"));
        assert!(
            answer.finish("msg_1").is_none(),
            "второй раз отдавать нечего"
        );
    }

    #[test]
    fn finish_of_another_message_drops_the_stale_text() {
        let mut answer = Answer::default();
        answer.feed("msg_1", "старое");
        assert!(answer.finish("msg_2").is_none());
    }

    #[test]
    fn question_keeps_header_text_and_options() {
        let questions = vec![serde_json::json!({
            "header": "Продолжить?",
            "question": "Применить правки к файлу?",
            "options": [{ "label": "Да" }, { "label": "Нет" }],
        })];
        let notice = question_notice(&questions).expect("заметка");
        assert!(notice.text.contains("Продолжить?"));
        assert!(notice.text.contains("Применить правки"));
        assert!(notice.text.contains("Да · Нет"));
        assert_eq!(notice.priority, Priority::High);
        assert!(question_notice(&[]).is_none());
    }

    #[test]
    fn long_answers_split_into_chunks() {
        let text = "слово ".repeat(1000);
        let parts = chunks(&text);
        assert!(parts.len() > 1);
        let joined = parts.join(" ");
        let words: Vec<&str> = joined.split_whitespace().collect();
        assert_eq!(words, text.split_whitespace().collect::<Vec<_>>());
    }
}

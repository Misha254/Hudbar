//! Мост: команды с телефона и события агента в одном месте.
//!
//! Мост — это два потока и общее состояние. Первый слушает `getUpdates` бота:
//! боту пишут команды, мост разбирает их в действия и выполняет в opencode.
//! Второй держит поток `GET /event`: ответы агента и запросы прав уходят в
//! Telegram, а заодно — в dunst на ноутбуке.
//!
//! Состояние общее и небольшое: активная сессия, папка, последний запрос прав
//! и текст последнего ответа. Разделяется мьютексом, но надолго его не держит
//! никто: блокировки стоят только вокруг чтения и записи полей, а сетевые
//! вызовы идут без них. Иначе команда с сетевым ожиданием останавливала бы
//! поток событий — проверялось бы это только живьём, поэтому правило простое:
//! мьютекс живёт ровно на время доступа к полям.
//!
//! События от чужих сессий не доходят до телефона: фильтр по `sessionID`
//! стоит на входе обработки, поэтому при открытом рабочем терминале в той же
//! папке бот молчит.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::api::{self, Api, Priority, chunk_text, escape_html};
use super::commands::{self, Outcome, PendingPermission};
use super::config::Config;
use super::events;
use super::opencode::{Client, Event};

/// Пауза между переподключениями подписок: сеть рвётся, мост возвращается.
const RECONNECT_PAUSE: Duration = Duration::from_secs(5);
/// Long polling Telegram: сервер держит ответ до этого времени.
const POLL_TIMEOUT: u32 = 30;
/// Правки ответа агента не чаще, чем раз в столько: иначе чат мигает каждой
/// строкой генерации.
const EDIT_THROTTLE: Duration = Duration::from_secs(3);

/// Мост с конфигом, состоянием и клиентами.
pub struct Bridge {
    config: Config,
    state: Arc<Mutex<commands::State>>,
    api: Api,
    client: Arc<Mutex<Client>>,
    /// Последний отправленный ответ: message_id первого куска, чтобы править
    /// его на месте.
    reply_message: Arc<Mutex<Option<ReplyMessage>>>,
    /// Накопитель текста ответа агента.
    answer: Arc<Mutex<events::Answer>>,
}

/// Отправленный ответ: что править, когда агент договорит.
#[derive(Clone, Debug)]
pub struct ReplyMessage {
    /// `message_id` первого куска в Telegram.
    pub message_id: i64,
    /// Чат, в котором он лежит.
    pub chat_id: i64,
    /// `messageID` ответа агента: правится только свой текст.
    pub agent_message: String,
    /// Когда правили в последний раз: чаще [`EDIT_THROTTLE`] нельзя.
    pub last_edit: Instant,
}

impl Bridge {
    /// Мост по готовому конфигу. Сервер должен быть поднят: готовность
    /// проверяет вызывающий.
    pub fn new(config: Config) -> Result<Self, String> {
        let mut client = Client::new(&config.opencode)?;
        client.set_directory(&config.directory);
        let state = commands::State {
            directory: config.directory.clone(),
            session: config.session.clone(),
            pending_permission: None,
        };
        Ok(Self {
            api: Api::new(&config.bot_token),
            client: Arc::new(Mutex::new(client)),
            config,
            state: Arc::new(Mutex::new(state)),
            reply_message: Arc::new(Mutex::new(None)),
            answer: Arc::new(Mutex::new(events::Answer::default())),
        })
    }

    /// Запускает оба потока и ждёт их конца. Возврата нет: кончается только
    /// сигналом, и systemd его обеспечивает.
    pub fn run(&self) {
        let commands = self.cloned();
        let updates = thread::Builder::new()
            .name("telegram-updates".to_string())
            .spawn(move || commands.commands_loop())
            .expect("поток команд");
        let events = self.cloned();
        let events = thread::Builder::new()
            .name("opencode-events".to_string())
            .spawn(move || events.events_loop())
            .expect("поток событий");
        let _ = updates.join();
        let _ = events.join();
    }

    fn cloned(&self) -> BridgeHandle {
        BridgeHandle {
            config: self.config.clone(),
            state: Arc::clone(&self.state),
            api: self.api.clone(),
            client: Arc::clone(&self.client),
            reply_message: Arc::clone(&self.reply_message),
            answer: Arc::clone(&self.answer),
        }
    }
}

/// Рабочая копия моста для потоков: те же указатели, без владения конфигом.
#[derive(Clone)]
pub struct BridgeHandle {
    pub config: Config,
    pub state: Arc<Mutex<commands::State>>,
    pub api: Api,
    pub client: Arc<Mutex<Client>>,
    pub reply_message: Arc<Mutex<Option<ReplyMessage>>>,
    pub answer: Arc<Mutex<events::Answer>>,
}

impl BridgeHandle {
    /// Цикл команд: long polling, разбор, выполнение, ответ.
    pub fn commands_loop(&self) {
        let mut offset: Option<i64> = None;
        loop {
            match self.api.poll(offset, POLL_TIMEOUT) {
                Ok(updates) => {
                    for update in &updates {
                        let update_id = update.get("update_id").and_then(Value::as_i64);
                        if let Some(id) = update_id {
                            offset = Some(
                                offset
                                    .map(|previous| previous.max(id + 1))
                                    .unwrap_or(id + 1),
                            );
                        }
                        if let Some(reply) = self.handle_update(update) {
                            self.send_text(&reply.text, reply.html);
                        }
                    }
                }
                Err(why) => {
                    super::log::warn(format!("мост: опрос Telegram: {why}"));
                    thread::sleep(RECONNECT_PAUSE);
                }
            }
        }
    }

    /// Одно обновление: текст или нажатие кнопки от своего чата. Чужие чаты
    /// и группы молча пропускаются — это не ошибка, а политика.
    pub fn handle_update(&self, update: &Value) -> Option<Outcome> {
        if let Some(callback) = update.get("callback_query") {
            return self.handle_callback(callback);
        }
        let message = update.get("message")?;
        let chat = message.get("chat")?.get("id")?.as_i64()?;
        if Some(chat) != self.config.chat_id {
            return None;
        }
        let text = message.get("text")?.as_str()?;
        let command = commands::parse(text)?;
        match self.execute(command) {
            Ok(outcome) => Some(outcome),
            Err(why) => Some(Outcome {
                text: format!("Не вышло: {}", escape_html(&short_error(&why))),
                html: true,
            }),
        }
    }

    /// Выполняет команду, не держа мьютексы на время сетевых вызовов. Каждое
    /// обращение к состоянию — короткая блокировка вокруг полей.
    fn execute(&self, command: commands::Command) -> Result<Outcome, String> {
        commands::execute(&self.client, &self.state, &command)
    }

    /// Нажатие кнопки: только `perm:<id>:<решение>`. Кнопка называется
    /// конкретный запрос: сверяем с тем, что ждёт, и несоответствие считаем
    /// устаревшей кнопкой, а не ошибкой.
    pub fn handle_callback(&self, callback: &Value) -> Option<Outcome> {
        let callback_id = callback.get("id")?.as_str()?;
        let data = callback.get("data")?.as_str()?;
        let from = callback
            .get("from")
            .and_then(|from| from.get("id"))
            .and_then(Value::as_i64);
        if from != self.config.chat_id {
            return None;
        }
        let (permission, reply) = match parse_callback(data) {
            Some(parsed) => parsed,
            None => {
                let _ = self.api.answer_callback(callback_id, "непонятная кнопка");
                return None;
            }
        };
        let pending: Option<PendingPermission> = self.state.lock().ok()?.pending_permission.clone();
        if pending
            .as_ref()
            .is_none_or(|pending| pending.id != permission)
        {
            let _ = self
                .api
                .answer_callback(callback_id, "запрос уже не актуален");
            return Some(Outcome {
                text: "Кнопка устарела: запрос уже разобран".to_string(),
                html: false,
            });
        }
        let pending = pending.expect("проверено выше");
        let result = self.client.lock().ok().and_then(|client| {
            client
                .reply_permission(&pending.session, &permission, reply.response())
                .ok()
                .map(|()| pending.clone())
        });
        match result {
            Some(pending) => {
                clear_pending(&self.state, &pending.id);
                let _ = self.api.answer_callback(callback_id, reply.label());
                Some(Outcome {
                    text: format!("{}: {}", reply.label(), pending.title),
                    html: false,
                })
            }
            None => Some(Outcome {
                text: "Не вышло: сервер не принял ответ".to_string(),
                html: false,
            }),
        }
    }

    /// Цикл событий: поток сервера, обработка, переподключение при обрыве.
    /// Клиент копируется на каждую итерацию: поток держит соединение минутами,
    /// и общий мьютекс на это время занимать нельзя — команды встали бы.
    pub fn events_loop(&self) {
        loop {
            let client = self.client.lock().ok().map(|client| client.clone());
            let Some(client) = client else {
                thread::sleep(RECONNECT_PAUSE);
                continue;
            };
            {
                let directory = self.state.lock().ok().map(|state| state.directory.clone());
                if let Some(directory) = directory {
                    let mut owned = client;
                    owned.set_directory(&directory);
                    let result = owned.stream_events(|event| self.handle_event(event));
                    if let Err(why) = result {
                        super::log::warn(format!("мост: поток событий: {why}"));
                    }
                }
            }
            thread::sleep(RECONNECT_PAUSE);
        }
    }

    /// Одно событие сервера. Возвращает `true`, чтобы остановить поток: мост
    /// его не останавливает никогда, остановка — это переподключение снаружи.
    pub fn handle_event(&self, event: &Event) -> bool {
        let session = event.properties.get("sessionID").and_then(Value::as_str);
        {
            let state = self.state.lock().ok();
            let active = state.as_ref().and_then(|state| state.session.as_deref());
            // Чужая сессия: молча пропускаем — событие могло прийти из
            // рабочего терминала на том же сервере.
            if session.is_some_and(|id| Some(id) != active) {
                return false;
            }
        }
        match event.event_type.as_str() {
            "message.part.updated" => self.on_part(&event.properties),
            "message.updated" => self.on_message(&event.properties),
            "permission.asked" => self.on_permission(&event.properties),
            "permission.replied" => self.on_permission_reply(&event.properties),
            "session.idle" => self.on_idle(),
            "session.error" => self.on_error(&event.properties),
            other if is_question_asked(other) => self.on_question(&event.properties),
            _ => {}
        }
        false
    }

    /// Кусок текста ответа: копим и правим сообщение на месте, если оно уже
    /// отправлено и прошло достаточно времени с прошлой правки.
    fn on_part(&self, properties: &Value) {
        let part = match properties.get("part") {
            Some(part) => part,
            None => return,
        };
        if part.get("type").and_then(Value::as_str) != Some("text") {
            return;
        }
        if part.get("synthetic").and_then(Value::as_bool) == Some(true) {
            return;
        }
        let (message_id, text) = match (
            part.get("messageID").and_then(Value::as_str),
            part.get("text").and_then(Value::as_str),
        ) {
            (Some(id), Some(text)) => (id, text),
            _ => return,
        };
        if let Ok(mut answer) = self.answer.lock() {
            answer.feed(message_id, text);
        }
        self.edit_reply(message_id);
    }

    /// Сообщение завершено: накопленный текст уходит в чат — правкой, если
    /// это продолжение того же ответа, иначе новым сообщением.
    fn on_message(&self, properties: &Value) {
        let info = match properties.get("info") {
            Some(info) => info,
            None => return,
        };
        if info.get("role").and_then(Value::as_str) != Some("assistant") {
            return;
        }
        if info
            .pointer("/time/completed")
            .and_then(Value::as_i64)
            .is_none()
        {
            return;
        }
        let message_id = match info.get("id").and_then(Value::as_str) {
            Some(id) => id.to_string(),
            None => return,
        };
        // Ошибка агента тоже приходит `completed`: текст ответа пуст, а
        // событие `session.error` уже ушло отдельным сообщением.
        if info.get("error").is_some() {
            if let Ok(mut answer) = self.answer.lock() {
                let _ = answer.finish(&message_id);
            }
            return;
        }
        let text = match self
            .answer
            .lock()
            .ok()
            .and_then(|mut answer| answer.finish(&message_id))
        {
            Some(text) => text,
            None => return,
        };
        let Some(notice) = events::answer_notice(&text) else {
            return;
        };
        self.finish_answer(&message_id, notice);
    }

    /// Правка ответа на месте: только свой текст, только в пределах лимита,
    /// не чаще троттлинга. Длинный текст ждёт финала — он уйдёт кусками.
    fn edit_reply(&self, agent_message: &str) {
        let snapshot = self
            .answer
            .lock()
            .ok()
            .and_then(|answer| answer.current(agent_message));
        let Some((_, text)) = snapshot else {
            return;
        };
        let Some(reply) = self
            .reply_message
            .lock()
            .ok()
            .and_then(|reply| reply.clone())
        else {
            return;
        };
        if reply.agent_message != agent_message {
            return;
        }
        if reply.last_edit.elapsed() < EDIT_THROTTLE {
            return;
        }
        let text = escape_html(&text);
        if text.chars().count() > api::MESSAGE_LIMIT {
            return;
        }
        if self
            .api
            .edit(reply.chat_id, reply.message_id, &text)
            .is_ok()
            && let Ok(mut current) = self.reply_message.lock()
            && let Some(reply) = current.as_mut()
            && reply.agent_message == agent_message
        {
            reply.last_edit = Instant::now();
        }
    }

    /// Финал ответа: правит сообщение, если оно отправлено, иначе — новое
    /// сообщение. Хвост делится на куски и уходит следом.
    fn finish_answer(&self, agent_message: &str, notice: events::Notice) {
        let chat = match self.config.chat_id {
            Some(chat) => chat,
            None => return,
        };
        let parts = events::chunks(&notice.text);
        let Some(first) = parts.first() else {
            return;
        };
        let reply = self
            .reply_message
            .lock()
            .ok()
            .and_then(|reply| reply.clone());
        let edited = reply
            .filter(|reply| reply.agent_message == agent_message)
            .is_some_and(|reply| self.api.edit(chat, reply.message_id, first).is_ok());
        if !edited {
            match self.api.send(chat, first) {
                Ok(message_id) => {
                    if let Ok(mut slot) = self.reply_message.lock() {
                        *slot = Some(ReplyMessage {
                            message_id,
                            chat_id: chat,
                            agent_message: agent_message.to_string(),
                            last_edit: Instant::now(),
                        });
                    }
                }
                Err(why) => super::log::warn(format!("мост: ответ не отправлен: {why}")),
            }
        }
        if self.config.dunst {
            mirror(&notice);
        }
        for tail in &parts[1..] {
            if let Err(why) = self.api.send(chat, tail) {
                super::log::warn(format!("мост: хвост не отправлен: {why}"));
                break;
            }
        }
    }

    /// Вопрос агента из `ask`: текст и варианты уходят в чат как есть. Кнопок
    /// нет: вопрос требует текста, а не выбора.
    fn on_question(&self, properties: &Value) {
        let questions = properties
            .get("questions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if questions.is_empty() {
            return;
        }
        let Some(notice) = events::question_notice(&questions) else {
            return;
        };
        self.send_notice(&notice);
    }

    /// Запрос прав: кнопки под сообщением и `pending` для команд текстом.
    fn on_permission(&self, properties: &Value) {
        let (id, session, permission) = match (
            properties.get("id").and_then(Value::as_str),
            properties.get("sessionID").and_then(Value::as_str),
            properties.get("permission").and_then(Value::as_str),
        ) {
            (Some(id), Some(session), Some(permission)) => (id, session, permission),
            _ => return,
        };
        let patterns: Vec<String> = properties
            .get("patterns")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let metadata = properties.get("metadata").cloned().unwrap_or(Value::Null);
        let (notice, pending) =
            events::permission_notice(session, id, permission, &patterns, &metadata);
        if let Ok(mut state) = self.state.lock() {
            state.pending_permission = Some(pending);
        }
        self.send_notice(&notice);
    }

    /// Ответ на запрос: `pending` снимается, кнопки остаются под сообщением —
    /// история действий видна по ленте.
    fn on_permission_reply(&self, properties: &Value) {
        let request = properties.get("requestID").and_then(Value::as_str);
        clear_pending_if(&self.state, request);
    }

    /// Сессия в idle: короткая отметка в ленту.
    fn on_idle(&self) {
        self.send_notice(&events::idle_notice());
    }

    /// Ошибка сессии: название и первая строка.
    fn on_error(&self, properties: &Value) {
        let (name, text) = error_details(properties);
        self.send_notice(&events::error_notice(name.as_deref(), text.as_deref()));
    }

    /// Уведомление одним сообщением, без правок: ошибки, вопросы прав,
    /// «готово», вопросы агента.
    fn send_notice(&self, notice: &events::Notice) {
        let chat = match self.config.chat_id {
            Some(chat) => chat,
            None => return,
        };
        let parts = events::chunks(&notice.text);
        let Some(first) = parts.first() else {
            return;
        };
        let result = if notice.buttons.is_empty() {
            self.api.send(chat, first)
        } else {
            self.api
                .send_with_keyboard(chat, first, notice.buttons.clone())
        };
        match result {
            Ok(_) => {
                if self.config.dunst {
                    mirror(notice);
                }
            }
            Err(why) => super::log::warn(format!("мост: уведомление не отправлено: {why}")),
        }
        for tail in &parts[1..] {
            if let Err(why) = self.api.send(chat, tail) {
                super::log::warn(format!("мост: хвост не отправлен: {why}"));
                break;
            }
        }
    }

    /// Текст команде в ответ: короткое сообщение без кнопок и зеркала.
    fn send_text(&self, text: &str, html: bool) {
        let chat = match self.config.chat_id {
            Some(chat) => chat,
            None => return,
        };
        if text.trim().is_empty() {
            return;
        }
        let body = if html {
            text.to_string()
        } else {
            escape_html(text)
        };
        for part in chunk_text(&body, api::MESSAGE_LIMIT) {
            if let Err(why) = self.api.send(chat, &part) {
                super::log::warn(format!("мост: ответ не отправлен: {why}"));
                break;
            }
        }
    }
}

/// Снимает `pending`, если это тот же запрос: иначе ответ по одной кнопке
/// стирал бы ожидание другой.
pub fn clear_pending(state: &Arc<Mutex<commands::State>>, id: &str) {
    if let Ok(mut state) = state.lock()
        && state
            .pending_permission
            .as_ref()
            .is_some_and(|pending| pending.id == id)
    {
        state.pending_permission = None;
    }
}

/// Снимает `pending` по id из события: `None` — ничего не снимать, такой ответ
/// вообще не про наш запрос.
fn clear_pending_if(state: &Arc<Mutex<commands::State>>, id: Option<&str>) {
    let Some(id) = id else {
        return;
    };
    clear_pending(state, id);
}

/// Разбор `callback_data` кнопки: `perm:<id>:<решение>`. Всё остальное — не
/// наша кнопка: мост такие не ставит, а значит, и выполнять их не должен.
pub fn parse_callback(data: &str) -> Option<(String, commands::PermissionReply)> {
    let rest = data.strip_prefix("perm:")?;
    let (id, reply) = rest.rsplit_once(':')?;
    if id.is_empty() {
        return None;
    }
    let reply = match reply {
        "once" => commands::PermissionReply::Once,
        "always" => commands::PermissionReply::Always,
        "reject" => commands::PermissionReply::Reject,
        _ => return None,
    };
    Some((id.to_string(), reply))
}

/// Вопрос агента: любой тип `question.*.asked`.
pub fn is_question_asked(event_type: &str) -> bool {
    event_type.starts_with("question.") && event_type.ends_with(".asked")
}

/// Детали ошибки сессии: название и текст из вложенного `error`.
fn error_details(properties: &Value) -> (Option<String>, Option<String>) {
    let error = properties.get("error");
    let name = properties
        .get("name")
        .or_else(|| error.and_then(|value| value.get("name")))
        .and_then(Value::as_str)
        .map(str::to_string);
    let text = error
        .and_then(|value| value.get("data"))
        .and_then(|data| data.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string);
    (name, text)
}

/// Зеркало в dunst: та же новость локально, заголовок — тема сообщения.
/// Тело — первая строка, чтобы уведомление не занимало пол-экрана.
pub fn mirror(notice: &events::Notice) {
    let body = notice
        .text
        .replace("<b>", "")
        .replace("</b>", "")
        .replace("<code>", "")
        .replace("</code>", "");
    let body = body.lines().next().unwrap_or("").trim();
    let urgency = match notice.priority {
        Priority::Low | Priority::Default => "low",
        Priority::High => "normal",
        Priority::Max => "critical",
    };
    let status = std::process::Command::new("notify-send")
        .args([
            "--urgency",
            urgency,
            "--app-name",
            "opencode",
            "--",
            &notice.subject,
            body,
        ])
        .status();
    if let Err(why) = status {
        super::log::warn(format!("мост: зеркало не отправлено: {why}"));
    }
}

/// Ошибка в одну строку для телефона: полный текст уже в журнале.
pub fn short_error(why: &str) -> String {
    why.lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_data_carries_the_request_and_the_reply() {
        assert_eq!(
            parse_callback("perm:abc123:once"),
            Some(("abc123".to_string(), commands::PermissionReply::Once))
        );
        assert_eq!(
            parse_callback("perm:abc123:always"),
            Some(("abc123".to_string(), commands::PermissionReply::Always))
        );
        assert_eq!(
            parse_callback("perm:abc123:reject"),
            Some(("abc123".to_string(), commands::PermissionReply::Reject))
        );
    }

    #[test]
    fn foreign_buttons_are_not_ours() {
        assert!(parse_callback("что-то другое").is_none());
        assert!(parse_callback("perm::once").is_none());
        assert!(parse_callback("perm:abc:then").is_none());
        assert!(parse_callback("perm:abc").is_none());
    }

    #[test]
    fn question_events_match_by_prefix_and_suffix() {
        assert!(is_question_asked("question.asked"));
        assert!(is_question_asked("question.v2.asked"));
        assert!(!is_question_asked("question.replied"));
        assert!(!is_question_asked("permission.asked"));
    }

    #[test]
    fn error_collapses_to_the_first_short_line() {
        assert_eq!(short_error("первая\nвторая"), "первая");
        assert!(short_error(&"x".repeat(500)).chars().count() <= 300);
    }

    /// Фильтр чужих сессий: событие чужой сессии не доходит до обработки.
    #[test]
    fn foreign_sessions_are_filtered_before_they_reach_the_handlers() {
        let config = Config {
            chat_id: Some(1),
            bot_token: "t".to_string(),
            ..Config::defaults()
        };
        let bridge = Bridge::new(config).expect("мост");
        {
            let mut state = bridge.state.lock().expect("состояние");
            state.session = Some("ses_mine".to_string());
        }
        let foreign = Event {
            id: "evt_1".to_string(),
            event_type: "session.idle".to_string(),
            properties: serde_json::json!({ "sessionID": "ses_other" }),
        };
        // Обработка не должна паниковать и не должна менять состояние: чужая
        // сессия не трогает pending и ответ.
        assert!(!bridge.cloned().handle_event(&foreign));
        assert!(
            bridge
                .state
                .lock()
                .expect("состояние")
                .pending_permission
                .is_none()
        );
    }

    #[test]
    fn permission_flow_keeps_the_request_and_clears_it_on_reply() {
        let config = Config {
            chat_id: Some(1),
            bot_token: "t".to_string(),
            ..Config::defaults()
        };
        let bridge = Bridge::new(config).expect("мост");
        {
            let mut state = bridge.state.lock().expect("состояние");
            state.session = Some("ses_mine".to_string());
        }
        let asked = Event {
            id: "evt_1".to_string(),
            event_type: "permission.asked".to_string(),
            properties: serde_json::json!({
                "id": "perm_1",
                "sessionID": "ses_mine",
                "permission": "bash",
                "patterns": [],
                "metadata": { "command": "ls" },
            }),
        };
        // Без сети отправка упадёт, но pending должен выставиться до неё.
        bridge.cloned().handle_event(&asked);
        assert_eq!(
            bridge
                .state
                .lock()
                .expect("состояние")
                .pending_permission
                .as_ref()
                .map(|pending| pending.id.as_str()),
            Some("perm_1")
        );
        let replied = Event {
            id: "evt_2".to_string(),
            event_type: "permission.replied".to_string(),
            properties: serde_json::json!({ "sessionID": "ses_mine", "requestID": "perm_1" }),
        };
        bridge.cloned().handle_event(&replied);
        assert!(
            bridge
                .state
                .lock()
                .expect("состояние")
                .pending_permission
                .is_none()
        );
    }
}

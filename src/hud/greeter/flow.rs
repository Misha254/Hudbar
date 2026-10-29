//! Конечный автомат входа поверх [`GreetdClient`].
//!
//! Здесь вся логика, которая не должна зависеть ни от Wayland, ни от greetd:
//! состояния, повторы после неверного пароля, многошаговый PAM, обязательный
//! `cancel_session` при сбое. Окно только зовёт `submit()` и рисует то, что
//! вернул автомат, поэтому весь сценарий входа проверяется на mock-клиенте.
//!
//! Пароль живёт в [`Secret`] и только здесь. В отрисовку уходит длина — по ней
//! рисуются точки; в лог, argv и файлы не уходит ничего.

use super::ipc::{GreetdClient, PromptKind, Reply};
use super::settings::Language;
use super::strings;

/// Сколько пояснений PAM подряд гритер готов подтвердить пустым ответом.
const MAX_INFO_STEPS: usize = 8;

/// Вид строки сообщения под полями: от него зависит цвет.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    /// Пояснение и вопросы PAM: обычный цвет.
    Info,
    /// Отказ и сбои: цвет `error`.
    Error,
}

/// Строка под полями. Место под неё reserved всегда, чтобы карточка не
/// прыгала при появлении текста.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    pub text: String,
}

/// Где сейчас вход.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Сеанса greetd ещё нет: следующий Enter создаст его.
    Idle,
    /// Запрос отправлен, ответа ждём.
    Authenticating,
    /// PAM ждёт ввод.
    Prompt(PromptKind),
    /// Пароль принят, запускаем сеанс.
    Starting,
    /// Сеанс запущен: гритер должен выйти, чтобы greetd начал сессию.
    Done,
    /// Вход невозможен: сокета нет или протокол сломан.
    Failed,
}

impl Stage {
    /// Можно ли отправлять ввод. Пока идёт запрос, второй Enter — не команда:
    /// иначе двойное нажатие создало бы второй сеанс greetd.
    pub fn busy(self) -> bool {
        matches!(self, Self::Authenticating | Self::Starting | Self::Done)
    }
}

/// Итог `submit()` для окна.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submit {
    /// Ничего не отправлено: пустой пароль или занятое состояние.
    Ignored,
    /// PAM попросил ещё один ответ (или прислал пояснение).
    Awaiting,
    /// Сеанс запущен.
    Done,
    /// Не вышло: причина в `message()`.
    Failed,
}

/// Пароль в виде байтов.
///
/// `String` здесь не годится: его нельзя затереть по стандарту, а пароль не
/// должен оставаться в куче после отправки. Буфер стирается нулями и при
/// `clear()`, и при `Drop`.
pub struct Secret {
    bytes: Vec<u8>,
}

impl Secret {
    pub fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    pub fn push(&mut self, ch: char) {
        let mut buf = [0u8; 4];
        self.bytes
            .extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
    }

    /// Стереть последний символ — по символу, а не по байту: кириллица и эмодзи
    /// занимают несколько байт, и срез посередине сломал бы UTF-8.
    pub fn pop(&mut self) {
        let text = std::str::from_utf8(&self.bytes).unwrap_or_default();
        if let Some((index, _)) = text.char_indices().next_back() {
            self.bytes[index..].fill(0);
            self.bytes.truncate(index);
        }
    }

    /// Затереть и опустошить. Вызывается после каждой отправки.
    pub fn clear(&mut self) {
        self.bytes.fill(0);
        self.bytes.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Длина в символах: по ней рисуются точки в поле пароля.
    pub fn chars(&self) -> usize {
        std::str::from_utf8(&self.bytes)
            .map(|text| text.chars().count())
            .unwrap_or(self.bytes.len())
    }

    /// Сырые байты для PAM. Только на время проверки; сам буфер остаётся
    /// затираемым и очищается вызывающим.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Текст для greetd. `None` — буфер не UTF-8 (так бывает только при
    /// программной ошибке: `push` принимает `char`).
    fn text(&self) -> Option<&str> {
        std::str::from_utf8(&self.bytes).ok()
    }
}

impl Default for Secret {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Автомат входа.
pub struct Login<C: GreetdClient> {
    client: C,
    lang: Language,
    stage: Stage,
    message: Option<Line>,
    username: String,
    password: Secret,
}

impl<C: GreetdClient> Login<C> {
    /// `username` — последний вошедший пользователь, если он известен.
    pub fn new(client: C, lang: Language, username: String) -> Self {
        Self {
            client,
            lang,
            stage: Stage::Idle,
            message: None,
            username,
            password: Secret::new(),
        }
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn message(&self) -> Option<&Line> {
        self.message.as_ref()
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn set_username(&mut self, name: &str) {
        self.username = name.to_string();
    }

    pub fn password_chars(&self) -> usize {
        self.password.chars()
    }

    pub fn type_char(&mut self, ch: char) {
        self.password.push(ch);
    }

    pub fn backspace(&mut self) {
        self.password.pop();
    }

    pub fn clear_password(&mut self) {
        self.password.clear();
    }

    /// Отмена из окна (Esc, закрытие): сеанс greetd снимается, если он был.
    pub fn cancel(&mut self) {
        if !matches!(self.stage, Stage::Idle | Stage::Done) {
            let _ = self.client.cancel_session();
        }
        self.password.clear();
        self.stage = Stage::Idle;
    }

    /// Отправить пароль. `cmd`/`env` — выбранный сеанс: их получает
    /// `start_session` после успешной проверки.
    pub fn submit(&mut self, cmd: &[String], env: &[String]) -> Submit {
        // Пустой пароль — не попытка: Enter на пустом поле ничего не делает и
        // не создаёт сеанс greetd.
        if self.password.is_empty() || self.stage.busy() {
            return Submit::Ignored;
        }
        if matches!(self.stage, Stage::Idle | Stage::Failed) {
            self.stage = Stage::Idle;
            self.message = None;
            let username = self.username.clone();
            self.stage = Stage::Authenticating;
            let reply = match self.client.create_session(&username) {
                Ok(reply) => reply,
                Err(error) => return self.connection_failed(error),
            };
            let reply = match self.pump(reply) {
                Ok(reply) => reply,
                Err(error) => return self.connection_failed(error),
            };
            match reply {
                Reply::Success => {}
                Reply::AuthMessage { kind, message } => {
                    self.stage = Stage::Prompt(kind);
                    if !kind.asks() {
                        // Пояснение без вопроса: отвечать нечем, ждём Enter.
                        self.note(kind, message);
                        return Submit::Awaiting;
                    }
                }
                Reply::Error {
                    error_type,
                    description,
                } => {
                    let _ = self.client.cancel_session();
                    return self.rejected(error_type, description);
                }
            }
        }

        self.stage = Stage::Authenticating;
        let reply = match self.post() {
            Ok(reply) => reply,
            Err(error) => return self.connection_failed(error),
        };
        self.password.clear();
        self.handle(reply, cmd, env)
    }

    /// Ответ на текущий запрос PAM.
    fn post(&mut self) -> Result<Reply, String> {
        let Login {
            client, password, ..
        } = self;
        match password.text() {
            Some(text) => client.post_auth_message_response(Some(text)),
            None => Err("пароль не UTF-8".to_string()),
        }
    }

    /// Пояснения PAM (`info`/`error`) не ждут текста, но протокол требует
    /// ответить и на них — пустым ответом. Без этого сеанс замирает на
    /// баннере «Добро пожаловать» и пароль никогда не проверяется.
    ///
    /// Предел шагов обязателен: сломанный PAM может слать пояснения вечно.
    fn pump(&mut self, mut reply: Reply) -> Result<Reply, String> {
        for _ in 0..MAX_INFO_STEPS {
            match reply {
                Reply::AuthMessage { kind, message } if !kind.asks() => {
                    self.note(kind, message);
                    reply = self.client.post_auth_message_response(None)?;
                }
                other => return Ok(other),
            }
        }
        Err("PAM не перестаёт присылать пояснения".to_string())
    }

    fn handle(&mut self, reply: Reply, cmd: &[String], env: &[String]) -> Submit {
        let reply = match self.pump(reply) {
            Ok(reply) => reply,
            Err(error) => return self.connection_failed(error),
        };
        match reply {
            Reply::AuthMessage { kind, message } => {
                // Многошаговый PAM: после пароля может прийти код, вопрос или
                // пояснение. Состояние снова «ждём ввод».
                self.note(kind, message);
                self.stage = Stage::Prompt(kind);
                Submit::Awaiting
            }
            Reply::Error {
                error_type,
                description,
            } => {
                // Обязательный шаг: без cancel greetd держит сеанс, и следующая
                // попытка получит отказ «session already exists».
                let _ = self.client.cancel_session();
                self.rejected(error_type, description)
            }
            Reply::Success => {
                self.stage = Stage::Starting;
                match self.client.start_session(cmd, env) {
                    Ok(Reply::Success) => {
                        self.stage = Stage::Done;
                        self.message = None;
                        Submit::Done
                    }
                    Ok(Reply::Error {
                        error_type,
                        description,
                    }) => {
                        let _ = self.client.cancel_session();
                        self.rejected(error_type, description)
                    }
                    // auth_message в ответ на start_session протоколом не
                    // предусмотрен: считаем это сбоем, а не поводом ждать.
                    Ok(Reply::AuthMessage { .. }) => {
                        let _ = self.client.cancel_session();
                        self.fail(strings::text(self.lang).session_failed.to_string())
                    }
                    Err(error) => self.connection_failed(error),
                }
            }
        }
    }

    /// Отказ PAM: пароль уже стёрт, сеанс снят, следующая попытка начнётся с
    /// нового `create_session`.
    fn rejected(&mut self, _error_type: String, description: String) -> Submit {
        self.password.clear();
        self.stage = Stage::Idle;
        let text = if description.trim().is_empty() {
            strings::text(self.lang).auth_failed.to_string()
        } else {
            description.trim().to_string()
        };
        self.message = Some(Line {
            kind: LineKind::Error,
            text,
        });
        Submit::Failed
    }

    /// Сбой связи или протокола: показываем причину по-русски/по-английски и
    /// больше не пытаемся: чинить сокет из окна нечем.
    fn connection_failed(&mut self, error: String) -> Submit {
        self.password.clear();
        let _ = self.client.cancel_session();
        self.stage = Stage::Failed;
        let text = format!("{}: {error}", strings::text(self.lang).no_greetd);
        self.message = Some(Line {
            kind: LineKind::Error,
            text,
        });
        Submit::Failed
    }

    fn fail(&mut self, text: String) -> Submit {
        self.password.clear();
        self.stage = Stage::Failed;
        self.message = Some(Line {
            kind: LineKind::Error,
            text,
        });
        Submit::Failed
    }

    /// Пояснение или вопрос PAM в строку сообщения. Текст от PAM не
    /// переводится: это данные.
    fn note(&mut self, kind: PromptKind, text: String) {
        if text.trim().is_empty() {
            return;
        }
        self.message = Some(Line {
            kind: if matches!(kind, PromptKind::Error) {
                LineKind::Error
            } else {
                LineKind::Info
            },
            text: text.trim().to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Подставной greetd: очередь ответов и журнал вызовов. Сокета нет.
    struct Mock {
        replies: VecDeque<Result<Reply, String>>,
        calls: Vec<String>,
    }

    impl Mock {
        fn new(replies: Vec<Result<Reply, String>>) -> Self {
            Self {
                replies: replies.into(),
                calls: Vec::new(),
            }
        }

        fn reply(&mut self, name: &str) -> Result<Reply, String> {
            self.calls.push(name.to_string());
            self.replies
                .pop_front()
                .unwrap_or_else(|| Err("mock: ответы кончились".to_string()))
        }

        fn calls(&self) -> Vec<&str> {
            self.calls.iter().map(String::as_str).collect()
        }
    }

    impl GreetdClient for Mock {
        fn create_session(&mut self, _username: &str) -> Result<Reply, String> {
            self.reply("create")
        }

        fn post_auth_message_response(&mut self, _response: Option<&str>) -> Result<Reply, String> {
            self.reply("post")
        }

        fn start_session(&mut self, _cmd: &[String], _env: &[String]) -> Result<Reply, String> {
            self.reply("start")
        }

        fn cancel_session(&mut self) -> Result<(), String> {
            self.calls.push("cancel".to_string());
            Ok(())
        }
    }

    fn prompt(kind: PromptKind) -> Result<Reply, String> {
        Ok(Reply::AuthMessage {
            kind,
            message: "Password: ".into(),
        })
    }

    fn cmd() -> Vec<String> {
        vec!["niri-session".into()]
    }

    /// Успешный вход: сеанс создан, пароль принят, сеанс запущен.
    #[test]
    fn a_correct_password_starts_the_session() {
        let mock = Mock::new(vec![
            prompt(PromptKind::Secret),
            Ok(Reply::Success),
            Ok(Reply::Success),
        ]);
        let mut login = Login::new(mock, Language::Ru, "mihail".into());
        for ch in "test".chars() {
            login.type_char(ch);
        }
        assert_eq!(login.submit(&cmd(), &[]), Submit::Done);
        assert_eq!(login.stage(), Stage::Done);
        assert_eq!(login.password_chars(), 0, "пароль стёрт после отправки");
        assert_eq!(login.client.calls(), ["create", "post", "start"]);
    }

    /// Неверный пароль: сообщение, снятый сеанс и готовая вторая попытка.
    #[test]
    fn a_wrong_password_cancels_and_allows_a_retry() {
        let mock = Mock::new(vec![
            prompt(PromptKind::Secret),
            Ok(Reply::Error {
                error_type: "auth_error".into(),
                description: "Authentication failure".into(),
            }),
            prompt(PromptKind::Secret),
            Ok(Reply::Success),
            Ok(Reply::Success),
        ]);
        let mut login = Login::new(mock, Language::Ru, "mihail".into());
        login.type_char('x');
        assert_eq!(login.submit(&cmd(), &[]), Submit::Failed);
        assert_eq!(login.stage(), Stage::Idle, "готов к новой попытке");
        assert_eq!(login.password_chars(), 0);
        let line = login.message().expect("сообщение об отказе");
        assert_eq!(line.kind, LineKind::Error);
        assert_eq!(line.text, "Authentication failure");
        assert_eq!(
            login.client.calls(),
            ["create", "post", "cancel"],
            "cancel обязателен после отказа"
        );

        // Вторая попытка: сеанс создаётся заново, пароль новый.
        login.type_char('y');
        assert_eq!(login.submit(&cmd(), &[]), Submit::Done);
        assert_eq!(
            login.client.calls(),
            ["create", "post", "cancel", "create", "post", "start"]
        );
    }

    /// Многошаговый PAM: пароль, потом код подтверждения.
    #[test]
    fn a_multi_step_pam_asks_again() {
        let mock = Mock::new(vec![
            prompt(PromptKind::Secret),
            prompt(PromptKind::Visible),
            Ok(Reply::Success),
            Ok(Reply::Success),
        ]);
        let mut login = Login::new(mock, Language::Ru, "mihail".into());
        login.type_char('p');
        assert_eq!(login.submit(&cmd(), &[]), Submit::Awaiting);
        assert_eq!(login.stage(), Stage::Prompt(PromptKind::Visible));
        let line = login.message().expect("вопрос PAM");
        assert_eq!(line.kind, LineKind::Info, "вопрос — не ошибка");

        login.type_char('1');
        login.type_char('2');
        assert_eq!(login.submit(&cmd(), &[]), Submit::Done);
        assert_eq!(login.client.calls(), ["create", "post", "post", "start"]);
    }

    /// Баннер `info` не ждёт текста, но требует пустой ответ, иначе сеанс
    /// замирает. После подтверждения вход идёт обычным порядком.
    #[test]
    fn an_info_banner_is_answered_before_the_password() {
        let mock = Mock::new(vec![
            Ok(Reply::AuthMessage {
                kind: PromptKind::Info,
                message: "Welcome".into(),
            }),
            prompt(PromptKind::Secret),
            Ok(Reply::Success),
            Ok(Reply::Success),
        ]);
        let mut login = Login::new(mock, Language::Ru, "mihail".into());
        login.type_char('p');
        assert_eq!(login.submit(&cmd(), &[]), Submit::Done);
        assert_eq!(
            login.client.calls(),
            ["create", "post", "post", "start"],
            "баннер подтверждён пустым ответом, затем ушёл пароль"
        );
    }

    /// Ошибка `error` от greetd (не пароль): показать, снять сеанс, не падать.
    #[test]
    fn a_greetd_error_is_shown_and_the_session_cancelled() {
        let mock = Mock::new(vec![
            prompt(PromptKind::Secret),
            Ok(Reply::Error {
                error_type: "error".into(),
                description: "PAM broken".into(),
            }),
        ]);
        let mut login = Login::new(mock, Language::En, "mihail".into());
        login.type_char('p');
        assert_eq!(login.submit(&cmd(), &[]), Submit::Failed);
        assert_eq!(login.stage(), Stage::Idle);
        assert_eq!(login.message().unwrap().text, "PAM broken");
        assert!(login.client.calls().contains(&"cancel"));
    }

    /// Обрыв сокета: понятное сообщение, `cancel_session` и состояние Failed.
    #[test]
    fn a_broken_socket_fails_with_a_message() {
        let mock = Mock::new(vec![Err("greetd закрыл сокет".into())]);
        let mut login = Login::new(mock, Language::Ru, "mihail".into());
        login.type_char('p');
        assert_eq!(login.submit(&cmd(), &[]), Submit::Failed);
        assert_eq!(login.stage(), Stage::Failed);
        let line = login.message().expect("причина");
        assert!(line.text.contains(strings::RU.no_greetd), "{}", line.text);
        assert_eq!(login.client.calls(), ["create", "cancel"]);
    }

    /// Пустой пароль — не попытка: ни одного запроса, ни одного сеанса.
    #[test]
    fn an_empty_password_sends_nothing() {
        let mock = Mock::new(vec![]);
        let mut login = Login::new(mock, Language::Ru, "mihail".into());
        assert_eq!(login.submit(&cmd(), &[]), Submit::Ignored);
        assert!(login.client.calls().is_empty());
        assert_eq!(login.stage(), Stage::Idle);
    }

    /// Повторный Enter на занятом состоянии не создаёт второй сеанс.
    #[test]
    fn a_second_enter_while_busy_is_ignored() {
        let mut login = Login::new(Mock::new(vec![]), Language::Ru, "mihail".into());
        login.type_char('p');
        login.stage = Stage::Starting;
        assert_eq!(login.submit(&cmd(), &[]), Submit::Ignored);
        assert!(login.client.calls().is_empty());
    }

    /// `start_session` не сработал — сеанс снимается, показана причина.
    #[test]
    fn a_failed_start_cancels_the_session() {
        let mock = Mock::new(vec![
            prompt(PromptKind::Secret),
            Ok(Reply::Success),
            Ok(Reply::Error {
                error_type: "error".into(),
                description: "no such session".into(),
            }),
        ]);
        let mut login = Login::new(mock, Language::Ru, "mihail".into());
        login.type_char('p');
        assert_eq!(login.submit(&cmd(), &[]), Submit::Failed);
        assert_eq!(login.client.calls(), ["create", "post", "start", "cancel"]);
        assert_eq!(login.message().unwrap().text, "no such session");
    }

    /// Отказ PAM без описания показывает локализованную строку, а не пустоту.
    #[test]
    fn an_empty_description_falls_back_to_the_label() {
        let mock = Mock::new(vec![
            prompt(PromptKind::Secret),
            Ok(Reply::Error {
                error_type: "auth_error".into(),
                description: String::new(),
            }),
        ]);
        let mut login = Login::new(mock, Language::En, "mihail".into());
        login.type_char('p');
        login.submit(&cmd(), &[]);
        assert_eq!(login.message().unwrap().text, strings::EN.auth_failed);
    }

    /// Отмена из окна снимает созданный сеанс, но не трогает нетронутый.
    #[test]
    fn cancel_only_touches_a_live_session() {
        let mut login = Login::new(Mock::new(vec![]), Language::Ru, "mihail".into());
        login.cancel();
        assert!(login.client.calls().is_empty(), "Idle снимать нечего");
    }

    /// Пароль стирается: после `clear` и после `pop` в буфере нули.
    #[test]
    fn the_secret_buffer_is_wiped() {
        let mut secret = Secret::new();
        for ch in "пароль".chars() {
            secret.push(ch);
        }
        assert_eq!(secret.chars(), 6);
        secret.pop();
        assert_eq!(secret.chars(), 5, "кириллица стирается по символу");
        let before = secret.bytes.clone();
        secret.clear();
        assert!(secret.is_empty());
        assert!(before.iter().any(|byte| *byte != 0), "в буфере был текст");
        assert!(secret.bytes.is_empty(), "буфер не держит хвост");
    }
}

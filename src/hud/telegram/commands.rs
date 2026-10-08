//! Команды с телефона: разбор текста и исполнение в opencode.
//!
//! Команда — это первая строка текста. Всё остальное — промпт. Список команд
//! короткий и зафиксирован, чтобы сообщение не нужно было думать: бот принимает
//! любое из них, когда оно стоит первым словом.
//!
//! В дереве меню подтверждения делаются уровнем с вопросом, и здесь тот же
//! приём другой стороной: опасное нажатие на телефоне — это всегда выбор из
//! кнопок, а не текст. Поэтому текстовых команд всего восемь, и у всех нет
//! необратимых действий: `abort` прерывает текущую работу агента — да, работа
//! прерывается, но состояние не теряется, а остальное только читает.
//!
//! Отдельное решение про текст `не команда`: он уходит в сессию как есть. Если
//! промпт начинается со слова команды (`new идеи`), спасает префикс `>`:
//! `> new идеи` — всегда промпт. Список команд виден по `/help`, и `/start`
//! тоже отвечает справкой, а не молчанием.

use std::sync::{Arc, Mutex};

use super::opencode::{self, Client, Session};

/// Исполняет команду в клиенте. Мьютексы держатся только вокруг чтения и
/// записи полей: сетевые вызовы идут без них, иначе команда с ожиданием
/// останавливала бы поток событий.
pub fn execute(
    client: &Arc<Mutex<Client>>,
    state: &Arc<Mutex<State>>,
    command: &Command,
) -> Result<Outcome, opencode::Error> {
    // Папка могла смениться командой: клиент держит её отдельно от
    // состояния, поэтому перед каждым обращением синхронизируем короткой
    // блокировкой, а не держим мьютекс на время сети.
    sync_directory(client, state);
    match command {
        Command::Help => Ok(Outcome {
            text: help_text(),
            html: true,
            card: None,
            ephemeral: false,
            delete_command: None,
        }),
        Command::New => {
            let session = with_client(client, |client| client.create_session(None))?;
            set_session(state, Some(session.id.clone()));
            Ok(Outcome {
                text: format!("Новая сессия: <code>{session}</code>"),
                html: true,
                card: None,
                ephemeral: false,
                delete_command: None,
            })
        }
        Command::Sessions => {
            let sessions = with_client(client, |client| client.list_sessions(8))?;
            if sessions.is_empty() {
                let _ = sessions;
                return Ok(Outcome {
                    text: "Сессий пока нет — напиши текст, и мост создаст первую".to_string(),
                    html: false,
                    card: None,
                    ephemeral: false,
                    delete_command: None,
                });
            }
            let active = state
                .lock()
                .map(|state| state.session.clone())
                .unwrap_or_default();
            let mut text = String::from("Сессии:");
            for (index, session) in sessions.iter().enumerate() {
                let marker = if Some(&session.id) == active.as_ref() {
                    " ●"
                } else {
                    ""
                };
                text.push_str(&format!("\n{}. <code>{session}</code>{marker}", index + 1));
            }
            text.push_str("\n\n/use 2 — перейти");
            Ok(Outcome {
                text,
                html: true,
                card: None,
                ephemeral: false,
                delete_command: None,
            })
        }
        Command::Use(argument) => {
            let sessions = with_client(client, |client| client.list_sessions(20))?;
            match find_session(&sessions, argument) {
                Some(session) => {
                    set_session(state, Some(session.id.clone()));
                    Ok(Outcome {
                        text: format!("Сессия: <code>{session}</code>"),
                        html: true,
                        card: None,
                        ephemeral: false,
                        delete_command: None,
                    })
                }
                None => Ok(Outcome {
                    text: "Такой сессии нет — посмотри /sessions".to_string(),
                    html: false,
                    card: None,
                    ephemeral: true,
                    delete_command: None,
                }),
            }
        }
        Command::Stop => match read_session(state) {
            Some(session) => {
                with_client(client, |client| client.abort_session(&session))?;
                Ok(Outcome {
                    text: "Остановлено".to_string(),
                    html: false,
                    card: None,
                    ephemeral: false,
                    delete_command: None,
                })
            }
            None => Ok(Outcome {
                text: "Активной сессии нет — нечего останавливать".to_string(),
                html: false,
                card: None,
                ephemeral: true,
                delete_command: None,
            }),
        },
        Command::Permission(reply) => match read_pending(state) {
            Some(pending) => {
                with_client(client, |client| {
                    client.reply_permission(&pending.id, reply.response())
                })?;
                clear_pending_if(state, &pending.id);
                Ok(Outcome {
                    text: format!("{}: {}", reply.label(), pending.title),
                    html: false,
                    card: None,
                    ephemeral: false,
                    delete_command: None,
                })
            }
            None => Ok(Outcome {
                text: "Запросов прав сейчас нет".to_string(),
                html: false,
                card: None,
                ephemeral: true,
                delete_command: None,
            }),
        },
        Command::Directory(path) => {
            if path.is_empty() {
                let directory = state
                    .lock()
                    .map(|state| state.directory.clone())
                    .unwrap_or_default();
                return Ok(Outcome {
                    text: format!(
                        "Сейчас: <code>{}</code>\n/dir /путь — сменить папку",
                        super::api::escape_html(&directory)
                    ),
                    html: true,
                    card: None,
                    ephemeral: false,
                    delete_command: None,
                });
            }
            if !std::path::Path::new(path).is_dir() {
                return Ok(Outcome {
                    text: "Такой папки нет на ноутбуке".to_string(),
                    html: false,
                    card: None,
                    ephemeral: false,
                    delete_command: None,
                });
            }
            if let Ok(mut state) = state.lock() {
                state.directory = path.to_string();
                state.session = None;
                state.pending_permission = None;
            }
            Ok(Outcome {
                text: format!("Папка: <code>{path}</code>. Сессия сброшена."),
                html: true,
                card: None,
                ephemeral: false,
                delete_command: None,
            })
        }
        Command::Status => {
            let sessions =
                with_client(client, |client| client.list_sessions(1)).unwrap_or_default();
            let title = sessions
                .first()
                .map(|session| session.title.clone())
                .unwrap_or_default();
            let active = read_session(state);
            let name = match &active {
                Some(id) if title.is_empty() => format!("сессия {id}"),
                Some(_) => format!("сессия «{title}»"),
                None => "сессии нет".to_string(),
            };
            let directory = state
                .lock()
                .map(|state| state.directory.clone())
                .unwrap_or_default();
            Ok(Outcome {
                text: format!(
                    "Папка <code>{}</code>, {name}",
                    super::api::escape_html(&directory)
                ),
                html: true,
                card: None,
                ephemeral: false,
                delete_command: None,
            })
        }
        Command::Prompt(text) => {
            let session = match read_session(state) {
                Some(id) => id,
                None => {
                    let created = with_client(client, |client| client.create_session(None))?;
                    set_session(state, Some(created.id.clone()));
                    created.id
                }
            };
            // Неблокирующая отправка: сервер отвечает 204, а результат
            // приходит только через поток событий. Блокирующий POST здесь
            // держал бы команду до конца работы агента.
            with_client(client, |client| client.send_prompt_async(&session, text))?;
            let label = Session {
                id: session.clone(),
                title: String::new(),
                directory: String::new(),
            }
            .to_string();
            Ok(Outcome {
                text: String::new(),
                html: false,
                card: Some(CardOpen {
                    session_id: session,
                    session_label: label,
                }),
                ephemeral: false,
                delete_command: None,
            })
        }
    }
}

/// Синхронизирует папку клиента с состоянием. Короткая двойная блокировка в
/// фиксированном порядке: сначала состояние, потом клиент — иначе два потока
/// встанут в клинч на обратном порядке.
fn sync_directory(client: &Arc<Mutex<Client>>, state: &Arc<Mutex<State>>) {
    let directory = state.lock().map(|state| state.directory.clone());
    if let Ok(directory) = directory
        && let Ok(mut client) = client.lock()
    {
        client.set_directory(&directory);
    }
}

/// Вызов клиента: блокировка живёт ровно на время вызова, не дольше.
fn with_client<T>(
    client: &Arc<Mutex<Client>>,
    call: impl FnOnce(&Client) -> Result<T, opencode::Error>,
) -> Result<T, opencode::Error> {
    let client = client.lock().map_err(|_| "клиент занят".to_string())?;
    call(&client)
}

/// Активная сессия из состояния.
fn read_session(state: &Arc<Mutex<State>>) -> Option<String> {
    state.lock().ok()?.session.clone()
}

/// Активная сессия в состояние.
fn set_session(state: &Arc<Mutex<State>>, session: Option<String>) {
    if let Ok(mut state) = state.lock() {
        state.session = session;
    }
}

/// Последний запрос прав из состояния.
fn read_pending(state: &Arc<Mutex<State>>) -> Option<PendingPermission> {
    state.lock().ok()?.pending_permission.clone()
}

/// Снимает `pending`, если это тот же запрос: между чтением и ответом мог
/// прийти новый запрос, и стирать его чужим ответом нельзя.
fn clear_pending_if(state: &Arc<Mutex<State>>, id: &str) {
    if let Ok(mut state) = state.lock()
        && state
            .pending_permission
            .as_ref()
            .is_some_and(|pending| pending.id == id)
    {
        state.pending_permission = None;
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionReply {
    /// Разрешить.
    Once,
    /// Разрешить этот вид запросов навсегда.
    Always,
    /// Отказать.
    Reject,
}

impl PermissionReply {
    /// Ключевое слово в `callback_data`.
    pub fn key(&self) -> &'static str {
        match self {
            PermissionReply::Once => "once",
            PermissionReply::Always => "always",
            PermissionReply::Reject => "reject",
        }
    }

    /// Подпись кнопки.
    pub fn label(&self) -> &'static str {
        match self {
            PermissionReply::Once => "Разрешить",
            PermissionReply::Always => "Всегда",
            PermissionReply::Reject => "Отмена",
        }
    }

    /// Значение, которое ждёт HTTP-API opencode.
    pub fn response(&self) -> &'static str {
        self.key()
    }
}

/// Команда, разобранная из текста сообщения.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Справочник команд.
    Help,
    /// Новая сессия в папке моста.
    New,
    /// Последние сессии папки для выбора.
    Sessions,
    /// Перейти в сессию: id, короткий id в `ses_…` или номер из `sessions`.
    Use(String),
    /// Прервать работу агента в активной сессии.
    Stop,
    /// Ответ на последний запрос прав, если он есть.
    Permission(PermissionReply),
    /// Сменить папку: мост пересоздаёт сессию там.
    Directory(String),
    /// Что показать в шапке раздела вместо состояния.
    Status,
    /// Промпт: весь остальной текст идёт агенту.
    Prompt(String),
}

/// Что исполняет команда: текст для телефона, запрос на карточку и пометки
/// для жизненного цикла сообщения.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Текст ответа ботом.
    pub text: String,
    /// HTML-разметка: мост экранирует приёмник, но командный текст содержит
    /// свои теги `<code>`, и их не надо трогать.
    pub html: bool,
    /// Открыть живую карточку промпта: сессия уже получила промпт.
    pub card: Option<CardOpen>,
    /// Короткое служебное подтверждение: бот удалит его через ~10 с, чтобы
    /// не засорять ленту.
    pub ephemeral: bool,
    /// Удалить команду пользователя после ответа: id сообщения и его дата.
    /// Ставится только для слэш-команд при включённой настройке.
    pub delete_command: Option<(i64, i64)>,
}

/// Запрос на открытие карточки: сессия с промптом и подпись для шапки.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardOpen {
    /// Id сессии, куда ушёл промпт.
    pub session_id: String,
    /// Подпись для шапки карточки: название или короткий id.
    pub session_label: String,
}

/// Разбирает текст сообщения в команду. Пустое сообщение — команда не
/// выполняется, потому что выполнять нечего.
pub fn parse(text: &str) -> Option<Command> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // `>` в начале — всегда промпт: цена двух символов меньше цены потери
    // текста, который совпал со словом команды.
    if let Some(prompt) = text.strip_prefix('>') {
        return Some(Command::Prompt(prompt.trim().to_string()));
    }
    let mut parts = text.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("").to_lowercase();
    let rest = parts.next().unwrap_or("").trim();
    match first.as_str() {
        "/start" | "/help" | "help" | "?" | "помощь" => Some(Command::Help),
        "/new" | "new" | "новая" => Some(Command::New),
        "/sessions" | "sessions" | "сессии" => Some(Command::Sessions),
        "/stop" | "stop" | "стоп" => Some(Command::Stop),
        "/status" | "status" | "статус" => Some(Command::Status),
        "/use" | "use" | "сессия" => Some(Command::Use(rest.to_string())),
        "/always" | "always" | "всегда" => Some(Command::Permission(PermissionReply::Always)),
        "/reject" | "reject" | "no" | "нет" | "отмена" => {
            Some(Command::Permission(PermissionReply::Reject))
        }
        "/yes" | "yes" | "y" | "да" | "ok" | "ок" => {
            Some(Command::Permission(PermissionReply::Once))
        }
        "/dir" | "dir" | "папка" => Some(Command::Directory(rest.to_string())),
        _ => Some(Command::Prompt(text.to_string())),
    }
}

/// Короткий id без префикса считается id, а не названием: иначе «abcdef»
/// искалось бы среди названий сессий, а не среди id.
fn is_short_id(sessions: &[Session], needle: &str) -> bool {
    !needle.is_empty()
        && sessions
            .iter()
            .any(|session| session.id.strip_prefix("ses_") == Some(needle))
}

/// Форматирует номер телефона в подсказку: `use 2` выбирает вторую сессию.
pub fn session_index(text: &str) -> Option<usize> {
    text.trim().parse::<usize>().ok().filter(|index| *index > 0)
}

/// Текст справки.
pub fn help_text() -> String {
    [
        "Команды:",
        "",
        "<b>текст</b> — промпт агенту: карточка статуса с кнопкой «Стоп», ответ — новым сообщением",
        "<b>/new</b> — новая сессия",
        "<b>/sessions</b> — последние сессии, для выбора",
        "<b>/use 2</b> или <b>/use ses_…</b> — перейти в сессию",
        "<b>/stop</b> — прервать работу агента",
        "<b>/status</b> — какая сессия активна",
        "<b>/dir /путь</b> — работать в другой папке",
        "",
        "Запросы прав приходят кнопками: Разрешить / Всегда / Отмена.",
    ]
    .join("\n")
}

/// Ищет сессию по аргументу: полному id, короткому id или номеру в списке.
pub fn find_session<'a>(sessions: &'a [Session], argument: &str) -> Option<&'a Session> {
    let argument = argument.trim();
    if argument.is_empty() {
        return None;
    }
    let needle = argument.strip_prefix("ses_").unwrap_or(argument);
    if argument.starts_with("ses_") || is_short_id(sessions, needle) {
        sessions.iter().find(|session| {
            session.id == argument || session.id.strip_prefix("ses_") == Some(needle)
        })
    } else if let Some(index) = session_index(argument) {
        sessions.get(index - 1)
    } else {
        sessions.iter().find(|session| {
            session
                .title
                .to_lowercase()
                .contains(&argument.to_lowercase())
        })
    }
}

/// Состояние моста, нужное командам: какая сессия активна и какой запрос прав
/// ждёт человека.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// Папка, в которой работает мост.
    pub directory: String,
    /// Активная сессия.
    pub session: Option<String>,
    /// Последний запрос прав, по которому ещё нет ответа.
    pub pending_permission: Option<PendingPermission>,
}

/// Запрос прав, ждущий человека.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPermission {
    /// Id запроса из события `permission.asked`.
    pub id: String,
    /// Сессия, из которой пришёл запрос.
    pub session: String,
    /// Название: что агент хочет сделать.
    pub title: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_words_become_commands_and_the_rest_is_a_prompt() {
        assert_eq!(parse("/help"), Some(Command::Help));
        assert_eq!(parse("помощь"), Some(Command::Help));
        assert_eq!(parse("/new"), Some(Command::New));
        assert_eq!(parse("сессии"), Some(Command::Sessions));
        assert_eq!(parse("стоп"), Some(Command::Stop));
        assert_eq!(parse("статус"), Some(Command::Status));
        assert_eq!(parse("use ses_abc"), Some(Command::Use("ses_abc".into())));
        assert_eq!(parse("сессия 2"), Some(Command::Use("2".into())));
        assert_eq!(parse("папка /tmp"), Some(Command::Directory("/tmp".into())));
        assert_eq!(
            parse("да"),
            Some(Command::Permission(PermissionReply::Once))
        );
        assert_eq!(
            parse("всегда"),
            Some(Command::Permission(PermissionReply::Always))
        );
        assert_eq!(
            parse("нет"),
            Some(Command::Permission(PermissionReply::Reject))
        );
        assert_eq!(
            parse("переименуй файл"),
            Some(Command::Prompt("переименуй файл".into()))
        );
    }

    #[test]
    fn chevron_forces_a_prompt_even_when_it_looks_like_a_command() {
        assert_eq!(
            parse("> new идеи"),
            Some(Command::Prompt("new идеи".into()))
        );
        assert_eq!(parse("> стоп"), Some(Command::Prompt("стоп".into())));
    }

    #[test]
    fn empty_text_is_not_a_command() {
        assert_eq!(parse("   "), None);
        assert_eq!(parse(""), None);
    }

    /// `y` в русском раскладе — `н`, поэтому одно «да» не должно цеплять
    /// латинское «y» из середины слова: срабатывает только целое первое слово.
    #[test]
    fn single_letters_match_only_whole_words() {
        assert_eq!(parse("y"), Some(Command::Permission(PermissionReply::Once)));
        assert_eq!(parse("yandex"), Some(Command::Prompt("yandex".into())));
    }

    #[test]
    fn session_lookup_accepts_short_id_number_and_title() {
        let sessions = [
            super::super::opencode::Session {
                id: "ses_abcdef123456".to_string(),
                title: "Мост".to_string(),
                directory: String::new(),
            },
            super::super::opencode::Session {
                id: "ses_zzz".to_string(),
                title: "Прочее".to_string(),
                directory: String::new(),
            },
        ];
        assert_eq!(
            find_session(&sessions, "ses_abcdef123456").map(|s| s.id.as_str()),
            Some("ses_abcdef123456")
        );
        assert_eq!(
            find_session(&sessions, "abcdef123456").map(|s| s.id.as_str()),
            Some("ses_abcdef123456")
        );
        assert_eq!(
            find_session(&sessions, "2").map(|s| s.id.as_str()),
            Some("ses_zzz")
        );
        assert_eq!(
            find_session(&sessions, "мост").map(|s| s.id.as_str()),
            Some("ses_abcdef123456")
        );
        assert!(find_session(&sessions, "").is_none());
        assert!(find_session(&sessions, "0").is_none());
        assert!(find_session(&sessions, "нет такой").is_none());
    }
}

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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::api::{self, Api, Priority, chunk_text, escape_html};
use super::card::{self, CardStatus, CardView};
use super::commands::{self, Outcome, PendingPermission};
use super::config::{self, CardRef, Config, PermRef};
use super::events;
use super::menu;
use super::opencode::{Client, Event, ModelRef};

/// Пауза между переподключениями подписок: сеть рвётся, мост возвращается.
const RECONNECT_PAUSE: Duration = Duration::from_secs(5);
/// Long polling Telegram: сервер держит ответ до этого времени.
const POLL_TIMEOUT: u32 = 30;
/// Правки карточки не чаще, чем раз в столько: иначе чат мигает каждой
/// строкой генерации. Финалы (готово, ошибка, остановка, ожидание) идут мимо
/// троттлинга: после них событий может не быть вообще.
const EDIT_THROTTLE: Duration = Duration::from_secs(3);
/// Через столько удаляются короткие служебные подтверждения: их прочитали —
/// и хватит.
const DELETE_DELAY: Duration = Duration::from_secs(10);
/// Сколько живёт «готово»: ответ агента уже пришёл отдельным сообщением, и
/// карточка в ленте только мешает. Новая работа в этой же сессии снимает
/// срок, и карточка остаётся.
const DONE_TTL: Duration = Duration::from_secs(5);
/// Проверка удалятора: чаще не нужно, реже — заметна задержка.
const DELETE_TICK: Duration = Duration::from_secs(5);
/// Старше этого удалять сообщения пользователей нельзя: ограничение Telegram.
const DELETE_LIMIT: Duration = Duration::from_secs(48 * 3600);
/// Карточка считается зависшей, если столько не было событий её сессии:
/// поток мог молча умереть, а сервер — продолжать работать.
const STALE_AFTER: Duration = Duration::from_secs(90);
/// Как часто вотчдог проверяет зависшие карточки.
const WATCH_INTERVAL: Duration = Duration::from_secs(60);
/// Пауза после флуд-контроля Telegram, если число секунд не разобралось.
const RETRY_FALLBACK: Duration = Duration::from_secs(5);
/// Потолок ожидания флуд-контроля: дольше минуты не ждём, ошибка уходит в
/// журнал как обычно.
const RETRY_CAP: Duration = Duration::from_secs(60);

/// Живая карточка: одно сообщение на весь промпт.
#[derive(Clone, Debug)]
pub(crate) struct LiveCard {
    /// Чат карточки.
    chat: i64,
    /// `message_id` карточки в Telegram.
    message: i64,
    /// Подпись сессии для шапки.
    label: String,
    /// Когда приняли промпт: отсюда прошедшее время.
    started: Instant,
    /// Когда правили в последний раз.
    last_edit: Instant,
    /// Что отправили в последний раз (текст + признак клавиатуры): повтор
    /// не отправляется.
    last_render: String,
    /// Текущий инструмент и его цель.
    tool: Option<(String, String)>,
    /// Состояние.
    status: CardStatus,
    /// Есть ли под карточкой клавиатура (кнопка «Стоп»).
    has_keyboard: bool,
    /// Когда пришло последнее событие сессии: по нему вотчдог отличает
    /// зависшую карточку от живой.
    last_event: Instant,
    /// Когда карточку пора удалить: проставится на финале «готово», чтобы
    /// «готово» само исчезло из ленты. Новое событие сессии снимает срок —
    /// карточка снова живая.
    expires: Option<Instant>,
}

/// Сообщение с запросом прав: кнопки ещё на экране.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PermMsg {
    /// Чат сообщения.
    chat: i64,
    /// `message_id` в Telegram.
    message: i64,
    /// Что спрашивал агент: для строки итога, когда `pending` уже снят.
    title: String,
}

/// Отложенное удаление: время, чат и сообщение.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Deletion {
    /// Когда удалять.
    due: Instant,
    /// Чат сообщения.
    chat: i64,
    /// `message_id` в Telegram.
    message: i64,
}

/// Очередь завершённых сообщений сессии: id сообщения и текст, по порядку
/// первого появления. Отдельный тип ради читаемости вложенных карт.
pub(crate) type PendingQueue = Vec<(String, String)>;

/// Строки списка моделей и текущая модель: отдаётся экрану меню одним значением.
type ModelRows = (Vec<(String, String)>, Option<ModelRef>);

/// Мост с конфигом, состоянием и клиентами.
pub struct Bridge {
    config: Config,
    state: Arc<Mutex<commands::State>>,
    state_path: std::path::PathBuf,
    api: Api,
    client: Arc<Mutex<Client>>,
    /// Живые карточки по id сессии: одна карточка на промпт.
    cards: Arc<Mutex<HashMap<String, LiveCard>>>,
    /// Сообщения с запросами прав по id запроса.
    perm_msgs: Arc<Mutex<HashMap<String, PermMsg>>>,
    /// Очередь отложенных удалений.
    deletions: Arc<Mutex<Vec<Deletion>>>,
    /// Текст частей по id сообщения: каждый `part.updated` несёт полный текст
    /// на текущий момент, а завершается сообщение отдельно.
    inflight: Arc<Mutex<HashMap<String, String>>>,
    /// Завершённые сообщения агента по сессии, ждущие конца хода: отправка и
    /// `Done` — только по `session.idle`, иначе промежуточный ответ ушёл бы
    /// раньше времени.
    pending: Arc<Mutex<HashMap<String, PendingQueue>>>,
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
            model: config.model.clone(),
        };
        Ok(Self {
            api: Api::new(&config.bot_token),
            client: Arc::new(Mutex::new(client)),
            state_path: Config::state_path(),
            config,
            state: Arc::new(Mutex::new(state)),
            cards: Arc::new(Mutex::new(HashMap::new())),
            perm_msgs: Arc::new(Mutex::new(HashMap::new())),
            deletions: Arc::new(Mutex::new(Vec::new())),
            inflight: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Проверяет сохранённую сессию на сервере: если её больше нет, состояние
    /// сбрасывается без падения. Недоступный сервер — не доказательство:
    /// тогда сессия остаётся, и мост разберётся живьём.
    fn restore_session(&self) {
        let session = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.session.clone());
        let Some(id) = session else {
            return;
        };
        let check = self
            .client
            .lock()
            .ok()
            .map(|client| client.get_session(&id));
        let Some(check) = check else {
            return;
        };
        self.apply_session_check(&check);
    }

    /// Решение по проверке сессии. Отдельно от запроса, чтобы правило
    /// проверялось тестом без сервера.
    fn apply_session_check(&self, check: &Result<Option<super::opencode::Session>, String>) {
        if !missing_session(check) {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            state.session = None;
        }
        persist_runtime(&self.cloned());
        super::log::warn("мост: сохранённой сессии больше нет, состояние сброшено");
    }

    /// Запускает три потока и ждёт их конца. Перед потоками чистит
    /// пережившие перезапуск кнопки и проверяет сохранённую сессию.
    /// Возврата нет: кончается только сигналом, и systemd его обеспечивает.
    pub fn run(&self) {
        self.startup_cleanup();
        self.restore_session();
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
        let deletions = self.cloned();
        let deletions = thread::Builder::new()
            .name("telegram-deletions".to_string())
            .spawn(move || deletions.deletion_loop())
            .expect("поток удалений");
        let watch = self.cloned();
        let watchdog = thread::Builder::new()
            .name("telegram-watchdog".to_string())
            .spawn(move || watch.watchdog_loop())
            .expect("поток вотчдога");
        let _ = updates.join();
        let _ = events.join();
        let _ = watchdog.join();
        let _ = deletions.join();
    }

    /// Чистка после перезапуска: пережившие его карточки правятся в
    /// «перезапущен» со снятием клавиатуры, у сообщений с правами клавиатура
    /// просто снимается, записи стираются. Ошибки пишутся в журнал и
    /// игнорируются: мост стартует в любом случае.
    fn startup_cleanup(&self) {
        let (cards, permissions) = config::load_refs_from(&self.state_path);
        for card in &cards {
            let text = format!(
                "🛠 <b>hudbar</b> · {}\n⚠ Бот перезапущен",
                api::escape_html(&card.session)
            );
            if let Err(why) = self.api.edit_drop_keyboard(card.chat, card.message, &text) {
                super::log::warn(format!("мост: чистка карточки: {why}"));
            }
        }
        for perm in &permissions {
            if let Err(why) = self
                .api
                .edit_reply_markup(perm.chat, perm.message, Vec::new())
            {
                super::log::warn(format!("мост: чистка кнопок: {why}"));
            }
        }
        if !cards.is_empty() || !permissions.is_empty() {
            persist_runtime(&self.cloned());
        }
    }

    fn cloned(&self) -> BridgeHandle {
        BridgeHandle {
            config: self.config.clone(),
            state: Arc::clone(&self.state),
            state_path: self.state_path.clone(),
            api: self.api.clone(),
            client: Arc::clone(&self.client),
            cards: Arc::clone(&self.cards),
            perm_msgs: Arc::clone(&self.perm_msgs),
            deletions: Arc::clone(&self.deletions),
            inflight: Arc::clone(&self.inflight),
            pending: Arc::clone(&self.pending),
        }
    }
}

/// Рабочая копия моста для потоков: те же указатели, без владения конфигом.
#[derive(Clone)]
pub struct BridgeHandle {
    pub config: Config,
    pub state: Arc<Mutex<commands::State>>,
    /// Куда пишется состояние. Отдельно от конфига, чтобы тесты писали во
    /// временный каталог, а не в живой state-файл.
    pub state_path: std::path::PathBuf,
    pub api: Api,
    pub client: Arc<Mutex<Client>>,
    /// Живые карточки по id сессии.
    pub cards: Arc<Mutex<HashMap<String, LiveCard>>>,
    /// Сообщения с запросами прав по id запроса.
    pub perm_msgs: Arc<Mutex<HashMap<String, PermMsg>>>,
    /// Очередь отложенных удалений.
    pub deletions: Arc<Mutex<Vec<Deletion>>>,
    /// Текст частей по id сообщения.
    pub inflight: Arc<Mutex<HashMap<String, String>>>,
    /// Завершённые сообщения по сессии, ждущие конца хода.
    pub pending: Arc<Mutex<HashMap<String, PendingQueue>>>,
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
                            let ids = self.send_text_ids(&reply.text, reply.html);
                            self.after_send(&reply, &ids);
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

    /// После отправки: стирает короткие служебные подтверждения и, если
    /// включено, команду пользователя. Удаляется только то, что мост только
    /// что отправил или только что прочитал, — ничего чужого.
    fn after_send(&self, reply: &Outcome, ids: &[i64]) {
        let chat = match self.config.chat_id {
            Some(chat) => chat,
            None => return,
        };
        if reply.ephemeral
            && let Some(id) = ids.first()
        {
            self.schedule_delete(chat, *id, DELETE_DELAY);
        }
        if let Some((message_id, date)) = reply.delete_command
            && within_delete_limit(date)
        {
            self.schedule_delete(chat, message_id, DELETE_DELAY);
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
        // Слэш-команда, а не промпт: при включённой настройке удалится после
        // ответа. Id и дата берутся из апдейта, лимит проверяется при отправке.
        let delete_command = if self.config.delete_commands
            && text.trim_start().starts_with('/')
            && !matches!(command, commands::Command::Prompt(_))
        {
            message
                .get("message_id")
                .and_then(Value::as_i64)
                .zip(message.get("date").and_then(Value::as_i64))
        } else {
            None
        };
        // Ответ прав текстом: запоминаем вариант, id и название до
        // исполнения, потому что исполнение снимает `pending`.
        let permission_before: Option<(commands::PermissionReply, String, String)> = match &command
        {
            commands::Command::Permission(reply) => self
                .state
                .lock()
                .ok()?
                .pending_permission
                .clone()
                .map(|pending| (*reply, pending.id, pending.title)),
            _ => None,
        };
        let is_stop = matches!(command, commands::Command::Stop);
        // `/menu` рисуется мостом: команда только сигналит, текста у неё нет.
        let wants_menu = matches!(command, commands::Command::Menu);
        let session_before = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.session.clone());
        match self.execute(command) {
            Ok(mut outcome) => {
                // Сессия и папка могли смениться (/new, /use, /dir, первый
                // промпт): состояние пишется при каждой команде, а не только
                // при смене, — так дешевле, чем отслеживать, что именно
                // поменялось.
                persist_runtime(self);
                // Сессия сменилась — старая лента больше не нужна: карточка
                // чужой сессии в чате только путает, поэтому она уходит
                // вместе с кнопками. Промптом это не задевает: новая карточка
                // откроется ниже, уже после чистки.
                let session_after = self
                    .state
                    .lock()
                    .ok()
                    .and_then(|state| state.session.clone());
                if session_after != session_before {
                    self.wipe_chat();
                }
                if let Some(card) = outcome.card.take() {
                    self.open_card(&card.session_id, &card.session_label);
                }
                // Панель приходит новым сообщением с кнопками: нажимая, человек
                // будет править уже его, поэтому id не нужно хранить.
                if wants_menu {
                    self.redraw_menu(chat, None, menu::Screen::Root);
                    return None;
                }
                if is_stop && self.finalize_stop() {
                    return None;
                }
                if let Some((reply, id, title)) = permission_before
                    && self.finish_permission_text(&id, reply, &title)
                {
                    return None;
                }
                outcome.delete_command = delete_command;
                Some(outcome)
            }
            Err(why) => Some(Outcome {
                text: format!("Не вышло: {}", escape_html(&short_error(&why))),
                html: true,
                card: None,
                ephemeral: false,
                delete_command: None,
            }),
        }
    }

    /// Выполняет команду, не держа мьютексы на время сетевых вызовов. Каждое
    /// обращение к состоянию — короткая блокировка вокруг полей.
    fn execute(&self, command: commands::Command) -> Result<Outcome, String> {
        commands::execute(&self.client, &self.state, &command)
    }

    /// Нажатие кнопки: `stop:<сессия>` останавливает работу, `perm:<id>:…`
    /// отвечает на запрос прав. Чужие кнопки отбрасываются: мост такие не
    /// ставит, а значит, и выполнять их не должен.
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
        if let Some(session_id) = card::parse_stop_callback(data) {
            return self.handle_stop_button(callback_id, session_id);
        }
        if let Some(action) = menu::parse_action(data) {
            // Меню правит своё же сообщение: id экрана приходит из апдейта,
            // а не из состояния — сообщение могло быть переслано.
            let message = callback
                .get("message")
                .and_then(|message| message.get("message_id"))
                .and_then(Value::as_i64);
            return self.handle_menu(callback_id, action, message);
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
                card: None,
                ephemeral: true,
                delete_command: None,
            });
        }
        let pending = pending.expect("проверено выше");
        let result = self.client.lock().ok().and_then(|client| {
            client
                .reply_permission(&permission, reply.response())
                .ok()
                .map(|()| pending.clone())
        });
        match result {
            Some(pending) => {
                clear_pending(&self.state, &pending.id);
                let _ = self.api.answer_callback(callback_id, reply.label());
                if self.finish_permission_button(&permission, reply, &pending.title) {
                    None
                } else {
                    // Сообщение запроса потерялось (не отправилось или стёрто
                    // вместе с записью): подтверждение уходит текстом, чтобы
                    // человек вообще узнал итог.
                    Some(Outcome {
                        text: format!("{}: {}", reply.label(), pending.title),
                        html: false,
                        card: None,
                        ephemeral: false,
                        delete_command: None,
                    })
                }
            }
            None => Some(Outcome {
                text: "Не вышло: сервер не принял ответ".to_string(),
                html: false,
                card: None,
                ephemeral: false,
                delete_command: None,
            }),
        }
    }

    /// Кнопка «Стоп»: тост, прерывание, карточка в «остановлено» без
    /// клавиатуры. Нового сообщения нет: карточка и так перед глазами.
    fn handle_stop_button(&self, callback_id: &str, session_id: &str) -> Option<Outcome> {
        let active = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.session.clone());
        if active.as_deref() != Some(session_id) {
            let _ = self
                .api
                .answer_callback(callback_id, "сессия уже не активна");
            self.drop_card_keyboard(session_id);
            return None;
        }
        let stopped = self
            .client
            .lock()
            .ok()
            .map(|client| client.abort_session(session_id).is_ok())
            .unwrap_or(false);
        if !stopped {
            let _ = self.api.answer_callback(callback_id, "не вышло остановить");
            return None;
        }
        let _ = self.api.answer_callback(callback_id, "останавливаю");
        self.finalize_card(session_id, CardStatus::Stopped, None);
        None
    }

    /// Нажатие в меню: экран перерисовывается в том же сообщении, поэтому
    /// ответом моста ничего не отправляется — только `answerCallbackQuery`,
    /// чтобы Telegram убрал часы на кнопке.
    ///
    /// Действия с побочным эффектом (`Новая сессия`, `Стоп`, выбор сессии или
    /// модели) выполняются здесь же, а сообщение меню после них
    /// перерисовывается на корень: экран «сессии» после перехода уже не про
    /// то, что человек видел.
    fn handle_menu(
        &self,
        callback_id: &str,
        action: menu::Action,
        message: Option<i64>,
    ) -> Option<Outcome> {
        let chat = self.config.chat_id?;
        match action {
            menu::Action::PickSession(index) => {
                self.menu_pick_session(callback_id, index);
            }
            menu::Action::PickModel(index) => {
                self.menu_pick_model(callback_id, index);
            }
            menu::Action::NewSession => match self.execute(commands::Command::New) {
                Ok(_) => {
                    persist_runtime(self);
                    let _ = self.api.answer_callback(callback_id, "новая сессия");
                }
                Err(why) => {
                    let _ = self.api.answer_callback(callback_id, "не вышло");
                    super::log::warn(format!("мост: меню, новая сессия: {why}"));
                    self.redraw_menu(chat, message, menu::Screen::Root);
                    return None;
                }
            },
            menu::Action::Stop => {
                let active = self
                    .state
                    .lock()
                    .ok()
                    .and_then(|state| state.session.clone());
                let stopped = active.as_deref().is_some_and(|session| {
                    self.client
                        .lock()
                        .ok()
                        .map(|client| client.abort_session(session).is_ok())
                        .unwrap_or(false)
                });
                let _ = self.api.answer_callback(
                    callback_id,
                    if stopped {
                        "останавливаю"
                    } else {
                        "нечего останавливать"
                    },
                );
                if let (true, Some(session)) = (stopped, active) {
                    self.finalize_card(&session, CardStatus::Stopped, None);
                }
            }
            menu::Action::Open(screen) => {
                let _ = self.api.answer_callback(callback_id, "");
                self.redraw_menu(chat, message, screen);
                return None;
            }
        }
        self.redraw_menu(chat, message, menu::Screen::Root);
        None
    }

    /// Выбор сессии из меню: номер — это позиция в том же списке, который
    /// человек видел, поэтому список пересобирается и берётся та же позиция.
    /// Число вне диапазона — кнопка испорчена или список успел измениться.
    fn menu_pick_session(&self, callback_id: &str, index: usize) {
        let listed = self
            .client
            .lock()
            .ok()
            .and_then(|client| client.list_sessions(20).ok())
            .unwrap_or_default();
        let Some(session) = listed.get(index - 1) else {
            let _ = self.api.answer_callback(callback_id, "сессии больше нет");
            return;
        };
        let session = session.clone();
        match self.execute(commands::Command::Use(session.id.clone())) {
            Ok(_) => {
                persist_runtime(self);
                let _ = self.api.answer_callback(callback_id, "перешёл");
            }
            Err(why) => {
                let _ = self.api.answer_callback(callback_id, "не вышло");
                super::log::warn(format!("мост: меню, выбор сессии: {why}"));
            }
        }
    }

    /// Выбор модели из меню: тот же номер из того же списка.
    fn menu_pick_model(&self, callback_id: &str, index: usize) {
        let listed = self
            .client
            .lock()
            .ok()
            .and_then(|client| client.list_models().ok())
            .unwrap_or_default();
        let Some(model) = listed.get(index - 1) else {
            let _ = self.api.answer_callback(callback_id, "модели больше нет");
            return;
        };
        let model = model.reference.clone();
        match self.execute(commands::Command::Model(model.full_id())) {
            Ok(_) => {
                persist_runtime(self);
                let _ = self.api.answer_callback(callback_id, "модель выбрана");
            }
            Err(why) => {
                super::log::warn(format!("мост: меню, выбор модели: {why}"));
                let _ = self.api.answer_callback(callback_id, "не вышло");
            }
        }
    }

    /// Перерисовка сообщения меню. Без id сообщения (пересланное нажатие)
    /// присылаем новое сообщение с тем же экраном: иначе нажатие было бы
    /// молчаливым.
    fn redraw_menu(&self, chat: i64, message: Option<i64>, target: menu::Screen) {
        let panel = match self.build_menu(target) {
            Ok(panel) => panel,
            Err(why) => {
                super::log::warn(format!("мост: меню не собрано: {why}"));
                menu::help("Экран недоступен: сервер opencode не отвечает.")
            }
        };
        match message {
            Some(id) => {
                if let Err(why) =
                    self.api
                        .edit_with_keyboard(chat, id, &panel.text, panel.keyboard.clone())
                {
                    super::log::warn(format!("мост: меню не перерисовано: {why}"));
                    // Сообщение могли удалить руками: тогда лучше новое, чем
                    // молчание после нажатия.
                    let _ = self
                        .api
                        .send_with_keyboard(chat, &panel.text, panel.keyboard);
                }
            }
            None => {
                let _ = self
                    .api
                    .send_with_keyboard(chat, &panel.text, panel.keyboard);
            }
        }
    }

    /// Собирает экран меню из живых данных: сессия, модель, списки. Сети
    /// здесь, поэтому функция падает вместе с ней — вызывающий покажет
    /// справку вместо пустого экрана.
    fn build_menu(&self, target: menu::Screen) -> Result<menu::Panel, String> {
        match target {
            menu::Screen::Help => Ok(menu::help(&commands::help_text())),
            menu::Screen::Sessions => {
                let rows = self.session_rows()?;
                let current = self
                    .state
                    .lock()
                    .ok()
                    .and_then(|state| state.session.clone());
                Ok(menu::sessions(&rows, current.as_deref()))
            }
            menu::Screen::Models => {
                let (rows, current) = self.model_rows()?;
                Ok(menu::models(&rows, current.as_ref()))
            }
            menu::Screen::Root => {
                let (id, title) = self
                    .state
                    .lock()
                    .ok()
                    .and_then(|state| state.session.clone())
                    .map(|id| {
                        let title = self
                            .client
                            .lock()
                            .ok()
                            .and_then(|client| client.list_sessions(20).ok())
                            .and_then(|sessions| {
                                sessions
                                    .into_iter()
                                    .find(|session| session.id == id)
                                    .map(|session| session.title)
                            })
                            .unwrap_or_default();
                        (id, title)
                    })
                    .unzip();
                let busy = id
                    .as_deref()
                    .and_then(|id| {
                        self.client
                            .lock()
                            .ok()
                            .and_then(|client| client.session_status().ok())
                            .and_then(|statuses| statuses.get(id).cloned())
                    })
                    .is_some_and(|kind| kind == "busy" || kind == "retry");
                let model = id
                    .as_deref()
                    .and_then(|id| {
                        self.client
                            .lock()
                            .ok()
                            .and_then(|client| client.session_model(id).ok())
                            .flatten()
                    })
                    .map(|model| model.full_id());
                let directory = self
                    .state
                    .lock()
                    .ok()
                    .map(|state| state.directory.clone())
                    .unwrap_or_default();
                Ok(menu::root(&menu::RootView {
                    session: id.map(|id| (id, title.unwrap_or_default())),
                    busy,
                    model,
                    directory,
                }))
            }
        }
    }

    /// Сессии для экрана меню: id и заголовок, свежие сверху.
    fn session_rows(&self) -> Result<Vec<(String, String)>, String> {
        let sessions = self
            .client
            .lock()
            .map_err(|_| "клиент занят".to_string())?
            .list_sessions(20)?;
        Ok(sessions
            .into_iter()
            .map(|session| (session.id, session.title))
            .collect())
    }

    /// Модели для экрана меню: id и имя, плюс текущая модель сессии.
    fn model_rows(&self) -> Result<ModelRows, String> {
        let models = self
            .client
            .lock()
            .map_err(|_| "клиент занят".to_string())?
            .list_models()?;
        let current = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.session.clone())
            .and_then(|id| {
                self.client
                    .lock()
                    .ok()
                    .and_then(|client| client.session_model(&id).ok())
                    .flatten()
            });
        Ok((
            models
                .into_iter()
                .map(|model| (model.reference.id, model.name))
                .collect(),
            current,
        ))
    }

    /// Строка итога по кнопке прав: «✔ Разрешено один раз · команда»,
    /// «✔ Разрешено всегда», «✖ Отклонено». Одна строка, экранирована.
    fn permission_result_line(reply: commands::PermissionReply, title: &str) -> String {
        match reply {
            commands::PermissionReply::Once => format!(
                "✔ Разрешено один раз · {}",
                escape_html(&card::one_line(title, 120))
            ),
            commands::PermissionReply::Always => "✔ Разрешено всегда".to_string(),
            commands::PermissionReply::Reject => "✖ Отклонено".to_string(),
        }
    }

    /// Итог по кнопке: правит сообщение запроса в строку итога и снимает
    /// клавиатуру. Возвращает `true`, если править было что: иначе вызывающий
    /// отправляет текст как обычно.
    fn finish_permission_button(
        &self,
        id: &str,
        reply: commands::PermissionReply,
        title: &str,
    ) -> bool {
        let record = self
            .perm_msgs
            .lock()
            .ok()
            .and_then(|mut messages| messages.remove(id));
        let Some(record) = record else {
            return false;
        };
        let line = Self::permission_result_line(reply, title);
        let ok = self
            .api
            .edit_drop_keyboard(record.chat, record.message, &line)
            .is_ok();
        persist_runtime(self);
        ok
    }

    /// Итог по текстовой команде: то же, что по кнопке. Возвращает `true`,
    /// если сообщение нашлось и правка ушла: тогда отдельное подтверждение
    /// не нужно.
    fn finish_permission_text(
        &self,
        id: &str,
        reply: commands::PermissionReply,
        title: &str,
    ) -> bool {
        self.finish_permission_button(id, reply, title)
    }

    /// Клавиатура живой карточки: одна кнопка «Стоп».
    fn stop_keyboard(session_id: &str) -> Vec<Vec<super::api::Button>> {
        vec![vec![super::api::Button::new(
            "⏹ Стоп",
            &card::stop_callback(session_id),
        )]]
    }

    /// Открывает карточку промпта: снимает клавиатуру с предыдущей карточки
    /// той же сессии, отправляет новую с кнопкой «Стоп» и запоминает её.
    /// Ошибка отправки пишется в журнал: промпт уже ушёл, карточка — лишь
    /// виджет поверх него.
    fn open_card(&self, session_id: &str, label: &str) {
        let chat = match self.config.chat_id {
            Some(chat) => chat,
            None => return,
        };
        self.drop_card_keyboard(session_id);
        let now = Instant::now();
        let text = card::render(&CardView {
            session_label: label,
            status: CardStatus::Running,
            elapsed_secs: 0,
            tool: None,
            error: None,
        });
        match self
            .api
            .send_with_keyboard(chat, &text, Self::stop_keyboard(session_id))
        {
            Ok(message) => {
                if let Ok(mut cards) = self.cards.lock() {
                    cards.insert(
                        session_id.to_string(),
                        LiveCard {
                            chat,
                            message,
                            label: label.to_string(),
                            started: now,
                            last_edit: now,
                            last_render: render_key(&text, true),
                            tool: None,
                            status: CardStatus::Running,
                            has_keyboard: true,
                            last_event: now,
                            expires: None,
                        },
                    );
                }
                persist_runtime(self);
            }
            Err(why) => super::log::warn(format!("мост: карточка не отправлена: {why}")),
        }
    }

    /// Снимает клавиатуру с карточки сессии, если она ещё на экране. Текст не
    /// трогает: карточка остаётся историей, просто без кнопок.
    fn drop_card_keyboard(&self, session_id: &str) {
        let record = self.cards.lock().ok().and_then(|mut cards| {
            cards.get_mut(session_id).and_then(|card| {
                if !card.has_keyboard {
                    return None;
                }
                card.has_keyboard = false;
                Some((card.chat, card.message))
            })
        });
        if let Some((chat, message)) = record {
            let _ = self.api.edit_reply_markup(chat, message, Vec::new());
        }
        persist_runtime(self);
    }

    /// Обновляет карточку: перерисовывает с новым временем и инструментом.
    /// Пропускает правку, если картинка не изменилась или троттлинг ещё не
    /// вышел, — чат не должен мигать каждой строкой генерации.
    fn refresh_card(&self, session_id: &str) {
        let (chat, message, text) = match self.cards.lock().ok().and_then(|mut cards| {
            cards.get_mut(session_id).map(|card| {
                let text = card::render(&CardView {
                    session_label: &card.label,
                    status: card.status,
                    elapsed_secs: card.started.elapsed().as_secs(),
                    tool: card
                        .tool
                        .as_ref()
                        .map(|(tool, target)| (tool.as_str(), target.as_str())),
                    error: None,
                });
                (card.chat, card.message, text)
            })
        }) {
            Some(rendered) => rendered,
            None => return,
        };
        let skip = self.cards.lock().ok().is_some_and(|cards| {
            cards.get(session_id).is_some_and(|card| {
                !should_render(&card.last_render, &render_key(&text, true), card.last_edit)
            })
        });
        if skip {
            return;
        }
        if self.edit_card_send(chat, message, &text, true, session_id) {
            self.note_card_render(session_id, &text, true);
        }
    }

    /// Обновление мимо троттлинга: смена фазы (ожидание, возврат в работу).
    /// Дубли всё равно пропускаются: та же картинка второй раз не уходит.
    fn refresh_card_bypass(&self, session_id: &str) {
        let (chat, message, text) = match self.cards.lock().ok().and_then(|cards| {
            cards.get(session_id).map(|card| {
                let text = card::render(&CardView {
                    session_label: &card.label,
                    status: card.status,
                    elapsed_secs: card.started.elapsed().as_secs(),
                    tool: card
                        .tool
                        .as_ref()
                        .map(|(tool, target)| (tool.as_str(), target.as_str())),
                    error: None,
                });
                (card.chat, card.message, text)
            })
        }) {
            Some(rendered) => rendered,
            None => return,
        };
        let duplicate = self.cards.lock().ok().is_some_and(|cards| {
            cards
                .get(session_id)
                .is_some_and(|card| card.last_render == render_key(&text, true))
        });
        if duplicate {
            return;
        }
        if self.edit_card_send(chat, message, &text, true, session_id) {
            self.note_card_render(session_id, &text, true);
        }
    }

    /// Чистит ленту при смене сессии: карточки и сообщения с правами прошлой
    /// сессии удаляются, записи стираются. Запросы прав в состоянии тоже
    /// снимаются — они belonged к прошлой сессии, и кнопка «Разрешить» после
    /// перехода била бы не туда.
    ///
    /// Удаление идёт через очередь, а не сразу: `deleteMessage` на сообщении
    /// старше 48 часов Telegram отвергнет, и лимит проверяет отправитель.
    /// Прямо здесь сеть не дёргается — мост не должен вставать из-за Telegram.
    fn wipe_chat(&self) {
        let cards: Vec<(i64, i64)> = self
            .cards
            .lock()
            .ok()
            .map(|mut cards| {
                let live: Vec<(i64, i64)> = cards
                    .values()
                    .map(|card| (card.chat, card.message))
                    .collect();
                cards.clear();
                live
            })
            .unwrap_or_default();
        let permissions: Vec<(i64, i64)> = self
            .perm_msgs
            .lock()
            .ok()
            .map(|mut messages| {
                let live: Vec<(i64, i64)> = messages
                    .values()
                    .map(|perm| (perm.chat, perm.message))
                    .collect();
                messages.clear();
                live
            })
            .unwrap_or_default();
        if let Ok(mut state) = self.state.lock() {
            state.pending_permission = None;
        }
        if cards.is_empty() && permissions.is_empty() {
            return;
        }
        // Удаляем сразу, а не через очередь: ответ о переходе отправляется
        // после выхода из `handle_update`, и старые карточки не должны
        // мелькнуть под ним. Ошибки — в журнал: нечего удалять тоже не повод
        // молчать о зависшей кнопке.
        for (chat, message) in cards.into_iter().chain(permissions) {
            if Some(chat) != self.config.chat_id {
                continue;
            }
            if let Err(why) = self.api.delete_message(chat, message) {
                super::log::warn(format!("мост: запись прошлой сессии не удалена: {why}"));
            }
        }
        persist_runtime(self);
    }

    /// Финал карточки: done, error или stopped. Идёт мимо троттлинга: после
    /// финала событий может не быть вообще. Клавиатура снимается, запись
    /// остаётся до `idle` — он гасит её молча, без дубля «готово».
    fn finalize_card(&self, session_id: &str, status: CardStatus, error: Option<&str>) {
        let (chat, message, label, started) = match self.cards.lock().ok().and_then(|mut cards| {
            cards.get_mut(session_id).map(|card| {
                // Состояние — сразу в запись: иначе опоздавшее событие
                // перерисует финал обратно в «В работе».
                card.status = status;
                (card.chat, card.message, card.label.clone(), card.started)
            })
        }) {
            Some(found) => found,
            None => return,
        };
        let text = card::render(&CardView {
            session_label: &label,
            status,
            elapsed_secs: started.elapsed().as_secs(),
            tool: None,
            error,
        });
        if self.edit_card_send(chat, message, &text, false, session_id) {
            self.note_card_render(session_id, &text, false);
        }
        persist_runtime(self);
    }

    /// Запоминает отправленную картинку: следующий рендер сравнится с ней.
    fn note_card_render(&self, session_id: &str, text: &str, keyboard: bool) {
        if let Ok(mut cards) = self.cards.lock()
            && let Some(card) = cards.get_mut(session_id)
        {
            card.last_render = render_key(text, keyboard);
            card.last_edit = Instant::now();
        }
    }

    /// Отправка правки с учётом флуд-контроля: 429 ждёт `retry_after` (не
    /// дольше минуты) и пробует ещё раз, остальное сразу в журнал.
    fn edit_card_send(
        &self,
        chat: i64,
        message: i64,
        text: &str,
        keyboard: bool,
        session_id: &str,
    ) -> bool {
        let attempt = || {
            if keyboard {
                self.api
                    .edit_with_keyboard(chat, message, text, Self::stop_keyboard(session_id))
            } else {
                self.api.edit_drop_keyboard(chat, message, text)
            }
        };
        match attempt() {
            Ok(()) => true,
            Err(why) if is_flood(&why) => {
                let wait = super::api::parse_retry_after(&why)
                    .map(Duration::from_secs)
                    .unwrap_or(RETRY_FALLBACK)
                    .min(RETRY_CAP);
                thread::sleep(wait);
                attempt().is_ok()
            }
            Err(why) => {
                super::log::warn(format!("мост: правка карточки: {why}"));
                false
            }
        }
    }

    /// Завершает карточку остановкой, если она есть. Возвращает `true`, когда
    /// карточка нашлась: тогда отдельное «Остановлено» сообщением не нужно.
    fn finalize_stop(&self) -> bool {
        let session = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.session.clone());
        let Some(session) = session else {
            return false;
        };
        let exists = self
            .cards
            .lock()
            .ok()
            .is_some_and(|cards| cards.contains_key(&session));
        if !exists {
            return false;
        }
        self.finalize_card(&session, CardStatus::Stopped, None);
        true
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
                    // Поток упал — состояние могло измениться мимо нас:
                    // проверяем зависшие карточки сразу, не дожидаясь тика.
                    self.watchdog_once();
                }
            }
            thread::sleep(RECONNECT_PAUSE);
        }
    }

    /// Одна проверка вотчдога: зависшие карточки сверяются с сервером.
    /// Снимок — под мьютексом, сеть — без него, применение — с перепроверкой:
    /// мьютекс не держится через сетевые вызовы никогда.
    pub fn watchdog_once(&self) {
        let now = Instant::now();
        let stale: Vec<(String, String)> = self
            .cards
            .lock()
            .ok()
            .map(|cards| {
                cards
                    .iter()
                    .filter(|(_, card)| {
                        !is_terminal(card.status)
                            && now.duration_since(card.last_event) >= STALE_AFTER
                    })
                    .map(|(session, card)| (session.clone(), card.label.clone()))
                    .collect()
            })
            .unwrap_or_default();
        if stale.is_empty() {
            return;
        }
        let client = self.client.lock().ok().map(|client| client.clone());
        let Some(client) = client else {
            return;
        };
        let directory = self.state.lock().ok().map(|state| state.directory.clone());
        let Some(directory) = directory else {
            return;
        };
        let mut client = client;
        client.set_directory(&directory);
        // Карта статусов одна на всех: лишний опрос на каждую сессию ни к чему.
        let statuses = client.session_status().ok();
        for (session, _) in &stale {
            let status = statuses
                .as_ref()
                .and_then(|map| map.get(session))
                .map(String::as_str);
            // `Ok(None)` — это 404, сессия удалена точно. `Err` — сеть
            // моргнула: отличить от удаления нельзя, поэтому бездействие.
            // Сервер уже сказал `idle` — точечная проверка не нужна.
            let missing = status != Some("idle") && matches!(client.get_session(session), Ok(None));
            let verdict = watchdog_verdict(true, status, missing);
            self.apply_watchdog_verdict(session, verdict);
        }
    }

    /// Применяет вердикт: перепроверяет свежесть (события могли прийти, пока
    /// шёл опрос) и только потом финалит.
    fn apply_watchdog_verdict(&self, session: &str, verdict: WatchVerdict) {
        match verdict {
            WatchVerdict::NoChange => {}
            WatchVerdict::FinalizeDone => {
                if self.card_still_stale(session) {
                    self.finish_turn(session);
                }
            }
            WatchVerdict::FinalizeGone => {
                if self.card_still_stale(session) {
                    // Накопленное не отправляем: сессия удалена вместе с
                    // историей, а обрывки чужой работы в чате не нужны.
                    if let Ok(mut pending) = self.pending.lock() {
                        pending.remove(session);
                    }
                    self.finalize_card(session, CardStatus::Error, Some("сессия не найдена"));
                    if let Ok(mut cards) = self.cards.lock() {
                        cards.remove(session);
                    }
                    persist_runtime(self);
                }
            }
        }
    }

    /// Карточка на месте и с последнего взгляда событий не было.
    fn card_still_stale(&self, session: &str) -> bool {
        self.cards.lock().ok().is_some_and(|cards| {
            cards.get(session).is_some_and(|card| {
                !is_terminal(card.status)
                    && Instant::now().duration_since(card.last_event) >= STALE_AFTER
            })
        })
    }

    /// Цикл вотчдога: раз в минуту сверяет зависшие карточки с сервером.
    /// Отдельный поток: опросы сети не должны тормозить ни команды, ни события.
    fn watchdog_loop(&self) {
        loop {
            thread::sleep(WATCH_INTERVAL);
            self.watchdog_once();
        }
    }

    /// Решение вотчдога: зависшая ли карточка, что сказал сервер, есть ли сессия.
    pub fn watchdog_verdict(
        stale: bool,
        status: Option<&str>,
        session_missing: bool,
    ) -> WatchVerdict {
        if !stale {
            return WatchVerdict::NoChange;
        }
        if status == Some("idle") {
            return WatchVerdict::FinalizeDone;
        }
        if session_missing {
            return WatchVerdict::FinalizeGone;
        }
        WatchVerdict::NoChange
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
        // Сессия жива: вотчдогу есть от чего отсчитывать простой.
        if let Some(id) = session
            && let Ok(mut cards) = self.cards.lock()
            && let Some(card) = cards.get_mut(id)
        {
            card.last_event = Instant::now();
            // Новое событие оживило карточку: срок удаления снимается, иначе
            // «готово» из прошлого хода снесло бы её посреди новой работы.
            card.expires = None;
        }
        match event.event_type.as_str() {
            "message.part.updated" => self.on_part(&event.properties),
            "message.updated" => self.on_message(&event.properties),
            "permission.asked" => self.on_permission(&event.properties),
            "permission.replied" => self.on_permission_reply(&event.properties),
            "session.status" => self.on_session_status(&event.properties),
            "session.idle" => self.on_idle(&event.properties),
            "session.error" => self.on_error(&event.properties),
            other if is_question_asked(other) => self.on_question(&event.properties),
            _ => {}
        }
        false
    }

    /// Кусок события: текст копим для финального ответа, инструмент —
    /// для третьей строки карточки. Стриминга в чат больше нет: карточка
    /// показывает состояние, а не текст.
    fn on_part(&self, properties: &Value) {
        let part = match properties.get("part") {
            Some(part) => part,
            None => return,
        };
        if part.get("type").and_then(Value::as_str) == Some("text") {
            if part.get("synthetic").and_then(Value::as_bool) == Some(true) {
                return;
            }
            if let (Some(id), Some(text)) = (
                part.get("messageID").and_then(Value::as_str),
                part.get("text").and_then(Value::as_str),
            ) && let Ok(mut inflight) = self.inflight.lock()
            {
                // Пустой текст не храним: нечего будет отправлять.
                if !text.trim().is_empty() {
                    inflight.insert(id.to_string(), text.to_string());
                }
            }
            return;
        }
        let session = match properties.get("sessionID").and_then(Value::as_str) {
            Some(session) => session.to_string(),
            None => return,
        };
        let tool = match events::running_tool(part) {
            Some(tool) => tool,
            None => return,
        };
        if let Ok(mut cards) = self.cards.lock()
            && let Some(card) = cards.get_mut(&session)
        {
            card.tool = Some(tool);
        }
        self.refresh_card(&session);
    }

    /// Сообщение завершено: текст переезжает из летящих частей в очередь
    /// сессии. В Telegram ничего не уходит и карточка не финалится: ход
    /// может продолжиться следующим сообщением, а конец хода — только
    /// `session.idle`. Значения `finish` спека не описывает, поэтому на него
    /// не смотрим вообще.
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
        let session = match properties.get("sessionID").and_then(Value::as_str) {
            Some(session) => session.to_string(),
            None => return,
        };
        // Ошибка агента тоже приходит `completed`: текст ответа пуст, а
        // событие `session.error` разберётся отдельно.
        if info.get("error").is_some() {
            if let Ok(mut inflight) = self.inflight.lock() {
                inflight.remove(&message_id);
            }
            return;
        }
        let text = self
            .inflight
            .lock()
            .ok()
            .and_then(|mut inflight| inflight.remove(&message_id));
        let Some(text) = text else {
            return;
        };
        if let Ok(mut pending) = self.pending.lock() {
            let queue = pending.entry(session).or_default();
            Self::stash_completed(queue, &message_id, &text);
        }
    }

    /// Кладет завершённое сообщение в очередь сессии: повтор по тому же id
    /// заменяет, а не дублирует. Порядок — по первому появлению.
    pub fn stash_completed(queue: &mut PendingQueue, id: &str, text: &str) {
        if let Some(slot) = queue.iter_mut().find(|(mid, _)| mid == id) {
            slot.1 = text.to_string();
        } else {
            queue.push((id.to_string(), text.to_string()));
        }
    }

    /// Забирает очередь целиком, отдавая только сообщения с видимым текстом,
    /// по порядку. Пустые забираются тоже: отправлять их нечего, а хранить —
    /// незачем.
    pub fn drain_visible(queue: &mut PendingQueue) -> Vec<String> {
        std::mem::take(queue)
            .into_iter()
            .filter_map(|(_, text)| {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .collect()
    }

    /// Отправляет накопленные завершённые сообщения сессии — каждое один раз,
    /// по порядку, новыми сообщениями. Вызывается только в конце хода.
    fn flush_pending(&self, session: &str) {
        let texts = self
            .pending
            .lock()
            .ok()
            .map(|mut pending| {
                pending
                    .remove(session)
                    .map(|mut queue| Self::drain_visible(&mut queue))
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        for text in &texts {
            if let Some(notice) = events::answer_notice(text) {
                self.send_answer(&notice);
            }
        }
    }

    /// Конец хода: накопленные ответы уходят, карточка — в `Done` без
    /// клавиатуры и через `DONE_TTL` исчезает сама. Дубля «готово» нет: это и
    /// есть конец. Запись пока остаётся — по ней удалитель узнает, что
    /// именно чистить.
    fn finish_turn(&self, session: &str) {
        self.flush_pending(session);
        self.finalize_card(session, CardStatus::Done, None);
        if let Ok(mut cards) = self.cards.lock()
            && let Some(card) = cards.get_mut(session)
        {
            card.expires = Some(Instant::now() + DONE_TTL);
        }
        persist_runtime(self);
        if self.config.dunst {
            mirror(&events::idle_notice());
        }
    }

    /// Ответ агента новыми сообщениями: первое и хвост. Зеркало в dunst —
    /// как раньше.
    fn send_answer(&self, notice: &events::Notice) {
        let chat = match self.config.chat_id {
            Some(chat) => chat,
            None => return,
        };
        let parts = events::chunks(&notice.text);
        let Some(first) = parts.first() else {
            return;
        };
        match self.api.send(chat, first) {
            Ok(_) => {
                if self.config.dunst {
                    mirror(notice);
                }
            }
            Err(why) => super::log::warn(format!("мост: ответ не отправлен: {why}")),
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

    /// Запрос прав: кнопки под сообщением, `pending` для команд текстом,
    /// карточка в ожидание. Сообщение запоминается: итог правит его на месте.
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
        let title = pending.title.clone();
        if let Ok(mut state) = self.state.lock() {
            state.pending_permission = Some(pending);
        }
        if let Some(message) = self.send_notice(&notice) {
            let chat = self.config.chat_id.unwrap_or(0);
            if let Ok(mut messages) = self.perm_msgs.lock() {
                messages.insert(
                    id.to_string(),
                    PermMsg {
                        chat,
                        message,
                        title,
                    },
                );
            }
            persist_runtime(self);
        }
        self.set_card_status(session, CardStatus::Waiting, true);
    }

    /// Ответ на запрос из любого источника: `pending` снимается, сообщение
    /// правится в строку итога, клавиатура снимается, карточка — обратно в
    /// работу. Кнопка, нажатая человеком, уже сделала то же самое: повторная
    /// правка совпадёт побайтово и пройдёт как «not modified».
    fn on_permission_reply(&self, properties: &Value) {
        let request = properties.get("requestID").and_then(Value::as_str);
        let reply = match properties.get("reply").and_then(Value::as_str) {
            Some("always") => commands::PermissionReply::Always,
            Some("reject") => commands::PermissionReply::Reject,
            _ => commands::PermissionReply::Once,
        };
        clear_pending_if(&self.state, request);
        let record = request.and_then(|id| {
            self.perm_msgs
                .lock()
                .ok()
                .and_then(|mut messages| messages.remove(id))
        });
        let Some(record) = record else {
            return;
        };
        let session = properties
            .get("sessionID")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let line = Self::permission_result_line(reply, &record.title);
        let _ = self
            .api
            .edit_drop_keyboard(record.chat, record.message, &line);
        persist_runtime(self);
        self.set_card_status(&session, CardStatus::Running, true);
    }

    /// Статус сессии: `busy` и `retry` — работа продолжается, `idle` — конец.
    /// Неизвестное игнорируется: спека на новые значения нет, а падать из-за
    /// них нельзя.
    fn on_session_status(&self, properties: &Value) {
        let session = match properties.get("sessionID").and_then(Value::as_str) {
            Some(session) => session,
            None => return,
        };
        match properties
            .get("status")
            .and_then(|status| status.get("type"))
            .and_then(Value::as_str)
        {
            Some("busy") | Some("retry") => {
                self.set_card_status(session, CardStatus::Running, false);
                self.refresh_card(session);
            }
            Some("idle") => self.on_idle(properties),
            _ => {}
        }
    }

    /// Переводит карточку в состояние без перерисовки времени: ожидание после
    /// вопроса прав, работа после ответа. Мимо троттлинга: это смена фазы,
    /// а не тиканье.
    fn set_card_status(&self, session_id: &str, status: CardStatus, render: bool) {
        if let Ok(mut cards) = self.cards.lock()
            && let Some(card) = cards.get_mut(session_id)
        {
            card.status = status;
        }
        if render {
            self.refresh_card_bypass(session_id);
        }
    }

    /// Сессия в idle: конец хода. Накопленные ответы уходят, карточка —
    /// в `Done`, запись гасится сразу: дубля «готово» нет, это и есть конец.
    /// Терминальная запись (готово, ошибка, остановка) уже показана — гасится
    /// молча. Без записи — старый fallback отдельным сообщением: лучше лишний
    /// «готово», чем тишина после работы.
    fn on_idle(&self, properties: &Value) {
        let session = properties
            .get("sessionID")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let status = self
            .cards
            .lock()
            .ok()
            .and_then(|cards| cards.get(&session).map(|card| card.status));
        match status {
            None => {
                self.flush_pending(&session);
                self.send_notice(&events::idle_notice());
            }
            Some(status) if is_terminal(status) => {
                // Уже показана (готово, ошибка, остановка) — гасим молча.
                self.flush_pending(&session);
                if let Ok(mut cards) = self.cards.lock() {
                    cards.remove(&session);
                }
                persist_runtime(self);
            }
            Some(_) => {
                self.finish_turn(&session);
            }
        }
    }

    /// Ошибка сессии: карточка в ❌ с короткой причиной, больше ничего.
    /// Без карточки — старый fallback отдельным сообщением.
    fn on_error(&self, properties: &Value) {
        let session = properties
            .get("sessionID")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (name, text) = error_details(properties);
        let reason = match (name, text) {
            (Some(name), Some(text)) => {
                let first = text.lines().next().unwrap_or("").trim();
                if first.is_empty() {
                    name
                } else {
                    format!("{name}: {first}")
                }
            }
            (Some(name), None) => name,
            (None, Some(text)) => text.lines().next().unwrap_or("").trim().to_string(),
            (None, None) => "неизвестная ошибка".to_string(),
        };
        let has_card = self
            .cards
            .lock()
            .ok()
            .is_some_and(|cards| cards.contains_key(session));
        if has_card {
            self.finalize_card(session, CardStatus::Error, Some(&reason));
        } else {
            self.send_notice(&events::error_notice(Some(&reason), None));
        }
    }

    /// Уведомление одним сообщением, без правок: ошибки, вопросы прав,
    /// «готово», вопросы агента. Возвращает id первого куска: он нужен
    /// записям о живых кнопках.
    fn send_notice(&self, notice: &events::Notice) -> Option<i64> {
        let chat = self.config.chat_id?;
        let parts = events::chunks(&notice.text);
        let first = parts.first()?;
        let result = if notice.buttons.is_empty() {
            self.api.send(chat, first)
        } else {
            self.api
                .send_with_keyboard(chat, first, notice.buttons.clone())
        };
        let id = match result {
            Ok(id) => {
                if self.config.dunst {
                    mirror(notice);
                }
                Some(id)
            }
            Err(why) => {
                super::log::warn(format!("мост: уведомление не отправлено: {why}"));
                None
            }
        };
        for tail in &parts[1..] {
            if let Err(why) = self.api.send(chat, tail) {
                super::log::warn(format!("мост: хвост не отправлен: {why}"));
                break;
            }
        }
        id
    }

    /// Текст команде в ответ: короткое сообщение без кнопок и зеркала.
    /// Возвращает id отправленных кусков: первый нужен удалятору для
    /// коротких служебных подтверждений.
    fn send_text_ids(&self, text: &str, html: bool) -> Vec<i64> {
        let chat = match self.config.chat_id {
            Some(chat) => chat,
            None => return Vec::new(),
        };
        if text.trim().is_empty() {
            return Vec::new();
        }
        let body = if html {
            text.to_string()
        } else {
            escape_html(text)
        };
        let mut ids = Vec::new();
        for part in chunk_text(&body, api::MESSAGE_LIMIT) {
            match self.api.send(chat, &part) {
                Ok(id) => ids.push(id),
                Err(why) => {
                    super::log::warn(format!("мост: ответ не отправлен: {why}"));
                    break;
                }
            }
        }
        ids
    }

    /// Кладет сообщение в очередь удалятора. Удаляется только то, что мост
    /// только что отправил или только что прочитал: промпты, ответы и
    /// запросы прав сюда не попадают никогда.
    fn schedule_delete(&self, chat: i64, message: i64, after: Duration) {
        if let Ok(mut deletions) = self.deletions.lock() {
            deletions.push(Deletion {
                due: Instant::now() + after,
                chat,
                message,
            });
        }
    }

    /// Забирает карточки, у которых истёк срок: возвращает их к удалению и
    /// стирает записи. Чистая функция ради тестов — время и коллекция на
    /// входе, список дел на выходе.
    pub fn take_expired(cards: &mut HashMap<String, LiveCard>, now: Instant) -> Vec<Deletion> {
        let expired: Vec<String> = cards
            .iter()
            .filter(|(_, card)| card.expires.is_some_and(|due| due <= now))
            .map(|(session, _)| session.clone())
            .collect();
        expired
            .into_iter()
            .filter_map(|session| {
                cards.remove(&session).map(|card| Deletion {
                    due: now,
                    chat: card.chat,
                    message: card.message,
                })
            })
            .collect()
    }

    /// Цикл удалятора: раз в несколько секунд сносит созревшие сообщения и
    /// карточки, которым истёк срок. Ошибки игнорируются: сообщение могли
    /// удалить руками, и это не повод шуметь в журнал каждым тиком.
    fn deletion_loop(&self) {
        loop {
            thread::sleep(DELETE_TICK);
            // Просроченные карточки становятся обычными удалениями: запись
            // стирается сразу, само сообщение уходит с текущим тиком.
            let stale_cards = self
                .cards
                .lock()
                .ok()
                .map(|mut cards| Self::take_expired(&mut cards, Instant::now()))
                .unwrap_or_default();
            if !stale_cards.is_empty()
                && let Ok(mut deletions) = self.deletions.lock()
            {
                deletions.extend(stale_cards);
                persist_runtime(self);
            }
            let due: Vec<Deletion> = self
                .deletions
                .lock()
                .ok()
                .map(|mut deletions| {
                    let now = Instant::now();
                    let (ready, later): (Vec<Deletion>, Vec<Deletion>) = deletions
                        .drain(..)
                        .partition(|deletion| deletion.due <= now);
                    *deletions = later;
                    ready
                })
                .unwrap_or_default();
            for deletion in &due {
                let _ = self.api.delete_message(deletion.chat, deletion.message);
            }
        }
    }
}

/// Вердикт вотчдога по зависшей карточке. Чистая функция ради тестов: сеть
/// уже опрошена, здесь только решение.
///
/// - простой меньше порога — не трогать, хоть сервер и говорит `idle`
///   (событие в пути, поток разберётся сам);
/// - сервер говорит `idle` — ход кончился, финалим `Done`;
/// - сессии нет (404, а не ошибка сети) — финалим «не найдена»;
/// - всё остальное (`busy`, `retry`, пусто, сеть моргнула) — бездействие.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchVerdict {
    /// Ничего не делать.
    NoChange,
    /// Ход кончился: отправить накопленное, карточка в `Done`.
    FinalizeDone,
    /// Сессия удалена: карточка в ❌ «сессия не найдена».
    FinalizeGone,
}

/// Решение вотчдога: зависшая ли карточка, что сказал сервер, есть ли сессия.
pub fn watchdog_verdict(stale: bool, status: Option<&str>, session_missing: bool) -> WatchVerdict {
    if !stale {
        return WatchVerdict::NoChange;
    }
    if status == Some("idle") {
        return WatchVerdict::FinalizeDone;
    }
    if session_missing {
        return WatchVerdict::FinalizeGone;
    }
    WatchVerdict::NoChange
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

/// Финальное ли состояние: после него карточка только ждёт `idle`, чтобы
/// тихо исчезнуть из записей.
pub fn is_terminal(status: CardStatus) -> bool {
    matches!(
        status,
        CardStatus::Done | CardStatus::Error | CardStatus::Stopped
    )
}

/// Ключ отправленной картинки: текст плюс признак клавиатуры. Сравнение по
/// ключу пропускает повторную правку: «message is not modified» тогда вообще
/// не возникает.
pub fn render_key(text: &str, keyboard: bool) -> String {
    if keyboard {
        format!("{text}\n[kb]")
    } else {
        format!("{text}\n[]")
    }
}

/// Пора ли править: картинка изменилась и троттлинг вышел. Финалы решают
/// сами и сюда не ходят.
pub fn should_render(last_render: &str, new_render: &str, last_edit: Instant) -> bool {
    last_render != new_render && last_edit.elapsed() >= EDIT_THROTTLE
}

/// Ошибка флуд-контроля: в тексте есть код 429.
pub fn is_flood(why: &str) -> bool {
    why.contains("429")
}

/// Сообщение пользователя ещё можно удалить: моложе 48 часов. Дата из апдейта
/// в секундах; часы телефона и ноутбука могут расходиться, поэтому запас —
/// только в одну сторону.
pub fn within_delete_limit(date_secs: i64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|age| age.as_secs() as i64)
        .unwrap_or(0);
    now.saturating_sub(date_secs) < DELETE_LIMIT.as_secs() as i64
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

/// Пишет состояние (сессия, папка, живые записи) в state-файл. Секреты не
/// трогаются — у снимка их нет в полях. Ошибка — предупреждение, а не смерть:
/// мост продолжит в памяти и попробует снова при следующей команде.
pub fn persist_runtime(handle: &BridgeHandle) {
    let snapshot = handle.state.lock().ok().map(|state| {
        let cards = handle
            .cards
            .lock()
            .ok()
            .map(|cards| {
                cards
                    .iter()
                    .map(|(session, card)| CardRef {
                        session: session.clone(),
                        chat: card.chat,
                        message: card.message,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let permissions = handle
            .perm_msgs
            .lock()
            .ok()
            .map(|messages| {
                messages
                    .iter()
                    .map(|(request, record)| PermRef {
                        request: request.clone(),
                        chat: record.chat,
                        message: record.message,
                    })
                    .collect()
            })
            .unwrap_or_default();
        super::config::Snapshot {
            opencode: handle.config.opencode.clone(),
            directory: state.directory.clone(),
            session: state.session.clone(),
            dunst: handle.config.dunst,
            delete_commands: handle.config.delete_commands,
            model: state.model.clone(),
            cards,
            permissions,
        }
    });
    if let Some(snapshot) = snapshot
        && let Err(why) = super::config::save_state_to(&handle.state_path, &snapshot)
    {
        super::log::warn(format!("мост: состояние не записано: {why}"));
    }
}

/// Сессии больше нет на сервере — только тогда состояние сбрасывается.
/// Ошибка проверки и живая сессия означают «оставить как есть»: недоступный
/// сервер — не доказательство отсутствия.
pub fn missing_session(check: &Result<Option<super::opencode::Session>, String>) -> bool {
    matches!(check, Ok(None))
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

    /// Смена сессии чистит ленту: карточки и сообщения с правами прошлой
    /// сессии уходят, записи стираются, `pending_permission` снимается —
    /// иначе кнопка «Разрешить» после перехода била бы не туда.
    #[test]
    fn switching_sessions_wipes_the_chat() {
        let dir = std::env::temp_dir().join(format!("hud-tg-wipe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        let (bridge, path) = bridge_with_temp_state(&dir);
        let handle = bridge.cloned();
        if let Ok(mut cards) = handle.cards.lock() {
            cards.insert(
                "ses_old".to_string(),
                LiveCard {
                    chat: 1,
                    message: 11,
                    label: "старая".to_string(),
                    started: Instant::now(),
                    last_edit: Instant::now(),
                    last_render: String::new(),
                    tool: None,
                    status: CardStatus::Done,
                    has_keyboard: false,
                    last_event: Instant::now(),
                    expires: None,
                },
            );
        }
        if let Ok(mut messages) = handle.perm_msgs.lock() {
            messages.insert(
                "perm_1".to_string(),
                PermMsg {
                    chat: 1,
                    message: 12,
                    title: "bash rm -rf".to_string(),
                },
            );
        }
        if let Ok(mut state) = handle.state.lock() {
            state.session = Some("ses_old".to_string());
            state.pending_permission = Some(commands::PendingPermission {
                id: "perm_1".to_string(),
                session: "ses_old".to_string(),
                title: "bash rm -rf".to_string(),
            });
        }
        // Сеть недоступна в тесте: удаление упадёт и уйдёт в журнал, а суть
        // проверки — что записи и `pending` исчезли.
        handle.wipe_chat();
        assert!(
            handle.cards.lock().expect("карточки").is_empty(),
            "карточки прошлой сессии стёрты"
        );
        assert!(
            handle.perm_msgs.lock().expect("права").is_empty(),
            "сообщения с правами стёрты"
        );
        assert!(
            handle
                .state
                .lock()
                .expect("состояние")
                .pending_permission
                .is_none(),
            "запрос прав прошлой сессии больше не ждёт ответа"
        );
        let value = read_temp_state(&path);
        let empty = |key: &str| {
            value
                .get(key)
                .and_then(serde_json::Value::as_array)
                .is_some_and(|items| items.is_empty())
        };
        assert!(
            empty("cards") && empty("permissions"),
            "state-файл чист: {value}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Мост для тестов состояния: живой путь заменён временным каталогом,
    /// чтобы тесты не трогали настоящий state-файл.
    fn bridge_with_temp_state(dir: &std::path::Path) -> (Bridge, std::path::PathBuf) {
        let config = Config {
            chat_id: Some(1),
            bot_token: "t".to_string(),
            ..Config::defaults()
        };
        let mut bridge = Bridge::new(config).expect("мост");
        let path = dir.join("telegram-state.json");
        bridge.state_path = path.clone();
        (bridge, path)
    }

    /// Читает JSON из временного state-файла.
    fn read_temp_state(path: &std::path::Path) -> serde_json::Value {
        let text = std::fs::read_to_string(path).expect("state-файл записан");
        serde_json::from_str(&text).expect("json")
    }

    /// Решение по проверке: сессии нет — сброс, остальное — оставить.
    #[test]
    fn missing_session_only_when_the_server_says_so() {
        assert!(missing_session(&Ok(None)), "404 — сессии нет");
        assert!(
            !missing_session(&Ok(Some(super::super::opencode::Session {
                id: "ses_1".to_string(),
                title: String::new(),
                directory: String::new(),
            }))),
            "живая сессия остаётся"
        );
        assert!(
            !missing_session(&Err("сервер недоступен".to_string())),
            "ошибка проверки — не доказательство"
        );
    }

    /// Откат: сохранённой сессии нет на сервере — состояние сбрасывается и
    /// пишется, мост не падает.
    #[test]
    fn fallback_clears_a_gone_session_and_persists_it() {
        let dir = std::env::temp_dir().join(format!("hud-tg-fallback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        let (bridge, path) = bridge_with_temp_state(&dir);
        {
            let mut state = bridge.state.lock().expect("состояние");
            state.session = Some("ses_gone".to_string());
            state.directory = "/tmp/мост".to_string();
        }
        bridge.apply_session_check(&Ok(None));
        assert!(
            bridge.state.lock().expect("состояние").session.is_none(),
            "сессия сброшена"
        );
        let value = read_temp_state(&path);
        assert_eq!(
            value.get("directory").and_then(serde_json::Value::as_str),
            Some("/tmp/мост"),
            "папка сохранена вместе со сбросом"
        );
        assert!(
            value.get("session").is_none(),
            "сброшенная сессия не пишется обратно"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ошибка проверки не трогает ни состояние, ни файл: сервер мог просто
    /// моргнуть.
    #[test]
    fn check_error_keeps_everything_untouched() {
        let dir = std::env::temp_dir().join(format!("hud-tg-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        let (bridge, path) = bridge_with_temp_state(&dir);
        {
            let mut state = bridge.state.lock().expect("состояние");
            state.session = Some("ses_mine".to_string());
        }
        bridge.apply_session_check(&Err("сервер недоступен".to_string()));
        assert_eq!(
            bridge.state.lock().expect("состояние").session.as_deref(),
            Some("ses_mine"),
            "сессия осталась"
        );
        assert!(!path.exists(), "файл не создан зря");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Сохранение после команды пишет сессию и папку во временный state-файл.
    #[test]
    fn persist_writes_session_and_directory_atomically() {
        let dir = std::env::temp_dir().join(format!("hud-tg-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        let (bridge, path) = bridge_with_temp_state(&dir);
        {
            let mut state = bridge.state.lock().expect("состояние");
            state.session = Some("ses_new".to_string());
            state.directory = "/tmp/новая".to_string();
        }
        persist_runtime(&bridge.cloned());
        let value = read_temp_state(&path);
        assert_eq!(
            value.get("session").and_then(serde_json::Value::as_str),
            Some("ses_new")
        );
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("каталог")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "остатки записи: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Правка уходит, только если картинка новая и троттлинг вышел. Финалы
    /// сюда не ходят: они решают сами.
    #[test]
    fn render_skips_duplicates_and_throttled_edits() {
        let fresh = render_key("новый текст", true);
        let old = Instant::now() - EDIT_THROTTLE - Duration::from_secs(1);
        assert!(
            should_render(&render_key("старый текст", true), &fresh, old),
            "свежая картинка уходит, когда троттлинг вышел"
        );
        assert!(
            !should_render(&fresh, &fresh, Instant::now()),
            "дубль не уходит"
        );
        assert!(
            !should_render(&render_key("старый текст", true), &fresh, Instant::now()),
            "троттлинг держит частые правки"
        );
        assert!(
            should_render(
                &render_key("старый текст", true),
                &fresh,
                Instant::now() - EDIT_THROTTLE
            ),
            "после паузы уходит"
        );
        assert!(
            !should_render(
                &render_key("текст", true),
                &render_key("текст", false),
                Instant::now()
            ),
            "смена клавиатуры — тоже новая картинка, но троттлинг всё равно держит"
        );
    }

    /// Все `callback_data` влезают в лимит Bot API 64 байта: и кнопки прав,
    /// и «Стоп». Длинный id запроса не должен ронять отправку клавиатуры.
    #[test]
    fn every_callback_data_fits_64_bytes() {
        let long_id = "per_".to_string() + &"x".repeat(40);
        for reply in [
            commands::PermissionReply::Once,
            commands::PermissionReply::Always,
            commands::PermissionReply::Reject,
        ] {
            let data = format!("perm:{long_id}:{}", reply.key());
            assert!(
                card::callback_len(&data) <= 64,
                "кнопка прав влезает: {data}"
            );
            assert_eq!(
                parse_callback(&data),
                Some((long_id.clone(), reply)),
                "длинный id разбирается обратно"
            );
        }
        let stop = card::stop_callback("ses_abcdef1234567890abcdef12");
        assert!(card::callback_len(&stop) <= 64, "стоп влезает: {stop}");
        assert_eq!(
            card::parse_stop_callback(&stop),
            Some("ses_abcdef1234567890abcdef12")
        );
    }

    /// Префиксы не пересекаются: кнопка меню не должна разбираться как права
    /// или как «Стоп», и наоборот. Иначе одно нажатие выполнило бы чужое
    /// действие — например, ответило бы «Разрешить» на пустом месте.
    #[test]
    fn menu_callbacks_do_not_collide_with_rights_or_stop() {
        for action in [
            menu::Action::Open(menu::Screen::Root),
            menu::Action::Open(menu::Screen::Sessions),
            menu::Action::Open(menu::Screen::Models),
            menu::Action::Open(menu::Screen::Help),
            menu::Action::PickSession(1),
            menu::Action::PickModel(1),
            menu::Action::NewSession,
            menu::Action::Stop,
        ] {
            let data = action.data();
            assert_eq!(
                parse_callback(&data),
                None,
                "меню не должно читаться как права: {data}"
            );
            assert_eq!(
                card::parse_stop_callback(&data),
                None,
                "меню не должно читаться как стоп: {data}"
            );
        }
        // И наоборот: настоящие кнопки прав и «Стоп» не попадают в меню.
        assert_eq!(menu::parse_action("perm:p1:once"), None);
        assert_eq!(menu::parse_action("stop:ses_abc"), None);
    }

    /// Кнопки меню не длиннее лимита Bot API — как и кнопки прав: проверка на
    /// экранах с длинными id сессий и названиями моделей.
    #[test]
    fn menu_buttons_fit_64_bytes_with_long_data() {
        let long = "x".repeat(300);
        let panels = [
            menu::root(&menu::RootView {
                session: Some((format!("ses_{long}"), long.clone())),
                busy: false,
                model: Some(long.clone()),
                directory: long.clone(),
            }),
            menu::sessions(&[(format!("ses_{long}"), long.clone())], None),
            menu::models(&[(long.clone(), long.clone())], None),
            menu::help("команда\n".repeat(500).as_str()),
        ];
        for panel in &panels {
            assert!(menu::within_callback_limit(panel), "кнопка длиннее 64 байт");
            for button in panel.keyboard.iter().flatten() {
                assert!(card::callback_len(&button.callback_data) <= 64);
            }
        }
    }

    /// Строки итога по правам: одна строка, команда экранирована, у «всегда»
    /// и «отклонено» команды нет.
    #[test]
    fn permission_result_lines_are_one_escaped_line() {
        let line = BridgeHandle::permission_result_line(
            commands::PermissionReply::Once,
            "Выполнить: rm -rf /tmp/x <test>",
        );
        assert!(
            line.starts_with("✔ Разрешено один раз · "),
            "префикс: {line}"
        );
        assert!(line.contains("&lt;test&gt;"), "экранировано: {line}");
        assert!(!line.contains('\n'), "одна строка");
        assert_eq!(
            BridgeHandle::permission_result_line(commands::PermissionReply::Always, "ls"),
            "✔ Разрешено всегда"
        );
        assert_eq!(
            BridgeHandle::permission_result_line(commands::PermissionReply::Reject, "ls"),
            "✖ Отклонено"
        );
    }

    /// Удалять можно только свежее: лимит Telegram — 48 часов.
    #[test]
    fn delete_limit_is_48_hours() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("время")
            .as_secs() as i64;
        assert!(within_delete_limit(now - 60), "минуту назад можно");
        assert!(within_delete_limit(now + 60), "часы вперёд не страшны");
        assert!(
            !within_delete_limit(now - 49 * 3600),
            "двухдневное трогать нельзя"
        );
    }

    /// Чистка при старте: карточки правятся в «перезапущен» со снятием
    /// клавиатуры, у прав клавиатура просто снимается, записи стираются.
    /// Сеть недоступна — правки падают в журнал, а записи всё равно чистятся:
    /// стартовать мост должен в любом случае.
    #[test]
    fn startup_cleanup_edits_and_clears_records() {
        let dir =
            std::env::temp_dir().join(format!("hud-tg-cleanup-records-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        let path = dir.join("telegram-state.json");
        let stale = serde_json::json!({
            "session": "ses_old",
            "cards": [{ "session": "ses_old", "chat": 1, "message": 11 }],
            "permissions": [{ "request": "perm_old", "chat": 1, "message": 12 }],
        });
        std::fs::write(&path, serde_json::to_string(&stale).expect("json")).expect("запись");
        let (bridge, _) = bridge_with_temp_state(&dir);
        // Путь уже подменён: чистка читает тот же временный файл.
        bridge.startup_cleanup();
        let value = read_temp_state(&path);
        assert!(
            value
                .get("cards")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|cards| cards.is_empty()),
            "карточки стёрты: {value}"
        );
        assert!(
            value
                .get("permissions")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|permissions| permissions.is_empty()),
            "права стёрты: {value}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Чистка с отсутствующим и битым файлом: молча нечего чистить, мост не
    /// падает, файл не создаётся зря… точнее, создаётся пустым снимком через
    /// persist — главное, что без паники и без чужих записей.
    #[test]
    fn startup_cleanup_survives_missing_and_broken_files() {
        let dir =
            std::env::temp_dir().join(format!("hud-tg-cleanup-broken-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        let (bridge, _) = bridge_with_temp_state(&dir);
        // Файла нет: чистить нечего, паники нет.
        bridge.startup_cleanup();
        let broken = dir.join("broken.json");
        std::fs::write(&broken, "{ не json").expect("запись");
        let (refs, _) = (config::load_refs_from(&broken), ());
        assert!(refs.0.is_empty() && refs.1.is_empty(), "битый файл — пусто");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// «Готово» исчезает само: просроченная карточка уходит к удалению и
    /// стирается из записей, а живая и без срока — остаётся на месте.
    #[test]
    fn done_card_expires_and_leaves_for_deletion() {
        fn card(expires: Option<Instant>) -> LiveCard {
            LiveCard {
                chat: 7,
                message: 42,
                label: "мост".to_string(),
                started: Instant::now(),
                last_edit: Instant::now(),
                last_render: String::new(),
                tool: None,
                status: CardStatus::Done,
                has_keyboard: false,
                last_event: Instant::now(),
                expires,
            }
        }
        let now = Instant::now();
        let mut cards = HashMap::from([
            (
                "ses_done".to_string(),
                card(Some(now - Duration::from_secs(1))),
            ),
            (
                "ses_live".to_string(),
                card(Some(now + Duration::from_secs(30))),
            ),
            ("ses_forever".to_string(), card(None)),
        ]);
        let mut expired = BridgeHandle::take_expired(&mut cards, now);
        expired.sort_by_key(|deletion| deletion.message);
        assert_eq!(
            expired
                .iter()
                .map(|deletion| (deletion.chat, deletion.message))
                .collect::<Vec<_>>(),
            vec![(7, 42)],
            "ушла только просроченная"
        );
        assert!(expired[0].due <= now, "удаление созрело сразу");
        assert_eq!(cards.len(), 2, "запись просроченной стёрта");
        assert!(!cards.contains_key("ses_done"));
        assert!(BridgeHandle::take_expired(&mut cards, now).is_empty());
    }

    /// Очередь завершённых: повтор по тому же id заменяет, порядок — по
    /// первому появлению, пустые не отдаются, но очередь чистят.
    #[test]
    fn completed_queue_replaces_dedupes_and_skips_empty() {
        let mut queue = Vec::new();
        BridgeHandle::stash_completed(&mut queue, "msg_1", "первая");
        BridgeHandle::stash_completed(&mut queue, "msg_2", "   ");
        BridgeHandle::stash_completed(&mut queue, "msg_1", "первая дописана");
        BridgeHandle::stash_completed(&mut queue, "msg_3", "третья");
        assert_eq!(
            BridgeHandle::drain_visible(&mut queue),
            vec!["первая дописана", "третья"],
            "порядок и замена, пустые мимо"
        );
        assert!(queue.is_empty(), "очередь забрана целиком");
        assert!(
            BridgeHandle::drain_visible(&mut queue).is_empty(),
            "второй раз отдавать нечего"
        );
    }

    /// Вердикт вотчдога: свежие не трогаем, `idle` финалим, удаление
    /// подтверждаем только точным 404, остальное — бездействие.
    #[test]
    fn watchdog_verdict_covers_stale_idle_gone_and_noise() {
        use WatchVerdict::*;
        assert_eq!(
            watchdog_verdict(false, Some("idle"), false),
            NoChange,
            "свежая карточка не трогается даже при idle: событие в пути"
        );
        assert_eq!(
            watchdog_verdict(true, Some("idle"), false),
            FinalizeDone,
            "сервер сказал idle — ход кончился"
        );
        assert_eq!(
            watchdog_verdict(true, None, true),
            FinalizeGone,
            "точный 404 — сессия удалена"
        );
        for status in [None, Some("busy"), Some("retry"), Some("unknown")] {
            assert_eq!(
                watchdog_verdict(true, status, false),
                NoChange,
                "без idle и без 404 — бездействие: {status:?}"
            );
        }
        assert_eq!(
            watchdog_verdict(true, Some("idle"), true),
            FinalizeDone,
            "idle важнее удаления: ход кончился штатно"
        );
    }

    /// Новейшая картинка побеждает: из быстрой серии правок уходит только
    /// последняя, промежуточные отбрасываются пропуском дублей и троттлингом.
    #[test]
    fn newest_render_wins_over_stale_edits() {
        let old = Instant::now() - EDIT_THROTTLE - Duration::from_secs(1);
        let first = render_key("текст 1", true);
        let second = render_key("текст 1 дописан", true);
        // Первая правка ушла…
        assert!(should_render(&render_key("черновик", true), &first, old));
        // …вторая пришла раньше троттлинга — ждёт…
        assert!(!should_render(&first, &second, Instant::now()));
        // …а когда троттлинг вышел, уходит именно она, а не первая.
        assert!(should_render(&first, &second, old));
        // Повтор той же картинки после отправки — не уходит никогда.
        assert!(!should_render(&second, &second, old));
    }
}

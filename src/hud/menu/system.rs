//! Системные providers меню: команды, поколения и динамические уровни.
//!
//! Статическому дереву из `tree` не хватает одного: списков, которые появляются
//! только после опроса системы (сети Wi-Fi, устройства звука и Bluetooth).
//! Опрашивать систему в момент отрисовки нельзя — зависший демон повесил бы
//! кадр, а запрос на каждый hover/scroll плодил бы процессы. Поэтому здесь три
//! маленькие вещи вместо фреймворка:
//!
//! * `CommandSpec` — что запустить: программа, аргументы без shell и
//!   секретный аргумент с отредактированным представлением для логов.
//!   Пароль Wi-Fi сейчас идёт через `CmdArg::Secret`, то есть в `argv`
//!   процесса; скрыт он только от журналов, статусов, `Debug` и снимков.
//!   Перевод на stdin — отдельная будущая задача.
//! * `CommandRunner` — как запустить: timeout, process group cleanup и
//!   записывающий `ScriptedRunner` для тестов;
//! * `ProviderSlot` — где лежит результат: поколение отсекает устаревшие
//!   ответы, повторный вход пользуется кэшем, явный refresh берёт новое
//!   поколение.
//!
//! Потоки принадлежат вызывающему циклу (как пул миниатюр в wallpaper):
//! `spawn_fetch` только запускает работу и отдаёт приёмник, UI-поток
//! подбирает готовое через `try_recv` и никогда ничего не ждёт. Конкретных
//! audio/Wi-Fi/Bluetooth providers здесь нет — только механизм и подставной
//! провайдер в тестах, показывающий цепочку W5.1.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::Duration;

use super::settings::Language;
use super::settings_icons;
use super::tree::Node;

/// Поколение запроса: монотонный счётчик слота. Ответ старше текущего
/// поколения выбрасывается, а не перетирает свежий snapshot.
pub type Generation = u64;

/// Ключ динамического раздела: чей snapshot лежит в слоте и какой уровень
/// обновлять после команды. Статические строки, как идентификаторы разделов
/// в `SECTIONS`: сравниваются дёшево и копируются.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProviderKey(&'static str);

impl ProviderKey {
    /// Ключ раздела по имени. `const`, чтобы будущие разделы объявлялись
    /// рядом с деревом, а не строились из `String` на каждый кадр.
    pub const fn new(name: &'static str) -> Self {
        ProviderKey(name)
    }

    /// Имя ключа для статусов и журнала. Секретов здесь нет по построению.
    pub const fn name(self) -> &'static str {
        self.0
    }

    /// Будущий раздел звука (W5.1).
    pub const AUDIO: ProviderKey = ProviderKey::new("audio");
    /// Будущий раздел Wi-Fi (W5.2).
    pub const WIFI: ProviderKey = ProviderKey::new("wifi");
    /// Будущий раздел Bluetooth (W5.3).
    pub const BLUETOOTH: ProviderKey = ProviderKey::new("bluetooth");
    /// Раздел дисплеев (W5.4): состояние выводов niri.
    pub const OUTPUTS: ProviderKey = ProviderKey::new("outputs");
    /// Раздел устройств (W5.5): накопители и безопасное извлечение.
    pub const STORAGE: ProviderKey = ProviderKey::new("storage");
    /// Раздел VPN (W5.7): режимы поверх селекторов Mihomo.
    pub const VPN: ProviderKey = ProviderKey::new("vpn");
}

/// Аргумент команды: обычное значение или секрет. Секрет не попадает в
/// `Debug`, статус, журнал и снимок — но в настоящий `argv` попадает, а значит
/// виден в списке процессов. Прячьте его оттуда только через stdin.
///
/// Про `Clone`: он нужен архитектуре (`Action` и дерево клонируются целиком),
/// поэтому `CmdArg::Secret(String)` технически клонируем вместе с командой.
/// Убрать это без большого рефактора нельзя — см. задачу ниже. Правило вместо
/// этого: секрет копируется только внутрь команды при submit и никогда —
/// наружу в текст.
#[derive(Clone, PartialEq, Eq)]
pub enum CmdArg {
    /// Обычный аргумент: виден в логах как есть.
    Plain(String),
    /// Секрет (пароль Wi-Fi): в логах — `<redacted>`, в `argv` — как есть.
    Secret(String),
}

impl CmdArg {
    /// Настоящее значение для `Command::arg`.
    pub fn value(&self) -> &str {
        match self {
            CmdArg::Plain(value) | CmdArg::Secret(value) => value,
        }
    }

    /// Секрет ли это: такие аргументы прячутся из любого текста.
    pub fn is_secret(&self) -> bool {
        matches!(self, CmdArg::Secret(_))
    }
}

impl std::fmt::Debug for CmdArg {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CmdArg::Plain(value) => formatter.debug_tuple("Plain").field(value).finish(),
            CmdArg::Secret(_) => formatter.write_str("Secret(<redacted>)"),
        }
    }
}

/// Байты на stdin команды. Механизм готов, но пароль Wi-Fi сейчас уходит
/// через `CmdArg::Secret`, то есть в `argv`: значение скрыто от журналов,
/// статусов и `Debug`, но доступно тому, кто читает `argv` процесса
/// (`/proc/<pid>/cmdline`, `ps`). Чтобы пароль не попадал туда, нужен ввод
/// через stdin — это отдельная задача. `Debug` здесь показывает длину.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretData(Vec<u8>);

impl SecretData {
    /// Скрытые байты из строки.
    pub fn from_str(text: &str) -> Self {
        SecretData(text.as_bytes().to_vec())
    }

    /// Настоящие байты для записи в stdin дочернего процесса.
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for SecretData {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "SecretData(<redacted {} bytes>)", self.0.len())
    }
}

/// Что запустить: программа, аргументы и необязательный секретный stdin.
/// Shell нет нигде: `argv` уходит напрямую в `Command`, `sh -c` запрещён
/// конструкцией — поля под строку команды просто нет.
#[derive(Clone, PartialEq, Eq)]
pub struct CommandSpec {
    program: String,
    args: Vec<CmdArg>,
    stdin: Option<SecretData>,
    /// Сколько ждать процесс. По умолчанию — общий короткий таймаут чтений;
    /// подключение к сети занимает секунды и живёт по своему правилу.
    timeout: Duration,
}

impl CommandSpec {
    /// Команда без аргументов и stdin.
    pub fn new(program: &str) -> Self {
        CommandSpec {
            program: program.to_string(),
            args: Vec::new(),
            stdin: None,
            timeout: super::super::data::oneshot::TIMEOUT,
        }
    }

    /// Свой таймаут для команды, которая по природе долгая: подключение к
    /// точке доступа. Бесконечным он быть не должен — иначе кнопка «Отменить»
    /// не спасёт, а worker-утипнет меню намертво.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Таймаут команды для исполнителя.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Обычный аргумент. Возвращает `self` для цепочки.
    pub fn arg(mut self, value: &str) -> Self {
        self.args.push(CmdArg::Plain(value.to_string()));
        self
    }

    /// Секретный аргумент: в `argv` как есть, в логах — `<redacted>`.
    /// Предпочтительнее stdin, но некоторые команды секрета иначе не берут.
    pub fn secret_arg(mut self, value: &str) -> Self {
        self.args.push(CmdArg::Secret(value.to_string()));
        self
    }

    /// Секретный stdin: пароль для будущей команды Wi-Fi. В логах не бывает.
    pub fn stdin(mut self, data: SecretData) -> Self {
        self.stdin = Some(data);
        self
    }

    /// Имя программы для статусов и `describe`. Секретов не содержит.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Настоящие `(программа, аргументы)` для `Command`. Секреты здесь
    /// настоящие — вызывать только в исполнителе, никогда в логах.
    pub fn argv(&self) -> (String, Vec<String>) {
        (
            self.program.clone(),
            self.args
                .iter()
                .map(|arg| arg.value().to_string())
                .collect(),
        )
    }

    /// Байты stdin для исполнителя. `None` — stdin не нужен.
    pub fn stdin_bytes(&self) -> Option<&[u8]> {
        self.stdin.as_ref().map(|data| data.bytes())
    }

    /// Есть ли секреты: нужно прятать даже сам факт — нет, только значения.
    /// Метод для тестов и для журнала: строка без секретов.
    pub fn has_secrets(&self) -> bool {
        self.stdin.is_some() || self.args.iter().any(CmdArg::is_secret)
    }

    /// Строка для журнала и статусов: секреты заменены на `<redacted>`,
    /// аргументы с пробелами — в кавычках, как в `log::command_line`.
    pub fn log_line(&self) -> String {
        let mut parts = vec![self.program.clone()];
        let mut secret_next = false;
        for arg in &self.args {
            // Совместимость со старым правилом журнала: значение после
            // `password` прячется, даже если его забыли пометить секретом.
            let text = if arg.is_secret() || secret_next || arg.value() == "password" {
                secret_next = arg.value() == "password" && !arg.is_secret();
                "<redacted>"
            } else {
                secret_next = false;
                arg.value()
            };
            parts.push(if text.contains(char::is_whitespace) {
                format!("{text:?}")
            } else {
                text.to_string()
            });
        }
        if self.stdin.is_some() {
            parts.push("<stdin redacted>".to_string());
        }
        parts.join(" ")
    }
}

impl std::fmt::Debug for CommandSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommandSpec")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("stdin", &self.stdin)
            .finish()
    }
}

/// Вывод выполненной команды: код, stdout и stderr. Ненулевой код — не
/// обязательно ошибка исполнителя: провайдер сам решает, что значит вывод.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutput {
    /// Код выхода. `None` — процесс не дождались (так не бывает: timeout
    /// убивает процесс и возвращает ошибку, а не пустой код).
    pub status: Option<i32>,
    /// Стандартный вывод как есть, без trim: парсер решает сам.
    pub stdout: String,
    /// Стандартный вывод ошибок: подсказка для сообщения в меню.
    pub stderr: String,
}

impl CommandOutput {
    /// Успешный пустой вывод: ответ записывающего исполнителя в тестах.
    pub fn empty_success() -> Self {
        CommandOutput {
            status: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// Успех ли это: только нулевой код.
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }
}

/// Почему команда не выполнилась. Ненулевой код сюда не попадает: это
/// нормальный `CommandOutput`, пусть и неуспешный.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandError {
    /// Программа не запустилась.
    Spawn(String),
    /// Превышен timeout: процесс и его группа уже убиты.
    Timeout {
        /// Что запускали: для сообщения в меню, секретов нет.
        program: String,
        /// Сколько ждали, мс.
        millis: u64,
    },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Spawn(error) => write!(formatter, "не запустился: {error}"),
            CommandError::Timeout { program, millis } => {
                write!(formatter, "{program} не ответил за {millis} мс")
            }
        }
    }
}

/// Исполнитель команд для провайдеров. Живой идёт через `oneshot` с timeout,
/// тестовый — `ScriptedRunner` с заранее записанными ответами.
pub trait CommandRunner {
    /// Выполнить команду и вернуть вывод. Блокирует вызывающий поток до
    /// timeout — поэтому живые вызовы идут только из worker-потоков.
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, CommandError>;
}

/// Живой исполнитель: `oneshot` с timeout и cleanup группы процессов.
/// Состояния не держит, поэтому один на все worker-потоки.
pub struct SystemCommandRunner {
    timeout: Duration,
}

impl SystemCommandRunner {
    /// Исполнитель с заданным timeout. Для сканирующих команд (будущий
    /// `nmcli device wifi list`) timeout берут больше секундного.
    pub fn with_timeout(timeout: Duration) -> Self {
        SystemCommandRunner { timeout }
    }
}

impl Default for SystemCommandRunner {
    fn default() -> Self {
        Self::with_timeout(super::super::data::oneshot::TIMEOUT)
    }
}

impl CommandRunner for SystemCommandRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, CommandError> {
        let (program, args) = spec.argv();
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        match super::super::data::oneshot::run_with_timeout_stdin(
            &program,
            &borrowed,
            spec.stdin_bytes(),
            self.timeout,
        ) {
            Ok((status, stdout, stderr)) => Ok(CommandOutput {
                status,
                stdout,
                stderr,
            }),
            Err(error) if error.contains("не ответил за") => {
                Err(CommandError::Timeout {
                    program,
                    millis: self.timeout.as_millis() as u64,
                })
            }
            Err(error) => Err(CommandError::Spawn(error)),
        }
    }
}

/// Записывающий исполнитель для тестов: отдаёт ответы по сценарию и пишет
/// только отредактированные строки — секрет из `argv` в записях не должен
/// появиться ни разу. Реальных процессов не запускает.
pub struct ScriptedRunner {
    calls: std::cell::RefCell<Vec<String>>,
    script: std::cell::RefCell<VecDeque<Result<CommandOutput, CommandError>>>,
}

impl ScriptedRunner {
    /// Исполнитель с очередью ответов: каждый `run` забирает первый.
    pub fn new(script: Vec<Result<CommandOutput, CommandError>>) -> Self {
        ScriptedRunner {
            calls: std::cell::RefCell::new(Vec::new()),
            script: std::cell::RefCell::new(script.into()),
        }
    }

    /// Что вызывали: отредактированные строки, без секретов.
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Сколько команд выполнено: проверка коалесинга запросов.
    pub fn call_count(&self) -> usize {
        self.calls.borrow().len()
    }
}

impl CommandRunner for ScriptedRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, CommandError> {
        self.calls.borrow_mut().push(spec.log_line());
        self.script
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(CommandError::Spawn(
                "сценарий пуст: ответ не записан".to_string(),
            )))
    }
}

/// Ошибка провайдера: типизированная, а не строка. Меню показывает
/// человеческое сообщение, а не вариант как есть.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderError {
    /// Нет инструмента или железа: раздел скрывается через `visible`.
    Unavailable(String),
    /// Команда превысила timeout.
    Timeout {
        /// Что запускали.
        program: String,
        /// Сколько ждали, мс.
        millis: u64,
    },
    /// Команда выполнилась, но ответ не годится: ненулевой код или мусор.
    Command {
        /// Что запускали.
        program: String,
        /// Короткая причина из stderr или парсера.
        message: String,
    },
    /// Вывод не разобрался.
    Parse(String),
}

impl ProviderError {
    /// Сообщение для строки меню: коротко и без жаргона.
    pub fn message(&self) -> String {
        match self {
            ProviderError::Unavailable(reason) => reason.clone(),
            ProviderError::Timeout { program, millis } => {
                format!("{program} не ответил за {millis} мс")
            }
            ProviderError::Command { program, message } => format!("{program}: {message}"),
            ProviderError::Parse(message) => message.clone(),
        }
    }
}

impl From<CommandError> for ProviderError {
    fn from(error: CommandError) -> Self {
        match error {
            CommandError::Spawn(message) => ProviderError::Command {
                program: String::new(),
                message,
            },
            CommandError::Timeout { program, millis } => ProviderError::Timeout { program, millis },
        }
    }
}

/// Состояние слота провайдера: пусто, загрузка, готовый snapshot, ошибка или
/// недоступность. Ошибки не ретраятся сами: повтор — только явный refresh.
#[derive(Clone, Debug, PartialEq)]
pub enum SlotState<Snapshot> {
    /// Запроса ещё не было: первый вход начнёт загрузку.
    Empty,
    /// Запрос в полёте: второй такой же не запускается.
    Loading,
    /// Готовый типизированный snapshot: повторный вход берёт его из кэша.
    Ready(Snapshot),
    /// Последний запрос не удался: строка ошибки и пункт обновления.
    Error(String),
    /// Инструмента или железа нет: раздел скрывается вызывающим кодом.
    Unavailable(String),
}

/// Слот провайдера: snapshot, поколение и кэш жизненного цикла. Потоков не
/// знает: worker принадлежит циклу окна, сюда приходит только готовый
/// результат с поколением. Параметр — тип snapshot, а не словарь строк.
#[derive(Clone, Debug)]
pub struct ProviderSlot<Snapshot> {
    key: ProviderKey,
    generation: Generation,
    state: SlotState<Snapshot>,
    /// Запрос в полёте поверх готового снимка: уровень продолжает показывать
    /// прежние строки и не мигает «Загрузка…». Отдельное поле, а не
    /// `Loading`, — иначе старый снимок пришлось бы куда-то класть.
    refreshing: bool,
}

impl<Snapshot> ProviderSlot<Snapshot> {
    /// Пустой слот нулевого поколения.
    pub fn new(key: ProviderKey) -> Self {
        ProviderSlot {
            key,
            generation: 0,
            state: SlotState::Empty,
            refreshing: false,
        }
    }

    /// Чей это слот.
    pub fn key(&self) -> ProviderKey {
        self.key
    }

    /// Текущее поколение: устаревшие ответы с меньшим отбрасываются.
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Текущее состояние для отрисовки уровня.
    pub fn state(&self) -> &SlotState<Snapshot> {
        &self.state
    }

    /// Готовый snapshot из кэша, если он есть.
    pub fn snapshot(&self) -> Option<&Snapshot> {
        match &self.state {
            SlotState::Ready(snapshot) => Some(snapshot),
            _ => None,
        }
    }

    /// Пометить раздел недоступным: инструмента или железа нет, запрашивать
    /// нечего. Поколение не растёт — это не запрос.
    pub fn mark_unavailable(&mut self, reason: &str) {
        self.state = SlotState::Unavailable(reason.to_string());
        self.refreshing = false;
    }

    /// Запросить snapshot: только из `Empty`. Из `Loading`, `Ready` и `Error`
    /// возвращает `None` — повторный вход не плодит процессы, ошибка не
    /// ретраится сама. `Some` — поколение, с которым ждать ответ.
    pub fn request(&mut self) -> Option<Generation> {
        if !matches!(self.state, SlotState::Empty) {
            return None;
        }
        self.generation += 1;
        self.state = SlotState::Loading;
        self.refreshing = false;
        Some(self.generation)
    }

    /// Явное обновление: новое поколение из любого состояния, кроме
    /// `Unavailable`. `Some` — поколение нового запроса.
    pub fn refresh(&mut self) -> Option<Generation> {
        if matches!(self.state, SlotState::Unavailable(_)) {
            return None;
        }
        self.generation += 1;
        self.state = SlotState::Loading;
        self.refreshing = false;
        Some(self.generation)
    }

    /// Обновление поверх готового снимка: пока идёт запрос, уровень показывает
    /// прежние строки. Так ведёт себя refresh после действия — «Загрузка…»
    /// после нажатия мигала бы списком, который пользователь только что
    /// читал. `None` — раздел недоступен.
    pub fn refresh_soft(&mut self) -> Option<Generation> {
        if matches!(self.state, SlotState::Unavailable(_)) {
            return None;
        }
        self.generation += 1;
        if matches!(self.state, SlotState::Ready(_)) {
            self.refreshing = true;
        } else {
            self.state = SlotState::Loading;
        }
        Some(self.generation)
    }

    /// Идёт ли мягкое обновление поверх готового снимка.
    pub fn is_refreshing(&self) -> bool {
        self.refreshing
    }

    /// Принять результат worker-потока. Чужое поколение — `false`, ответ
    /// выброшен. Своё, но не в `Loading` (двойная доставка) — тоже `false`,
    /// состояние не трогаем. Иначе — `true`, состояние обновлено.
    pub fn apply(&mut self, generation: Generation, result: Result<Snapshot, String>) -> bool {
        let waiting = matches!(self.state, SlotState::Loading) || self.refreshing;
        if generation != self.generation || !waiting {
            return false;
        }
        self.refreshing = false;
        self.state = match result {
            Ok(snapshot) => SlotState::Ready(snapshot),
            Err(message) => SlotState::Error(message),
        };
        true
    }
}

/// Запущенная выборка: поколение и приёмник результата. Создаётся
/// [`spawn_fetch`], проверяется через `try_recv` без ожидания.
pub struct Fetch<Result> {
    /// Поколение, с которым ждать ответ: прикладывается к `apply`.
    pub generation: Generation,
    receiver: mpsc::Receiver<Result>,
}

impl<Result> Fetch<Result> {
    /// Забрать готовый результат, если worker уже ответил. `None` — ещё
    /// работает или канал пуст: UI продолжает рисовать `Loading`.
    pub fn try_take(&self) -> Option<Result> {
        self.receiver.try_recv().ok()
    }
}

/// Запустить выборку в worker-потоке и сразу вернуться: UI-поток никогда
/// не ждёт системную команду. Результат подбирается через `try_take` в тике
/// цикла (там же, где опрос Wayland), затем уходит в `apply` с поколением.
pub fn spawn_fetch<Result: Send + 'static>(
    generation: Generation,
    job: impl FnOnce() -> Result + Send + 'static,
) -> Fetch<Result> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("hud-menu-fetch".to_string())
        .spawn(move || {
            let _ = tx.send(job());
        })
        .expect("поток выборки меню");
    Fetch {
        generation,
        receiver: rx,
    }
}

/// Read-only провайдер системного раздела: snapshot получает, систему не
/// меняет. Конкретные audio/Wi-Fi/Bluetooth реализации появятся в W5.1–W5.3,
/// здесь только форма, под которую они пишутся.
pub trait SystemProvider: Send + Sync {
    /// Тип snapshot: громкости, сети, устройства — но не словарь строк.
    type Snapshot: Clone + Send + 'static;

    /// Ключ раздела: какой слот и уровень обновлять.
    fn key(&self) -> ProviderKey;

    /// Получить snapshot через исполнителя. Только чтение: менять состояние
    /// системы (подключать, переключать) здесь запрещено.
    fn fetch(&self, runner: &dyn CommandRunner) -> Result<Self::Snapshot, ProviderError>;

    /// Строки меню из готового snapshot. Пустой список — тоже норма:
    /// уровень покажет «нет элементов», а не пустоту.
    fn nodes(&self, snapshot: &Self::Snapshot, lang: Language) -> Vec<Node>;
}

/// Заголовок строки загрузки на языке меню.
pub fn loading_title(lang: Language) -> &'static str {
    match lang {
        Language::Ru => "Загрузка…",
        Language::En => "Loading…",
    }
}

/// Заголовок пустого готового списка на языке меню.
pub fn empty_title(lang: Language) -> &'static str {
    match lang {
        Language::Ru => "Нет элементов",
        Language::En => "Nothing here",
    }
}

/// Заголовок пункта явного обновления на языке меню.
pub fn refresh_title(lang: Language) -> &'static str {
    match lang {
        Language::Ru => "Обновить",
        Language::En => "Refresh",
    }
}

/// Узлы уровня из состояния слота: те же `Node`, что и у статического
/// дерева, поэтому отрисовка не меняется. Ошибка даёт две строки —
/// сообщение и пункт обновления; пустой готовый список — одну строку.
pub fn slot_nodes(key: ProviderKey, state: &SlotState<Vec<Node>>, lang: Language) -> Vec<Node> {
    match state {
        SlotState::Empty | SlotState::Loading => {
            vec![Node::info(settings_icons::DOT, loading_title(lang))]
        }
        SlotState::Ready(nodes) if nodes.is_empty() => {
            vec![Node::info(settings_icons::DOT, empty_title(lang))]
        }
        SlotState::Ready(nodes) => nodes.clone(),
        SlotState::Error(message) => vec![
            Node::info(settings_icons::DOT, message),
            Node::action(
                settings_icons::UP,
                refresh_title(lang),
                super::action::Action::RefreshDynamic(key),
            ),
        ],
        SlotState::Unavailable(reason) => {
            vec![Node::info(settings_icons::DOT, reason)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::action::Action;
    use super::*;

    /// Подставной snapshot: громкость и mute. Стоит здесь, а не в W5.1,
    /// чтобы доказать цепочку «команда → разбор → тип → строки» на чём-то
    /// настоящем, но без реального `wpctl`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct FakeVolume {
        level: u8,
        muted: bool,
    }

    /// Подставной провайдер: разбирает мини-формат `vol=42 muted=0`.
    struct FakeVolumeProvider;

    impl SystemProvider for FakeVolumeProvider {
        type Snapshot = FakeVolume;

        fn key(&self) -> ProviderKey {
            ProviderKey::AUDIO
        }

        fn fetch(&self, runner: &dyn CommandRunner) -> Result<FakeVolume, ProviderError> {
            let spec = CommandSpec::new("wpctl").arg("get-volume").arg("@DEFAULT@");
            let output = runner.run(&spec)?;
            if !output.success() {
                return Err(ProviderError::Command {
                    program: spec.program().to_string(),
                    message: output.stderr.trim().to_string(),
                });
            }
            parse_fake_volume(&output.stdout)
                .ok_or_else(|| ProviderError::Parse("не разобран вывод wpctl".to_string()))
        }

        fn nodes(&self, snapshot: &FakeVolume, lang: Language) -> Vec<Node> {
            let title = match lang {
                Language::Ru => format!("Громкость: {}%", snapshot.level),
                Language::En => format!("Volume: {}%", snapshot.level),
            };
            let _ = snapshot.muted;
            vec![Node::info(settings_icons::DOT, &title)]
        }
    }

    fn parse_fake_volume(text: &str) -> Option<FakeVolume> {
        let mut level = None;
        let mut muted = false;
        for part in text.split_whitespace() {
            let (key, value) = part.split_once('=')?;
            match key {
                "vol" => level = value.parse::<u8>().ok(),
                "muted" => muted = value != "0",
                _ => {}
            }
        }
        Some(FakeVolume {
            level: level?,
            muted,
        })
    }

    fn ok_output(stdout: &str) -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            status: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    /// Спецификация хранит программу, аргументы и stdin отдельно.
    #[test]
    fn command_spec_holds_program_args_and_stdin() {
        let spec = CommandSpec::new("nmcli")
            .arg("device")
            .arg("wifi")
            .stdin(SecretData::from_str("s3cr3t"));
        assert_eq!(spec.program(), "nmcli");
        let (program, args) = spec.argv();
        assert_eq!((program.as_str(), args.len()), ("nmcli", 2));
        assert_eq!(spec.stdin_bytes(), Some("s3cr3t".as_bytes()));
        assert!(spec.has_secrets());
        assert!(!CommandSpec::new("true").has_secrets());
    }

    /// Секреты не светятся ни в `Debug`, ни в строке журнала, но едут в argv.
    #[test]
    fn secrets_are_redacted_everywhere_except_argv() {
        let spec = CommandSpec::new("nmcli")
            .arg("connect")
            .secret_arg("s3cr3t-pw")
            .stdin(SecretData::from_str("s3cr3t-pw"));
        let debug = format!("{spec:?}");
        assert!(!debug.contains("s3cr3t-pw"), "секрет в Debug: {debug}");
        let line = spec.log_line();
        assert!(!line.contains("s3cr3t-pw"), "секрет в журнале: {line}");
        assert!(line.contains("<redacted>"), "нет метки: {line}");
        let (_, args) = spec.argv();
        assert!(
            args.contains(&"s3cr3t-pw".to_string()),
            "секрет потерян из argv"
        );
    }

    /// Старое правило журнала живо: значение после `password` прячется, даже
    /// если его забыли пометить секретом.
    #[test]
    fn password_value_is_redacted_even_when_unmarked() {
        let spec = CommandSpec::new("nmcli").arg("password").arg("oops");
        let line = spec.log_line();
        assert!(!line.contains("oops"), "пароль в журнале: {line}");
        assert!(line.contains("<redacted>"));
    }

    /// Записывающий исполнитель пишет отредактированное и отдаёт по сценарию.
    #[test]
    fn scripted_runner_records_redacted_and_replays() {
        let runner = ScriptedRunner::new(vec![ok_output("vol=42 muted=0")]);
        let spec = CommandSpec::new("wpctl").secret_arg("pw");
        let output = runner.run(&spec).expect("ответ по сценарию");
        assert!(output.success());
        assert_eq!(runner.call_count(), 1);
        let calls = runner.calls();
        assert!(!calls[0].contains("pw"), "секрет в записи: {:?}", calls);
    }

    /// Пустой сценарий — честная ошибка, а не паника и не успех.
    #[test]
    fn scripted_runner_without_script_is_an_error() {
        let runner = ScriptedRunner::new(vec![]);
        let error = runner
            .run(&CommandSpec::new("true"))
            .expect_err("сценарий пуст");
        assert!(matches!(error, CommandError::Spawn(_)));
    }

    /// Ненулевой код — обычный вывод, а не ошибка исполнителя.
    #[test]
    fn nonzero_exit_is_output_not_runner_error() {
        let runner = ScriptedRunner::new(vec![Ok(CommandOutput {
            status: Some(4),
            stdout: String::new(),
            stderr: "нет сети".to_string(),
        })]);
        let output = runner.run(&CommandSpec::new("nmcli")).expect("вывод");
        assert!(!output.success());
        assert_eq!(output.status, Some(4));
    }

    /// Провайдер разбирает вывод в типизированный snapshot и строит строки.
    #[test]
    fn provider_maps_command_to_typed_snapshot_and_nodes() {
        let runner = ScriptedRunner::new(vec![ok_output("vol=42 muted=0")]);
        let provider = FakeVolumeProvider;
        let snapshot = provider.fetch(&runner).expect("snapshot");
        assert_eq!(
            snapshot,
            FakeVolume {
                level: 42,
                muted: false
            }
        );
        let nodes = provider.nodes(&snapshot, Language::Ru);
        assert_eq!(nodes.len(), 1);
        assert_eq!(runner.call_count(), 1);
    }

    /// Ненулевой код превращается в ошибку провайдера с текстом из stderr.
    #[test]
    fn provider_turns_failing_command_into_error() {
        let runner = ScriptedRunner::new(vec![Ok(CommandOutput {
            status: Some(1),
            stdout: String::new(),
            stderr: "нет демона".to_string(),
        })]);
        let error = FakeVolumeProvider
            .fetch(&runner)
            .expect_err("команда неуспешна");
        assert!(matches!(error, ProviderError::Command { .. }));
        assert!(error.message().contains("нет демона"));
    }

    /// Мусор в выводе — ошибка разбора, а не паника и не пустой snapshot.
    #[test]
    fn provider_rejects_garbage_output() {
        let runner = ScriptedRunner::new(vec![ok_output("mutado")]);
        let error = FakeVolumeProvider
            .fetch(&runner)
            .expect_err("мусор не разбирается");
        assert!(matches!(error, ProviderError::Parse(_)));
    }

    /// Жизненный цикл: пусто → загрузка → готов. Повторный запрос из
    /// готового — `None`, процесса нет.
    #[test]
    fn slot_goes_empty_loading_ready_without_extra_requests() {
        let mut slot = ProviderSlot::<FakeVolume>::new(ProviderKey::AUDIO);
        assert!(slot.snapshot().is_none());
        let generation = slot.request().expect("первый запрос");
        assert_eq!(generation, 1);
        assert!(matches!(slot.state(), SlotState::Loading));
        assert!(slot.request().is_none(), "второй запрос в полёте");
        let snapshot = FakeVolume {
            level: 10,
            muted: false,
        };
        assert!(slot.apply(generation, Ok(snapshot.clone())));
        assert_eq!(slot.snapshot(), Some(&snapshot));
        assert!(slot.request().is_none(), "готовый кэш не перезапрашивается");
    }

    /// Устаревший ответ отбрасывается и не перетирает свежий snapshot.
    #[test]
    fn stale_generation_never_wins() {
        let mut slot = ProviderSlot::<FakeVolume>::new(ProviderKey::WIFI);
        let old = slot.request().expect("первый запрос");
        let fresh = slot.refresh().expect("обновление");
        assert_ne!(old, fresh);
        let stale = FakeVolume {
            level: 1,
            muted: false,
        };
        assert!(!slot.apply(old, Ok(stale)), "устаревший ответ принят");
        assert!(slot.snapshot().is_none(), "мусор перетёр состояние");
        let current = FakeVolume {
            level: 2,
            muted: true,
        };
        assert!(slot.apply(fresh, Ok(current.clone())));
        assert_eq!(slot.snapshot(), Some(&current));
    }

    /// Ошибка не ретраится сама, но явный refresh запускает новый запрос.
    #[test]
    fn error_waits_for_explicit_refresh() {
        let mut slot = ProviderSlot::<FakeVolume>::new(ProviderKey::BLUETOOTH);
        let generation = slot.request().expect("запрос");
        assert!(slot.apply(generation, Err("демон молчит".to_string())));
        assert!(matches!(slot.state(), SlotState::Error(_)));
        assert!(slot.request().is_none(), "ошибка ретраится сама");
        let next = slot.refresh().expect("явное обновление");
        assert_eq!(next, generation + 1);
        assert!(matches!(slot.state(), SlotState::Loading));
    }

    /// Недоступный раздел не запрашивается ни так, ни через refresh.
    #[test]
    fn unavailable_slot_never_requests() {
        let mut slot = ProviderSlot::<FakeVolume>::new(ProviderKey::WIFI);
        slot.mark_unavailable("нет адаптера");
        assert!(slot.request().is_none());
        assert!(slot.refresh().is_none());
        assert!(!slot.apply(
            1,
            Ok(FakeVolume {
                level: 0,
                muted: false
            })
        ));
    }

    /// Выборка не блокирует: сначала пусто, после работы потока — ответ.
    #[test]
    fn fetch_returns_immediately_and_delivers_later() {
        let fetch = spawn_fetch(7, || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            42
        });
        assert_eq!(fetch.generation, 7);
        assert!(fetch.try_take().is_none(), "поток ещё работает");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let value = loop {
            if let Some(value) = fetch.try_take() {
                break value;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "worker не ответил вовремя"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(value, 42);
    }

    /// Состояния уровня: загрузка, готовый список, пустой, ошибка с
    /// обновлением, недоступность. Все — обычными узлами.
    #[test]
    fn slot_states_render_as_plain_nodes() {
        let key = ProviderKey::new("test");
        let loading = slot_nodes(key, &SlotState::Loading, Language::Ru);
        assert_eq!(loading.len(), 1);
        assert_eq!(loading[0].title, "Загрузка…");

        let empty: SlotState<Vec<Node>> = SlotState::Ready(Vec::new());
        let nodes = slot_nodes(key, &empty, Language::Ru);
        assert_eq!(nodes.len(), 1);

        let ready: SlotState<Vec<Node>> =
            SlotState::Ready(vec![Node::info(settings_icons::DOT, "a")]);
        assert_eq!(slot_nodes(key, &ready, Language::Ru).len(), 1);

        let error: SlotState<Vec<Node>> = SlotState::Error("демон молчит".to_string());
        let nodes = slot_nodes(key, &error, Language::Ru);
        assert_eq!(nodes.len(), 2, "сообщение и обновление");
        assert_eq!(nodes[0].title, "демон молчит");
        assert_eq!(nodes[1].title, "Обновить");
        assert!(
            matches!(
                &nodes[1].kind,
                super::super::tree::NodeKind::Action(Action::RefreshDynamic(found)) if *found == key
            ),
            "обновление не ведёт на раздел"
        );

        let off: SlotState<Vec<Node>> = SlotState::Unavailable("нет адаптера".to_string());
        let nodes = slot_nodes(key, &off, Language::Ru);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].title, "нет адаптера");
    }

    /// Английские подписи placeholders тоже заданы явно.
    #[test]
    fn placeholder_titles_are_translated() {
        assert_eq!(loading_title(Language::En), "Loading…");
        assert_eq!(refresh_title(Language::En), "Refresh");
        assert!(!empty_title(Language::En).is_empty());
    }
}

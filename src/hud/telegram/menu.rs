//! Меню бота: один экран — одно сообщение с кнопками, нажатие перерисовывает
//! его же.
//!
//! Меню нужно, чтобы с телефона не помнить команды. Текстовые команды остались
//! (`/stop` удобнее кнопки, когда он на обходе), но кнопки — основной путь:
//! список сессий и моделей длинный, и набирать `/model 28` пальцем неудобно.
//!
//! Форма выбрана одна: сообщение с меню редактируется на месте. Каждый экран
//! приходит новым сообщением — в ленте накапливалось бы десять почти
//! одинаковых сообщений, и ориентироваться в них было бы невозможно.
//!
//! `callback_data` несёт экран и номер строки, а не текст: `menu:sess:3`,
//! `menu:model:7`. Лимит Bot API — 64 байта, полный id сессии или `провайдер/id`
//! модели в него иногда не влезают, а номер влезает всегда. Смысл номера
//! зашит в состояние бота: список перерисовывается целиком при каждом
//! нажатии, поэтому номер не успевает устареть.
//!
//! Модуль чистый: [`Screen`] собирает текст и кнопки из готовых данных, сети
//! здесь нет вообще. Сеть — в мосте, который и держит сообщение с меню.

use super::api::Button;
use super::opencode::ModelRef;

/// Предел Bot API на `callback_data`, байты.
const CALLBACK_LIMIT: usize = 64;

/// Префикс нажатий меню. Кнопки прав и «Стоп» живут под своими префиксами,
/// поэтому меню не перехватывает чужие нажатия.
const PREFIX: &str = "menu:";

/// Экран меню.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Корневая панель: что сейчас активно и куда можно пойти.
    Root,
    /// Список сессий для выбора.
    Sessions,
    /// Список моделей для выбора.
    Models,
    /// Справка по командам.
    Help,
}

impl Screen {
    /// Короткое имя для `callback_data` и заголовка.
    pub fn slug(self) -> &'static str {
        match self {
            Screen::Root => "root",
            Screen::Sessions => "sess",
            Screen::Models => "model",
            Screen::Help => "help",
        }
    }

    /// Заголовок экрана: жирной строкой над содержимым.
    pub fn title(self) -> &'static str {
        match self {
            Screen::Root => "Мост opencode",
            Screen::Sessions => "Сессии",
            Screen::Models => "Модели",
            Screen::Help => "Команды",
        }
    }

    /// Разбор экрана из имени. Неизвестное имя — корень: как и с прямым входом
    /// в раздел `hud-menu`, непонятное имя не должно быть ошибкой.
    pub fn from_slug(slug: &str) -> Self {
        match slug {
            "sess" => Screen::Sessions,
            "model" => Screen::Models,
            "help" => Screen::Help,
            _ => Screen::Root,
        }
    }
}

/// Что нажали в меню.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Открыть экран.
    Open(Screen),
    /// Выбрать сессию по номеру из списка.
    PickSession(usize),
    /// Выбрать модель по номеру из списка.
    PickModel(usize),
    /// Новая сессия в текущей папке.
    NewSession,
    /// Прервать работу агента.
    Stop,
}

impl Action {
    /// `callback_data` для кнопки. Всегда короче лимита: проверяется тестом,
    /// но и конструкция берёт короткие имена, чтобы зависеть от этой
    /// проверки не пришлось.
    pub fn data(self) -> String {
        match self {
            Action::Open(screen) => format!("{PREFIX}{}", screen.slug()),
            Action::PickSession(index) => format!("{PREFIX}sess:{index}"),
            Action::PickModel(index) => format!("{PREFIX}model:{index}"),
            Action::NewSession => format!("{PREFIX}new"),
            Action::Stop => format!("{PREFIX}stop"),
        }
    }
}

/// Разбор нажатия меню. `None` — нажатие не наше: мост отдаст его дальше, к
/// правам и «Стоп».
pub fn parse_action(data: &str) -> Option<Action> {
    let rest = data.strip_prefix(PREFIX)?;
    let (slug, arg) = match rest.split_once(':') {
        Some((slug, arg)) => (slug, Some(arg)),
        None => (rest, None),
    };
    let screen = match slug {
        "new" => return Some(Action::NewSession),
        "stop" => return Some(Action::Stop),
        "root" | "sess" | "model" | "help" => Screen::from_slug(slug),
        // Неизвестное имя экрана без номера — корень: кнопка могла устареть
        // после правки меню, и вернуться наверх лучше, чем молчать.
        _ => return arg.is_none().then_some(Action::Open(Screen::Root)),
    };
    let Some(arg) = arg else {
        return Some(Action::Open(screen));
    };
    // Номер строки: ноль и мусор — не нажатие, а испорченная кнопка.
    let index = arg.parse::<usize>().ok().filter(|index| *index > 0)?;
    match screen {
        Screen::Sessions => Some(Action::PickSession(index)),
        Screen::Models => Some(Action::PickModel(index)),
        // Номера есть только у списков: «выбрать 3 в помощи» не значит ничего,
        // и такое нажатие лучше проигнорировать.
        _ => None,
    }
}

/// Данные для корневого экрана: что сейчас активно.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RootView {
    /// Активная сессия: короткое имя и её заголовок.
    pub session: Option<(String, String)>,
    /// Работает ли агент сейчас.
    pub busy: bool,
    /// Модель сессии как `провайдер/id`.
    pub model: Option<String>,
    /// Папка сессии.
    pub directory: String,
}

/// Готовый экран: текст сообщения и кнопки под ним.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    /// Текст сообщения.
    pub text: String,
    /// Кнопки строками.
    pub keyboard: Vec<Vec<Button>>,
}

impl Panel {
    fn new(text: String, keyboard: Vec<Vec<Button>>) -> Self {
        Self { text, keyboard }
    }
}

/// Корневая панель: состояние сверху, кнопки снизу. Обратимые кнопки — в
/// верхних рядах, «Стоп» и «Новая» — в нижних: на узком экране до нижних
/// тянутся большим пальцем, и туда их лучше не класть по ошибке.
pub fn root(view: &RootView) -> Panel {
    let mut text = String::from("🏠 <b>Мост opencode</b>\n");
    match &view.session {
        Some((id, title)) => {
            let state = if view.busy {
                "работает"
            } else {
                "ждёт"
            };
            if title.is_empty() {
                text.push_str(&format!(
                    "Сессия <code>{}</code> · {state}",
                    super::api::escape_html(id)
                ));
            } else {
                text.push_str(&format!(
                    "Сессия «{}» · {state}",
                    super::api::escape_html(title)
                ));
            }
        }
        None => text.push_str("Сессии нет — напиши текст, мост создаст первую"),
    }
    text.push_str(&format!(
        "\nПапка <code>{}</code>",
        super::api::escape_html(&view.directory)
    ));
    if let Some(model) = &view.model {
        text.push_str(&format!(
            "\nМодель <code>{}</code>",
            super::api::escape_html(model)
        ));
    }
    text.push_str("\n\nПиши текст — это промпт агенту.");
    Panel::new(
        text,
        vec![
            vec![
                Button::new("🗂 Сессии", &Action::Open(Screen::Sessions).data()),
                Button::new("🤖 Модели", &Action::Open(Screen::Models).data()),
            ],
            vec![Button::new("➕ Новая сессия", &Action::NewSession.data())],
            vec![Button::new("⏹ Стоп", &Action::Stop.data())],
            vec![Button::new(
                "❓ Команды",
                &Action::Open(Screen::Help).data(),
            )],
        ],
    )
}

/// Список сессий. Активная помечена, выбор — по номеру из этого же списка.
pub fn sessions(rows: &[(String, String)], current: Option<&str>) -> Panel {
    let mut text = String::from("🗂 <b>Сессии</b>");
    if rows.is_empty() {
        text.push_str("\n\nСессий пока нет — напиши текст, и мост создаст первую.");
        return Panel::new(
            text,
            vec![vec![Button::new(
                "⬅ Меню",
                &Action::Open(Screen::Root).data(),
            )]],
        );
    }
    let mut keyboard: Vec<Vec<Button>> = Vec::new();
    let mut line: Vec<Button> = Vec::new();
    for (index, (id, title)) in rows.iter().enumerate() {
        let mark = if current == Some(id.as_str()) {
            "● "
        } else {
            ""
        };
        let caption = if title.is_empty() {
            format!("{mark}{}", short(id))
        } else {
            format!("{mark}{}", truncate(title, 24))
        };
        text.push_str(&format!(
            "\n{}. {} <code>{}</code>",
            index + 1,
            super::api::escape_html(&caption),
            super::api::escape_html(short(id))
        ));
        line.push(Button::new(
            &caption,
            &Action::PickSession(index + 1).data(),
        ));
        // Три кнопки в ряду: на телефоне это читаемая ширина, четыре уже
        // подписи обрезаются.
        if line.len() == 3 {
            keyboard.push(std::mem::take(&mut line));
        }
    }
    if !line.is_empty() {
        keyboard.push(line);
    }
    keyboard.push(vec![Button::new(
        "⬅ Меню",
        &Action::Open(Screen::Root).data(),
    )]);
    Panel::new(text, keyboard)
}

/// Список моделей. Подпись — короткий номер и имя: полное `провайдер/id` в
/// кнопке не помещается, а одинаковые имена у разных провайдеров есть.
pub fn models(rows: &[(String, String)], current: Option<&ModelRef>) -> Panel {
    let mut text = String::from("🤖 <b>Модели</b> (● — текущая)");
    if rows.is_empty() {
        text.push_str("\n\nПодключённых провайдеров нет — проверь ключи в opencode.");
        return Panel::new(
            text,
            vec![vec![Button::new(
                "⬅ Меню",
                &Action::Open(Screen::Root).data(),
            )]],
        );
    }
    let mut keyboard: Vec<Vec<Button>> = Vec::new();
    let mut line: Vec<Button> = Vec::new();
    for (index, (id, name)) in rows.iter().enumerate() {
        let is_current = current.is_some_and(|model| &model.id == id);
        let mark = if is_current { "● " } else { "" };
        let caption = format!("{}{}", mark, if name.is_empty() { id } else { name });
        text.push_str(&format!(
            "\n{}. {}",
            index + 1,
            super::api::escape_html(&truncate(&caption, 40))
        ));
        line.push(Button::new(
            &truncate(&caption, 20),
            &Action::PickModel(index + 1).data(),
        ));
        if line.len() == 2 {
            keyboard.push(std::mem::take(&mut line));
        }
    }
    if !line.is_empty() {
        keyboard.push(line);
    }
    keyboard.push(vec![Button::new(
        "⬅ Меню",
        &Action::Open(Screen::Root).data(),
    )]);
    Panel::new(text, keyboard)
}

/// Справка по командам: текстом, потому что команды и не кнопки — их нажатие
/// набирается пальцем, а читается быстрее взглядом, чем прокликивается.
pub fn help(commands_text: &str) -> Panel {
    Panel::new(
        format!("❓ <b>Команды</b>\n\n{commands_text}"),
        vec![vec![Button::new(
            "⬅ Меню",
            &Action::Open(Screen::Root).data(),
        )]],
    )
}

/// Короткое имя сессии: `калм-cabin` из `ses_abc123` не выводится, поэтому
/// берём хвост, который у opencode читаемый.
fn short(id: &str) -> &str {
    id.strip_prefix("ses_").unwrap_or(id)
}

/// Подпись кнопки: обрезка по символам, многоточие в конце. Обрезка нужна
/// подписям, текст сообщения не режем — там пусть строка переносится.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// `callback_data` не длиннее лимита Bot API. Проверяется для всех кнопок
/// всех экранов: превышение молча ломает нажатие в Telegram.
pub fn within_callback_limit(screen: &Panel) -> bool {
    screen
        .keyboard
        .iter()
        .flatten()
        .all(|button| button.callback_data.len() <= CALLBACK_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buttons(screen: &Panel) -> Vec<&Button> {
        screen.keyboard.iter().flatten().collect()
    }

    #[test]
    fn root_shows_session_state_and_path() {
        let view = RootView {
            session: Some(("ses_abc123def456".into(), "Мост".into())),
            busy: true,
            model: Some("vibecode-claude/claude-sonnet-4-6".into()),
            directory: "/home/mihail/code/hudbar".into(),
        };
        let screen = root(&view);
        assert!(screen.text.contains("Мост opencode"));
        assert!(
            screen.text.contains("Мост"),
            "заголовок сессии: {}",
            screen.text
        );
        assert!(
            screen.text.contains("работает"),
            "агент занят: {}",
            screen.text
        );
        assert!(screen.text.contains("vibecode-claude/claude-sonnet-4-6"));
        assert!(screen.text.contains("/home/mihail/code/hudbar"));
        let labels: Vec<&str> = buttons(&screen).iter().map(|b| b.text.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "🗂 Сессии",
                "🤖 Модели",
                "➕ Новая сессия",
                "⏹ Стоп",
                "❓ Команды"
            ]
        );
    }

    #[test]
    fn root_without_a_session_says_it_and_shows_no_title() {
        let view = RootView {
            directory: "/tmp".into(),
            ..RootView::default()
        };
        let screen = root(&view);
        assert!(screen.text.contains("Сессии нет"), "{}", screen.text);
        assert!(!screen.text.contains("Модель"), "модели без сессии нет");
    }

    /// Подпись сессии экранируется: заголовок приходит из opencode, а в нём
    /// может быть `<`, и без экранирования Telegram отверг бы сообщение.
    #[test]
    fn session_titles_are_escaped() {
        let rows = vec![("ses_a".to_string(), "<script> & Co".to_string())];
        let screen = sessions(&rows, None);
        assert!(
            screen.text.contains("&lt;script&gt;"),
            "экранировано: {}",
            screen.text
        );
        assert!(screen.text.contains("&amp;"), "amp тоже: {}", screen.text);
        assert!(!screen.text.contains("<script>"), "сырого тега нет");
    }

    #[test]
    fn sessions_list_is_numbered_and_marks_the_current_one() {
        let rows = vec![
            ("ses_first".to_string(), "Первая".to_string()),
            ("ses_second".to_string(), String::new()),
            ("ses_third".to_string(), "Третья".to_string()),
        ];
        let screen = sessions(&rows, Some("ses_second"));
        assert!(screen.text.contains("\n1. Первая"));
        assert!(
            screen.text.contains("\n2. ● second"),
            "метка и хвост id: {}",
            screen.text
        );
        assert!(screen.text.contains("\n3. Третья"));
        // Номера в тексте и номера в кнопках — одни и те же.
        let picks: Vec<&str> = buttons(&screen)
            .iter()
            .filter(|b| b.callback_data.contains("sess:"))
            .map(|b| b.callback_data.as_str())
            .collect();
        assert_eq!(picks, vec!["menu:sess:1", "menu:sess:2", "menu:sess:3"]);
        assert!(
            buttons(&screen).last().unwrap().text.contains("Меню"),
            "есть возврат"
        );
    }

    #[test]
    fn empty_lists_say_so_instead_of_showing_only_a_back_button() {
        let screen = sessions(&[], None);
        assert!(screen.text.contains("Сессий пока нет"), "{}", screen.text);
        assert_eq!(buttons(&screen).len(), 1, "одна кнопка «Меню»");
        let screen = models(&[], None);
        assert!(
            screen.text.contains("Подключённых провайдеров нет"),
            "{}",
            screen.text
        );
        assert_eq!(buttons(&screen).len(), 1);
    }

    #[test]
    fn models_list_marks_current_and_numbers_match_buttons() {
        let rows = vec![
            ("gpt-5.6-luna".to_string(), "GPT 5.6 Luna".to_string()),
            (
                "claude-sonnet-4-6".to_string(),
                "Claude Sonnet 4.6".to_string(),
            ),
        ];
        let current = ModelRef::new("vibecode-claude", "claude-sonnet-4-6");
        let screen = models(&rows, Some(&current));
        assert!(screen.text.contains("\n1. GPT 5.6 Luna"), "{}", screen.text);
        assert!(
            screen.text.contains("\n2. ● Claude Sonnet 4.6"),
            "{}",
            screen.text
        );
        let picks: Vec<&str> = buttons(&screen)
            .iter()
            .filter(|b| b.callback_data.contains("model:"))
            .map(|b| b.callback_data.as_str())
            .collect();
        assert_eq!(picks, vec!["menu:model:1", "menu:model:2"]);
    }

    /// Две кнопки в ряду у моделей, три у сессий: имена моделей длиннее, чем
    /// заголовки сессий, и втроем они не читаются.
    #[test]
    fn rows_fit_the_phone_width() {
        let sessions_rows: Vec<(String, String)> = (0..4)
            .map(|i| (format!("ses_{i}"), format!("Сессия {i}")))
            .collect();
        let screen = sessions(&sessions_rows, None);
        assert_eq!(screen.keyboard.len(), 3, "4 сессии: 3 + 1, потом «Меню»");
        assert_eq!(screen.keyboard[0].len(), 3);
        let models_rows: Vec<(String, String)> = (0..4)
            .map(|i| (format!("m{i}"), format!("Модель {i}")))
            .collect();
        let screen = models(&models_rows, None);
        assert_eq!(screen.keyboard.len(), 3, "4 модели: 2 + 2, потом «Меню»");
        assert_eq!(screen.keyboard[0].len(), 2);
    }

    /// Нажатия разбираются в обе стороны: то, что кнопка положила в
    /// `callback_data`, разбирается обратно в то же действие.
    #[test]
    fn actions_round_trip_through_callback_data() {
        for action in [
            Action::Open(Screen::Root),
            Action::Open(Screen::Sessions),
            Action::Open(Screen::Models),
            Action::Open(Screen::Help),
            Action::PickSession(3),
            Action::PickModel(12),
            Action::NewSession,
            Action::Stop,
        ] {
            let data = action.data();
            assert!(
                data.len() <= CALLBACK_LIMIT,
                "длиннее лимита Bot API: {data} ({} байт)",
                data.len()
            );
            assert_eq!(parse_action(&data), Some(action), "не разобралось: {data}");
        }
    }

    /// Кнопки прав и «Стоп» живут под своими префиксами: меню не должно
    /// перехватывать их нажатия.
    #[test]
    fn foreign_callbacks_are_not_ours() {
        assert_eq!(parse_action("stop:ses_abc"), None);
        assert_eq!(parse_action("perm:p1:once"), None);
        assert_eq!(parse_action("что-то"), None);
        assert_eq!(parse_action(""), None);
        assert_eq!(parse_action("menu:"), Some(Action::Open(Screen::Root)));
    }

    /// Мусор в номере строки не превращается в нажатие: иначе битая кнопка
    /// выбрала бы нулевую сессию.
    #[test]
    fn broken_numbers_are_not_actions() {
        assert_eq!(parse_action("menu:sess:0"), None);
        assert_eq!(parse_action("menu:sess:-1"), None);
        assert_eq!(parse_action("menu:sess:abc"), None);
        // Номера есть только у списков: «выбрать 1» в корне или помощи не
        // значит ничего, и такое нажатие игнорируется.
        assert_eq!(parse_action("menu:root:1"), None);
        assert_eq!(parse_action("menu:help:1"), None);
        // Неизвестный экран с номером — тоже испорченная кнопка: номер
        // пришёл не из списка, и выбирать нечего.
        assert_eq!(parse_action("menu:модель:1"), None);
    }

    #[test]
    fn unknown_screen_falls_back_to_root() {
        assert_eq!(Screen::from_slug("что-то"), Screen::Root);
        assert_eq!(
            parse_action("menu:что-то"),
            Some(Action::Open(Screen::Root))
        );
    }

    /// Лимит `callback_data` проверяется на реальных экранах, а не на
    /// вручную: превышение в Telegram ломает нажатие молча.
    #[test]
    fn every_button_of_every_screen_fits_the_callback_limit() {
        let long_id = "ses_".to_string() + &"x".repeat(200);
        let long_title = "Модель с очень длинным именем ".repeat(10);
        let screens = [
            root(&RootView {
                session: Some((long_id.clone(), long_title.clone())),
                busy: true,
                model: Some(long_title.clone()),
                directory: long_title.clone(),
            }),
            sessions(&[(long_id.clone(), long_title.clone())], Some(&long_id)),
            models(&[(long_id.clone(), long_title.clone())], None),
            help("команда\n".repeat(500).as_str()),
        ];
        for screen in &screens {
            assert!(
                within_callback_limit(screen),
                "кнопка длиннее {CALLBACK_LIMIT} байт: {:?}",
                screen
                    .keyboard
                    .iter()
                    .flatten()
                    .map(|b| b.callback_data.len())
                    .max()
            );
        }
    }

    /// Длинные подписи обрезаются, а не раздувают кнопку: в кнопке Telegram
    /// обрезает текст сам, но уже после того, как подпись уедет в разметку.
    #[test]
    fn long_captions_are_truncated_with_an_ellipsis() {
        assert_eq!(truncate("коротко", 20), "коротко");
        let cut = truncate(&"я".repeat(50), 10);
        assert_eq!(cut.chars().count(), 10);
        assert!(cut.ends_with('…'));
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn help_wraps_the_command_list() {
        let screen = help("Команды:\n/new — новая");
        assert!(screen.text.contains("Команды"), "{}", screen.text);
        assert!(screen.text.contains("/new — новая"));
        assert_eq!(buttons(&screen).len(), 1);
        assert!(buttons(&screen)[0].callback_data.contains("root"));
    }
}

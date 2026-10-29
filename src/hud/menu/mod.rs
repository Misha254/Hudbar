//! Меню HUDbar: модель без отрисовки и без Wayland.
//!
//! Состав из модулей повторяет устройство окна настроек, но ничего не
//! разделяет с ним: `hud_keybinds.rs`, `hud_yazibinds.rs` и окно настроек
//! не менялись. Общее у меню с окном настроек только три кирпичика — токены
//! `ui_tokens`, текст `text` и глифы `settings_icons`, и то по чтению.
//!
//! Модули по порядку снизу вверх: `action` — что делает пункт, `item` — строка
//! и список, `tree` — дерево пунктов, `state` — стек уровней, `view` —
//! отрисовка (M2). Модель ничего не знает про пиксели, поэтому её можно
//! проверять тестами без композитора.

#[allow(dead_code)]
pub mod action;
#[allow(dead_code)]
pub mod audio;
#[allow(dead_code)]
pub mod bluetooth;
#[allow(dead_code)]
pub mod displays;
#[allow(dead_code)]
pub mod exec;
/// Скользящая подсветка строки: состояние хода, без отрисовки.
#[allow(dead_code)]
pub mod highlight;
#[allow(dead_code)]
pub mod input;
#[allow(dead_code)]
pub mod instance;
#[allow(dead_code)]
pub mod item;
#[allow(dead_code)]
pub mod real;
#[allow(dead_code)]
pub mod secret;
#[allow(dead_code)]
pub mod snapshot;
#[allow(dead_code)]
pub mod state;
#[allow(dead_code)]
pub mod storage;
/// Паспорт машины в меню: строки-справки из /proc и /sys, без команд.
#[allow(dead_code)]
pub mod sysinfo;
#[allow(dead_code)]
pub mod system;
#[allow(dead_code)]
pub mod tree;
#[allow(dead_code)]
pub mod values;
#[allow(dead_code)]
pub mod view;
/// VPN-раздел: режимы поверх селекторов Mihomo (модель без сети).
#[allow(dead_code)]
pub mod vpn;
/// HTTP-клиент к external controller Mihomo.
#[allow(dead_code)]
pub mod vpn_api;
#[allow(dead_code)]
pub mod wifi;

// Модули hud, нужные подмодулям меню. Реэкспорт, а не `super::super::`: в
// `main.rs` меню лежит в `hud::menu`, а в бинаре меню — в корне
// крейта, и путь до общих модулей у них разный.
use action::{Action, Live};
use settings::Language;
use state::Menu;
use tree::Node;

#[allow(unused_imports)]
pub(crate) use super::{
    bind_data, canvas, config_io, geom, i18n, palette, settings, settings_icons, text, ui_tokens,
};

/// Заголовок приложения в крошках: «HUD › Стиль».
pub const CRUMB_ROOT: &str = "HUD";

/// Подписи и подсказки меню в одном месте. Русские константы ниже — язык по
/// умолчанию; переводы отдаёт [`strings::chrome`]: окно берёт язык из
/// `settings.json` при старте, дерево строится на нём же.
pub mod strings {
    use super::super::settings::Language;

    /// Поле поиска.
    pub const SEARCH_PLACEHOLDER: &str = "Поиск…";
    /// Пустой результат поиска.
    pub const NOTHING_FOUND: &str = "Ничего не найдено";
    /// Разделитель крошек: «HUD › Стиль».
    pub const CRUMBS_SEPARATOR: &str = " › ";
    /// Разделитель разделов в caption найденной строки.
    pub const PATH_SEPARATOR: &str = " › ";
    /// Подсказки подвала слева. Заглавные — как в макете.
    pub const HINT_SELECT: &str = "↑↓ ВЫБОР";
    pub const HINT_OPEN: &str = "ENTER ОТКРЫТЬ";
    pub const HINT_BACK: &str = "← НАЗАД";
    pub const HINT_CLOSE: &str = "ESC ЗАКРЫТЬ";
    /// Короткие подсказки: в пиксельном шрифте полные не влезают, и вместо
    /// наезда на счётчик подвал берёт эти. Клавиша остаётся, пропадает слово.
    pub const HINT_SELECT_SHORT: &str = "↑↓";
    pub const HINT_OPEN_SHORT: &str = "ENTER";
    pub const HINT_BACK_SHORT: &str = "←";
    pub const HINT_CLOSE_SHORT: &str = "ESC";
    /// Средние подсказки: между полными и одними клавишами. Полные не влезают
    /// в карточку 440 px по ширине, а одни клавиши не говорят, что клавиша
    /// делает, поэтому слово остаётся — обрезанным до корня.
    pub const HINT_SELECT_MEDIUM: &str = "↑↓ ВЫБОР";
    pub const HINT_OPEN_MEDIUM: &str = "ENTER ОТКР.";
    pub const HINT_BACK_MEDIUM: &str = "← НАЗАД";
    pub const HINT_CLOSE_MEDIUM: &str = "ESC ЗАКР.";
    /// Подсказка «Enter» по виду строки: у тумблера он переключает, у выбора
    /// меняет вариант, у числа — ничего, там шаг на `Shift+←/→`. Общая
    /// «ENTER ОТКРЫТЬ» на таких строках врала о поведении.
    pub const HINT_TOGGLE: &str = "ENTER ВКЛ/ВЫКЛ";
    pub const HINT_CHOICE: &str = "ENTER ВАРИАНТ";
    pub const HINT_ADJUST: &str = "SHIFT+←→ ЗНАЧЕНИЕ";
    pub const HINT_ADJUST_MEDIUM: &str = "SHIFT+←→ ЗНАЧ.";
    pub const HINT_ADJUST_SHORT: &str = "⇧←→";
    pub const HINT_TOGGLE_SHORT: &str = "ENTER";
    pub const HINT_CHOICE_SHORT: &str = "ENTER";
    /// Статус после действия.
    pub const STATUS_APPLIED: &str = "ПРИМЕНЕНО";
    pub const STATUS_FAILED: &str = "ОШИБКА";

    /// Все подписи хрома одним куском: отрисовка берёт их по языку кадра, а
    /// не по константам выше.
    pub struct Chrome {
        pub search: &'static str,
        /// Заглушка поля поиска внутри раздела: запрос фильтрует этот
        /// уровень, а не всё дерево.
        pub search_scope: &'static str,
        pub nothing: &'static str,
        pub select: &'static str,
        pub open: &'static str,
        pub back: &'static str,
        pub close: &'static str,
        pub select_short: &'static str,
        pub open_short: &'static str,
        pub back_short: &'static str,
        pub close_short: &'static str,
        /// Средний уровень: ключ и короткое слово.
        pub select_medium: &'static str,
        pub open_medium: &'static str,
        pub back_medium: &'static str,
        pub close_medium: &'static str,
        /// Подсказка действия по виду выбранной строки.
        pub toggle: &'static str,
        pub choice: &'static str,
        pub adjust: &'static str,
        pub adjust_medium: &'static str,
        pub adjust_short: &'static str,
        pub toggle_short: &'static str,
        pub choice_short: &'static str,
        pub applied: &'static str,
        pub failed: &'static str,
        /// Подпись включённого тумблера: «вкл» или «on».
        pub on: &'static str,
        /// Подпись выключенного тумблера: «выкл» или «off».
        pub off: &'static str,
        /// Подпись поля пароля: «Пароль» или «Password».
        pub password: &'static str,
        /// Подсказка пустого поля пароля.
        pub password_placeholder: &'static str,
        /// Подсказка подтверждения ввода пароля.
        pub password_connect: &'static str,
        /// Подсказка отмены ввода пароля.
        pub password_cancel: &'static str,
    }

    /// Подписи хрома на языке окна.
    pub fn chrome(lang: Language) -> Chrome {
        match lang {
            Language::Ru => Chrome {
                search: SEARCH_PLACEHOLDER,
                search_scope: "В разделе…",
                nothing: NOTHING_FOUND,
                select: HINT_SELECT,
                open: HINT_OPEN,
                back: HINT_BACK,
                close: HINT_CLOSE,
                select_short: HINT_SELECT_SHORT,
                open_short: HINT_OPEN_SHORT,
                back_short: HINT_BACK_SHORT,
                close_short: HINT_CLOSE_SHORT,
                select_medium: HINT_SELECT_MEDIUM,
                open_medium: HINT_OPEN_MEDIUM,
                back_medium: HINT_BACK_MEDIUM,
                close_medium: HINT_CLOSE_MEDIUM,
                toggle: HINT_TOGGLE,
                choice: HINT_CHOICE,
                adjust: HINT_ADJUST,
                adjust_medium: HINT_ADJUST_MEDIUM,
                adjust_short: HINT_ADJUST_SHORT,
                toggle_short: HINT_TOGGLE_SHORT,
                choice_short: HINT_CHOICE_SHORT,
                applied: STATUS_APPLIED,
                failed: STATUS_FAILED,
                on: "вкл",
                off: "выкл",
                password: "Пароль",
                password_placeholder: "введите пароль",
                password_connect: "ENTER ПОДКЛЮЧИТЬСЯ",
                password_cancel: "ESC ОТМЕНА",
            },
            Language::En => Chrome {
                search: "Search…",
                search_scope: "In section…",
                nothing: "Nothing found",
                select: "↑↓ SELECT",
                open: "ENTER OPEN",
                back: "← BACK",
                close: "ESC CLOSE",
                select_short: "↑↓",
                open_short: "ENTER",
                back_short: "←",
                close_short: "ESC",
                select_medium: "↑↓ SEL",
                open_medium: "ENTER OPEN",
                back_medium: "← BACK",
                close_medium: "ESC CLOSE",
                toggle: "ENTER ON/OFF",
                choice: "ENTER NEXT",
                adjust: "SHIFT+←→ VALUE",
                adjust_medium: "SHIFT+←→",
                adjust_short: "⇧←→",
                toggle_short: "ENTER",
                choice_short: "ENTER",
                applied: "APPLIED",
                failed: "FAILED",
                on: "on",
                off: "off",
                password: "Password",
                password_placeholder: "enter password",
                password_connect: "ENTER CONNECT",
                password_cancel: "ESC CANCEL",
            },
        }
    }
}

/// Фиктивные значения для снимков и тестов: реальные настройки и панель в
/// модели не нужны, а чтение файлов сделало бы тесты зависимыми от домашней
/// папки. Значения лежат в атомиках, потому что `get`/`set` — обычные
/// функции, как того требует дерево.
pub mod fake {
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Индекс языка: 0 — русский, 1 — английский.
    pub static LANGUAGE: AtomicU32 = AtomicU32::new(0);
    /// Высота панели в пикселях.
    pub static HEIGHT: AtomicU32 = AtomicU32::new(27);
    /// Включён ли «не беспокоить».
    pub static DND: AtomicU32 = AtomicU32::new(1);
    /// Громкость.
    pub static VOLUME: AtomicU32 = AtomicU32::new(42);
    /// Запись экрана.
    pub static RECORDING: AtomicU32 = AtomicU32::new(0);
    /// Индекс выбранных обоев.
    pub static WALLPAPER: AtomicU32 = AtomicU32::new(2);

    /// Индекс схемы matugen.
    pub static SCHEME: AtomicU32 = AtomicU32::new(0);

    pub const LANGUAGE_NAMES: [&str; 2] = ["Русский", "English"];
    pub const SCHEME_NAMES: [&str; 2] = ["tonal-spot", "vibrant"];
    pub const WALLPAPERS: [&str; 4] = [
        "anime/103.png",
        "pixelart/light/city.png",
        "nature/forest-01.jpg",
        "renders/01.jpg",
    ];

    pub fn language() -> usize {
        LANGUAGE.load(Ordering::Relaxed) as usize
    }

    pub fn set_language(index: usize) {
        LANGUAGE.store(index as u32, Ordering::Relaxed);
    }

    pub fn height() -> f32 {
        HEIGHT.load(Ordering::Relaxed) as f32
    }

    pub fn set_height(value: f32) {
        HEIGHT.store(value.round() as u32, Ordering::Relaxed);
    }

    pub fn dnd() -> bool {
        DND.load(Ordering::Relaxed) != 0
    }

    pub fn set_dnd(on: bool) {
        DND.store(u32::from(on), Ordering::Relaxed);
    }

    pub fn volume() -> f32 {
        VOLUME.load(Ordering::Relaxed) as f32
    }

    pub fn recording() -> bool {
        RECORDING.load(Ordering::Relaxed) != 0
    }

    pub fn set_recording(on: bool) {
        RECORDING.store(u32::from(on), Ordering::Relaxed);
    }

    pub fn wallpaper() -> usize {
        WALLPAPER.load(Ordering::Relaxed) as usize
    }

    pub fn set_wallpaper(index: usize) {
        WALLPAPER.store(index as u32, Ordering::Relaxed);
    }

    pub fn scheme() -> usize {
        SCHEME.load(Ordering::Relaxed) as usize
    }

    pub fn set_scheme(index: usize) {
        SCHEME.store(index as u32, Ordering::Relaxed);
    }
}

/// Тестовое дерево меню: девять разделов на корне, как в настоящем меню без
/// «Питания». Значения фиктивные, поэтому снимки меню воспроизводимы, а смена
/// темы меняет только цвета и шрифт.
pub fn demo_tree() -> Vec<Node> {
    vec![
        Node::submenu(
            settings_icons::APPS,
            "Приложения",
            vec![
                Node::picker(settings_icons::APPS, "Запустить", "apps"),
                Node::action(
                    settings_icons::TERMINAL,
                    "Терминал",
                    Action::Run("kitty", &["-e", "zsh"]),
                ),
                Node::action(
                    settings_icons::PANEL,
                    "Меню приложений",
                    Action::Niri(&["msg", "action", "launcher", "--toggle"]),
                ),
            ],
        ),
        Node::submenu(
            settings_icons::PANEL,
            "Панель",
            vec![
                Node::toggle(settings_icons::DND, "Режим DND", || false, |_| {}),
                Node::choice(
                    settings_icons::PANEL,
                    "Позиция",
                    &["Сверху", "Снизу"],
                    || 0,
                    |_| {},
                ),
                Node::number(
                    settings_icons::PANEL,
                    "Высота",
                    20.0,
                    48.0,
                    1.0,
                    " px",
                    fake::height,
                    fake::set_height,
                ),
                Node::toggle(
                    settings_icons::RECORDING,
                    "Запись экрана",
                    fake::recording,
                    fake::set_recording,
                ),
            ],
        ),
        Node::submenu(
            settings_icons::STYLE,
            "Стиль",
            vec![
                Node::picker(settings_icons::PICTURE, "Обои", "wallpaper"),
                Node::choice(
                    settings_icons::SCHEME,
                    "Схема matugen",
                    &fake::SCHEME_NAMES,
                    fake::scheme,
                    fake::set_scheme,
                ),
            ],
        ),
        Node::submenu(
            settings_icons::NOTIFICATIONS,
            "Уведомления",
            vec![
                Node::toggle(
                    settings_icons::DND,
                    "Не беспокоить (DND)",
                    fake::dnd,
                    fake::set_dnd,
                ),
                Node::number(
                    settings_icons::NOTIFICATIONS,
                    "Размер шрифта",
                    9.0,
                    20.0,
                    1.0,
                    " px",
                    || 13.0,
                    |_| {},
                ),
            ],
        ),
        Node::choice(
            settings_icons::LANGUAGE,
            "Язык",
            &fake::LANGUAGE_NAMES,
            fake::language,
            fake::set_language,
        )
        .with_id("language"),
        Node::submenu(
            settings_icons::SYSTEM,
            "Система",
            vec![Node::action(
                settings_icons::NETWORK,
                "Wi-Fi",
                Action::Live(Live::WifiRadio(true)),
            )],
        ),
        Node::submenu(
            settings_icons::CAPTURE,
            "Захват",
            vec![Node::action(
                settings_icons::CAPTURE,
                "Снимок экрана",
                Action::Run("grim", &["-"]),
            )],
        ),
        Node::submenu(
            settings_icons::KEYBINDS,
            "Бинды",
            vec![Node::action(
                settings_icons::KEYBINDS,
                "Бинды niri",
                Action::Run("hud-keybinds-rs", &[]),
            )],
        ),
        Node::submenu(
            settings_icons::ABOUT,
            "Hudbar",
            vec![Node::action(
                settings_icons::ABOUT,
                "Версия",
                Action::Niri(&["msg", "--version"]),
            )],
        ),
    ]
}

/// Меню на тестовом дереве.
pub fn demo_menu() -> Menu {
    Menu::new(demo_tree())
}

/// Разделы корня и их идентификаторы для прямого входа: `hud-menu style`.
/// Идентификатор английский, чтобы набирать его было не только в русской
/// раскладке; подписи обеих языковых версий хранятся рядом, потому что дерево
/// строится на текущем языке.
pub const SECTIONS: [(&str, &str, &str); 9] = [
    ("apps", "Приложения", "Applications"),
    ("panel", "Панель", "Panel"),
    ("style", "Стиль", "Style"),
    ("notifications", "Уведомления", "Notifications"),
    ("language", "Язык", "Language"),
    ("system", "Система", "System"),
    ("capture", "Захват", "Capture"),
    ("keybinds", "Бинды", "Keybinds"),
    ("about", "Hudbar", "Hudbar"),
];

/// Разделы без строки в корне: «Питание» остаётся доступным по бинду
/// `super+P` (`hud-menu power`), но список корня и поиск им не засоряются.
pub const HIDDEN_SECTIONS: [(&str, &str, &str); 1] = [("power", "Питание", "Power")];

/// Заголовок раздела по идентификатору. Вложенный путь `system/control`
/// заголовка не имеет: у него есть только родитель.
pub fn section_title(id: &str) -> Option<&'static str> {
    if let Some((parent, _)) = id.split_once('/') {
        return section_title(parent);
    }
    SECTIONS
        .iter()
        .chain(HIDDEN_SECTIONS.iter())
        .find(|(key, _, _)| *key == id)
        .map(|(_, title, _)| *title)
}

/// Подпись раздела на языке меню: по ней раздел ищется в дереве.
fn section_title_in(lang: Language, id: &str) -> Option<&'static str> {
    SECTIONS
        .iter()
        .chain(HIDDEN_SECTIONS.iter())
        .find(|(key, _, _)| *key == id)
        .map(|(_, ru, en)| match lang {
            Language::Ru => *ru,
            Language::En => *en,
        })
}

/// Такой раздел (возможно, вложенный) есть в дереве.
pub fn has_section(id: &str) -> bool {
    section_title(id).is_some()
}

/// Открывает раздел сразу при старте. Возвращает `true`, если раздел найден:
/// неизвестный идентификатор должен открывать корень, а не пустой уровень.
/// Раздел ищется по подписи на языке меню, а не по позиции: настоящий корень
/// строится на текущем языке, а скрытые разделы вообще не попадают в список.
pub fn open_section(menu: &mut Menu, id: &str) -> bool {
    // Вложенный путь: сначала родительский раздел, потом строка с такой
    // личностью. Нужен биндам вида `super+Escape → hud-menu-rs system/control`,
    // чтобы до подменю можно было доехать в один запуск.
    if let Some((parent, _)) = id.split_once('/') {
        if !open_section(menu, parent) {
            return false;
        }
        // Личность строки записана целиком (`system/control`), а не хвостом:
        // хвост у разных разделов повторяется.
        let Some(index) = menu
            .current()
            .list
            .items()
            .iter()
            .position(|item| item.id == id)
        else {
            return false;
        };
        menu.current_mut().list.select(index);
        return menu.enter() == state::Outcome::Pushed;
    }
    let Some(title) = section_title_in(menu.lang(), id) else {
        return false;
    };
    // `push` не смотрит на `visible`: «Питание» спрятано из корня, но прямой
    // вход `hud-menu power` обязан открыть его так же, как раньше.
    if menu.push(title) {
        return true;
    }
    // В корне есть строки-значения (язык): «открыть» им нечего, а `Enter`
    // на выборе переключил бы значение прямо из командной строки.
    false
}

#[cfg(test)]
mod tests {
    use super::state::Outcome;
    use super::*;

    /// Вложенный вход: `system/control` открывает подменю сразу, за один
    /// запуск. Так бинд доезжает до нужного места без второго нажатия.
    ///
    /// Дерево синтетическое: в настоящем строка `system/control` появляется
    /// только когда конфиги лежат на диске, и на чистой машине тест проверял
    /// бы отсутствие `~/.config`, а не вход.
    #[test]
    fn a_nested_path_enters_the_child_level() {
        let mut menu = Menu::new(nested_tree());
        assert!(super::open_section(&mut menu, "system/control"));
        assert_eq!(menu.depth(), 2, "вход в подменю раздела");
        assert_eq!(menu.current().title, "Панель управления");
        let selected = menu
            .current()
            .list
            .selected_item()
            .expect("выбранная строка");
        assert_eq!(selected.id, "control/niri");
    }

    /// Корень как у настоящего меню: разделы из `SECTIONS` по порядку, во
    /// «Системе» — подменю `system/control` с одной строкой. Остальные
    /// разделы — листья: входу нужны только позиция раздела и личность
    /// подменю.
    fn nested_tree() -> Vec<Node> {
        SECTIONS
            .iter()
            .map(|(key, title, _)| {
                if *key == "system" {
                    Node::submenu(
                        settings_icons::SYSTEM,
                        title,
                        vec![
                            Node::submenu(
                                settings_icons::CONTROLS,
                                "Панель управления",
                                vec![
                                    Node::action(
                                        settings_icons::SYSTEM,
                                        "niri",
                                        Action::Run("kitty", &["-e", "nvim"]),
                                    )
                                    .with_id("control/niri"),
                                ],
                            )
                            .with_id("system/control"),
                        ],
                    )
                } else {
                    Node::action(settings_icons::DOT, title, Action::Back)
                }
            })
            .collect()
    }

    /// Несуществующий вложенный уровень не должен оставлять меню в
    /// полуоткрытом состоянии: максимум — корень.
    #[test]
    fn an_unknown_child_stays_where_it_was() {
        let mut menu = super::real::menu();
        assert!(!super::open_section(&mut menu, "system/нет-такого"));
        assert_eq!(menu.depth(), 1, "остались на разделе, а не провалились");
    }

    /// «Язык» — строка-значение, а не раздел: прямой вход не должен
    /// переключить язык как побочный эффект запуска.
    #[test]
    fn a_value_row_cannot_be_opened_as_a_section() {
        let mut menu = super::real::menu();
        let before = menu.frame().items.len();
        assert!(!super::open_section(&mut menu, "language"));
        assert_eq!(menu.depth(), 0, "остались на корне");
        assert_eq!(menu.frame().items.len(), before);
    }

    #[test]
    fn demo_tree_is_valid() {
        assert!(state::validate(&demo_tree()).is_ok());
    }

    #[test]
    fn demo_root_has_nine_sections() {
        let menu = demo_menu();
        assert_eq!(menu.depth(), 0);
        assert_eq!(
            menu.frame()
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            [
                "Приложения",
                "Панель",
                "Стиль",
                "Уведомления",
                "Язык",
                "Система",
                "Захват",
                "Бинды",
                "Hudbar"
            ]
        );
    }

    #[test]
    fn style_section_shows_two_rows_with_values() {
        let mut menu = demo_menu();
        menu.move_sel(2);
        assert_eq!(menu.enter(), Outcome::Pushed);
        let frame = menu.frame();
        assert_eq!(frame.items.len(), 2);
        assert_eq!(
            frame
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["Обои", "Схема matugen"]
        );
        assert_eq!(
            frame.items[0].value, "",
            "у обоев вместо значения миниатюра"
        );
        assert!(!frame.items[1].value.is_empty(), "схема показывает имя");
    }

    #[test]
    fn searching_dnd_on_the_root_finds_two_rows() {
        let mut menu = demo_menu();
        for ch in "dnd".chars() {
            menu.type_char(ch);
        }
        let frame = menu.frame();
        assert_eq!(frame.query, "dnd");
        assert_eq!(frame.items.len(), 2, "две строки на запрос dnd");
        assert_eq!(frame.items[0].title, "Режим DND");
        assert_eq!(frame.items[0].caption.as_deref(), Some("Панель ›"));
        assert_eq!(frame.items[1].caption.as_deref(), Some("Уведомления ›"));
    }

    #[test]
    fn every_demo_node_has_an_icon() {
        fn walk(nodes: &[Node]) {
            for node in nodes {
                assert!(
                    node.icon.chars().count() == 1,
                    "у «{}» иконка не один глиф: {:?}",
                    node.title,
                    node.icon
                );
                if let Some(children) = node.children() {
                    walk(children);
                }
            }
        }
        walk(&demo_tree());
    }

    #[test]
    fn strings_are_collected_in_one_place() {
        assert_eq!(strings::SEARCH_PLACEHOLDER, "Поиск…");
        assert_eq!(strings::NOTHING_FOUND, "Ничего не найдено");
        for hint in [
            strings::HINT_SELECT,
            strings::HINT_OPEN,
            strings::HINT_BACK,
            strings::HINT_CLOSE,
        ] {
            assert!(!hint.is_empty(), "подсказка не должна быть пустой");
            assert!(
                hint.chars().all(|ch| !ch.is_control()),
                "в подсказке «{hint}» есть управляющий символ"
            );
        }
        assert_eq!(strings::CRUMBS_SEPARATOR, " › ");
    }

    #[test]
    fn chrome_follows_the_window_language() {
        let ru = strings::chrome(super::settings::Language::Ru);
        assert_eq!(ru.search, strings::SEARCH_PLACEHOLDER);
        assert_eq!(ru.search_scope, "В разделе…");
        assert_eq!(ru.nothing, strings::NOTHING_FOUND);
        assert_eq!(ru.on, "вкл");
        assert_eq!(ru.off, "выкл");
        let en = strings::chrome(super::settings::Language::En);
        assert_eq!(en.search, "Search…");
        assert_eq!(en.search_scope, "In section…");
        assert_eq!(en.on, "on");
        assert_eq!(en.off, "off");
        assert_ne!(en.select, ru.select);
    }

    #[test]
    fn fake_values_move_with_their_setters() {
        fake::set_height(31.0);
        assert_eq!(fake::height(), 31.0);
        fake::set_dnd(false);
        assert!(!fake::dnd());
        fake::set_scheme(1);
        assert_eq!(fake::scheme(), 1);
        fake::set_scheme(0);
        fake::set_dnd(true);
        fake::set_height(27.0);
    }
}

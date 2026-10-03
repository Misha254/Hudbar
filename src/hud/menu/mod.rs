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
pub mod item;
#[allow(dead_code)]
pub mod snapshot;
#[allow(dead_code)]
pub mod state;
#[allow(dead_code)]
pub mod tree;
#[allow(dead_code)]
pub mod view;

// Модули hud, нужные подмодулям меню. Реэкспорт, а не `super::super::`: в
// `main.rs` меню лежит в `hud::menu`, а в `hud-settings-rs` — в корне
// крейта, и путь до общих модулей у них разный.
use action::{Action, Live};
use state::Menu;
use tree::Node;

#[allow(unused_imports)]
pub(crate) use super::{
    bind_data, config_io, palette, settings, settings_icons, settings_ui, settings_view,
    settings_widgets, text, ui_tokens,
};

/// Заголовок приложения в крошках: «HUD › Стиль».
pub const CRUMB_ROOT: &str = "HUD";

/// Подписи и подсказки меню в одном месте. Русские до появления переводов:
/// когда появится язык из `settings.json`, здесь появятся ключи, а строки
/// переедут в словарь. Пока строки живут тут, их нельзя разбросать по коду.
pub mod strings {
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
    /// Статус после действия.
    pub const STATUS_APPLIED: &str = "ПРИМЕНЕНО";
    pub const STATUS_FAILED: &str = "ОШИБКА";
}

/// Фиктивные значения для снимков и тестов: реальные настройки и панель в
/// модели не нужны, а чтение файлов сделало бы тесты зависимыми от домашней
/// папки. Значения лежат в атомиках, потому что `get`/`set` — обычные
/// функции, как того требует дерево.
pub mod fake {
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Индекс темы: 0 — обычная, 1 — Pixel.
    pub static THEME: AtomicU32 = AtomicU32::new(0);
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
    /// Индекс шрифта интерфейса.
    pub static FONT: AtomicU32 = AtomicU32::new(0);

    pub const THEME_NAMES: [&str; 2] = ["Обычная", "Pixel"];
    pub const LANGUAGE_NAMES: [&str; 2] = ["Русский", "English"];
    pub const SCHEME_NAMES: [&str; 2] = ["tonal-spot", "vibrant"];
    pub const FONT_NAMES: [&str; 2] = ["JetBrainsMono", "Minecraft Rus"];
    pub const WALLPAPERS: [&str; 4] = [
        "anime/103.png",
        "pixelart/light/city.png",
        "nature/forest-01.jpg",
        "renders/01.jpg",
    ];

    pub fn theme() -> usize {
        THEME.load(Ordering::Relaxed) as usize
    }

    pub fn set_theme(index: usize) {
        THEME.store(index as u32, Ordering::Relaxed);
    }

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

    pub fn font() -> usize {
        FONT.load(Ordering::Relaxed) as usize
    }

    pub fn set_font(index: usize) {
        FONT.store(index as u32, Ordering::Relaxed);
    }
}

/// Тестовое дерево меню: восемь разделов на корне, как в макете. Значения
/// фиктивные, поэтому снимки меню воспроизводимы, а смена темы меняет только
/// цвета и шрифт.
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
                Node::choice(
                    settings_icons::THEME,
                    "Тема",
                    &fake::THEME_NAMES,
                    fake::theme,
                    fake::set_theme,
                ),
                Node::choice(
                    settings_icons::LANGUAGE,
                    "Язык",
                    &fake::LANGUAGE_NAMES,
                    fake::language,
                    fake::set_language,
                ),
                Node::picker(settings_icons::PICTURE, "Обои", "wallpaper"),
                Node::choice(
                    settings_icons::SCHEME,
                    "Схема matugen",
                    &fake::SCHEME_NAMES,
                    fake::scheme,
                    fake::set_scheme,
                ),
                Node::choice(
                    settings_icons::FONT_ICON,
                    "Шрифт",
                    &fake::FONT_NAMES,
                    fake::font,
                    fake::set_font,
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
            settings_icons::SYSTEM,
            "Система",
            vec![
                Node::toggle(settings_icons::SYSTEM, "Переключить DND", || false, |_| {}),
                Node::action(
                    settings_icons::NETWORK,
                    "Wi-Fi",
                    Action::Live(Live::WifiRadio(true)),
                ),
            ],
        ),
        Node::submenu(
            settings_icons::ABOUT,
            "О программе",
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

#[cfg(test)]
mod tests {
    use super::state::Outcome;
    use super::*;

    #[test]
    fn demo_tree_is_valid() {
        assert!(state::validate(&demo_tree()).is_ok());
    }

    #[test]
    fn demo_root_has_eight_sections() {
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
                "Захват",
                "Бинды",
                "Система",
                "О программе"
            ]
        );
    }

    #[test]
    fn style_section_shows_five_rows_with_values() {
        let mut menu = demo_menu();
        menu.move_sel(2);
        assert_eq!(menu.enter(), Outcome::Pushed);
        let frame = menu.frame();
        assert_eq!(frame.items.len(), 5);
        assert_eq!(
            frame
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["Тема", "Язык", "Обои", "Схема matugen", "Шрифт"]
        );
        assert_eq!(frame.items[0].value, "Обычная");
        assert_eq!(frame.items[1].value, "Русский");
        assert_eq!(
            frame.items[2].value, "",
            "у обоев вместо значения миниатюра"
        );
    }

    #[test]
    fn searching_dnd_on_the_root_finds_three_rows() {
        let mut menu = demo_menu();
        for ch in "dnd".chars() {
            menu.type_char(ch);
        }
        let frame = menu.frame();
        assert_eq!(frame.query, "dnd");
        assert_eq!(frame.items.len(), 3, "три строки на запрос dnd");
        assert_eq!(frame.items[0].title, "Режим DND");
        assert_eq!(frame.items[0].caption.as_deref(), Some("Панель ›"));
        assert_eq!(frame.items[1].caption.as_deref(), Some("Уведомления ›"));
        assert_eq!(frame.items[2].caption.as_deref(), Some("Система ›"));
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
    fn fake_values_move_with_their_setters() {
        fake::set_height(31.0);
        assert_eq!(fake::height(), 31.0);
        fake::set_theme(1);
        assert_eq!(fake::theme(), 1);
        fake::set_dnd(false);
        assert!(!fake::dnd());
        fake::set_scheme(1);
        assert_eq!(fake::scheme(), 1);
        fake::set_font(1);
        assert_eq!(fake::font(), 1);
        fake::set_scheme(0);
        fake::set_font(0);
        fake::set_dnd(true);
        fake::set_theme(0);
        fake::set_height(27.0);
    }
}

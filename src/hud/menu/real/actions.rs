use super::*;

/// Куда открывать: каталог или файл — без разницы, `nvim` разберётся сам.
pub(crate) fn open_in_editor(path: String) -> Action {
    // Путь приходит из `$HOME` и в дереве живёт `'static`: меню строится
    // один раз на запуск окна, а процесс короткоживущий, так что утечка
    // десятка строк здесь честнее, чем новый тип строки в `Action`.
    Action::Spawn(
        "kitty",
        Box::leak(Box::new(["-e", "nvim", Box::leak(path.into_boxed_str())])),
    )
}

/// Цели быстрого доступа. Порядок — по частоте правки: сначала композитор и
/// его бинды, потом панель и терминал, потом остальное окружение.
pub(crate) fn targets() -> Vec<(&'static str, &'static str, String)> {
    let home = std::env::var("HOME").unwrap_or_default();
    targets_in(std::path::Path::new(&home))
}

/// Цели для заданного домашнего каталога. Вынесено ради тестов: список строк
/// строится по тому, что реально лежит на диске, и проверять это на машине
/// разработчика — значит зависеть от его `~/.config`.
pub(crate) fn targets_in(home: &std::path::Path) -> Vec<(&'static str, &'static str, String)> {
    let path = |parts: &str| home.join(parts);
    vec![
        (settings_icons::SYSTEM, "niri", path(".config/niri")),
        (
            settings_icons::KEYBINDS,
            "binds · niri/binds.kdl",
            path(".config/niri/binds.kdl"),
        ),
        (settings_icons::PANEL, "waybar", path(".config/waybar")),
        (settings_icons::TERMINAL, "kitty", path(".config/kitty")),
        (
            settings_icons::NOTIFICATIONS,
            "dunst",
            path(".config/dunst"),
        ),
        (
            settings_icons::SEARCH,
            "rofi",
            path(".config/rofi/config.rasi"),
        ),
        (settings_icons::THEME, "matugen", path(".config/matugen")),
        (settings_icons::FONT_ICON, "zsh", path(".config/zsh")),
        (settings_icons::APPS, "nvim", path(".config/nvim")),
        (settings_icons::OVERVIEW, "hudbar", path("code/hudbar")),
        (settings_icons::FOLDER, "scripts", path(".local/bin")),
    ]
    .into_iter()
    .map(|(icon, title, path)| (icon, title, path.to_string_lossy().into_owned()))
    .collect()
}

/// Вопрос подтверждения необратимого действия: отдельный уровень с двумя
/// строками, а не флаг в строке. Плюсы такого решения: `Esc` уводит назад
/// сам, «Отмена» — обычное действие [`Action::Back`], а у вопроса вообще нет
/// команды, поэтому исполнителю она и не достанется. Внутренние строки
/// скрыты от глобального поиска: в общем списке два «Отмена» — шум.
pub(crate) fn confirm_actions(icon: &'static str, question: &str, yes: Node, cancel: Node) -> Node {
    Node::submenu(
        icon,
        question,
        vec![yes.hidden_from_search(), cancel.hidden_from_search()],
    )
}

pub(crate) fn confirm_node(
    icon: &'static str,
    question: &'static str,
    yes: &'static str,
    cancel: &'static str,
    program: &'static str,
    args: &'static [&'static str],
) -> Node {
    confirm_actions(
        icon,
        question,
        Node::action(settings_icons::CHECK, yes, Action::Spawn(program, args)),
        Node::action(settings_icons::TIMES, cancel, Action::Back),
    )
}

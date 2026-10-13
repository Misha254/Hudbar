//! Desktop-файлы: где искать, что из них брать и что отбрасывать.
//!
//! Формат — freedesktop.org Desktop Entry Specification: группы в квадратных
//! скобках, внутри — `Key=Value` с продолжением строки обратной косой чертой.
//! Нас интересует только группа `[Desktop Entry]`: остальные группы (`Actions`,
//! `Desktop Action …`) — это обработчики правого клика, а не само приложение.
//!
//! Порядок каталогов задаёт спецификация, и он же решает, чья копия победит:
//! `$XDG_DATA_HOME/applications` перекрывает `$XDG_DATA_DIRS`, а каталоги из
//! `XDG_DATA_DIRS` — по порядку. Поздний файл с тем же desktop-id заменяет
//! ранний, поэтому перекрытие сделано словарём, а не списком: старый скрипт
//! на awk с этим справлялся, но правило «что именно перекрывает» приходилось
//! там держать в голове.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Каталоги с desktop-файлами в порядке приоритета: поздние перекрывают ранние.
pub fn application_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty());
    if let Some(home) = data_home {
        dirs.push(home.join("applications"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for dir in std::env::split_paths(&data_dirs) {
        dirs.push(dir.join("applications"));
    }
    // Flatpak держит приложения вне `XDG_DATA_DIRS`, если его не прописали, а
    // старый скрипт лаунчера перечислял оба экспорта руками. Пользовательский
    // экспорт идёт первым: он перекрывает системный.
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
        // Явный пользовательский каталог в конце: на системе без
        // `XDG_DATA_HOME` он всё равно должен читаться, а `~/.local/share`
        // есть у всех.
        dirs.push(home.join(".local/share/applications"));
    }
    dirs
}

/// Приложение из desktop-файла. Только то, что нужно для показа и запуска.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// Desktop-id: имя файла без `.desktop`. Он же ключ закрепления и команда
    /// `gtk-launch`, поэтому должен совпадать с именем файла буквально.
    pub id: String,
    /// Локализованное имя для строки списка.
    pub name: String,
    /// Имя без локализации: по нему ищут, когда ввели латиницей.
    pub name_plain: String,
    /// `GenericName`: «Терминал», «Почтовый клиент». Хороший синоним для поиска.
    pub generic: String,
    /// `Keywords` и `Categories`: по ним «steam» находится по «игре».
    pub keywords: Vec<String>,
    /// `Exec` как есть: разбирается при запуске, где есть правила полей.
    pub exec: String,
    /// `Icon`: либо имя в теме иконок, либо абсолютный путь (так бывает у
    /// AppImage и игровых лаунчеров).
    pub icon: String,
    /// Запускать в терминале.
    pub terminal: bool,
    /// Активировать через D-Bus, а не запускать `Exec` (flatpak, GNOME-приложения).
    pub dbus_activatable: bool,
    /// Путь к файлу: нужен для перезапуска окна, когда `.desktop` обновился.
    pub path: PathBuf,
}

impl App {
    /// Имя для строки: локализованное, а если его нет — обычное.
    fn display_name(entry: &Entry) -> String {
        localized(entry, "Name").unwrap_or_else(|| entry.raw("Name").unwrap_or_default())
    }
}

/// Одна запись `[Desktop Entry]`: ключи с локализованными вариантами.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    /// Ключ без локализации → значение.
    pub plain: HashMap<String, String>,
    /// Ключ с суффиксом локали (`Name[ru]`) → значение.
    pub localized: HashMap<String, String>,
}

impl Entry {
    fn raw(&self, key: &str) -> Option<String> {
        self.plain.get(key).cloned()
    }

    /// Значение ключа `true`, регистр и пробелы — по спецификации.
    pub fn flag(&self, key: &str) -> bool {
        self.raw(key)
            .map(|value| value.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }
}

/// Язык интерфейса из окружения: `ru_RU.UTF-8` → `ru`.
pub fn locale() -> Option<String> {
    let raw = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.is_empty() && *value != "C" && *value != "POSIX")?;
    let locale = raw.split('.').next().unwrap_or(&raw);
    Some(locale.to_string())
}

/// Локализованное значение ключа: сперва точный язык (`ru_RU`), потом его
/// префикс (`ru`), потом значение без локали.
pub fn localized(entry: &Entry, key: &str) -> Option<String> {
    if let Some(locale) = locale() {
        let candidates = [
            locale.clone(),
            locale.split('_').next().unwrap_or(&locale).to_string(),
        ];
        for candidate in candidates {
            let found = entry
                .localized
                .get(&format!("{key}[{candidate}]"))
                .filter(|value| !value.is_empty());
            if let Some(value) = found {
                return Some(unescape(value));
            }
        }
    }
    entry
        .raw(key)
        .map(|value| unescape(&value))
        .filter(|value| !value.is_empty())
}

/// Раскрытие escape-последовательностей из спецификации: в именах встречается
/// `\s` (пробел), реже `\n`, `\t`, `\r` и `\\`.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            // Неизвестная escape-последовательность по спецификации остаётся
            // как есть: терять символ из-за опечатки в чужом файле незачем.
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Desktop-id из пути: имя файла без расширения.
pub fn desktop_id(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

/// Разбирает desktop-файл и отдаёт группу `[Desktop Entry]`.
///
/// Возвращает `None`, если группы нет или она не первая по стандарту:
/// секции до `[Desktop Entry]` в desktop-файлах не встречаются, а пустой
/// разбор безопаснее, чем молчаливый `Entry` без единого ключа.
pub fn parse(content: &str) -> Option<Entry> {
    let mut entry = Entry::default();
    let mut in_group = false;
    // Ключ, значение которого продолжается следующей строкой: по спецификации
    // признак — обратная косая черта в конце.
    let mut pending: Option<String> = None;
    for raw in content.lines() {
        let line = raw.trim_end_matches(['\r', '\n']);
        let continued = line.ends_with('\\');
        // Ведущие пробелы строки-продолжения в значение не входят: иначе
        // «Some long \\\n  text» склеился бы в «Some long   text».
        let piece = line.strip_suffix('\\').unwrap_or(line).trim_start();
        if let Some(key) = pending.clone() {
            extend(&mut entry, &key, piece);
            if !continued {
                pending = None;
            }
            continue;
        }
        let trimmed = piece.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(group) = trimmed
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            in_group = group == "Desktop Entry";
            continue;
        }
        if !in_group {
            continue;
        }
        // Делится не обрезанная строка: хвост перед обратной косой чертой —
        // часть значения, и обрезать его здесь значило бы склеить слова.
        let Some((name, value)) = piece.split_once('=') else {
            continue;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        // Пробел после `=` не часть значения, а вот хвост перед обратной
        // косой — часть: он и отделяет слова в «A \\ B».
        let value = value.trim_start();
        let value = if continued { value } else { value.trim_end() };
        insert(&mut entry, &name, value);
        if continued {
            pending = Some(name);
        }
    }
    Some(entry)
}

/// Дописывает кусок значения к ключу: продолжение строки приклеивается к уже
/// записанному значению, а не заменяет его.
fn extend(entry: &mut Entry, key: &str, value: &str) {
    // Локализованный ключ лежит в словаре дважды — под полным именем и под
    // базовым, — и дописывать надо оба, иначе продолжение строки попало бы
    // только в одну из двух копий.
    if let Some(base) = base_key(key) {
        for name in [base, key.to_string()] {
            if let Some(existing) = entry.localized.get_mut(&name) {
                existing.push_str(value);
            }
        }
        return;
    }
    if let Some(existing) = entry.plain.get_mut(key) {
        existing.push_str(value);
    } else {
        entry.plain.insert(key.to_string(), value.to_string());
    }
}

/// Ключ без локали или сам с ней: суффикс в квадратных скобках отделяет одно
/// значение от другого.
fn base_key(key: &str) -> Option<String> {
    match key.split_once('[') {
        Some((base, rest)) if rest.ends_with(']') => Some(base.to_string()),
        _ => None,
    }
}

/// Записывает значение ключа. Повторный ключ не перетирает первый: как и в самом
/// формате, первое значение главное.
fn insert(entry: &mut Entry, key: &str, value: &str) {
    let base = base_key(key);
    match base {
        Some(base) => {
            entry
                .localized
                .entry(base)
                .or_insert_with(|| value.to_string());
            entry
                .localized
                .entry(key.to_string())
                .or_insert_with(|| value.to_string());
        }
        None => {
            entry
                .plain
                .entry(key.to_string())
                .or_insert_with(|| value.to_string());
        }
    }
}

/// Есть ли программа в `PATH`. Нужен для `TryExec`: файл с недоустановленным
/// бинарником в меню показывать незачем.
pub fn has_program(program: &str) -> bool {
    let program = program.trim();
    if program.is_empty() {
        return false;
    }
    if program.contains('/') {
        return Path::new(program).exists();
    }
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).exists()))
}

/// Собирает приложения из всех каталогов. Поздние перекрывают ранние по
/// desktop-id, порядок — по имени, чтобы список не прыгал при перезапуске.
pub fn scan() -> Vec<App> {
    let mut by_id: HashMap<String, App> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for dir in application_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "desktop"))
            .filter(|path| path.is_file())
            .collect();
        files.sort();
        for path in files {
            let Some(id) = desktop_id(&path) else {
                continue;
            };
            // Desktop-id, начинающийся с дефиса, — принудительно скрытый:
            // так добавляют приложения, которые видны в меню «приложений» GNOME,
            // но не должны попадать в лаунчер.
            if id.starts_with('-') {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(entry) = parse(&content) else {
                continue;
            };
            let Some(app) = build(id.clone(), entry, path) else {
                // Файл без валидного приложения должен убрать перекрытую
                // запись, а не оставить её от прошлого каталога.
                by_id.remove(&id);
                continue;
            };
            if !order.contains(&id) {
                order.push(id.clone());
            }
            by_id.insert(id, app);
        }
    }
    let mut apps: Vec<App> = order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect();
    apps.sort_by_key(|app| (app.name.to_lowercase(), app.id.to_lowercase()));
    apps
}

/// Собирает приложение из разобранной группы. `None`, если файл скрыт или это
/// не приложение.
fn build(id: String, entry: Entry, path: PathBuf) -> Option<App> {
    let kind = entry.raw("Type").unwrap_or_default();
    // Тип по умолчанию — `Application`, но указывать его необязательно, поэтому
    // пустое значение считаем приложением, а всё остальное отбрасываем.
    if !kind.is_empty() && kind != "Application" {
        return None;
    }
    if entry.flag("NoDisplay") || entry.flag("Hidden") {
        return None;
    }
    if let Some(try_exec) = entry.raw("TryExec")
        && !has_program(&try_exec)
    {
        return None;
    }
    let exec = entry.raw("Exec").unwrap_or_default();
    let dbus_activatable = entry.flag("DBusActivatable");
    if exec.trim().is_empty() && !dbus_activatable {
        return None;
    }
    let name = App::display_name(&entry);
    if name.trim().is_empty() {
        return None;
    }
    let name_plain = entry.raw("Name").unwrap_or_else(|| name.clone());
    let generic = localized(&entry, "GenericName").unwrap_or_default();
    let mut keywords: Vec<String> = Vec::new();
    for key in ["Keywords", "Categories"] {
        if let Some(value) = localized(&entry, key) {
            for word in value.split(';') {
                let word = word.trim();
                if !word.is_empty() && !keywords.iter().any(|item| item == word) {
                    keywords.push(word.to_string());
                }
            }
        }
    }
    Some(App {
        id,
        name,
        name_plain,
        generic,
        keywords,
        exec,
        icon: entry.raw("Icon").unwrap_or_default(),
        terminal: entry.flag("Terminal"),
        dbus_activatable,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = "[Desktop Entry]\n\
        Name=Firefox\n\
        Name[ru]=Файрфокс\n\
        GenericName=Web Browser\n\
        GenericName[ru]=Веб-браузер\n\
        Comment=Browse the World Wide Web\n\
        Exec=firefox %u\n\
        Icon=firefox\n\
        Terminal=false\n\
        Type=Application\n\
        Categories=Network;WebBrowser;\n\
        Keywords=web;браузер;\n";

    #[test]
    fn a_plain_entry_is_parsed() {
        let entry = parse(FIREFOX).expect("группа Desktop Entry должна читаться");
        assert_eq!(entry.raw("Icon").as_deref(), Some("firefox"));
        assert_eq!(entry.raw("Exec").as_deref(), Some("firefox %u"));
        assert!(!entry.flag("Terminal"));
        assert!(!entry.flag("Hidden"));
    }

    #[test]
    fn localized_names_do_not_overwrite_the_plain_one() {
        let entry = parse(FIREFOX).expect("разбор");
        // Суффикс локали не должен затирать `Name`: без него поиск по-английски
        // перестал бы работать.
        assert_eq!(entry.raw("Name").as_deref(), Some("Firefox"));
        assert_eq!(
            entry.localized.get("Name[ru]").map(String::as_str),
            Some("Файрфокс")
        );
        assert_eq!(
            entry.localized.get("Name[ru]"),
            Some(&"Файрфокс".to_string()),
            "значение лежит и по полному ключу"
        );
    }

    #[test]
    fn flags_accept_any_case_and_surrounding_spaces() {
        let entry = parse("[Desktop Entry]\nType=Application\nTerminal= True \n").expect("разбор");
        assert!(entry.flag("Terminal"));
        assert!(!entry.flag("NoDisplay"));
        assert!(
            !entry.flag("Missing"),
            "отсутствующий ключ — не истина, а отсутствие"
        );
    }

    #[test]
    fn other_groups_are_ignored() {
        let content = "[Desktop Entry]\nName=Приложение\nExec=app\n\
            [Desktop Action new]\nName=Новое окно\nExec=app --new\n";
        let entry = parse(content).expect("разбор");
        // `Name` из чужой группы не должен попасть в запись приложения.
        assert_eq!(entry.raw("Name").as_deref(), Some("Приложение"));
        assert_eq!(entry.raw("Exec").as_deref(), Some("app"));
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let entry = parse("# комментарий\n\n[Desktop Entry]\n# ещё\nName=A\n\n").expect("разбор");
        assert_eq!(entry.raw("Name").as_deref(), Some("A"));
    }

    #[test]
    fn a_multiline_value_is_joined() {
        let entry = parse("[Desktop Entry]\nName=A \\\n  B\nExec=app\n").expect("разбор");
        assert_eq!(entry.raw("Name").as_deref(), Some("A B"));
    }

    #[test]
    fn escapes_are_expanded_in_names() {
        assert_eq!(unescape(r"Один\stwo"), "Один two");
        assert_eq!(unescape(r"путь\\к"), r"путь\к");
        assert_eq!(unescape(r"хвост\"), r"хвост\");
    }

    #[test]
    fn a_hidden_entry_is_not_an_app() {
        let hidden = "[Desktop Entry]\nName=A\nExec=a\nNoDisplay=true\n";
        let entry = parse(hidden).expect("разбор");
        assert!(build("a".into(), entry, PathBuf::from("a.desktop")).is_none());

        let other = "[Desktop Entry]\nName=A\nExec=a\nHidden=true\n";
        let entry = parse(other).expect("разбор");
        assert!(build("a".into(), entry, PathBuf::from("a.desktop")).is_none());
    }

    #[test]
    fn a_link_entry_is_not_an_app() {
        let entry = parse("[Desktop Entry]\nType=Link\nName=A\nURL=https://x\n").expect("разбор");
        assert!(build("a".into(), entry, PathBuf::from("a.desktop")).is_none());
    }

    /// Тип по умолчанию — `Application`, поэтому файл без `Type` остаётся
    /// приложением: так пишут половина ярлыков в `~/.local/share`.
    #[test]
    fn a_missing_type_means_application() {
        let entry = parse("[Desktop Entry]\nName=A\nExec=a\n").expect("разбор");
        let app = build("a".into(), entry, PathBuf::from("a.desktop"))
            .expect("файл без Type должен остаться приложением");
        assert_eq!(app.name, "A");
    }

    #[test]
    fn an_entry_without_exec_or_dbus_is_dropped() {
        let entry = parse("[Desktop Entry]\nName=A\n").expect("разбор");
        assert!(build("a".into(), entry, PathBuf::from("a.desktop")).is_none());
    }

    /// D-Bus-приложениям `Exec` не нужен: их активирует `gtk-launch`, и файл без
    /// команды — норма, а не мусор.
    #[test]
    fn a_dbus_entry_survives_without_exec() {
        let entry = parse("[Desktop Entry]\nName=A\nDBusActivatable=true\n").expect("разбор");
        let app = build("a".into(), entry, PathBuf::from("a.desktop"))
            .expect("D-Bus-приложение должно остаться");
        assert!(app.dbus_activatable);
        assert!(app.exec.is_empty());
    }

    #[test]
    fn try_exec_hides_an_app_without_its_binary() {
        let missing = "[Desktop Entry]\nName=A\nExec=a\nTryExec=definitely-not-here\n";
        let entry = parse(missing).expect("разбор");
        assert!(build("a".into(), entry, PathBuf::from("a.desktop")).is_none());

        let present = "[Desktop Entry]\nName=A\nExec=a\nTryExec=sh\n";
        let entry = parse(present).expect("разбор");
        assert!(build("a".into(), entry, PathBuf::from("a.desktop")).is_some());
    }

    #[test]
    fn a_terminal_app_is_marked() {
        let entry = parse("[Desktop Entry]\nName=A\nExec=a\nTerminal=true\n").expect("разбор");
        let app = build("a".into(), entry, PathBuf::from("a.desktop")).expect("приложение");
        assert!(app.terminal);
    }

    /// Категории и ключевые слова становятся синонимами для поиска: «steam»
    /// должен находиться по «игре».
    #[test]
    fn keywords_and_categories_become_synonyms() {
        let entry = parse(FIREFOX).expect("разбор");
        let app =
            build("firefox".into(), entry, PathBuf::from("firefox.desktop")).expect("приложение");
        assert!(app.keywords.iter().any(|word| word == "WebBrowser"));
        assert!(app.keywords.iter().any(|word| word == "браузер"));
        // `GenericName[ru]` — локализованный, но в этом файле его нет, поэтому
        // берётся английский: пустое поле в поиске хуже перевода.
        assert_eq!(app.generic, "Web Browser");
    }

    #[test]
    fn the_desktop_id_is_the_file_stem() {
        assert_eq!(
            desktop_id(Path::new("/usr/share/applications/org.foo.Bar.desktop")).as_deref(),
            Some("org.foo.Bar")
        );
        // Имя с дефисом на конце теряет его при разборе расширения — поэтому
        // desktop-id скрытых ярлыков приходится отсекать по началу.
        assert_eq!(
            desktop_id(Path::new("-hidden.desktop")).as_deref(),
            Some("-hidden")
        );
        // У каталога нет ни имени, ни расширения: id не из чего взять.
        assert_eq!(desktop_id(Path::new("/")), None);
    }

    #[test]
    fn a_program_is_looked_up_in_path() {
        assert!(has_program("sh"), "sh есть в PATH");
        assert!(!has_program("definitely-not-here-42"));
        assert!(!has_program("   "));
        assert!(
            has_program("/bin/sh"),
            "абсолютный путь проверяется как есть"
        );
    }

    /// Скан на живой системе обязан найти хоть что-то и не принести мусора:
    /// проверка на реальных каталогах ловит опечатки в путях, которые на
    /// синтетических файлах не видны.
    #[test]
    fn the_scan_of_this_machine_returns_real_apps() {
        let apps = scan();
        if application_dirs().iter().all(|dir| !dir.is_dir()) {
            return; // тест бессмысленен там, где нет ни одного каталога
        }
        assert!(!apps.is_empty(), "скан должен что-то найти");
        assert!(
            apps.iter().all(|app| !app.name.trim().is_empty()),
            "в списке не должно быть безымянных строк"
        );
        assert!(
            apps.iter().all(|app| !app.id.starts_with('-')),
            "скрытые desktop-id в список не попадают"
        );
        let mut ids: Vec<&str> = apps.iter().map(|app| app.id.as_str()).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "desktop-id не должны повторяться");
    }
}

//! Общие данные для окон биндов (KP_8 — niri, KP_9 — yazi).
//! Порт логики `keybind.sh` / `yazibind.sh`: только std, без внешних зависимостей.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: String,
    pub desc: String,
    pub search: String,
}

impl Entry {
    pub fn new(key: String, desc: String) -> Self {
        let search = format!("{key} {desc}").to_lowercase();
        Entry { key, desc, search }
    }

    pub fn matches(&self, q: &str) -> bool {
        q.is_empty() || self.search.contains(q)
    }
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

// ---------------------------------------------------------------- niri (KP_8)

const NIRI_TITLE_MARK: &str = "hotkey-overlay-title=\"";

/// Разбор одной строки kdl: `    KP_8 hotkey-overlay-title="Текст" { ... }`.
pub fn parse_niri_line(line: &str) -> Option<(String, String)> {
    let pos = line.find(NIRI_TITLE_MARK)?;
    let title_start = pos + NIRI_TITLE_MARK.len();
    let title_end = line[title_start..].find('"')?;
    let title = line[title_start..title_start + title_end].to_string();
    let bind = line[..pos].split_whitespace().next()?.to_string();
    if bind.is_empty() {
        return None;
    }
    Some((bind, title))
}

/// Все бинды из `~/.config/niri/*.kdl`, сгруппированные по ближайшему `//`-комментарию.
pub fn load_niri() -> Vec<Entry> {
    load_niri_grouped()
        .into_iter()
        .flat_map(|(_, entries)| entries)
        .collect()
}

/// Имя группы из `//`-комментария kdl. Разделители `===` и закомментированный
/// код группой не считаются — возвращается `None`.
pub fn group_for_comment(line: &str) -> Option<String> {
    let name = line.trim().strip_prefix("//")?.trim();
    let code_like = name.contains('{') || name.contains(';') || name.contains('}');
    if name.is_empty() || name.starts_with('=') || code_like {
        return None;
    }
    Some(name.trim_matches('=').trim().to_string())
}

pub fn load_niri_grouped() -> Vec<(String, Vec<Entry>)> {
    let dir = home().join(".config/niri");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "kdl"))
        .collect();
    files.sort();
    let mut groups: Vec<(String, Vec<Entry>)> = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut group = String::from("Общее");
        for line in text.lines() {
            if let Some(name) = group_for_comment(line) {
                group = name;
            }
            if let Some((bind, title)) = parse_niri_line(line) {
                let entry = Entry::new(bind, title);
                if let Some((_, entries)) = groups.iter_mut().find(|(name, _)| name == &group) {
                    entries.push(entry);
                } else {
                    groups.push((group.clone(), vec![entry]));
                }
            }
        }
    }
    groups
}

// ---------------------------------------------------------------- yazi (KP_9)

/// Группа для биндов, которым не предшествует комментарий-подзаголовок.
const YAZI_FALLBACK_GROUP: &str = "Прочее";

/// Имя блока ходовых биндов, который собирается в начало окна.
pub const YAZI_FAVORITES_GROUP: &str = "Частые";

/// Самые ходовые бинды для работы с файлами и каталогами — блок «Частые»
/// в начале окна. Ссылаемся на описание, а не на клавишу: клавиша в keymap
/// записывается по-русски, а в окне показывается транслитерацией. Чего в
/// keymap нет — то в блок просто не попадает.
pub const YAZI_FAVORITES: [&str; 18] = [
    "Открыть выбранное",
    "Копировать",
    "Вырезать",
    "Вставить",
    "Вставить с перезаписью",
    "Отменить копирование",
    "В корзину",
    "Удалить навсегда",
    "Создать файл/каталог",
    "Создать несколько",
    "Переименовать",
    "Поиск по имени",
    "Поиск по содержимому",
    "Фильтр",
    "Копировать путь",
    "Скопировать путь каталога",
    "Домой",
    "Переход через fzf",
];

/// Ходовые бинды, найденные среди групп, — в том же порядке, что и список.
/// Повторяющиеся описания берём по одному разу.
pub fn yazi_favorites(groups: &[(String, Vec<Entry>)]) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for want in YAZI_FAVORITES {
        let found = groups
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .find(|e| e.desc == want);
        if let Some(entry) = found
            && !out.iter().any(|e| e.desc == entry.desc)
        {
            out.push(entry.clone());
        }
    }
    out
}

/// `"<s-X>"` -> `"X+Shift"`, запятая -> плюс, мусор из `on = ...` чистится.
pub fn transform_yazi_key(raw_on: &str) -> String {
    let mut s: String = raw_on.chars().filter(|&c| c != '"').collect();
    s = s.trim().to_string();
    s = s.replace(['[', ']'], "");
    s = s.chars().filter(|c| !c.is_whitespace()).collect();
    // <s-X> -> X+Shift (глобально)
    let mut out = String::with_capacity(s.len() + 8);
    let mut rest = s.as_str();
    while let Some(i) = rest.find("<s-") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 3..];
        match after.find('>') {
            Some(end) => {
                out.push_str(&after[..end]);
                out.push_str("+Shift");
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.replace(',', "+")
}

/// Разбор одной строки секции `[mgr]`: достаёт `on` и `desc`.
pub fn parse_yazi_line(line: &str) -> Option<(String, String)> {
    let on_pos = line.find("on =")?;
    let mut v = line[on_pos + 4..].trim_start();
    let raw_on: &str;
    if v.starts_with('[') {
        let end = v.find(']')?;
        raw_on = &v[..=end];
        v = &v[end + 1..];
    } else if v.starts_with('"') {
        let end = v[1..].find('"')?;
        raw_on = &v[..=1 + end];
        v = &v[1 + end + 1..];
    } else {
        return None;
    }
    let desc_pos = v.find("desc =")?;
    let mut d = v[desc_pos + 6..].trim_start();
    if !d.starts_with('"') {
        return None;
    }
    d = &d[1..];
    let end = d.find('"')?;
    let mut desc = d[..end].to_string();
    if let Some(p) = desc.find(" (") {
        desc.truncate(p);
    }
    Some((transform_yazi_key(raw_on), desc))
}

/// Комментарий-подзаголовок секции `[mgr]`: имя группы для биндов ниже.
/// Хвост вида `(chord: с + ...)` отбрасываем — префикс и так есть в самом бинде.
pub fn yazi_group_comment(line: &str) -> Option<String> {
    let name = line.trim_start().strip_prefix('#')?.trim();
    let name = match name.find(" (") {
        Some(p) => name[..p].trim(),
        None => name,
    };
    (!name.is_empty()).then(|| name.to_string())
}

/// Бинды секции `[mgr]`, разложенные по группам из комментариев-подзаголовков.
/// Порядок групп и биндов внутри них — как в самом keymap.toml.
pub fn parse_yazi_groups(text: &str) -> Vec<(String, Vec<Entry>)> {
    let mut inmgr = false;
    let mut group = YAZI_FALLBACK_GROUP.to_string();
    let mut groups: Vec<(String, Vec<Entry>)> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            inmgr = t.starts_with("[mgr]");
            if inmgr {
                group = YAZI_FALLBACK_GROUP.to_string();
            }
            continue;
        }
        if !inmgr {
            continue;
        }
        if let Some(name) = yazi_group_comment(line) {
            group = name;
            continue;
        }
        let Some((key, desc)) = parse_yazi_line(line) else {
            continue;
        };
        let entry = yazi_entry(&key, &desc);
        match groups.iter_mut().find(|(name, _)| *name == group) {
            Some((_, entries)) => entries.push(entry),
            None => groups.push((group.clone(), vec![entry])),
        }
    }
    groups
}

fn yazi_entry(key: &str, desc: &str) -> Entry {
    let key = transliterate(key);
    // `Enter` и `o` в yazi открывают одно и то же — показываем оба варианта.
    if key == "o" && desc == "Открыть выбранное" {
        return Entry::new("Enter / o".to_string(), desc.to_string());
    }
    Entry::new(key, desc.to_string())
}

fn translit_char(c: char) -> char {
    match c {
        'й' => 'q',
        'ц' => 'w',
        'у' => 'e',
        'к' => 'r',
        'е' => 't',
        'н' => 'y',
        'г' => 'u',
        'ш' => 'i',
        'щ' => 'o',
        'з' => 'p',
        'х' => '[',
        'ъ' => ']',
        'ф' => 'a',
        'ы' => 's',
        'в' => 'd',
        'а' => 'f',
        'п' => 'g',
        'р' => 'h',
        'о' => 'j',
        'л' => 'k',
        'д' => 'l',
        'ж' => ';',
        'э' => '\'',
        'я' => 'z',
        'ч' => 'x',
        'с' => 'c',
        'м' => 'v',
        'и' => 'b',
        'т' => 'n',
        'ь' => 'm',
        'б' => ',',
        'ю' => '.',
        'Й' => 'Q',
        'Ц' => 'W',
        'У' => 'E',
        'К' => 'R',
        'Е' => 'T',
        'Н' => 'Y',
        'Г' => 'U',
        'Ш' => 'I',
        'Щ' => 'O',
        'З' => 'P',
        'Х' => '[',
        'Ъ' => ']',
        'Ф' => 'A',
        'Ы' => 'S',
        'В' => 'D',
        'А' => 'F',
        'П' => 'G',
        'Р' => 'H',
        'О' => 'J',
        'Л' => 'K',
        'Д' => 'L',
        'Ж' => ';',
        'Э' => '\'',
        'Я' => 'Z',
        'Ч' => 'X',
        'С' => 'C',
        'М' => 'V',
        'И' => 'B',
        'Т' => 'N',
        'Ь' => 'M',
        'Б' => ',',
        'Ю' => '.',
        _ => c,
    }
}

/// ЙЦУКЕН -> QWERTY.
pub fn transliterate(s: &str) -> String {
    s.chars().map(translit_char).collect()
}

/// Группы биндов yazi из `~/.config/yazi/keymap.toml`: подзаголовки `# ...`
/// секции `[mgr]` делят бинды на секции, имена и порядок — как в файле.
/// В начало добавляется блок «Частые» — те же бинды, но собранные вместе;
/// в своих группах они остаются, так что ни один бинд не теряется.
pub fn load_yazi_grouped() -> Vec<(String, Vec<Entry>)> {
    let Ok(text) = std::fs::read_to_string(home().join(".config/yazi/keymap.toml")) else {
        return Vec::new();
    };
    let mut groups = parse_yazi_groups(&text);
    let favorites = yazi_favorites(&groups);
    if !favorites.is_empty() {
        groups.insert(0, (YAZI_FAVORITES_GROUP.to_string(), favorites));
    }
    groups
}

// ---------------------------------------------------------------- тема

/// Тема оформления из `~/.config/hudbar/settings.json` (как `hud-theme current`).
/// Читаем значение ключа `"theme"`, а не ищем подстроку: любое другое поле
/// с этим словом (`"normal"` в шрифте, в описании) переключало тему не туда.
pub fn read_theme() -> String {
    let Ok(text) = std::fs::read_to_string(home().join(".config/hudbar/settings.json")) else {
        return "pixel".to_string();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return "pixel".to_string();
    };
    value
        .get("appearance")
        .and_then(|v| v.get("theme"))
        .and_then(serde_json::Value::as_str)
        .filter(|theme| matches!(*theme, "pixel" | "normal"))
        .unwrap_or("pixel")
        .to_string()
}

pub fn pixel_mode() -> bool {
    read_theme() != "normal"
}

pub fn configured_font() -> String {
    if pixel_mode() {
        "Minecraft Rus".to_string()
    } else {
        "JetBrainsMono Nerd Font Propo".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn niri_line_parses_bind_and_title() {
        let line = r#"    KP_8 hotkey-overlay-title="Показать список" { spawn-sh "x"; }"#;
        assert_eq!(
            parse_niri_line(line),
            Some(("KP_8".to_string(), "Показать список".to_string()))
        );
    }

    #[test]
    fn niri_line_skips_null_titles() {
        assert_eq!(
            parse_niri_line("    super+T hotkey-overlay-title=null { spawn \"kitty\"; }"),
            None
        );
    }

    #[test]
    fn niri_group_names_skip_separators_and_commented_code() {
        assert_eq!(
            group_for_comment("    // Focus navigation"),
            Some("Focus navigation".to_string())
        );
        assert_eq!(group_for_comment("// === Launchers ==="), None);
        assert_eq!(
            group_for_comment("// super+Escape allow-inhibiting=false { x; }"),
            None
        );
        assert_eq!(group_for_comment("//"), None);
        assert_eq!(group_for_comment("    super+T { spawn \"kitty\"; }"), None);
    }

    #[test]
    fn yazi_key_simple_and_shift() {
        assert_eq!(transform_yazi_key(r#""й""#), "й");
        assert_eq!(transform_yazi_key(r#""<s-Й>""#), "Й+Shift");
    }

    #[test]
    fn yazi_key_chord_joins_with_plus() {
        assert_eq!(transform_yazi_key(r#"[ "с", "с" ]"#), "с+с");
        assert_eq!(transform_yazi_key(r#"[ "с", "<s-С>" ]"#), "с+С+Shift");
    }

    #[test]
    fn yazi_key_comma_chord_matches_shell() {
        // как в оригинале: `с,с` -> `с+с`, `,,ь` -> `++ь`
        assert_eq!(transform_yazi_key(r#"[ ",", "ь" ]"#), "++ь");
    }

    #[test]
    fn yazi_line_cuts_desc_suffix() {
        let line = r#"{ on = "й", run = "quit", desc = "Выход (RU)" },"#;
        assert_eq!(
            parse_yazi_line(line),
            Some(("й".to_string(), "Выход".to_string()))
        );
    }

    #[test]
    fn yazi_line_chord() {
        let line = r#"{ on = [ "с", "с" ], run = "copy path", desc = "Скопировать путь (RU)" },"#;
        assert_eq!(
            parse_yazi_line(line),
            Some(("с+с".to_string(), "Скопировать путь".to_string()))
        );
    }

    #[test]
    fn theme_reads_the_theme_field_only() {
        fn parse_theme(settings: &str) -> Option<String> {
            let value: serde_json::Value = serde_json::from_str(settings).ok()?;
            value
                .get("appearance")?
                .get("theme")?
                .as_str()
                .filter(|theme| matches!(*theme, "pixel" | "normal"))
                .map(str::to_string)
        }

        assert_eq!(
            parse_theme(r#"{"appearance":{"theme":"normal"}}"#).as_deref(),
            Some("normal")
        );
        assert_eq!(
            parse_theme(r#"{"appearance":{"theme":"pixel"},"hudbar":{"font":"normal"}}"#)
                .as_deref(),
            Some("pixel")
        );
        assert_eq!(parse_theme("{}"), None);
        assert_eq!(parse_theme(r#"{"appearance":{"theme":3}}"#), None);
    }

    #[test]
    fn translit_covers_layout() {
        assert_eq!(transliterate("щод"), "ojl");
        assert_eq!(transliterate("с+с"), "c+c");
        assert_eq!(transliterate("ghj"), "ghj");
    }

    #[test]
    fn yazi_groups_follow_keymap_comments() {
        let text = r#"
# Комментарий-шапка, до [mgr] — в группы не идёт
[mgr]
append_keymap = [
    # Навигация
    { on = "д", run = "enter", desc = "Войти в каталог (RU)" },
    { on = "<s-Р>", run = "back", desc = "К предыдущему каталогу (RU)" },

    # Копирование пути (chord: с + ...)
    { on = [ "с", "с" ], run = "copy path", desc = "Скопировать путь (RU)" },
]

[tasks]
append_keymap = [
    { on = "ц", run = "close", desc = "Закрыть задачи (RU)" },
]
"#;
        let groups = parse_yazi_groups(text);
        let names: Vec<&str> = groups.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Навигация", "Копирование пути"]);
        let keys: Vec<&str> = groups
            .iter()
            .flat_map(|(_, e)| e.iter())
            .map(|e| e.key.as_str())
            .collect();
        // хвост «(chord: …)» отброшен, клавиши транслитерированы в QWERTY
        assert_eq!(keys, ["l", "H+Shift", "c+c"]);
        assert_eq!(groups[1].1[0].desc, "Скопировать путь");
    }

    #[test]
    fn yazi_groups_fall_back_and_skip_other_sections() {
        let text = r#"
[mgr]
append_keymap = [
    { on = "й", run = "quit", desc = "Выход (RU)" },
    # пустой комментарий игнорируется
    #Сортировка
]
[input]
append_keymap = [
    { on = "ш", run = "insert", desc = "Ввод (RU)" },
]
"#;
        let groups = parse_yazi_groups(text);
        // бинд без подзаголовка попадает в «Прочее», другая секция не читается
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "Прочее");
        assert_eq!(groups[0].1.len(), 1);
    }

    #[test]
    fn yazi_group_comment_cuts_chord_hint() {
        assert_eq!(
            yazi_group_comment("\t# Переходы (chord: п + ...)"),
            Some("Переходы".to_string())
        );
        assert_eq!(
            yazi_group_comment("    # Режимы строки"),
            Some("Режимы строки".to_string())
        );
        assert_eq!(yazi_group_comment("\t# "), None);
        assert_eq!(yazi_group_comment(r#"{ on = "й" }"#), None);
    }

    #[test]
    fn yazi_favorites_pick_by_desc_in_listed_order() {
        let text = r#"
[mgr]
append_keymap = [
    # Операции с файлами
    { on = "ч", run = "yank --cut", desc = "Вырезать (RU)" },
    { on = "н", run = "yank", desc = "Копировать (RU)" },
    # Прочее
    { on = "о", run = "quit", desc = "Выход (RU)" },
]
"#;
        let groups = parse_yazi_groups(text);
        let fav = yazi_favorites(&groups);
        // порядок задан списком, а не порядком в keymap
        let pairs: Vec<(&str, &str)> = fav
            .iter()
            .map(|e| (e.key.as_str(), e.desc.as_str()))
            .collect();
        assert_eq!(pairs, [("y", "Копировать"), ("x", "Вырезать")]);
    }

    #[test]
    fn yazi_favorites_skip_absent_descs() {
        // ни одного ходового бинда в keymap нет — блок остаётся пустым
        let groups = parse_yazi_groups("[mgr]\nappend_keymap = [\n# Ничего\n]\n");
        assert!(yazi_favorites(&groups).is_empty());
    }
}

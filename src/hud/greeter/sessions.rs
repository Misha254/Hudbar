//! Список сеансов из `/usr/share/wayland-sessions/*.desktop`.
//!
//! Разбор нарочно простой: `Name` и `Exec` из группы `[Desktop Entry]`, без
//! локализованных `Name[ru]` и без раскрытия `%`-кодов. Код `%f`/`%u` в
//! сеансе не встречается, а если встретится — он отбрасывается, а не
//! передаётся как аргумент.
//!
//! `Exec` разбирается на argv здесь же, без shell: строка вида
//! `env FOO=1 niri-session` становится тремя аргументами, кавычки снимаются,
//! `|`, `;` и `$` остаются обычными символами и никуда не уходят.

use std::path::{Path, PathBuf};

/// Каталог сеансов Wayland.
pub const SESSIONS_DIR: &str = "/usr/share/wayland-sessions";

/// Сеанс: то, что показывается в подвале, и то, что запускает greetd.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    /// `Name` из `.desktop` — данные пользователя, не переводится.
    pub name: String,
    /// `Exec` как есть.
    pub exec: String,
}

impl Session {
    /// argv для `start_session`.
    pub fn argv(&self) -> Vec<String> {
        exec_argv(&self.exec)
    }
}

/// Список сеансов из каталога. Отсутствующий каталог — пустой список, а не
/// ошибка: на машине без niri гритер обязан показать хоть что-то.
pub fn list_from(dir: &Path) -> Vec<Session> {
    let mut sessions = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return sessions;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(session) = parse_desktop(&text) {
            sessions.push(session);
        }
    }
    // Сортировка по имени: порядок каталога не определён, а F2 должен
    // переключать сеансы одинаково между запусками.
    sessions.sort_by(|a, b| a.name.cmp(&b.name));
    sessions
}

/// Сеансы машины.
pub fn list() -> Vec<Session> {
    list_from(Path::new(SESSIONS_DIR))
}

/// `Name` и `Exec` из группы `[Desktop Entry]`.
pub fn parse_desktop(text: &str) -> Option<Session> {
    let mut name = None;
    let mut exec = None;
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            // `Name[ru]` — локализованная подпись; берём базовую `Name`, чтобы
            // не зависеть от локали гритера.
            "Name" if name.is_none() => name = Some(value.to_string()),
            "Exec" if exec.is_none() => exec = Some(value.to_string()),
            _ => {}
        }
    }
    Some(Session {
        name: name?,
        exec: exec?,
    })
}

/// Строка `Exec` в argv. Кавычки снимаются, экранирование через `\`
/// поддерживается, `%`-коды отбрасываются целиком.
pub fn exec_argv(exec: &str) -> Vec<String> {
    let mut argv = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in exec.chars() {
        if escaped {
            current.push(ch);
            started = true;
            escaped = false;
            continue;
        }
        match ch {
            '\\' if quote != Some('\'') => escaped = true,
            '"' | '\'' if quote == Some(ch) => quote = None,
            '"' | '\'' if quote.is_none() => {
                quote = Some(ch);
                // Пустые кавычки — тоже аргумент: `""` это пустая строка.
                started = true;
            }
            ch if quote.is_none() && ch.is_whitespace() => {
                if started {
                    argv.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            ch => {
                current.push(ch);
                started = true;
            }
        }
    }
    if started {
        argv.push(current);
    }
    // `%f`, `%U`, `%%` и прочие коды — не аргументы.
    argv.retain(|arg| !arg.starts_with('%'));
    argv
}

/// Индекс сеанса по умолчанию: niri, если он есть, иначе первый.
pub fn default_index(sessions: &[Session]) -> Option<usize> {
    if sessions.is_empty() {
        return None;
    }
    let niri = sessions.iter().position(|session| {
        let haystack = format!("{} {}", session.name, session.exec).to_lowercase();
        haystack.contains("niri")
    });
    Some(niri.unwrap_or(0))
}

/// Следующий сеанс по кругу — `F2`.
pub fn next_index(len: usize, current: usize) -> usize {
    if len == 0 { 0 } else { (current + 1) % len }
}

/// Каталог домашних сеансов пользователя (`~/.local/share/wayland-sessions`):
/// в нём лежат сеансы, поставленные без пакетного менеджера.
pub fn user_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
        })?;
    Some(base.join("wayland-sessions"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_file_becomes_a_session() {
        let text = "\
[Desktop Entry]
Name=niri
Comment=A scrollable-tiling Wayland compositor
Exec=niri-session
Type=Application
";
        assert_eq!(
            parse_desktop(text),
            Some(Session {
                name: "niri".into(),
                exec: "niri-session".into()
            })
        );
    }

    /// Локализованные подписи не подменяют базовую: `Name[ru]` идёт после
    /// `Name`, и взять его значило бы зависеть от языка приложения.
    #[test]
    fn a_localized_name_does_not_replace_the_base_one() {
        let text = "[Desktop Entry]\nName=niri\nName[ru]=нири\nExec=niri-session\n";
        let session = parse_desktop(text).expect("сеанс");
        assert_eq!(session.name, "niri");
    }

    /// Группа, отличная от `[Desktop Entry]`, не участвует.
    #[test]
    fn another_group_is_ignored() {
        let text = "[Desktop Action New]\nName=New\nExec=foot\n";
        assert_eq!(parse_desktop(text), None);
    }

    #[test]
    fn a_file_without_exec_is_not_a_session() {
        assert_eq!(parse_desktop("[Desktop Entry]\nName=niri\n"), None);
        assert_eq!(parse_desktop(""), None);
    }

    /// `%`-коды и кавычки: argv без shell.
    #[test]
    fn exec_is_split_without_a_shell() {
        assert_eq!(exec_argv("niri-session"), ["niri-session"]);
        assert_eq!(
            exec_argv("niri -c /etc/niri.kdl"),
            ["niri", "-c", "/etc/niri.kdl"]
        );
        assert_eq!(
            exec_argv("env FOO=1 \"my session\" %U"),
            ["env", "FOO=1", "my session"],
            "%U отброшен, кавычки сняты, пробел внутри кавычек сохранён"
        );
        // Символы, которые shell истолковал бы, остаются аргументами.
        assert_eq!(
            exec_argv("sh -c 'echo hi; rm -rf /'"),
            ["sh", "-c", "echo hi; rm -rf /"],
            "строка с ; не разбирается shell"
        );
        assert_eq!(exec_argv("prog --flag=\"a b\""), ["prog", "--flag=a b"]);
        assert!(exec_argv("%f").is_empty());
        assert!(exec_argv("   ").is_empty());
    }

    #[test]
    fn the_default_session_is_niri() {
        let sessions = vec![
            Session {
                name: "Plasma".into(),
                exec: "startplasma-wayland".into(),
            },
            Session {
                name: "niri".into(),
                exec: "niri-session".into(),
            },
        ];
        assert_eq!(default_index(&sessions), Some(1));
        assert_eq!(default_index(&[]), None);
        // Без niri берётся первый, а не «никакой».
        let only = vec![sessions[0].clone()];
        assert_eq!(default_index(&only), Some(0));
    }

    #[test]
    fn f2_wraps_around() {
        assert_eq!(next_index(3, 0), 1);
        assert_eq!(next_index(3, 2), 0);
        assert_eq!(next_index(0, 0), 0);
    }

    /// Каталог читается целиком: два `.desktop`, мусор пропускается, порядок
    /// стабильный.
    #[test]
    fn a_directory_becomes_a_sorted_list() {
        let dir =
            std::env::temp_dir().join(format!("hudbar-greeter-sessions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        std::fs::write(
            dir.join("b.desktop"),
            "[Desktop Entry]\nName=Zwift\nExec=zwift\n",
        )
        .expect("файл");
        std::fs::write(
            dir.join("a.desktop"),
            "[Desktop Entry]\nName=niri\nExec=niri-session\n",
        )
        .expect("файл");
        std::fs::write(dir.join("note.txt"), "не сеанс").expect("файл");

        let sessions = list_from(&dir);
        assert_eq!(
            sessions.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["Zwift", "niri"]
        );
        assert_eq!(
            default_index(&sessions),
            Some(1),
            "niri выбран по умолчанию независимо от порядка сортировки"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_directory_is_an_empty_list() {
        let dir =
            std::env::temp_dir().join(format!("hudbar-greeter-no-sessions-{}", std::process::id()));
        assert!(list_from(&dir).is_empty());
    }
}

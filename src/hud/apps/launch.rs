//! Запуск приложения: разбор строки `Exec` и запуск в фоне.
//!
//! Строка `Exec` — не командная строка: по спецификации это список аргументов,
//! где кавычки отделяют аргумент, а `%`-поля означают файлы и URL, которые мы
//! не передаём. Разбирать её через `sh -c` нельзя: `Exec=foo %f` с пробелом в
//! имени приложения развалился бы, а проценты в имени файла съедались бы.
//!
//! Приложение уходит в фоне: окно закрывается сразу, а процесс должен пережить
//! и окно, и его pid-файл. `setsid -f` для этого — одна внешняя программа,
//! после которой не нужен ни `fork` в коде, ни прямая зависимость от libc.

use std::process::{Command, Stdio};

use super::super::settings::Language;
use super::scan::{App, has_program};

/// Терминал для `Terminal=true`. kitty: он же терминал по умолчанию в панели,
/// и правило слоя в niri уже размывает его окно.
pub const TERMINAL: &str = "kitty";

/// Разбирает `Exec` в программу и аргументы.
///
/// Поля под файлы и URL отбрасываются: окно запускается без аргументов, и
/// подставлять вместо них пустые строки нельзя — программа получила бы лишний
/// аргумент. `%i` — единственное поле, у которого есть содержательная замена:
/// это `--icon` со значением `Icon`, и без него часть GTK-приложений запускается
/// с иконкой компьютера.
pub fn argv(exec: &str, icon: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut started = false;
    for ch in exec.chars() {
        if escaped {
            word.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if !quoted => escaped = true,
            // Кавычка не часть аргумента, а переключатель режима: символ
            // внутри кавычек идёт в то же слово, и пробел внутри них тоже.
            // `started` поднимается здесь: пустые кавычки — это пустой аргумент,
            // и он должен дойти, иначе `app "" --flag` потеряет позицию флага.
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            _ => {
                word.push(ch);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }

    // `%i` разворачивается в два аргумента, поэтому поля раскрываются после
    // разбиения на слова, а не вместо него.
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut expand_icon = false;
    for word in words {
        match word.as_str() {
            "%i" => expand_icon = true,
            field if is_field(field) => {}
            other => out.push(other.to_string()),
        }
    }
    if expand_icon && !icon.is_empty() && !icon.contains('/') {
        out.push("--icon".to_string());
        out.push(icon.to_string());
    }
    out
}

/// Поле подстановки: одиночный `%` с буквой. Два знака подряд — экранированный
/// процент, а не поле, и оставляется как есть.
fn is_field(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next() == Some('%') && chars.next().is_some() && chars.next().is_none()
}

/// Запускает приложение. `Err` — только когда запускать нечем: у строки `Exec`
/// нет программы или нет самого терминала. Ошибки самого процесса сюда не
/// приходят: процесс уже отсоединён, и ждать его завершения нельзя.
pub fn launch(app: &App, lang: Language) -> Result<(), String> {
    if app.dbus_activatable && has_program("gtk-launch") {
        // D-Bus-приложения запускаются через desktop-id: их `Exec` зачастую
        // ведёт на заглушку (`/usr/bin/true`) или отсутствует вовсе.
        return spawn("gtk-launch", &[app.id.as_str()], lang);
    }
    let mut words = argv(&app.exec, &app.icon);
    if words.is_empty() {
        return Err(match lang {
            Language::Ru => format!("у «{}» нет команды запуска", app.name),
            Language::En => format!("“{}” has no launch command", app.name),
        });
    }
    let program = words.remove(0);
    if app.terminal {
        if !has_program(TERMINAL) {
            return Err(match lang {
                Language::Ru => format!("нет терминала {TERMINAL}"),
                Language::En => format!("terminal {TERMINAL} not found"),
            });
        }
        let mut args = vec!["-e".to_string(), program];
        args.extend(words);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        return spawn(TERMINAL, &borrowed, lang);
    }
    let borrowed: Vec<&str> = words.iter().map(String::as_str).collect();
    spawn(&program, &borrowed, lang)
}

/// Запуск в новой сессии. `setsid -f` форкается и сразу возвращает управление,
/// поэтому его собственный процесс живёт доли секунды: без `wait` он остался бы
/// зомби до конца жизни окна. Поток-пожиратель снимает его через `wait`.
fn spawn(program: &str, args: &[&str], lang: Language) -> Result<(), String> {
    let child = Command::new("setsid")
        .arg("-f")
        .arg(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
        Err(error) => Err(match lang {
            Language::Ru => format!("не запустить {program}: {error}"),
            Language::En => format!("failed to launch {program}: {error}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn argv_of(exec: &str) -> Vec<String> {
        argv(exec, "")
    }

    #[test]
    fn a_plain_command_splits_by_whitespace() {
        assert_eq!(argv_of("firefox"), vec!["firefox"]);
        assert_eq!(
            argv_of("env FOO=1 steam -silent"),
            vec!["env", "FOO=1", "steam", "-silent"]
        );
    }

    /// Кавычки — часть формата `Exec`, а не оболочки: имя с пробелом должно
    /// остаться одним аргументом.
    #[test]
    fn quotes_keep_an_argument_whole() {
        assert_eq!(
            argv_of(r#""C:\Program Files\app.exe" --flag"#),
            vec![
                r"C:\Program Files\app.exe".to_string(),
                "--flag".to_string()
            ]
        );
        assert_eq!(
            argv_of(r#"sh -c "echo привет""#),
            vec!["sh", "-c", "echo привет"]
        );
    }

    #[test]
    fn a_backslash_escapes_the_next_character() {
        assert_eq!(
            argv_of(r"app name\ with\ spaces"),
            vec!["app".to_string(), "name with spaces".to_string()]
        );
    }

    /// Пустые кавычки дают пустой аргумент, и он должен дойти: иначе команда
    /// вида `app "" --flag` потеряет позицию флага.
    #[test]
    fn an_empty_quoted_argument_survives() {
        assert_eq!(
            argv_of(r#"app "" --flag"#),
            vec!["app".to_string(), String::new(), "--flag".to_string()]
        );
    }

    /// Поля под файлы и URL отбрасываются, а не превращаются в пустые строки:
    /// лишний аргумент у приложения меняет поведение.
    #[test]
    fn file_and_url_fields_are_dropped() {
        assert_eq!(argv_of("gimp %U"), vec!["gimp"]);
        assert_eq!(argv_of("gimp %u %f"), vec!["gimp"]);
        assert_eq!(argv_of("gimp %F"), vec!["gimp"]);
    }

    /// Устаревшие поля выбрасываются молча: по спецификации их полагается
    /// игнорировать, а не показывать пользователю ошибку запуска.
    #[test]
    fn deprecated_fields_are_dropped() {
        assert_eq!(argv_of("app %d %D %n %N %v %m"), vec!["app"]);
    }

    #[test]
    fn the_icon_field_expands_into_two_arguments() {
        let words = argv("app %i", "firefox");
        assert_eq!(words, vec!["app", "--icon", "firefox"]);
    }

    /// Иконка-путь в `%i` не годится: `--icon` ждёт имя из темы, а абсолютный
    /// путь приложение не найдёт. Лучше без иконки, чем с битой.
    #[test]
    fn an_icon_path_is_not_passed_as_an_icon_name() {
        assert_eq!(argv("app %i", "/tmp/icon.png"), vec!["app"]);
        assert_eq!(argv("app %i", ""), vec!["app"]);
    }

    /// Экранированный процент — это данные, а не поле: `50%` в имени файла не
    /// должен исчезнуть.
    #[test]
    fn an_escaped_percent_is_not_a_field() {
        assert_eq!(argv_of(r"app 100\%"), vec!["app", "100%"]);
    }

    #[test]
    fn an_empty_exec_yields_nothing() {
        assert!(argv_of("").is_empty());
        assert!(argv_of("   ").is_empty());
    }

    #[test]
    fn a_lone_percent_is_not_a_field() {
        assert_eq!(argv_of("app %"), vec!["app", "%"]);
    }

    /// Запуск без команды — ошибка, а не тихий успех: в списке такое приложение
    /// можно встретить только с битым `Exec`.
    #[test]
    fn launching_an_app_without_exec_fails() {
        let app = App {
            id: "broken".into(),
            name: "Broken".into(),
            name_plain: "Broken".into(),
            generic: String::new(),
            keywords: Vec::new(),
            exec: String::new(),
            icon: String::new(),
            terminal: false,
            dbus_activatable: false,
            path: PathBuf::from("broken.desktop"),
        };
        assert!(launch(&app, Language::Ru).is_err());
    }

    /// Реальный запуск на машине тестов: команда безобидная, и она доказывает,
    /// что путь `setsid` → `sh` → файл действительно работает и не оставляет
    /// зомби. Проверка файла вместо вывода: `setsid` отдаёт stdout тому, кого
    /// запустил, а не нам.
    #[test]
    fn a_real_command_actually_runs() {
        if !has_program("setsid") {
            return;
        }
        let marker = std::env::temp_dir().join(format!("hudbar-launch-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let script = format!("printf ok > {}", marker.display());
        spawn("sh", &["-c", &script], Language::Ru).expect("запуск");

        // Процесс отсоединён, поэтому ждать можно только по файлу и с потолком.
        let mut done = false;
        for _ in 0..200 {
            if marker.exists() {
                done = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = std::fs::remove_file(&marker);
        assert!(done, "отсоединённый процесс должен был отработать");
    }
}

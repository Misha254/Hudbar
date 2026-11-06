//! Список файлов обоев для окна настроек.
//!
//! Каталог берётся из того же места, что и в `~/.local/bin/wall.sh`
//! (`WALLPAPER_DIR="$HOME/wallpapers/"`). Если окно и скрипт смотрят в разные
//! папки, список в окне врёт: там не будет того файла, который скрипт готов
//! поставить. Обход рекурсивный, потому что обои лежат по подпапкам
//! (`~/wallpapers/anime/`, `~/wallpapers/pixelart/light/`), а сортировка по
//! пути держит порядок списка стабильным между запусками.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

// Модель выбора обоев. Файл подключается в несколько бинарников через
// `#[path]`, а Rust ищет такие подмодули рядом с самим файлом, а не в
// одноимённой папке — поэтому пути указываем явно.
#[path = "wallpaper/cache.rs"]
pub mod cache;
/// Скользящая рамка ячейки и появление миниатюр: состояние, без отрисовки.
#[path = "wallpaper/frame.rs"]
pub mod frame;
#[path = "wallpaper/grid.rs"]
pub mod grid;
#[path = "wallpaper/imginfo.rs"]
pub mod imginfo;
#[path = "wallpaper/input.rs"]
pub mod input;
#[path = "wallpaper/layout.rs"]
pub mod layout;
#[path = "wallpaper/scan.rs"]
pub mod scan;
#[path = "wallpaper/state.rs"]
pub mod state;
#[path = "wallpaper/swatches.rs"]
pub mod swatches;
#[path = "wallpaper/thumb.rs"]
pub mod thumb;
#[path = "wallpaper/view.rs"]
pub mod view;

// Модули `hud` живут по двум путям: в бинарниках это плоские `mod` у корня,
// в `hudbar` — вложенные в `hud`. Подмодули модели обращаются к ним через
// `super::`, поэтому реэкспорт повторяет то, что делает `menu/mod.rs`.
#[allow(unused_imports)]
pub(crate) use super::{
    bind_data, canvas, geom, log, palette, settings, settings_icons, text, ui_tokens,
};

/// Подписи окна выбора обоев. Лежат здесь, а не в `menu`: окно обоев
/// подключает `wallpaper.rs` без `menu`, а словарь должен быть один.
pub mod strings {
    /// Крошка-раздел окна обоев.
    pub const WALLPAPERS: &str = "Обои";
    pub const WALLPAPERS_EN: &str = "Wallpapers";
    /// Заголовок полосы схем.
    pub const SCHEME_MATUGEN: &str = "Схема matugen";
    pub const SCHEME_MATUGEN_EN: &str = "matugen scheme";
    /// Подсказки подвала: четыре позиции, применение, зона, выход.
    pub const HINT_ARROWS: &str = "↑↓←→ ВЫБОР";
    pub const HINT_ARROWS_EN: &str = "↑↓←→ SELECT";
    pub const HINT_APPLY: &str = "ENTER ПРИМЕНИТЬ";
    pub const HINT_APPLY_EN: &str = "ENTER APPLY";
    pub const HINT_ZONE: &str = "TAB ЗОНА";
    pub const HINT_ZONE_EN: &str = "TAB ZONE";
    pub const HINT_CLOSE: &str = "ESC ЗАКРЫТЬ";
    pub const HINT_CLOSE_EN: &str = "ESC CLOSE";
    /// Пустой результат.
    pub const NOTHING_FOUND: &str = "Ничего не найдено";
    pub const NOTHING_FOUND_EN: &str = "Nothing found";
    /// Поле поиска.
    pub const SEARCH_PLACEHOLDER: &str = "Поиск…";
    pub const SEARCH_PLACEHOLDER_EN: &str = "Search…";
    /// Корни крошек и разделитель между ними.
    pub const CRUMB_ROOT: &str = "HUD";
    pub const CRUMB: &str = "›";
    /// Бейдж текущих обоев.
    pub const BADGE_ACTIVE: &str = "ACTIVE";

    /// Подсказки подвала на языке окна.
    pub fn hints(ru: bool) -> [&'static str; 4] {
        if ru {
            [HINT_ARROWS, HINT_APPLY, HINT_ZONE, HINT_CLOSE]
        } else {
            [HINT_ARROWS_EN, HINT_APPLY_EN, HINT_ZONE_EN, HINT_CLOSE_EN]
        }
    }
}

/// Форматы, которые окно считает обоями. В `fd` внутри `wall.sh` есть ещё
/// `gif`, но окно показывает только четыре формата: анимация фоном не
/// становится, а листать её в списке бессмысленно.
pub const EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];

/// Каталог обоев: ровно то, что `WALLPAPER_DIR` в `wall.sh`.
pub fn root() -> PathBuf {
    home().join("wallpapers")
}

/// `HOME` с запасным вариантом: без него окно просто покажет пустой список.
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Подходит ли путь под обои. Регистр не важен: `PHOTO.JPG` из телефона
/// пропускать нельзя.
pub fn is_wallpaper(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXTENSIONS.iter().any(|ok| ext.eq_ignore_ascii_case(ok)))
}

/// Рекурсивный обход каталога: файлы из вложенных папок тоже в списке.
/// Результат отсортирован по пути. Нечитаемая папка, битый симлинк или
/// цикл ссылок не ломают обход — они просто пропускаются.
pub fn scan(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut seen: HashSet<PathBuf> = HashSet::new();
    while let Some(dir) = stack.pop() {
        let key = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if !seen.insert(key) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // Каталог определяем через metadata: симлинк на папку обходим так же,
            // как и настоящую папку, но `seen` не даёт уйти в цикл.
            match std::fs::metadata(&path) {
                Ok(meta) if meta.is_dir() => stack.push(path),
                Ok(_) if is_wallpaper(&path) => files.push(path),
                _ => {}
            }
        }
    }
    files.sort();
    files
}

/// Каталог для списка окна: тот же `~/wallpapers`, что у `wall.sh`.
pub fn list() -> Vec<String> {
    scan(&root())
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// Применяет обои через `wall.sh --set` и ждёт результат. Общий путь для
/// окна настроек и меню: оба вызывают одно и то же, а не держат каждый
/// свою копию запуска скрипта. `wall.sh` пишет причину в stderr — она и
/// возвращается, а не код выхода.
pub fn apply(file: &str, scheme: &str) -> Result<(), String> {
    let output = Command::new(home().join(".local/bin/wall.sh"))
        .arg("--set")
        .arg(file)
        .arg(scheme)
        .output()
        .map_err(|error| format!("wall.sh не запустился: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let reason = String::from_utf8_lossy(&output.stderr);
    let reason = reason
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("неизвестная ошибка")
        .to_string();
    Err(format!(
        "{reason} (код {})",
        output.status.code().unwrap_or(-1)
    ))
}

/// Применяет обои, не дожидаясь: для меню, которое не может висеть на
/// пересчёте matugen. Ошибка уходит в журнал, окно уже закрыто.
pub fn apply_detached(file: &str, scheme: &str) {
    let (file, scheme) = (file.to_string(), scheme.to_string());
    std::thread::spawn(move || {
        if let Err(error) = apply(&file, &scheme) {
            super::log::warn(format!("обои не применились: {error}"));
        }
    });
}

/// Применяет обои и, когда готова палитра, открывает окно выбора схемы.
///
/// Работа уходит в отдельную сессию, а не в поток этого окна. Окно обоев
/// закрывается сразу после Enter, и процесс умирал через миллисекунды, не
/// дождавшись конца `wall.sh`: поток уносил с собой и запуск окна схем —
/// обои менялись, а окно не появлялось.
///
/// `wall.sh` запускается в фоне, а не через `&&`: его хвост (`sleep 1.5`,
/// перезапуск портала, spicetify) занимал ещё секунды и не имел отношения к
/// схеме. Окно ждёт только новую палитру — по pid-файлу процесса, который её
/// считает.
///
/// Если смена сорвалась, окно не открывается: выбирать схему не для чего.
pub fn apply_then_open_schemes(file: &str, scheme: &str) {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        super::log::warn("схема не открылась: нет HOME");
        return;
    };
    let wall_sh = home.join(".local/bin/wall.sh");
    let schemes_bin = schemes_binary(std::env::var_os("HUDBAR_BIN_DIR"), &home);
    // Момент палитры на старте: окно ждёт, пока он изменится.
    let stamp = super::palette::modified()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_nanos().to_string())
        .unwrap_or_default();

    // Аргументы идут позиционно, а не склейкой в строку: путь к обоям может
    // содержать пробелы и кавычки, и shell не должен его разбирать.
    let script = r#""$1" --set "$2" "$3" >/dev/null 2>&1 &
export HUDBAR_APPLY_PID=$! HUDBAR_PALETTE_MTIME="$5"
exec "$4""#;
    let result = std::process::Command::new("setsid")
        .arg("-f")
        .arg("sh")
        .arg("-c")
        .arg(script)
        .arg("hud-wallpaper-apply")
        .arg(&wall_sh)
        .arg(file)
        .arg(scheme)
        .arg(&schemes_bin)
        .arg(&stamp)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if let Err(error) = result {
        super::log::warn(format!("смена не запущена: {error}"));
    }
}

/// Жив ли ещё процесс, который считает палитру. `None` — pid не передан.
///
/// Записи в `/proc` мало: `wall.sh` остаётся зомби-ребёнком окна схем и после
/// своей смерти. Состояние `Z` и значит «уже не работает».
pub fn applier_alive() -> Option<bool> {
    let pid = std::env::var("HUDBAR_APPLY_PID")
        .ok()?
        .parse::<u32>()
        .ok()?;
    Some(process_state(pid).is_some_and(|state| state != 'Z'))
}

/// Состояние процесса из `/proc/<pid>/stat`: `R`, `S`, `Z`… `None` — процесса
/// нет. Поле имени может содержать пробелы и скобки, поэтому состояние ищется
/// после последней закрывающей скобки.
fn process_state(pid: u32) -> Option<char> {
    let stat = std::fs::read_to_string(
        std::path::Path::new("/proc")
            .join(pid.to_string())
            .join("stat"),
    )
    .ok()?;
    stat.rsplit(')').next()?.trim_start().chars().next()
}

/// Где лежит окно схем: `HUDBAR_BIN_DIR`, затем `~/.local/bin`, затем `PATH`.
///
/// Правило вынесено сюда целиком, а не двумя строками на месте запуска: его
/// проверяет тест, а окно схем ставится и пакетом в `/usr/bin` — тогда
/// переменную окружения задавать не нужно, и имя ищется в `PATH`.
fn schemes_binary(
    configured: Option<std::ffi::OsString>,
    home: &std::path::Path,
) -> std::ffi::OsString {
    let directory = configured
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".local/bin"));
    let candidate = directory.join("hud-schemes-rs");
    if candidate.is_file() {
        return candidate.into_os_string();
    }
    std::ffi::OsString::from("hud-schemes-rs")
}

#[cfg(test)]
mod tests {
    use super::{process_state, schemes_binary};

    #[test]
    fn schemes_binary_prefers_the_configured_directory_and_falls_back_to_path() {
        let directory = std::env::temp_dir().join(format!("hudbar-schemes-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("hud-schemes-rs"), b"").unwrap();

        assert_eq!(
            schemes_binary(
                Some(directory.clone().into_os_string()),
                std::path::Path::new("/nonexistent")
            ),
            directory.join("hud-schemes-rs").into_os_string(),
            "настроенный каталог важнее PATH"
        );
        assert_eq!(
            schemes_binary(None, std::path::Path::new("/nonexistent")),
            std::ffi::OsString::from("hud-schemes-rs"),
            "нет файла рядом с настройками — имя ищется в PATH"
        );

        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// Зомби — уже не работа: запись в `/proc` живёт, пока процесс не пожнут,
    /// и ожидание по одному её наличию не кончалось бы никогда.
    #[test]
    fn a_zombie_is_not_a_running_applier() {
        assert_ne!(process_state(std::process::id()), Some('Z'), "текущий жив");

        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("exit 0")
            .spawn()
            .expect("процесс");
        let pid = child.id();
        let mut state = None;
        for _ in 0..200 {
            state = process_state(pid);
            if state == Some('Z') {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(state, Some('Z'), "умерший ребёнок ждёт жатвы");

        child.wait().expect("дождаться");
        assert_eq!(
            process_state(pid),
            None,
            "пожатый процесс исчезает из /proc"
        );
    }
}

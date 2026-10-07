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
    bind_data, log, palette, settings, settings_icons, settings_ui, settings_view, text, ui_tokens,
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

/// Применяет обои и, когда смена закончилась, открывает окно выбора схемы.
///
/// Работа уходит в отдельную сессию, а не в поток этого окна. Окно обоев
/// закрывается сразу после Enter, и процесс умирал через миллисекунды, не
/// дождавшись конца `wall.sh`: поток уносил с собой и запуск окна схем —
/// обои менялись, а окно не появлялось.
///
/// Если смена сорвалась, окно не открывается: выбирать схему не для чего.
pub fn apply_then_open_schemes(file: &str, scheme: &str) {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        super::log::warn("схема не открылась: нет HOME");
        return;
    };
    let wall_sh = home.join(".local/bin/wall.sh");
    let schemes_bin = home.join(".local/bin/hud-schemes-rs");

    // Аргументы идут позиционно, а не склейкой в строку: путь к обоям может
    // содержать пробелы и кавычки, и shell не должен его разбирать.
    let script = r#""$1" --set "$2" "$3" && exec "$4""#;
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
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if let Err(error) = result {
        super::log::warn(format!("смена не запущена: {error}"));
    }
}

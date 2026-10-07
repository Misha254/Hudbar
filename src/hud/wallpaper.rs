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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hudbar-wallpaper-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"x").unwrap();
    }

    /// Вложенные папки — обычное дело для `~/wallpapers`, а не крайний случай:
    /// список обязан их видеть.
    #[test]
    fn scan_walks_nested_folders() {
        let root = temp_dir("nested");
        touch(&root.join("top.jpg"));
        touch(&root.join("anime/one.png"));
        touch(&root.join("pixelart/light/deep/three.webp"));
        touch(&root.join("nature/four.jpeg"));

        let found: Vec<String> = scan(&root)
            .iter()
            .map(|path| {
                path.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();

        assert_eq!(
            found,
            vec![
                "anime/one.png",
                "nature/four.jpeg",
                "pixelart/light/deep/three.webp",
                "top.jpg"
            ],
            "вложенные файлы пропущены или порядок не по пути"
        );
    }

    /// Расширение решает всё: мусор вроде `.md` и `.gif` в список не попадает.
    #[test]
    fn scan_keeps_only_the_four_wallpaper_extensions() {
        let root = temp_dir("ext");
        for name in [
            "a.jpg",
            "b.JPEG",
            "c.png",
            "d.WebP",
            "skip.gif",
            "skip.bmp",
            "skip.md",
            "skip.txt",
            "noextension",
            ".hidden.jpg.bak",
        ] {
            touch(&root.join(name));
        }

        let found: Vec<String> = scan(&root)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert_eq!(found, vec!["a.jpg", "b.JPEG", "c.png", "d.WebP"]);
    }

    /// Пустой каталог и отсутствующий каталог — не ошибка, а пустой список:
    /// в разделе «Обои» показывается заглушка, а окно не падает.
    #[test]
    fn scan_of_empty_or_missing_root_is_empty() {
        let root = temp_dir("empty");
        assert!(scan(&root).is_empty(), "пустой каталог дал файлы");

        let missing = root.join("does-not-exist");
        assert!(scan(&missing).is_empty(), "нет каталога — есть файлы");
    }

    /// Сортировка по пути: список не должен прыгать между запусками окна.
    #[test]
    fn scan_sorts_by_path() {
        let root = temp_dir("sort");
        for name in ["z.jpg", "a/2.jpg", "a/1.jpg", "b/10.jpg", "b/2.jpg"] {
            touch(&root.join(name));
        }
        let found = scan(&root);
        let mut sorted = found.clone();
        sorted.sort();
        assert_eq!(found, sorted);
    }

    /// Ссылка на родителя не должна уводить обход в бесконечный цикл.
    #[cfg(unix)]
    #[test]
    fn scan_survives_a_symlink_loop() {
        let root = temp_dir("loop");
        touch(&root.join("real/one.jpg"));
        std::os::unix::fs::symlink(&root, root.join("real/loop")).unwrap();

        let found = scan(&root);
        assert_eq!(found.len(), 1, "цикл ссылок не ограничен");
        assert!(found[0].ends_with("one.jpg"));
    }

    /// Каталог окна и каталог `wall.sh` должны совпадать. Проверка читает
    /// живой скрипт, когда он есть: расхождение означало бы, что в окне
    /// показывают файлы, которых скрипт не применит.
    #[test]
    fn wallpaper_dir_matches_wall_script() {
        let script = home().join(".local/bin/wall.sh");
        let Ok(text) = std::fs::read_to_string(&script) else {
            return;
        };
        assert!(
            text.contains("WALLPAPER_DIR=\"$HOME/wallpapers/\""),
            "wall.sh сменил каталог: список окна придётся переписать"
        );
        assert_eq!(
            root(),
            home().join("wallpapers"),
            "каталог окна должен быть ~/wallpapers"
        );
    }
}

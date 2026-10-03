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

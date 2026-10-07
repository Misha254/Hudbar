//! Модель «папки обоев» поверх `wallpaper::scan`: верхний обход по подпапкам,
//! файлы рекурсивно, но сетка группируется по верхней папке.

use std::path::{Path, PathBuf};

/// Корневой узел «все».
pub const ALL: &str = "all";

/// Одна верхняя папка с вложенными файлами (включая вложенные папки).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// Имя верхней папки в `~/wallpapers`.
    pub name: String,
    /// Полный путь к ней.
    pub path: PathBuf,
    /// Количество обоев (jpg/jpeg/png/webp) рекурсивно.
    pub count: usize,
    /// Полный список файлов внутри (пусть и целых, независимо от фильтра).
    pub files: Vec<PathBuf>,
}

/// Итог сканирования корня.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// Верхняя папка «all»: список всех файлов обоев.
    pub all: Vec<PathBuf>,
    /// Список папок (только верхние подпапки, в стабильном порядке).
    pub folders: Vec<Folder>,
    /// Всего обоев во всех папках.
    pub total: usize,
}

/// Сканирует корень: для каждой верхней подпапки считает её рекурсивную
/// совокупность обоев; `wallpaper::is_wallpaper` фильтрует расширения;
/// файл докладывает о папке, в которой он лежит росым подпапкой пибо.
pub fn scan(root: &Path) -> Scan {
    let mut folders: Vec<Folder> = Vec::new();
    let mut all: Vec<PathBuf> = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return Scan::default();
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_dir() {
            // Файл прямо в корне обоев попадает только в «all»: своей папки
            // у него нет, и в списке папок он был бы неуместен.
            if super::is_wallpaper(&path) {
                all.push(path);
            }
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let files = super::scan(&path);
        let count = files.len();
        all.extend(files.iter().cloned());
        folders.push(Folder {
            name: name.to_string(),
            path,
            count,
            files,
        });
    }

    folders.sort_by(|left, right| left.name.cmp(&right.name));
    all.sort();
    all.dedup();
    let total = all.len();

    Scan {
        all,
        folders,
        total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_file(dir: &std::path::Path, rel: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap_or(dir)).unwrap();
        std::fs::write(&path, b"x").unwrap();
    }

    /// Структура копирует реальные данные: вложенный `pixelart/light` входит в
    /// `pixelart`; README игнорируется.
    #[test]
    fn scan_groups_nested_folders_under_the_top_one() {
        let dir = std::env::temp_dir().join(format!("hudbar-wp-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        mk_file(&dir, "anime/1024x576.png");
        mk_file(&dir, "anime/old.jpeg");
        mk_file(&dir, "pixelart/tile.png");
        mk_file(&dir, "pixelart/light/lamp.webp");
        mk_file(&dir, "pixelart/README.md");
        mk_file(&dir, "misc/a.JPG");
        mk_file(&dir, "root_file.png"); // в самом корне: не из подпапки

        let result = scan(dir.as_path());
        let names: Vec<_> = result.folders.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["anime", "misc", "pixelart"],
            "стабильный порядок по имени каталога"
        );

        let anime = result.folders.iter().find(|f| f.name == "anime").unwrap();
        assert_eq!(anime.count, 2, "jpg/jpeg попадают в счётчик");
        let pixelart = result
            .folders
            .iter()
            .find(|f| f.name == "pixelart")
            .unwrap();
        assert_eq!(
            pixelart.count, 2,
            "вложенный light считается внутри pixelart"
        );
        assert!(
            result
                .all
                .iter()
                .any(|path| path.ends_with("pixelart/light/lamp.webp"))
        );
        assert!(!result.all.iter().any(|path| path.ends_with("README.md")));
        assert!(
            result
                .all
                .iter()
                .any(|path| path.ends_with("root_file.png"))
        );
        assert!(
            result.all.iter().any(|path| path
                .to_string_lossy()
                .to_lowercase()
                .ends_with("misc/a.jpg")),
            "регистр расширения не важен"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

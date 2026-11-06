//! Фон экрана блокировки: PNG-кэш вместо требования к формату обоев.
//!
//! Замок рисует фон только из PNG: у него нет декодера JPEG, и тащить его в
//! бинарь, который обязан подняться за доли секунды, незачем. Поэтому не-PNG
//! обои один раз конвертируются в `~/.cache/hudbar/background.png` тем же
//! способом, что и у экрана входа (`greeter::image::to_png`).
//!
//! Конвертация идёт в отдельном потоке: экран блокировки должен появиться
//! сразу, даже если `vipsthumbnail` думает над двадцатимегапиксельным снимком.
//! Пока новый кэш готовится, показывается прошлый — он лучше сплошного цвета.
//! Готовый файл подменяется через `rename`, поэтому наполовину записанный PNG
//! никогда не станет фоном.
//!
//! Кэш привязан к источнику: рядом лежит `background.src` с путём обоев. Если
//! в `~/.config/wallpaper` записали другой файл, кэш пересчитывается, даже
//! когда у нового файла старее время правки.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::greeter::image::{self, Background};

/// Обои и место их PNG-кэша.
#[derive(Clone, Debug)]
pub struct Wallpaper {
    source: PathBuf,
    cache: PathBuf,
    marker: PathBuf,
}

/// Обои из `~/.config/wallpaper`. `None` — файла нет или путь пустой.
pub fn wallpaper() -> Option<Wallpaper> {
    let home = std::env::var_os("HOME")?;
    let text = std::fs::read_to_string(PathBuf::from(&home).join(".config/wallpaper")).ok()?;
    let source = PathBuf::from(text.trim());
    if source.as_os_str().is_empty() {
        return None;
    }
    let dir = cache_dir(
        Path::new(&home),
        std::env::var_os("XDG_CACHE_HOME").as_deref(),
    );
    Some(Wallpaper::at(source, &dir))
}

/// Каталог кэша: `$XDG_CACHE_HOME/hudbar` или `~/.cache/hudbar`.
fn cache_dir(home: &Path, xdg: Option<&OsStr>) -> PathBuf {
    xdg.map(PathBuf::from)
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| home.join(".cache"))
        .join("hudbar")
}

impl Wallpaper {
    fn at(source: PathBuf, dir: &Path) -> Self {
        Self {
            source,
            cache: dir.join("background.png"),
            marker: dir.join("background.src"),
        }
    }

    /// Фон для первого кадра.
    ///
    /// PNG читается напрямую. Для остальных форматов берётся кэш: сначала
    /// свежий, затем любой уцелевший — старые обои лучше сплошной заливки,
    /// пока новый файл готовится.
    pub fn load(&self) -> Option<Background> {
        if image::is_png(&self.source) {
            return Background::load(&self.source);
        }
        Background::load(&self.cache)
    }

    /// Нужна ли конвертация: источник не PNG и кэш не соответствует ему.
    pub fn needs_conversion(&self) -> bool {
        !image::is_png(&self.source) && !self.cache_fresh()
    }

    /// Кэш описывает ровно текущий источник и не старше него.
    fn cache_fresh(&self) -> bool {
        let Ok(cached) = std::fs::metadata(&self.cache) else {
            return false;
        };
        if cached.len() == 0 {
            return false;
        }
        let Ok(marker) = std::fs::read_to_string(&self.marker) else {
            return false;
        };
        let Ok(source) = std::fs::metadata(&self.source) else {
            return false;
        };
        marker_matches(&marker, &self.source)
            && fresh(cached.modified().ok(), source.modified().ok())
    }

    /// Конвертирует обои в PNG-кэш. Блокирующая: вызывать из отдельного
    /// потока, а не из цикла отрисовки.
    pub fn convert(&self) -> Result<(), String> {
        let dir = self
            .cache
            .parent()
            .ok_or_else(|| format!("у {} нет каталога", self.cache.display()))?;
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("не создать {}: {error}", dir.display()))?;
        // Формат `vipsthumbnail` и `magick` берут из расширения, поэтому
        // временный файл обязан оканчиваться на `.png`.
        let temp = dir.join(format!("background.tmp.{}.png", std::process::id()));
        if let Err(error) = image::to_png(&self.source, &temp, image::MAX_WIDTH) {
            let _ = std::fs::remove_file(&temp);
            return Err(error);
        }
        if let Err(error) = std::fs::rename(&temp, &self.cache) {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("не заменить {}: {error}", self.cache.display()));
        }
        std::fs::write(&self.marker, self.source.to_string_lossy().as_bytes())
            .map_err(|error| format!("не записать {}: {error}", self.marker.display()))
    }
}

/// Маркер относится к тому же файлу обоев. Путь сравнивается как записан в
/// `~/.config/wallpaper`: символические ссылки не разворачиваются, иначе
/// подмена ссылки на другой файл не заметила бы смену обоев.
fn marker_matches(marker: &str, source: &Path) -> bool {
    marker.trim() == source.to_string_lossy()
}

/// Кэш не старше источника. Равное время — не повод пересчитывать: `rename`
/// мог сохранить секундную точность, и лишняя конвертация на каждом запуске
/// была бы хуже.
fn fresh(cached: Option<SystemTime>, source: Option<SystemTime>) -> bool {
    match (cached, source) {
        (Some(cached), Some(source)) => cached >= source,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn cache_lives_in_xdg_cache_when_it_is_set() {
        let home = Path::new("/home/user");
        assert_eq!(
            cache_dir(home, Some(OsStr::new("/var/cache/user"))),
            Path::new("/var/cache/user/hudbar")
        );
        assert_eq!(
            cache_dir(home, None),
            Path::new("/home/user/.cache/hudbar"),
            "без XDG — домашний кэш"
        );
        assert_eq!(
            cache_dir(home, Some(OsStr::new(""))),
            Path::new("/home/user/.cache/hudbar"),
            "пустой XDG не должен давать относительный путь"
        );
    }

    #[test]
    fn marker_identifies_the_wallpaper_file() {
        let source = Path::new("/home/user/walls/a.jpg");
        assert!(marker_matches("/home/user/walls/a.jpg\n", source));
        assert!(!marker_matches("/home/user/walls/b.jpg", source));
        assert!(!marker_matches("", source));
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hudbar-lock-background-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        dir
    }

    #[test]
    fn conversion_writes_the_cache_and_its_marker() {
        let dir = temp("convert");
        let source = dir.join("wall.png");
        tiny_skia::Pixmap::new(40, 20)
            .expect("буфер")
            .save_png(&source)
            .expect("png");
        let wall = Wallpaper::at(source.clone(), &dir.join("cache"));

        wall.convert().expect("конвертация");

        assert!(wall.cache.exists(), "кэш записан");
        assert!(wall.cache_fresh(), "маркер указывает на источник");
        assert!(
            std::fs::read_to_string(&wall.marker)
                .expect("маркер")
                .contains("wall.png"),
            "в маркере путь обоев"
        );
        assert!(wall.load().is_some(), "кэш читается как фон");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_must_not_be_older_than_the_wallpaper() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let older = now - Duration::from_secs(10);
        let newer = now + Duration::from_secs(10);

        assert!(fresh(Some(now), Some(now)), "равное время — свежий кэш");
        assert!(fresh(Some(newer), Some(now)));
        assert!(!fresh(Some(older), Some(now)));
        assert!(!fresh(None, Some(now)), "без времени кэш не доверяем");
        assert!(!fresh(Some(now), None), "источника нет — пересчитать");
    }
}

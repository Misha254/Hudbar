//! `--sync`: переносит тему обычного пользователя в каталог данных гритера.
//!
//! Гритер работает от `greeter` и не видит `$HOME` вошедшего пользователя,
//! поэтому палитру, обои и язык ему нужно положить в свой каталог. Делает это
//! сам пользователь — из своей сессии, без sudo: писать в
//! `/var/lib/hudbar-greeter` ему разрешает установщик, а `sudo` здесь был бы
//! способом получить root-файлы с чужими правами.
//!
//! Запись атомарная: временный файл рядом с целевым и `rename`. Гритер может
//! читать каталог в любой момент, и половина `greeter.json` для него — битый
//! JSON, то есть сброс на русский язык посреди входа.
//!
//! Автоматического вызова из смены обоев нет: это отдельное решение, и в
//! `docs/greeter.md` описан ручной хук.

use std::path::{Path, PathBuf};

use super::config::{self, Config};
use super::image;
use super::settings::Language;

/// Что удалось синхронизировать. Печатается в stdout по строке на пункт.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub dir: PathBuf,
    /// Скопирован ли `colors.css`.
    pub colors: bool,
    /// Чем конвертировали обои: `png`, `vipsthumbnail`, `magick` или `None`,
    /// если обоев нет.
    pub background: Option<&'static str>,
    pub language: Language,
}

/// Дом пользователя из окружения. Отдельно от `sync`, чтобы тест не трогал
/// настоящий `$HOME`.
pub fn home_from_env() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "HOME не задан: не знаю, откуда брать тему".to_string())
}

/// Синхронизация. `home` — дом обычного пользователя, `dir` — каталог данных
/// гритера.
pub fn sync(dir: &Path, home: &Path) -> Result<Report, String> {
    let colors_src = home.join(".config/hudbar/colors.css");
    let settings_src = home.join(".config/hudbar/settings.json");
    let wallpaper_src = home.join(".config/wallpaper");

    // Каталог создаётся до первой записи: иначе ошибка прав выглядела бы как
    // «не создался временный файл» без указания, куда именно.
    std::fs::create_dir_all(dir).map_err(|error| access_error(dir, &error))?;

    let colors = if colors_src.is_file() {
        let bytes = std::fs::read(&colors_src)
            .map_err(|error| format!("не прочитать {}: {error}", colors_src.display()))?;
        write_atomic(&config::colors_path(dir), &bytes)?;
        true
    } else {
        false
    };

    // Язык берётся из settings.json; нет файла — русский, как и у панели.
    let language = read_language(&settings_src);
    let mut current = config::load(dir);
    current.language = language;
    write_config(dir, &current)?;

    let background = sync_background(dir, &wallpaper_src)?;

    Ok(Report {
        dir: dir.to_path_buf(),
        colors,
        background,
        language,
    })
}

/// Язык из `settings.json`. Разбор повторяет правило `settings::load`:
/// неизвестное значение — русский.
fn read_language(path: &Path) -> Language {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Language::Ru;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Language::Ru;
    };
    value
        .get("language")
        .and_then(serde_json::Value::as_str)
        .map(Language::from_key)
        .unwrap_or(Language::Ru)
}

/// `greeter.json` целиком, с сохранением `dim` и `font` из прошлой
/// синхронизации: язык — не повод сбрасывать настроенный вид.
fn write_config(dir: &Path, config: &Config) -> Result<(), String> {
    let payload = serde_json::json!({
        "language": config.language.key(),
        "dim": config.dim,
        "card_plate": config.card_plate,
        "font": config.font,
    });
    let text = serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("greeter.json не собрался: {error}"))?;
    write_atomic(&config::config_path(dir), text.as_bytes())
}

/// Текущие обои из `~/.config/wallpaper`: путь пишет `wall.sh` после успешной
/// смены. Нет файла или он пуст — синхронизация проходит без фона.
fn sync_background(dir: &Path, source_list: &Path) -> Result<Option<&'static str>, String> {
    let Ok(text) = std::fs::read_to_string(source_list) else {
        return Ok(None);
    };
    let path = text.trim();
    if path.is_empty() {
        return Ok(None);
    }
    let path = Path::new(path);
    if !path.is_file() {
        return Ok(None);
    }
    let target = config::background_path(dir);
    let tmp = temp_path(&target);
    let tool = match image::to_png(path, &tmp, image::MAX_WIDTH) {
        Ok(tool) => tool,
        Err(error) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(error);
        }
    };
    if let Err(error) = std::fs::rename(&tmp, &target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("не переименовать {}: {error}", target.display()));
    }
    Ok(Some(tool))
}

/// Атомарная запись: временный файл рядом (rename в пределах одной ФС) и
/// переименование поверх цели.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| access_error(parent, &error))?;
    }
    let tmp = temp_path(path);
    std::fs::write(&tmp, bytes).map_err(|error| access_error(&tmp, &error))?;
    if let Err(error) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("не переименовать {}: {error}", path.display()));
    }
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "greeter".to_string());
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

/// Ошибка доступа с человеческим объяснением: без sudo и без «попробуйте
/// ещё раз».
fn access_error(path: &Path, error: &std::io::Error) -> String {
    format!(
        "нет прав на запись в {}: {error}. \
         Запусти `hud-greeter-rs --sync` от своего пользователя — sudo не нужен",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Pixmap;

    fn tree(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("hudbar-greeter-sync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let data = root.join("data");
        std::fs::create_dir_all(home.join(".config/hudbar")).expect("каталог");
        (root, home, data)
    }

    #[test]
    fn a_full_sync_copies_colors_wallpaper_and_language() {
        let (root, home, data) = tree("full");
        std::fs::write(
            home.join(".config/hudbar/colors.css"),
            "@define-color base #010203;\n",
        )
        .expect("палитра");
        std::fs::write(
            home.join(".config/hudbar/settings.json"),
            r#"{"language": "en"}"#,
        )
        .expect("настройки");

        let wallpaper = home.join("wallpapers/wide.png");
        std::fs::create_dir_all(wallpaper.parent().expect("папка")).expect("каталог");
        let mut image = Pixmap::new(400, 200).expect("буфер");
        image.fill(tiny_skia::Color::from_rgba8(1, 2, 3, 255));
        image.save_png(&wallpaper).expect("обои");
        std::fs::write(
            home.join(".config/wallpaper"),
            wallpaper.to_string_lossy().as_bytes(),
        )
        .expect("текущие обои");

        let report = sync(&data, &home).expect("синхронизация");
        assert!(report.colors);
        assert_eq!(report.background, Some("png"));
        assert_eq!(report.language, Language::En);

        let colors = std::fs::read_to_string(data.join("colors.css")).expect("colors");
        assert!(colors.contains("#010203"));
        let config = config::load(&data);
        assert_eq!(config.language, Language::En);
        let background = Pixmap::load_png(data.join("background.png")).expect("фон");
        assert_eq!(background.width(), 400);
        assert_eq!(background.height(), 200);

        // Временных файлов не осталось: в каталоге только три результата.
        let names: Vec<String> = std::fs::read_dir(&data)
            .expect("каталог")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 3, "{names:?}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Папка гритера может быть каталогом, а обоев/палитры у пользователя
    /// ещё нет: синхронизация не падает, язык — русский по умолчанию.
    #[test]
    fn an_empty_home_still_produces_a_config() {
        let (root, home, data) = tree("empty");
        let report = sync(&data, &home).expect("синхронизация");
        assert!(!report.colors);
        assert_eq!(report.background, None);
        assert_eq!(report.language, Language::Ru);
        assert!(config::config_path(&data).is_file());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Повторный запуск сохраняет `dim` из прошлого `greeter.json`: язык не
    /// должен сбрасывать настроенный вид.
    #[test]
    fn a_second_sync_keeps_the_configured_dim() {
        let (root, home, data) = tree("dim");
        std::fs::create_dir_all(&data).expect("каталог");
        std::fs::write(
            config::config_path(&data),
            r#"{"language": "ru", "dim": 0.7, "card_plate": 0.4, "font": "Fira Code"}"#,
        )
        .expect("конфиг");

        sync(&data, &home).expect("синхронизация");
        let config = config::load(&data);
        assert!((config.dim - 0.7).abs() < 1e-6, "{}", config.dim);
        assert!(
            (config.card_plate - 0.4).abs() < 1e-6,
            "{}",
            config.card_plate
        );
        assert_eq!(config.font, "Fira Code");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Каталог недоступен для записи: ошибка с путём и советом, без паники.
    /// Родитель — обычный файл, поэтому каталог не создаётся независимо от
    /// того, под кем идут тесты.
    #[test]
    fn an_unwritable_directory_is_a_clear_error() {
        let (root, home, _data) = tree("denied");
        std::fs::create_dir_all(&root).expect("корень");
        let blocker = root.join("blocker");
        std::fs::write(&blocker, b"x").expect("файл");
        let dir = blocker.join("data");

        let error = sync(&dir, &home).expect_err("запись невозможна");
        assert!(error.contains("нет прав"), "{error}");
        assert!(error.contains("--sync"), "{error}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// `write_atomic` заменяет файл целиком, а не дописывает: повторный вызов
    /// с коротким содержимым не оставляет хвост прошлого.
    #[test]
    fn atomic_write_replaces_the_whole_file() {
        let (root, _, data) = tree("atomic");
        let path = data.join("greeter.json");
        write_atomic(&path, b"{\"language\":\"en\"}").expect("первая запись");
        write_atomic(&path, b"{}").expect("вторая запись");
        assert_eq!(std::fs::read_to_string(&path).expect("чтение"), "{}");

        let _ = std::fs::remove_dir_all(&root);
    }
}

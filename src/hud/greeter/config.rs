//! Данные гритера: каталог, палитра, фон и настройки вида.
//!
//! Гритер работает от пользователя `greeter`, у которого нет нашего `$HOME` и
//! нет `~/.config/hudbar`. Поэтому всё, что панель ищет в домашнем каталоге,
//! здесь лежит в одном каталоге данных (по умолчанию `/var/lib/hudbar-greeter`)
//! и читается по путям от него:
//!
//! * `colors.css` — палитра matugen, раскладывается `hud-greeter-rs --sync`;
//! * `background.png` — текущие обои, уменьшенные до ≤1920 по ширине;
//! * `greeter.json` — язык, затемнение и имя шрифта.
//!
//! Нет файла — значение по умолчанию: экран входа обязан появиться на чистой
//! машине, даже если синхронизацию ни разу не запускали.

use std::path::{Path, PathBuf};

use super::settings::Language;

/// Каталог данных по умолчанию. Переопределяется `--data` и
/// `HUDBAR_GREETER_DATA` — иначе тесты и `--preview` писали бы в системный.
pub const DEFAULT_DATA_DIR: &str = "/var/lib/hudbar-greeter";

/// Затемнение фона по умолчанию: картинка должна читаться, но не спорить с
/// карточкой.
pub const DIM_DEFAULT: f32 = 0.45;

/// Предел затемнения: полностью чёрный фон — это уже не обои.
pub const DIM_MAX: f32 = 0.9;

/// Настройки вида.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub language: Language,
    /// Доля затемнения поверх картинки, 0..=`DIM_MAX`.
    pub dim: f32,
    /// Шрифт интерфейса. Пустое имя — шрифт по умолчанию из `ui_tokens`.
    pub font: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: Language::Ru,
            dim: DIM_DEFAULT,
            font: super::ui_tokens::font_name(),
        }
    }
}

/// Каталог данных из окружения.
pub fn data_dir() -> PathBuf {
    std::env::var_os("HUDBAR_GREETER_DATA")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR))
}

pub fn colors_path(dir: &Path) -> PathBuf {
    dir.join("colors.css")
}

pub fn background_path(dir: &Path) -> PathBuf {
    dir.join("background.png")
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("greeter.json")
}

/// Настройки из каталога: нет файла или он битый — значения по умолчанию.
pub fn load(dir: &Path) -> Config {
    let Ok(text) = std::fs::read_to_string(config_path(dir)) else {
        return Config::default();
    };
    parse(&text)
}

/// Разбор `greeter.json` без диска.
pub fn parse(text: &str) -> Config {
    let mut config = Config::default();
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str(text) else {
        return config;
    };
    if let Some(language) = map.get("language").and_then(serde_json::Value::as_str) {
        config.language = Language::from_key(language);
    }
    if let Some(dim) = map.get("dim").and_then(serde_json::Value::as_f64) {
        config.dim = (dim as f32).clamp(0.0, DIM_MAX);
    }
    if let Some(font) = map.get("font").and_then(serde_json::Value::as_str) {
        let font = font.trim();
        if !font.is_empty() {
            config.font = font.to_string();
        }
    }
    config
}

/// Палитра из каталога данных. Нет файла — палитра по умолчанию.
pub fn palette_of(dir: &Path) -> super::palette::Palette {
    super::palette::load_from(&colors_path(dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_readable_without_files() {
        let dir = std::env::temp_dir().join(format!(
            "hudbar-greeter-config-empty-{}",
            std::process::id()
        ));
        let config = load(&dir);
        assert_eq!(config.language, Language::Ru);
        assert!((config.dim - DIM_DEFAULT).abs() < 1e-6);
        assert!(!config.font.is_empty());
    }

    #[test]
    fn the_file_sets_language_dim_and_font() {
        let config = parse(r#"{"language": "en", "dim": 0.7, "font": "Fira Code"}"#);
        assert_eq!(config.language, Language::En);
        assert!((config.dim - 0.7).abs() < 1e-6);
        assert_eq!(config.font, "Fira Code");
    }

    /// Мусор и крайние значения не ломают экран: затемнение зажато, язык по
    /// правилу `settings.json` (неизвестный — русский).
    #[test]
    fn broken_values_fall_back() {
        assert_eq!(parse("{").language, Language::Ru);
        assert_eq!(parse("[]").language, Language::Ru);
        assert_eq!(parse(r#"{"language": "de"}"#).language, Language::Ru);
        assert!((parse(r#"{"dim": 5.0}"#).dim - DIM_MAX).abs() < 1e-6);
        assert!(parse(r#"{"dim": -1.0}"#).dim.abs() < 1e-6);
        assert_eq!(
            parse(r#"{"font": "   "}"#).font,
            super::super::ui_tokens::font_name(),
            "пробелы — это «шрифт по умолчанию»"
        );
    }
}

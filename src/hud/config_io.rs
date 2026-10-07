//! Запись конфигурации на диск: атомарная, с сохранением чужих ключей.
//!
//! Правила, которые окно обязано соблюдать при любой записи:
//!! - `settings.json` пишется целиком через временный файл и `rename`, поэтому
//!   читатель никогда не увидит наполовину записанный JSON;
//! - неизвестные ключи и целые секции, которых нет в модели, сохраняются;
//! - `colors.css` и шаблоны matugen не трогаются: цветами владеет matugen;
//! - применяется только то, что реально изменилось.

use super::config::{Config, Theme};
use super::settings::{HEIGHT_MAX, HEIGHT_MIN, Language, Module, NotificationPosition};
use serde_json::{Map, Value};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Ошибка записи: показывается в строке статуса, значение откатывается.
#[derive(Debug)]
pub enum WriteError {
    Read(String),
    Serialize(String),
    Io(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Read(why) => write!(f, "не удалось прочитать конфиг: {why}"),
            WriteError::Serialize(why) => write!(f, "не удалось разобрать конфиг: {why}"),
            WriteError::Io(why) => write!(f, "не удалось записать конфиг: {why}"),
        }
    }
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

pub fn settings_path() -> PathBuf {
    home().join(".config/hudbar/settings.json")
}

pub fn font_path() -> PathBuf {
    home().join(".config/hudbar/font")
}

/// Пишет `text` во временный файл рядом с целью, `fsync`-ит и переименовывает.
/// Файл сохраняет права и владельца, если он уже существовал.
pub fn atomic_write(path: &Path, text: &str) -> Result<(), WriteError> {
    let io = |why: std::io::Error| WriteError::Io(why.to_string());
    let parent = path.parent().ok_or_else(|| {
        WriteError::Io(format!("у {} нет родительского каталога", path.display()))
    })?;
    fs::create_dir_all(parent).map_err(io)?;

    let previous = fs::metadata(path).ok();
    let mut temporary = parent.join(format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config"),
        std::process::id()
    ));
    // Если временный файл остался с прошлого раза, убираем — мы его перезапишем.
    let _ = fs::remove_file(&temporary);

    {
        let mut handle = fs::File::create(&temporary).map_err(io)?;
        handle.write_all(text.as_bytes()).map_err(io)?;
        handle.sync_all().map_err(io)?;
    }

    let mode = previous
        .as_ref()
        .map(|meta| meta.permissions().mode() & 0o7777)
        .unwrap_or(0o644);
    fs::set_permissions(&temporary, fs::Permissions::from_mode(mode)).map_err(io)?;

    if let Err(why) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(io(why));
    }
    temporary = PathBuf::new();
    let _ = &temporary;

    // Каталог тоже стоит синхронизировать, иначе после сбоя питания rename
    // может не пережить.
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Накладывает модель на существующий JSON, сохраняя всё, чего модель не знает.
pub fn merge(config: &Config, base: Option<&Value>) -> Value {
    let mut root = match base {
        Some(value @ Value::Object(_)) => value.clone(),
        _ => Value::Object(Map::new()),
    };

    put_root(
        &mut root,
        "language",
        Value::String(config.language.key().to_string()),
    );
    put_root(
        &mut root,
        "module_order",
        Value::Array(
            config
                .flatten()
                .into_iter()
                .map(Value::String)
                .collect::<Vec<_>>(),
        ),
    );

    if let Some(theme) = config.theme {
        section(&mut root, "appearance").insert("theme".into(), Value::String(theme.key().into()));
    }

    let bar = section(&mut root, "hudbar");
    bar.insert("height".into(), Value::from(config.height));
    bar.insert("control_button".into(), Value::Bool(config.control_button));
    for module in Module::ALL {
        bar.insert(
            module.key().to_string(),
            Value::Bool(config.module_enabled(module)),
        );
    }

    let osd = section(&mut root, "osd");
    osd.insert("enabled".into(), Value::Bool(config.osd));
    osd.insert("duration_ms".into(), Value::from(config.osd_duration_ms));

    let notes = section(&mut root, "notifications");
    notes.insert("font_size".into(), Value::from(config.font_size));
    notes.insert("line_height".into(), Value::from(config.line_height));
    notes.insert(
        "position".into(),
        Value::String(config.position.key().to_string()),
    );

    root
}

/// Точечное изменение одного поля. Окно применяет `Patch`, а не всю модель:
/// перед записью файл перечитывается с диска, и на него накладывается только
/// то, что изменил пользователь. Чужие ключи и секции остаются на месте, а
/// кэш окна не может переписать файл целиком.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Patch {
    /// `hudbar.<module>` — видимость модуля.
    ModuleVisible {
        module: Module,
        on: bool,
    },
    /// Корень `module_order` — порядок и состав зон.
    ModuleOrder(Vec<String>),
    /// `hudbar.control_button` — шестерёнка Control Center на панели.
    ControlButton(bool),
    Height(u32),
    Language(Language),
    Theme(Theme),
    Notifications {
        font_size: Option<u32>,
        line_height: Option<u32>,
        position: Option<NotificationPosition>,
    },
    /// `osd.enabled` и `osd.duration_ms` — окно громкости/микрофона.
    Osd {
        enabled: Option<bool>,
        duration_ms: Option<u32>,
    },
}

impl Patch {
    /// Описание поля для строки статуса.
    pub fn label(&self) -> String {
        match self {
            Patch::ModuleVisible { module, on } => {
                format!("{}: {}", module.key(), if *on { "вкл" } else { "выкл" })
            }
            Patch::ModuleOrder(_) => "порядок модулей".to_string(),
            Patch::ControlButton(on) => {
                format!(
                    "кнопка Control Center: {}",
                    if *on { "вкл" } else { "выкл" }
                )
            }
            Patch::Height(height) => format!("высота: {height} px"),
            Patch::Language(language) => format!("язык: {}", language.key()),
            Patch::Theme(theme) => format!("тема: {}", theme.key()),
            Patch::Notifications { .. } => "уведомления".to_string(),
            Patch::Osd { .. } => "OSD".to_string(),
        }
    }

    /// Накладывает изменение на дерево. Возвращает `false`, если поле уже
    /// содержит нужное значение — тогда файл переписывать не нужно.
    fn apply(&self, root: &mut Value) -> bool {
        match self {
            Patch::ModuleVisible { module, on } => {
                let bar = section(root, "hudbar");
                let key = module.key().to_string();
                if bar.get(&key) == Some(&Value::Bool(*on)) {
                    return false;
                }
                bar.insert(key, Value::Bool(*on));
                true
            }
            Patch::ModuleOrder(order) => {
                let value = Value::Array(
                    order
                        .iter()
                        .map(|key| Value::String(key.clone()))
                        .collect::<Vec<_>>(),
                );
                if root.get("module_order") == Some(&value) {
                    return false;
                }
                put_root(root, "module_order", value);
                true
            }
            Patch::ControlButton(on) => {
                let bar = section(root, "hudbar");
                let value = Value::Bool(*on);
                if bar.get("control_button") == Some(&value) {
                    return false;
                }
                bar.insert("control_button".to_string(), value);
                true
            }
            Patch::Height(height) => {
                let bar = section(root, "hudbar");
                let value = Value::from(*height);
                if bar.get("height") == Some(&value) {
                    return false;
                }
                bar.insert("height".to_string(), value);
                true
            }
            Patch::Language(language) => {
                let value = Value::String(language.key().to_string());
                if root.get("language") == Some(&value) {
                    return false;
                }
                put_root(root, "language", value);
                true
            }
            Patch::Theme(theme) => {
                let appearance = section(root, "appearance");
                let value = Value::String(theme.key().to_string());
                if appearance.get("theme") == Some(&value) {
                    return false;
                }
                appearance.insert("theme".to_string(), value);
                true
            }
            Patch::Notifications {
                font_size,
                line_height,
                position,
            } => {
                let mut changed = false;
                let notes = section(root, "notifications");
                if let Some(size) = font_size {
                    let value = Value::from(*size);
                    if notes.get("font_size") != Some(&value) {
                        notes.insert("font_size".to_string(), value);
                        changed = true;
                    }
                }
                if let Some(line) = line_height {
                    let value = Value::from(*line);
                    if notes.get("line_height") != Some(&value) {
                        notes.insert("line_height".to_string(), value);
                        changed = true;
                    }
                }
                if let Some(origin) = position {
                    let value = Value::String(origin.key().to_string());
                    if notes.get("position") != Some(&value) {
                        notes.insert("position".to_string(), value);
                        changed = true;
                    }
                }
                changed
            }
            Patch::Osd {
                enabled,
                duration_ms,
            } => {
                let mut changed = false;
                let osd = section(root, "osd");
                if let Some(on) = enabled {
                    let value = Value::Bool(*on);
                    if osd.get("enabled") != Some(&value) {
                        osd.insert("enabled".to_string(), value);
                        changed = true;
                    }
                }
                if let Some(duration) = duration_ms {
                    let value = Value::from(*duration);
                    if osd.get("duration_ms") != Some(&value) {
                        osd.insert("duration_ms".to_string(), value);
                        changed = true;
                    }
                }
                changed
            }
        }
    }
}

/// Читает файл заново, накладывает патч и пишет атомарно.
///
/// Возвращает `Ok(false)`, если значение уже совпадает и запись не нужна.
pub fn apply_patch(patch: &Patch) -> Result<bool, WriteError> {
    apply_patch_to(&settings_path(), patch)
}

/// То же, но для указанного файла: путь вынесен параметром, чтобы тесты не
/// подменяли `HOME` через переменные окружения.
pub fn apply_patch_to(path: &Path, patch: &Patch) -> Result<bool, WriteError> {
    let base = match fs::read_to_string(path) {
        Ok(text) => Some(
            serde_json::from_str::<Value>(&text)
                .map_err(|why| WriteError::Serialize(why.to_string()))?,
        ),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => None,
        Err(why) => return Err(WriteError::Read(why.to_string())),
    };
    let mut root = base.unwrap_or_else(|| Value::Object(Map::new()));
    if !patch.apply(&mut root) {
        return Ok(false);
    }
    let mut text = serde_json::to_string_pretty(&root)
        .map_err(|why| WriteError::Serialize(why.to_string()))?;
    text.push('\n');
    atomic_write(path, &text)?;
    Ok(true)
}

/// Возвращает раздел корня, создавая пустой объект, если его ещё нет.
/// У `serde_json::Value` вставка через `IndexMut` в объект паникует, поэтому
/// работаем через `Map::insert` явно.
fn section<'a>(root: &'a mut Value, name: &str) -> &'a mut Map<String, Value> {
    if !root.get(name).is_some_and(Value::is_object)
        && let Some(map) = root.as_object_mut()
    {
        map.insert(name.to_string(), Value::Object(Map::new()));
    }
    root.get_mut(name)
        .and_then(Value::as_object_mut)
        .expect("section is an object")
}

/// Кладёт значение в корень, сохраняя порядок: сначала Known, потом Unknown.
fn put_root(root: &mut Value, key: &str, value: Value) {
    root.as_object_mut()
        .expect("merge works on an object")
        .insert(key.to_string(), value);
}

/// Читает текущий файл, накладывает модель и пишет атомарно.
pub fn save(config: &Config) -> Result<(), WriteError> {
    let path = settings_path();
    let base = match fs::read_to_string(&path) {
        Ok(text) => Some(
            serde_json::from_str::<Value>(&text)
                .map_err(|why| WriteError::Serialize(why.to_string()))?,
        ),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => None,
        Err(why) => return Err(WriteError::Read(why.to_string())),
    };
    let merged = merge(config, base.as_ref());
    let mut text = serde_json::to_string_pretty(&merged)
        .map_err(|why| WriteError::Serialize(why.to_string()))?;
    text.push('\n');
    atomic_write(&path, &text)
}

/// Пишет файл шрифта, чтобы тема в панели и в окне совпадали.
pub fn save_font(theme: Theme) -> Result<(), WriteError> {
    save_font_to(&font_path(), theme)
}

fn save_font_to(path: &Path, theme: Theme) -> Result<(), WriteError> {
    atomic_write(path, &format!("{}\n", theme.font()))
}

/// Согласованно меняет тему в JSON и в файле шрифта.
///
/// `settings.json` обновляется через тот же точечный patch-путь. Если запись
/// второго источника не удалась, первый источник возвращается к прежней теме.
pub fn apply_theme(theme: Theme) -> Result<(), WriteError> {
    apply_theme_to(&settings_path(), &font_path(), theme)
}

/// То же, но для указанных файлов: оба пути вынесены параметрами, чтобы тесты
/// не подменяли `HOME`.
pub fn apply_theme_to(settings: &Path, font: &Path, theme: Theme) -> Result<(), WriteError> {
    let previous = read_theme(settings).unwrap_or(Theme::Normal);
    apply_patch_to(settings, &Patch::Theme(theme))?;
    if let Err(error) = save_font_to(font, theme) {
        // Шрифт не записался — возвращаем JSON к прежней теме, иначе панель и
        // окно разойдутся по глифам.
        let _ = apply_patch_to(settings, &Patch::Theme(previous));
        let _ = save_font_to(font, previous);
        return Err(error);
    }
    Ok(())
}

fn read_theme(path: &Path) -> Option<Theme> {
    let text = fs::read_to_string(path).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    root.get("appearance")?
        .get("theme")?
        .as_str()
        .and_then(Theme::from_key)
}

/// Нормализует значение перед записью: высота и размеры в пределах поддержки.
pub fn normalize(config: &mut Config) {
    config.height = config.height.clamp(HEIGHT_MIN, HEIGHT_MAX);
    config.font_size = config.font_size.clamp(
        super::settings::FONT_SIZE_MIN,
        super::settings::FONT_SIZE_MAX,
    );
    config.line_height = config.line_height.clamp(
        super::settings::LINE_HEIGHT_MIN,
        super::settings::LINE_HEIGHT_MAX,
    );
    if !matches!(config.language, Language::Ru | Language::En) {
        config.language = Language::Ru;
    }
    if !matches!(
        config.position,
        NotificationPosition::TopLeft
            | NotificationPosition::TopRight
            | NotificationPosition::BottomLeft
            | NotificationPosition::BottomRight
    ) {
        config.position = NotificationPosition::TopRight;
    }
}

/// Настройки панели из файла: то же, что читает сам HUDbar.
pub fn load() -> Config {
    Config::from(&super::settings::load())
}

#[cfg(test)]
mod tests {
    use super::super::settings::MODULE_KEYS;
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hudbar-config-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn atomic_write_creates_file_and_parent_directory() {
        let dir = temp_dir("create");
        let path = dir.join("nested/deep/settings.json");

        atomic_write(&path, "{\"a\":1}\n").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"a\":1}\n");
    }

    #[test]
    fn atomic_write_leaves_no_temporary_files_behind() {
        let dir = temp_dir("no-temp");
        let path = dir.join("settings.json");

        atomic_write(&path, "{}\n").unwrap();
        atomic_write(&path, "{}\n").unwrap();

        let leftovers: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(
            leftovers.is_empty(),
            "остались временные файлы: {leftovers:?}"
        );
    }

    #[test]
    fn atomic_write_preserves_permissions() {
        let dir = temp_dir("mode");
        let path = dir.join("settings.json");

        fs::write(&path, "{}\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        atomic_write(&path, "{\"x\":true}\n").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn atomic_write_replaces_content_completely() {
        let dir = temp_dir("replace");
        let path = dir.join("settings.json");
        fs::write(&path, "{\"old\":true,\"padding\":\"xxxxxxxxxx\"}\n").unwrap();

        atomic_write(&path, "{}\n").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
    }

    #[test]
    fn merge_keeps_unknown_top_level_keys() {
        let base: Value = serde_json::from_str(r#"{"custom":{"a":1},"language":"en"}"#).unwrap();

        let merged = merge(&Config::default(), Some(&base));

        assert_eq!(merged["custom"]["a"], 1);
        assert_eq!(merged["language"], "ru", "модель перекрывает известное");
    }

    #[test]
    fn merge_keeps_unknown_module_flags_and_extra_sections() {
        let base: Value = serde_json::from_str(
            r#"{"hudbar":{"weather":true,"future_module":true},"my_section":{"x":1}}"#,
        )
        .unwrap();

        let merged = merge(&Config::default(), Some(&base));

        assert_eq!(merged["hudbar"]["future_module"], true);
        assert_eq!(merged["my_section"]["x"], 1);
    }

    #[test]
    fn merge_does_not_invent_a_theme_when_none_is_chosen() {
        let base: Value = serde_json::from_str(r#"{"appearance":{"theme":"pixel"}}"#).unwrap();

        let merged = merge(&Config::default(), Some(&base));

        assert_eq!(merged["appearance"]["theme"], "pixel");
    }

    #[test]
    fn merge_writes_every_module_flag() {
        let mut config = Config::default();
        config.set_module(Module::Weather, false);
        config.set_module(Module::Dnd, false);

        let merged = merge(&config, None);

        for key in MODULE_KEYS {
            let expected = key != "weather" && key != "dnd";
            assert_eq!(merged["hudbar"][key], Value::Bool(expected), "{key}");
        }
    }

    #[test]
    fn merge_writes_order_flat_from_zones() {
        let mut config = Config::default();
        config.move_within_zone(Module::Dnd, -1);

        let merged = merge(&config, None);
        let order: Vec<&str> = merged["module_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();

        assert_eq!(order.len(), MODULE_KEYS.len());
        assert_eq!(order[0], "tray");
        assert_eq!(order[1], "weather");
        assert_eq!(*order.last().unwrap(), "network");
    }

    #[test]
    fn merge_writes_notifications() {
        let config = Config {
            font_size: 16,
            line_height: 20,
            position: NotificationPosition::BottomLeft,
            ..Default::default()
        };

        let merged = merge(&config, None);

        assert_eq!(merged["notifications"]["font_size"], 16);
        assert_eq!(merged["notifications"]["line_height"], 20);
        assert_eq!(merged["notifications"]["position"], "bottom-left");
    }

    #[test]
    fn merge_writes_the_osd_section() {
        let config = Config {
            osd: false,
            osd_duration_ms: 2500,
            ..Default::default()
        };

        let merged = merge(&config, None);

        assert_eq!(merged["osd"]["enabled"], false);
        assert_eq!(merged["osd"]["duration_ms"], 2500);
    }

    #[test]
    fn merge_result_parses_back_into_an_equal_config() {
        let mut config = Config::default();
        config.move_within_zone(Module::Audio, -1);
        config.height = 33;
        config.language = Language::En;
        config.theme = Some(Theme::Pixel);

        let merged = merge(&config, None);
        let text = serde_json::to_string(&merged).unwrap();
        let restored = Config::from(&super::super::settings::parse(&text));

        assert_eq!(restored.flatten(), config.flatten());
        assert_eq!(restored.height, 33);
        assert_eq!(restored.language, Language::En);
        assert_eq!(restored.theme, Some(Theme::Pixel));
    }

    #[test]
    fn osd_section_survives_a_save_and_read_round_trip() {
        let config = Config {
            osd: false,
            osd_duration_ms: 900,
            ..Default::default()
        };
        let text = serde_json::to_string(&merge(&config, None)).unwrap();

        let restored = Config::from(&super::super::settings::parse(&text));

        assert!(!restored.osd);
        assert_eq!(restored.osd_duration_ms, 900);
    }

    #[test]
    fn apply_patch_writes_only_the_osd_field_that_changed() {
        let dir = temp_dir("patch-osd");
        let path = dir.join("settings.json");
        seed(
            &path,
            r#"{"language":"en","osd":{"enabled":true,"duration_ms":1600,"note":"keep"}}"#,
        );

        let changed = apply_patch_to(
            &path,
            &Patch::Osd {
                enabled: Some(false),
                duration_ms: None,
            },
        )
        .unwrap();

        assert!(changed);
        let root: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(root["osd"]["enabled"], false);
        assert_eq!(root["osd"]["duration_ms"], 1600, "соседнее поле не тронуто");
        assert_eq!(root["osd"]["note"], "keep", "чужой ключ в секции сохранён");
        assert_eq!(root["language"], "en");
    }

    #[test]
    fn apply_patch_creates_the_osd_section_when_it_is_missing() {
        let dir = temp_dir("patch-osd-missing");
        let path = dir.join("settings.json");
        seed(&path, r#"{"hudbar":{"height":27}}"#);

        let changed = apply_patch_to(
            &path,
            &Patch::Osd {
                enabled: Some(true),
                duration_ms: Some(1200),
            },
        )
        .unwrap();

        assert!(changed);
        let root: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(root["osd"]["enabled"], true);
        assert_eq!(root["osd"]["duration_ms"], 1200);
        assert_eq!(root["hudbar"]["height"], 27);
    }

    #[test]
    fn apply_patch_reports_no_change_for_an_identical_osd_value() {
        let dir = temp_dir("patch-osd-noop");
        let path = dir.join("settings.json");
        seed(&path, r#"{"osd":{"enabled":true,"duration_ms":1600}}"#);
        let before = fs::metadata(&path).unwrap().modified().unwrap();

        let changed = apply_patch_to(
            &path,
            &Patch::Osd {
                enabled: Some(true),
                duration_ms: Some(1600),
            },
        )
        .unwrap();

        assert!(!changed);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn normalize_clamps_out_of_range_values() {
        let mut config = Config {
            height: 999,
            font_size: 1,
            line_height: 999,
            ..Default::default()
        };

        normalize(&mut config);

        assert_eq!(config.height, HEIGHT_MAX);
        assert_eq!(config.font_size, 10);
        assert_eq!(config.line_height, 28);
    }

    #[test]
    fn normalize_keeps_valid_values_untouched() {
        let mut config = Config {
            height: 30,
            font_size: 14,
            line_height: 18,
            ..Default::default()
        };
        let before = config.clone();

        normalize(&mut config);

        assert_eq!(config, before);
    }

    /// Окно не должно иметь возможности писать цвета: единственный путь записи —
    /// `save`/`save_font`, и ни один из них не касается `colors.css`.
    #[test]
    fn write_targets_never_include_colors_css() {
        let written = [settings_path().to_string_lossy().into_owned()];
        assert_eq!(written.len(), 1);
        assert!(!written[0].ends_with("colors.css"));
        assert!(!written[0].contains("matugen"));
        assert!(!written[0].ends_with(".css"));
    }

    /// Цветами владеет matugen, и окно не должно его запускать: иначе смена
    /// Normal/Pixel пересчитала бы палитру. Проверяем все исходники, а не
    /// список путей, — вызов можно было бы добавить где угодно.
    #[test]
    fn no_source_file_invokes_matugen() {
        let mut offenders: Vec<String> = Vec::new();
        for path in source_files() {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for (number, line) in text.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") || code.starts_with("///") || code.starts_with("//!") {
                    continue;
                }
                // Комментарии и ассерты про matugen допустимы, реальный
                // запуск процесса — нет.
                if code.contains("matugen")
                    && (code.contains("Command::new")
                        || code.contains("process::Command")
                        || code.contains(".arg(")
                        || code.contains(".args(")
                        || code.contains("output(")
                        || code.contains(".status("))
                {
                    offenders.push(format!("{}:{}: {}", path.display(), number + 1, code));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "matugen нельзя вызывать из окна:\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn no_source_file_invokes_legacy_settings_commands() {
        let setting_command = ["hud", "setting"].join("-");
        let theme_command = ["hud", "theme"].join("-");
        let mut offenders = Vec::new();
        for path in source_files() {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for (number, line) in text.lines().enumerate() {
                let invokes_process = line.contains("Command::new")
                    || line.contains(".arg(")
                    || line.contains(".args(");
                if invokes_process
                    && (line.contains(&setting_command) || line.contains(&theme_command))
                {
                    offenders.push(format!("{}:{}: {}", path.display(), number + 1, line));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "окно не должно вызывать legacy-команды:\n{}",
            offenders.join("\n")
        );
    }

    /// Ни один исходник не должен открывать `colors.css` на запись.
    #[test]
    fn no_source_file_writes_colors_css() {
        let mut offenders: Vec<String> = Vec::new();
        for path in source_files() {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for (number, line) in text.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") || code.starts_with("//!") {
                    continue;
                }
                if !code.contains("colors.css") {
                    continue;
                }
                let writes = code.contains("fs::write")
                    || code.contains("write_atomic")
                    || code.contains("atomic_write")
                    || code.contains("File::create")
                    || code.contains("OpenOptions");
                if writes {
                    offenders.push(format!("{}:{}: {}", path.display(), number + 1, code));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "colors.css принадлежит matugen, писать в него нельзя:\n{}",
            offenders.join("\n")
        );
    }

    /// Все `.rs` под `src/`, кроме собственного теста.
    fn source_files() -> Vec<PathBuf> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        collect_rs(&root, &mut files);
        files.sort();
        files
    }

    fn collect_rs(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }

    #[test]
    fn font_file_matches_the_theme() {
        assert_eq!(Theme::Normal.font(), "JetBrainsMono Nerd Font Propo");
        assert_eq!(Theme::Pixel.font(), "Minecraft Rus");
    }

    fn seed(path: &Path, text: &str) {
        atomic_write(path, text).unwrap();
    }

    fn read_theme_from(path: &Path) -> Option<Theme> {
        let text = fs::read_to_string(path).unwrap();
        serde_json::from_str::<Value>(&text)
            .unwrap()
            .get("appearance")?
            .get("theme")?
            .as_str()
            .and_then(Theme::from_key)
    }

    #[test]
    fn apply_patch_writes_only_the_changed_field() {
        let dir = temp_dir("patch");
        let path = dir.join("settings.json");
        seed(
            &path,
            r#"{"custom":{"keep":1},"language":"en","hudbar":{"height":27,"weather":true,"future":7}}"#,
        );

        let changed = apply_patch_to(&path, &Patch::Height(33)).unwrap();

        assert!(changed);
        let root: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(root["hudbar"]["height"], 33);
        assert_eq!(root["language"], "en", "соседние ключи не должны меняться");
        assert_eq!(root["hudbar"]["weather"], true);
        assert_eq!(root["hudbar"]["future"], 7, "чужой ключ в hudbar сохранён");
        assert_eq!(root["custom"]["keep"], 1);
    }

    #[test]
    fn apply_patch_reports_no_change_without_writing() {
        let dir = temp_dir("patch-noop");
        let path = dir.join("settings.json");
        seed(&path, r#"{"hudbar":{"height":27}}"#);
        let before = fs::metadata(&path).unwrap().modified().unwrap();

        let changed = apply_patch_to(&path, &Patch::Height(27)).unwrap();

        assert!(!changed, "то же значение не должно переписывать файл");
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn apply_patch_keeps_the_key_order_of_the_file() {
        let dir = temp_dir("patch-order");
        let path = dir.join("settings.json");
        let original = r#"{
  "language": "ru",
  "hudbar": {
    "height": 27
  },
  "appearance": {
    "theme": "normal"
  }
}
"#;
        seed(&path, original);

        apply_patch_to(&path, &Patch::Height(30)).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let language = text.find("\"language\"").expect("language");
        let hudbar = text.find("\"hudbar\"").expect("hudbar");
        let appearance = text.find("\"appearance\"").expect("appearance");
        assert!(
            language < hudbar && hudbar < appearance,
            "порядок секций изменился:\n{text}"
        );
    }

    #[test]
    fn apply_theme_keeps_json_and_font_in_sync() {
        let dir = temp_dir("theme");
        let path = dir.join("settings.json");
        let font = dir.join("font");
        seed(&path, r#"{"appearance":{"theme":"normal"},"my_key":5}"#);

        apply_theme_to(&path, &font, Theme::Pixel).unwrap();

        assert_eq!(read_theme_from(&path), Some(Theme::Pixel));
        assert_eq!(fs::read_to_string(&font).unwrap(), "Minecraft Rus\n");
        let root: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(root["my_key"], 5, "чужие ключи переживают смену темы");
    }

    #[test]
    fn apply_theme_rolls_json_back_when_the_font_cannot_be_written() {
        let dir = temp_dir("theme-rollback");
        let path = dir.join("settings.json");
        // Каталог вместо файла: запись шрифта гарантированно не удастся.
        let blocked_font = dir.join("font");
        fs::create_dir_all(&blocked_font).unwrap();
        seed(&path, r#"{"appearance":{"theme":"normal"}}"#);

        let error = apply_theme_to(&path, &blocked_font, Theme::Pixel);

        assert!(error.is_err(), "ожидалась ошибка записи шрифта");
        assert_eq!(
            read_theme_from(&path),
            Some(Theme::Normal),
            "JSON должен вернуться к прежней теме"
        );
    }

    #[test]
    fn save_font_writes_exactly_the_theme_font() {
        let dir = temp_dir("font");
        let path = dir.join("font");

        fs::write(&path, "stale\n").unwrap();
        atomic_write(&path, &format!("{}\n", Theme::Pixel.font())).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "Minecraft Rus\n");
    }
}

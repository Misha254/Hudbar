use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

/// Границы значений. Те же пределы показывают ползунки в окне настроек.
pub const HEIGHT_MIN: u32 = 24;
pub const HEIGHT_MAX: u32 = 48;
pub const FONT_SIZE_MIN: u32 = 10;
pub const FONT_SIZE_MAX: u32 = 18;
pub const LINE_HEIGHT_MIN: u32 = 14;
pub const LINE_HEIGHT_MAX: u32 = 28;

/// Модули панели в порядке по умолчанию. Единственный список на весь проект:
/// и панель, и окно настроек, и `hud-setting` берут порядок отсюда.
pub const MODULE_KEYS: [&str; 10] = [
    "tray", "weather", "webcam", "clock", "recorder", "battery", "system", "audio", "network",
    "dnd",
];

pub fn clamp(value: i64, min: u32, max: u32) -> u32 {
    value.clamp(min as i64, max as i64) as u32
}

pub fn default_module_order() -> Vec<String> {
    MODULE_KEYS.iter().map(|key| key.to_string()).collect()
}

/// Приводит произвольный список ключей к полному перечислению без дублей:
/// неизвестные ключи выбрасываются, пропущенные дописываются в конец.
pub fn normalize_module_order(keys: &[String]) -> Vec<String> {
    let mut parsed: Vec<String> = Vec::with_capacity(MODULE_KEYS.len());
    for key in keys {
        if MODULE_KEYS.contains(&key.as_str()) && !parsed.contains(key) {
            parsed.push(key.clone());
        }
    }
    for key in MODULE_KEYS {
        if !parsed.iter().any(|item| item == key) {
            parsed.push(key.to_string());
        }
    }
    parsed
}

/// Зона панели. Порядок модулей задаётся только внутри зоны: `tray` закреплён
/// слева, погода/камера/часы живут в центре, остальное — справа.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Left,
    Center,
    Right,
}

impl Zone {
    pub const ALL: [Zone; 3] = [Zone::Left, Zone::Center, Zone::Right];

    /// Модули зоны в порядке по умолчанию.
    pub fn default_modules(self) -> Vec<Module> {
        MODULE_KEYS
            .iter()
            .filter_map(|key| Module::from_key(key))
            .filter(|module| module.zone() == self)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Module {
    Tray,
    Weather,
    Webcam,
    Clock,
    Recorder,
    Battery,
    System,
    Audio,
    Network,
    Dnd,
}

impl Module {
    pub const ALL: [Module; 10] = [
        Module::Tray,
        Module::Weather,
        Module::Webcam,
        Module::Clock,
        Module::Recorder,
        Module::Battery,
        Module::System,
        Module::Audio,
        Module::Network,
        Module::Dnd,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Module::Tray => "tray",
            Module::Weather => "weather",
            Module::Webcam => "webcam",
            Module::Clock => "clock",
            Module::Recorder => "recorder",
            Module::Battery => "battery",
            Module::System => "system",
            Module::Audio => "audio",
            Module::Network => "network",
            Module::Dnd => "dnd",
        }
    }

    pub fn from_key(key: &str) -> Option<Module> {
        MODULE_KEYS
            .iter()
            .position(|item| *item == key)
            .map(|index| Module::ALL[index])
    }

    pub fn zone(self) -> Zone {
        match self {
            Module::Tray => Zone::Left,
            Module::Weather | Module::Webcam | Module::Clock => Zone::Center,
            _ => Zone::Right,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Ru,
    En,
}

impl Language {
    /// Сериализуемое значение для `settings.json`.
    #[allow(dead_code)]
    pub fn key(self) -> &'static str {
        match self {
            Language::Ru => "ru",
            Language::En => "en",
        }
    }

    /// Любое значение, кроме `en`, трактуется как `ru` — как в старом парсере.
    pub fn from_key(key: &str) -> Language {
        match key {
            "en" => Language::En,
            _ => Language::Ru,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl NotificationPosition {
    pub const ALL: [NotificationPosition; 4] = [
        NotificationPosition::TopLeft,
        NotificationPosition::TopRight,
        NotificationPosition::BottomLeft,
        NotificationPosition::BottomRight,
    ];

    /// Dunst ждёт ровно эти строки в `origin`.
    pub fn key(self) -> &'static str {
        match self {
            NotificationPosition::TopLeft => "top-left",
            NotificationPosition::TopRight => "top-right",
            NotificationPosition::BottomLeft => "bottom-left",
            NotificationPosition::BottomRight => "bottom-right",
        }
    }

    pub fn from_key(key: &str) -> Option<NotificationPosition> {
        NotificationPosition::ALL
            .into_iter()
            .find(|position| position.key() == key)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub language: Language,
    pub module_order: Vec<String>,
    pub height: u32,
    pub tray: bool,
    pub weather: bool,
    pub webcam: bool,
    pub clock: bool,
    pub recorder: bool,
    pub battery: bool,
    pub system: bool,
    pub audio: bool,
    pub network: bool,
    pub dnd: bool,
    /// Показывать ли шестерёнку Control Center на панели.
    pub control_button: bool,
    pub theme: Option<String>,
    pub font_size: u32,
    pub line_height: u32,
    pub position: NotificationPosition,
}

impl Settings {
    /// Признак включённости модуля по его ключу из `module_order`.
    #[allow(dead_code)]
    pub fn enabled(&self, key: &str) -> bool {
        match key {
            "tray" => self.tray,
            "weather" => self.weather,
            "webcam" => self.webcam,
            "clock" => self.clock,
            "recorder" => self.recorder,
            "battery" => self.battery,
            "system" => self.system,
            "audio" => self.audio,
            "network" => self.network,
            "dnd" => self.dnd,
            _ => false,
        }
    }

    /// Позиция модуля в `module_order`. Рендер сортирует группы по этому
    /// индексу, поэтому порядок в коде больше не задаёт вид панели.
    pub fn order_index(&self, key: &str) -> usize {
        self.module_order
            .iter()
            .position(|item| item == key)
            .unwrap_or(usize::MAX)
    }

    /// Нормализованная высота панели в пикселях.
    #[allow(dead_code)]
    pub fn height_px(&self) -> u32 {
        clamp(self.height as i64, HEIGHT_MIN, HEIGHT_MAX)
    }

    #[allow(dead_code)]
    pub fn font_size(&self) -> u32 {
        clamp(self.font_size as i64, FONT_SIZE_MIN, FONT_SIZE_MAX)
    }

    #[allow(dead_code)]
    pub fn line_height(&self) -> u32 {
        clamp(self.line_height as i64, LINE_HEIGHT_MIN, LINE_HEIGHT_MAX)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::Ru,
            module_order: default_module_order(),
            height: 27,
            tray: true,
            weather: true,
            webcam: true,
            clock: true,
            recorder: true,
            battery: true,
            system: true,
            audio: true,
            network: true,
            dnd: true,
            control_button: true,
            theme: None,
            font_size: 13,
            line_height: 15,
            position: NotificationPosition::TopRight,
        }
    }
}

fn path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/hudbar/settings.json")
}

pub fn modified() -> Option<SystemTime> {
    fs::metadata(path()).ok()?.modified().ok()
}

fn get_bool(object: &serde_json::Map<String, Value>, key: &str, fallback: bool) -> bool {
    object.get(key).and_then(Value::as_bool).unwrap_or(fallback)
}

pub fn load() -> Settings {
    let mut settings = Settings::default();
    let Ok(content) = fs::read_to_string(path()) else {
        return settings;
    };
    parse_into(&mut settings, &content);
    settings
}

/// Разбор текста конфига в заданную модель. Нужен окну настроек, чтобы
/// применить ровно ту же нормализацию, что и у панели.
#[allow(dead_code)]
pub fn parse(content: &str) -> Settings {
    let mut settings = Settings::default();
    parse_into(&mut settings, content);
    settings
}

fn parse_into(settings: &mut Settings, content: &str) {
    let Ok(value @ Value::Object(_)) = serde_json::from_str::<Value>(content) else {
        return;
    };
    let object = value
        .get("hudbar")
        .and_then(Value::as_object)
        .or_else(|| value.as_object())
        .expect("object checked above");
    if let Some(language) = value.get("language").and_then(Value::as_str) {
        settings.language = Language::from_key(language);
    }
    if let Some(order) = value.get("module_order").and_then(Value::as_array) {
        let keys: Vec<String> = order
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        settings.module_order = normalize_module_order(&keys);
    }
    if let Some(theme) = value
        .get("appearance")
        .and_then(|appearance| appearance.get("theme"))
        .and_then(Value::as_str)
    {
        settings.theme = Some(theme.to_string());
    }
    if let Some(height) = object.get("height").and_then(Value::as_u64) {
        settings.height = clamp(height as i64, HEIGHT_MIN, HEIGHT_MAX);
    }
    settings.tray = get_bool(object, "tray", settings.tray);
    settings.weather = get_bool(object, "weather", settings.weather);
    settings.webcam = get_bool(object, "webcam", settings.webcam);
    settings.clock = get_bool(object, "clock", settings.clock);
    settings.recorder = get_bool(object, "recorder", settings.recorder);
    settings.battery = get_bool(object, "battery", settings.battery);
    settings.system = get_bool(object, "system", settings.system);
    settings.audio = get_bool(object, "audio", settings.audio);
    settings.network = get_bool(object, "network", settings.network);
    settings.dnd = get_bool(object, "dnd", settings.dnd);
    // По умолчанию кнопка есть: выключать её нужно явно.
    settings.control_button = get_bool(object, "control_button", true);

    if let Some(notes) = value.get("notifications").and_then(Value::as_object) {
        if let Some(size) = notes.get("font_size").and_then(Value::as_u64) {
            settings.font_size = clamp(size as i64, FONT_SIZE_MIN, FONT_SIZE_MAX);
        }
        if let Some(line) = notes.get("line_height").and_then(Value::as_u64) {
            settings.line_height = clamp(line as i64, LINE_HEIGHT_MIN, LINE_HEIGHT_MAX);
        }
        if let Some(origin) = notes.get("position").and_then(Value::as_str)
            && let Some(position) = NotificationPosition::from_key(origin)
        {
            settings.position = position;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FONT_SIZE_MAX, Language, MODULE_KEYS, Module, NotificationPosition, Settings, Zone,
        default_module_order, normalize_module_order, parse_into,
    };

    #[test]
    fn clamps_height_and_preserves_missing_options() {
        let mut settings = Settings::default();
        parse_into(
            &mut settings,
            r#"{"hudbar": {"height": 100, "weather": false}}"#,
        );

        assert_eq!(settings.height, 48);
        assert!(!settings.weather);
        assert!(settings.tray);
    }

    #[test]
    fn ignores_invalid_json() {
        let mut settings = Settings::default();
        parse_into(&mut settings, "not json");

        assert_eq!(settings.height, Settings::default().height);
        assert_eq!(settings.weather, Settings::default().weather);
    }

    #[test]
    fn reads_legacy_flat_settings() {
        let mut settings = Settings::default();
        parse_into(&mut settings, r#"{"height": 31, "tray": false}"#);

        assert_eq!(settings.height, 31);
        assert!(!settings.tray);
        assert!(settings.weather);
    }

    #[test]
    fn reads_language_and_normalizes_module_order() {
        let mut settings = Settings::default();
        parse_into(
            &mut settings,
            r#"{"language":"en","module_order":["audio","audio","unknown","tray"]}"#,
        );

        assert_eq!(settings.language, Language::En);
        assert_eq!(settings.module_order[0], "audio");
        assert_eq!(settings.module_order[1], "tray");
        assert_eq!(settings.module_order.len(), 10);
    }

    #[test]
    fn rejects_unknown_language() {
        let mut settings = Settings::default();
        parse_into(&mut settings, r#"{"language":"de"}"#);

        assert_eq!(settings.language, Language::Ru);
    }

    #[test]
    fn normalization_drops_duplicates_and_unknown_keys() {
        let order = normalize_module_order(&[
            "audio".to_string(),
            "audio".to_string(),
            "nope".to_string(),
            "tray".to_string(),
        ]);

        assert_eq!(order[0], "audio");
        assert_eq!(order[1], "tray");
        assert_eq!(order.len(), MODULE_KEYS.len());
        assert_eq!(order.iter().filter(|key| *key == "audio").count(), 1);
    }

    #[test]
    fn normalization_appends_missing_modules_in_default_order() {
        let order = normalize_module_order(&["dnd".to_string(), "clock".to_string()]);

        assert_eq!(order[0], "dnd");
        assert_eq!(order[1], "clock");
        assert_eq!(order[2], "tray");
        assert_eq!(order[MODULE_KEYS.len() - 1], "network");
        assert_eq!(order.len(), MODULE_KEYS.len());
    }

    #[test]
    fn normalization_of_empty_list_yields_defaults() {
        assert_eq!(normalize_module_order(&[]), default_module_order());
    }

    #[test]
    fn normalization_is_idempotent() {
        let once = normalize_module_order(&["system".to_string(), "battery".to_string()]);

        assert_eq!(normalize_module_order(&once), once);
    }

    #[test]
    fn normalization_drops_out_of_zone_keys() {
        // Ключ, который не является модулем панели, не должен попасть в порядок.
        let order = normalize_module_order(&["height".to_string(), "tray".to_string()]);

        assert!(!order.contains(&"height".to_string()));
        assert_eq!(order.len(), MODULE_KEYS.len());
    }

    #[test]
    fn enabled_reads_module_flags_by_key() {
        let settings = Settings {
            audio: false,
            dnd: true,
            ..Default::default()
        };

        assert!(!settings.enabled("audio"));
        assert!(settings.enabled("dnd"));
        assert!(!settings.enabled("height"));
    }

    #[test]
    fn module_zone_matches_the_bar_layout() {
        assert_eq!(Module::Tray.zone(), Zone::Left);
        assert_eq!(Module::Weather.zone(), Zone::Center);
        assert_eq!(Module::Webcam.zone(), Zone::Center);
        assert_eq!(Module::Clock.zone(), Zone::Center);
        for module in [
            Module::Recorder,
            Module::Battery,
            Module::System,
            Module::Audio,
            Module::Network,
            Module::Dnd,
        ] {
            assert_eq!(module.zone(), Zone::Right, "{module:?} должен быть справа");
        }
    }

    #[test]
    fn zone_default_modules_partition_all_modules() {
        let mut all: Vec<Module> = Zone::ALL
            .iter()
            .flat_map(|zone| zone.default_modules())
            .collect();

        assert_eq!(all.len(), MODULE_KEYS.len());
        all.sort_by_key(|module| module.key());
        all.dedup();
        assert_eq!(all.len(), MODULE_KEYS.len(), "модуль не попал в две зоны");
    }

    #[test]
    fn module_key_round_trips_for_every_module() {
        for module in Module::ALL {
            assert_eq!(Module::from_key(module.key()), Some(module));
        }
        assert_eq!(Module::from_key("height"), None);
    }

    #[test]
    fn language_falls_back_to_russian_for_unknown_values() {
        assert_eq!(Language::from_key("en"), Language::En);
        assert_eq!(Language::from_key("ru"), Language::Ru);
        assert_eq!(Language::from_key("de"), Language::Ru);
        assert_eq!(Language::from_key(""), Language::Ru);
        assert_eq!(Language::En.key(), "en");
    }

    #[test]
    fn notification_position_round_trips() {
        for position in NotificationPosition::ALL {
            assert_eq!(
                NotificationPosition::from_key(position.key()),
                Some(position)
            );
        }
        assert_eq!(NotificationPosition::from_key("middle"), None);
        assert_eq!(NotificationPosition::BottomLeft.key(), "bottom-left");
    }

    #[test]
    fn parses_notifications_with_clamping() {
        let mut settings = Settings::default();
        parse_into(
            &mut settings,
            r#"{"notifications":{"font_size":99,"line_height":1,"position":"bottom-left"}}"#,
        );

        assert_eq!(settings.font_size(), FONT_SIZE_MAX);
        assert_eq!(settings.line_height(), 14);
        assert_eq!(settings.position, NotificationPosition::BottomLeft);
    }

    #[test]
    fn keeps_notification_defaults_for_unknown_position() {
        let mut settings = Settings::default();
        parse_into(&mut settings, r#"{"notifications":{"position":"middle"}}"#);

        assert_eq!(settings.position, NotificationPosition::TopRight);
        assert_eq!(settings.font_size(), 13);
    }

    #[test]
    fn parses_theme_from_appearance() {
        let mut settings = Settings::default();
        parse_into(&mut settings, r#"{"appearance":{"theme":"pixel"}}"#);

        assert_eq!(settings.theme.as_deref(), Some("pixel"));
    }

    #[test]
    fn missing_notifications_section_keeps_defaults() {
        let mut settings = Settings::default();
        parse_into(&mut settings, r#"{"hudbar":{"height":30}}"#);

        assert_eq!(settings.font_size(), 13);
        assert_eq!(settings.line_height(), 15);
        assert_eq!(settings.position, NotificationPosition::TopRight);
    }
}

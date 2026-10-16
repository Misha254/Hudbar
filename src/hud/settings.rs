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
/// Границы длительности окна OSD. По умолчанию — значение для новых установок.
pub const OSD_DURATION_MIN_MS: u32 = 600;
pub const OSD_DURATION_MAX_MS: u32 = 4000;
pub const OSD_DURATION_DEFAULT_MS: u32 = 1600;
/// Границы скорости анимаций: 0.5 — вдвое медленнее, 2.0 — вдвое быстрее.
pub const ANIMATION_SPEED_MIN: f32 = 0.5;
pub const ANIMATION_SPEED_MAX: f32 = 2.0;
pub const ANIMATION_SPEED_DEFAULT: f32 = 1.0;
/// Адрес external controller Mihomo по умолчанию.
pub const VPN_CONTROLLER_DEFAULT: &str = "127.0.0.1:9090";
/// Границы таймаута обращения к контроллеру.
pub const VPN_TIMEOUT_MIN_MS: u32 = 300;
pub const VPN_TIMEOUT_MAX_MS: u32 = 5000;
pub const VPN_TIMEOUT_DEFAULT_MS: u32 = 1500;

/// Модули панели в порядке по умолчанию. Единственный список на весь проект:
/// и панель, и окно настроек, и `hud-setting` берут порядок отсюда.
pub const MODULE_KEYS: [&str; 10] = [
    "tray", "weather", "webcam", "clock", "recorder", "battery", "system", "audio", "network",
    "dnd",
];

pub fn clamp(value: i64, min: u32, max: u32) -> u32 {
    value.clamp(min as i64, max as i64) as u32
}

/// То же для дробных значений: границы анимаций заданы как `f32`, а в JSON
/// число может прийти и строкой, и `null`, и `NaN`.
pub fn clamp_f(value: f64, min: f32, max: f32) -> f32 {
    if value.is_nan() {
        return min;
    }
    (value as f32).clamp(min, max)
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

    /// Короткая подпись кнопки в окне: «РУС» и «EN». Окно настроек
    /// подключает `settings.rs` отдельно от панели, поэтому метод может
    /// казаться неиспользуемым в самом `hudbar`.
    #[allow(dead_code)]
    pub fn short(self) -> &'static str {
        match self {
            Language::Ru => "РУС",
            Language::En => "EN",
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
    /// Показывать ли своё окно громкости/микрофона вместо уведомления dunst.
    pub osd: bool,
    /// Сколько миллисекунд висит окно OSD.
    pub osd_duration_ms: u32,
    /// Анимации включены. Выключенные анимации — это нулевые длительности,
    /// то есть значения сразу на своих местах.
    pub animations: bool,
    /// Множитель длительности анимаций: 2.0 — вдвое быстрее.
    pub animation_speed: f32,
    /// Адрес external controller Mihomo для VPN-окна.
    pub vpn_controller: String,
    /// Таймаут обращения к контроллеру.
    pub vpn_timeout_ms: u32,
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

    /// Длительность окна OSD с учётом границ: значение могли прийти из правки
    /// файла руками, и панель не должна показывать окно дольше или короче
    /// пределов ползунка.
    /// Таймаут контроллера для VPN-окна.
    pub fn vpn_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(clamp(
            self.vpn_timeout_ms as i64,
            VPN_TIMEOUT_MIN_MS,
            VPN_TIMEOUT_MAX_MS,
        ) as u64)
    }

    /// Длительность окна OSD с учётом границ: значение могли прийти из правки
    pub fn osd_duration(&self) -> std::time::Duration {
        std::time::Duration::from_millis(clamp(
            self.osd_duration_ms as i64,
            OSD_DURATION_MIN_MS,
            OSD_DURATION_MAX_MS,
        ) as u64)
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
            // Выключено по умолчанию: окно создаёт и уничтожает слой
            // layer-shell, а на nvidia с niri такое churn — лишний риск.
            // Включается пользователем в Control Center.
            osd: false,
            osd_duration_ms: OSD_DURATION_DEFAULT_MS,
            animations: true,
            animation_speed: ANIMATION_SPEED_DEFAULT,
            vpn_controller: VPN_CONTROLLER_DEFAULT.to_string(),
            vpn_timeout_ms: VPN_TIMEOUT_DEFAULT_MS,
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

    // Анимации включаются по умолчанию: без них значения в панели появляются
    // рывком, но выключать их нужно явно — как и кнопку шестерёнки.
    settings.animations = get_bool(object, "animations", settings.animations);
    if let Some(speed) = object.get("animation_speed").and_then(Value::as_f64) {
        settings.animation_speed = clamp_f(speed, ANIMATION_SPEED_MIN, ANIMATION_SPEED_MAX);
    }

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

    // Секция `vpn`: адрес и таймаут контроллера. Секрета здесь нет и быть не
    // должно — он читается из окружения или файла, а не из `settings.json`.
    if let Some(vpn) = value.get("vpn").and_then(Value::as_object) {
        if let Some(controller) = vpn.get("controller").and_then(Value::as_str) {
            let controller = controller.trim();
            if !controller.is_empty() {
                settings.vpn_controller = controller.to_string();
            }
        }
        if let Some(timeout) = vpn.get("timeout_ms").and_then(Value::as_u64) {
            settings.vpn_timeout_ms = clamp(timeout as i64, VPN_TIMEOUT_MIN_MS, VPN_TIMEOUT_MAX_MS);
        }
    }

    // Секция `osd` необязательна: старый файл без неё даёт значения по
    // умолчанию, поэтому включённость выключается только явным `false`.
    if let Some(osd) = value.get("osd").and_then(Value::as_object) {
        settings.osd = get_bool(osd, "enabled", settings.osd);
        if let Some(duration) = osd.get("duration_ms").and_then(Value::as_u64) {
            settings.osd_duration_ms =
                clamp(duration as i64, OSD_DURATION_MIN_MS, OSD_DURATION_MAX_MS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ANIMATION_SPEED_DEFAULT, ANIMATION_SPEED_MAX, ANIMATION_SPEED_MIN, FONT_SIZE_MAX,
        FONT_SIZE_MIN, HEIGHT_MAX, HEIGHT_MIN, LINE_HEIGHT_MAX, LINE_HEIGHT_MIN, Language,
        MODULE_KEYS, Module, NotificationPosition, OSD_DURATION_DEFAULT_MS, OSD_DURATION_MAX_MS,
        OSD_DURATION_MIN_MS, Settings, VPN_CONTROLLER_DEFAULT, VPN_TIMEOUT_DEFAULT_MS,
        VPN_TIMEOUT_MAX_MS, VPN_TIMEOUT_MIN_MS, Zone, clamp_f, default_module_order,
        normalize_module_order, parse_into,
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
    fn reads_the_osd_section_and_defaults_when_it_is_absent() {
        let mut with_section = Settings::default();
        parse_into(
            &mut with_section,
            r#"{"osd":{"enabled":false,"duration_ms":2400}}"#,
        );
        let mut without_section = Settings::default();
        parse_into(&mut without_section, r#"{"hudbar":{"height":27}}"#);

        assert!(!with_section.osd);
        assert_eq!(with_section.osd_duration().as_millis(), 2400);
        assert!(
            !without_section.osd,
            "старый файл без секции: OSD выключен по умолчанию"
        );
        assert_eq!(
            without_section.osd_duration().as_millis(),
            OSD_DURATION_DEFAULT_MS as u128
        );
    }

    #[test]
    fn clamps_an_out_of_range_osd_duration() {
        let mut settings = Settings::default();
        parse_into(&mut settings, r#"{"osd":{"duration_ms":99999}}"#);

        assert_eq!(
            settings.osd_duration().as_millis(),
            OSD_DURATION_MAX_MS as u128
        );
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
        assert_eq!(Language::Ru.short(), "РУС");
        assert_eq!(Language::En.short(), "EN");
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

    #[test]
    fn animations_are_on_by_default() {
        let mut settings = Settings::default();
        parse_into(&mut settings, r#"{"hudbar":{"height":27}}"#);

        assert!(
            settings.animations,
            "старый файл без полей: анимации включены"
        );
        assert_eq!(settings.animation_speed, ANIMATION_SPEED_DEFAULT);
    }

    #[test]
    fn animations_can_be_switched_off_explicitly() {
        let mut settings = Settings::default();
        parse_into(&mut settings, r#"{"hudbar":{"animations":false}}"#);

        assert!(!settings.animations);
    }

    #[test]
    fn clamps_an_out_of_range_animation_speed() {
        let mut settings = Settings::default();
        parse_into(&mut settings, r#"{"hudbar":{"animation_speed":9.0}}"#);

        assert_eq!(settings.animation_speed, ANIMATION_SPEED_MAX);

        let mut slow = Settings::default();
        parse_into(&mut slow, r#"{"hudbar":{"animation_speed":0.01}}"#);

        assert_eq!(slow.animation_speed, ANIMATION_SPEED_MIN);
    }

    #[test]
    fn a_broken_animation_speed_falls_back_to_the_default() {
        let mut settings = Settings::default();
        parse_into(
            &mut settings,
            r#"{"hudbar":{"animation_speed":"fast","height":27}}"#,
        );

        assert_eq!(
            settings.animation_speed, ANIMATION_SPEED_DEFAULT,
            "строка вместо числа — значение по умолчанию"
        );
    }

    #[test]
    fn clamp_f_rejects_nan() {
        assert_eq!(
            clamp_f(f64::NAN, ANIMATION_SPEED_MIN, ANIMATION_SPEED_MAX),
            ANIMATION_SPEED_MIN
        );
        assert_eq!(clamp_f(1.5, ANIMATION_SPEED_MIN, ANIMATION_SPEED_MAX), 1.5);
        assert_eq!(
            clamp_f(100.0, ANIMATION_SPEED_MIN, ANIMATION_SPEED_MAX),
            ANIMATION_SPEED_MAX
        );
    }

    /// Схема `settings.json` лежит в `docs/settings.schema.json`: по ней редактор
    /// проверяет файл, а этот тест держит её вместе с кодом. Ключи, диапазоны и
    /// перечисления иначе разошлись бы молча: панель зажимала бы значение по
    /// одному числу, а схема разрешала бы другое.
    #[test]
    fn the_settings_schema_matches_the_parser() {
        let schema = schema();
        let properties = schema["properties"].as_object().expect("секции схемы");

        // Секции: ровно те, что читает `parse_into`, плюс ссылка на схему.
        let mut sections: Vec<&str> = properties.keys().map(String::as_str).collect();
        sections.sort_unstable();
        assert_eq!(
            sections,
            [
                "$schema",
                "appearance",
                "hudbar",
                "language",
                "module_order",
                "notifications",
                "osd",
                "vpn"
            ],
            "секции схемы"
        );

        // Секция бара: высота, кнопка шестерёнки, анимации и все ключи модулей.
        let hudbar = properties["hudbar"]["properties"]
            .as_object()
            .expect("секция hudbar");
        let mut keys: Vec<&str> = hudbar.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected: Vec<&str> = MODULE_KEYS.to_vec();
        expected.extend(["animation_speed", "animations", "control_button", "height"]);
        expected.sort_unstable();
        assert_eq!(keys, expected, "ключи секции hudbar");

        // Диапазоны — те же числа, что в коде: по ним зажимаются настройки,
        // пришедшие из правки файла руками.
        let range = |path: &[&str]| -> (i64, i64) {
            let mut node = &schema;
            for step in path {
                node = &node[step];
            }
            (
                node["minimum"].as_i64().expect("minimum"),
                node["maximum"].as_i64().expect("maximum"),
            )
        };
        assert_eq!(
            range(&["properties", "hudbar", "properties", "height"]),
            (HEIGHT_MIN as i64, HEIGHT_MAX as i64),
            "высота бара"
        );
        assert_eq!(
            range(&["properties", "notifications", "properties", "font_size"]),
            (FONT_SIZE_MIN as i64, FONT_SIZE_MAX as i64),
            "кегль уведомлений"
        );
        assert_eq!(
            range(&["properties", "notifications", "properties", "line_height"]),
            (LINE_HEIGHT_MIN as i64, LINE_HEIGHT_MAX as i64),
            "межстрочный интервал"
        );
        assert_eq!(
            range(&["properties", "osd", "properties", "duration_ms"]),
            (OSD_DURATION_MIN_MS as i64, OSD_DURATION_MAX_MS as i64),
            "длительность OSD"
        );
        assert_eq!(
            range(&["properties", "vpn", "properties", "timeout_ms"]),
            (VPN_TIMEOUT_MIN_MS as i64, VPN_TIMEOUT_MAX_MS as i64),
            "таймаут контроллера"
        );
        let speed = &properties["hudbar"]["properties"]["animation_speed"];
        assert_eq!(
            (speed["minimum"].as_f64(), speed["maximum"].as_f64()),
            (
                Some(ANIMATION_SPEED_MIN as f64),
                Some(ANIMATION_SPEED_MAX as f64)
            ),
            "множитель анимаций"
        );

        // Перечисления совпадают с кодом: язык, позиция уведомлений, модули.
        assert_eq!(
            strings(&properties["language"]["enum"]),
            [Language::Ru.key(), Language::En.key()],
            "языки"
        );
        assert_eq!(
            strings(&properties["notifications"]["properties"]["position"]["enum"]),
            NotificationPosition::ALL.map(NotificationPosition::key),
            "позиции уведомлений"
        );
        assert_eq!(
            strings(&properties["module_order"]["items"]["enum"]),
            MODULE_KEYS,
            "ключи модулей"
        );

        // Значения по умолчанию, у которых есть имя в коде, — те же: иначе
        // отсутствие ключа и его явное значение давали бы разные настройки.
        assert_eq!(
            properties["vpn"]["properties"]["controller"]["default"].as_str(),
            Some(VPN_CONTROLLER_DEFAULT),
            "адрес контроллера"
        );
        assert_eq!(
            properties["vpn"]["properties"]["timeout_ms"]["default"].as_i64(),
            Some(VPN_TIMEOUT_DEFAULT_MS as i64),
            "таймаут контроллера"
        );
        assert_eq!(
            properties["osd"]["properties"]["duration_ms"]["default"].as_i64(),
            Some(OSD_DURATION_DEFAULT_MS as i64),
            "длительность OSD"
        );
        assert_eq!(
            properties["hudbar"]["properties"]["animation_speed"]["default"].as_f64(),
            Some(ANIMATION_SPEED_DEFAULT as f64),
            "множитель анимаций"
        );
    }

    /// Установочный файл — это записанные значения по умолчанию из схемы. Иначе
    /// `default` в схеме и то, что получает новый пользователь, разошлись бы, и
    /// схема врала бы про файл, который сама же описывает.
    #[test]
    fn the_installed_settings_match_the_schema_defaults() {
        let schema = schema();
        let installed: serde_json::Value =
            serde_json::from_str(include_str!("../../config/hudbar/settings.json"))
                .expect("установочный конфиг разбирается");

        assert_eq!(
            installed["$schema"].as_str(),
            Some("./settings.schema.json"),
            "файл и схема лежат рядом, поэтому ссылка относительная"
        );
        check_defaults(&schema, &installed, "");
    }

    /// Схема настроек как она лежит в репозитории.
    fn schema() -> serde_json::Value {
        serde_json::from_str(include_str!("../../docs/settings.schema.json"))
            .expect("схема настроек разбирается")
    }

    /// Строки перечисления в том же порядке.
    fn strings(value: &serde_json::Value) -> Vec<String> {
        value
            .as_array()
            .unwrap_or_else(|| panic!("не перечисление: {value}"))
            .iter()
            .map(|item| item.as_str().expect("строка в перечислении").to_string())
            .collect()
    }

    /// Сверяет файл со схемой: каждый описанный ключ на месте и равен значению
    /// по умолчанию, вложенные секции — так же.
    fn check_defaults(schema: &serde_json::Value, file: &serde_json::Value, path: &str) {
        let properties = schema["properties"].as_object().expect("свойства схемы");
        let object = file
            .as_object()
            .unwrap_or_else(|| panic!("{path}: не объект: {file}"));
        for (key, property) in properties {
            if key == "$schema" {
                continue;
            }
            let value = object
                .get(key)
                .unwrap_or_else(|| panic!("{path}{key}: нет в установочном файле"));
            if let Some(default) = property.get("default") {
                assert_eq!(value, default, "{path}{key}: значение по умолчанию");
            }
            if property["type"] == "object" {
                check_defaults(property, value, &format!("{path}{key}."));
            }
        }
    }
}

use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone)]
pub struct Settings {
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
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

fn parse_into(settings: &mut Settings, content: &str) {
    let Ok(value @ Value::Object(_)) = serde_json::from_str::<Value>(content) else {
        return;
    };
    let object = value
        .get("hudbar")
        .and_then(Value::as_object)
        .or_else(|| value.as_object())
        .expect("object checked above");
    if let Some(height) = object.get("height").and_then(Value::as_u64) {
        settings.height = height.clamp(24, 48) as u32;
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
}

#[cfg(test)]
mod tests {
    use super::{Settings, parse_into};

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
}

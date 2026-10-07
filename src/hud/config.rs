//! Модель настроек окна: зоны модулей, перестановка и сериализация.
//!
//! В `settings.json` порядок модулей хранится одним плоским списком, но панель
//! рисуется тремя зонами, и переставлять модули можно только внутри зоны.
//! Поэтому модель держит зоны явно, а `module_order` — развёрнутое представление
//! того же. Настройки применяются сразу, поэтому здесь нет черновика и флага
//! несохранённых изменений: значение в модели всегда совпадает с файлом.

use super::settings::{
    HEIGHT_MAX, HEIGHT_MIN, Language, MODULE_KEYS, Module, NotificationPosition,
    OSD_DURATION_DEFAULT_MS, Settings, VPN_CONTROLLER_DEFAULT, VPN_TIMEOUT_DEFAULT_MS, Zone, clamp,
};

/// Высота панели в пикселях.
pub fn clamp_height(value: i64) -> u32 {
    clamp(value, HEIGHT_MIN, HEIGHT_MAX)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub left: Vec<Module>,
    pub center: Vec<Module>,
    pub right: Vec<Module>,
    pub modules: Vec<(Module, bool)>,
    pub height: u32,
    pub language: Language,
    pub theme: Option<Theme>,
    pub font_size: u32,
    pub line_height: u32,
    pub position: NotificationPosition,
    /// Показывать ли шестерёнку Control Center на панели.
    pub control_button: bool,
    /// Своё окно громкости/микрофона вместо уведомления dunst.
    pub osd: bool,
    pub osd_duration_ms: u32,
    /// Адрес и таймаут контроллера VPN живут в самой панели: окно настроек их
    /// не правит, поэтому модель хранит только значения по умолчанию.
    pub vpn_controller: String,
    pub vpn_timeout_ms: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Normal,
    Pixel,
}

impl Theme {
    pub fn key(self) -> &'static str {
        match self {
            Theme::Normal => "normal",
            Theme::Pixel => "pixel",
        }
    }

    pub fn from_key(key: &str) -> Option<Theme> {
        match key {
            "normal" => Some(Theme::Normal),
            "pixel" => Some(Theme::Pixel),
            _ => None,
        }
    }

    /// Имя файла шрифта, который читает HUDbar (`ui.rs:configured_font`).
    /// Тема и файл шрифта обязаны совпадать, иначе панель и окно рисуют разное.
    pub fn font(self) -> &'static str {
        match self {
            Theme::Normal => "JetBrainsMono Nerd Font Propo",
            Theme::Pixel => "Minecraft Rus",
        }
    }
}

impl Config {
    pub fn module_enabled(&self, module: Module) -> bool {
        self.modules
            .iter()
            .find(|(item, _)| *item == module)
            .map(|(_, on)| *on)
            .unwrap_or(true)
    }

    pub fn set_module(&mut self, module: Module, on: bool) {
        if let Some(entry) = self.modules.iter_mut().find(|(item, _)| *item == module) {
            entry.1 = on;
        }
    }

    pub fn toggle_module(&mut self, module: Module) {
        let next = !self.module_enabled(module);
        self.set_module(module, next);
    }

    /// Модули зоны в том порядке, в каком они лежат в модели.
    pub fn zone(&self, zone: Zone) -> &[Module] {
        match zone {
            Zone::Left => &self.left,
            Zone::Center => &self.center,
            Zone::Right => &self.right,
        }
    }

    /// Перемещает модуль внутри своей зоны на `delta` шагов. Перенос через
    /// границу зоны запрещён: `-1` на первом и `+1` на последнем — no-op,
    /// иначе перестановка путалась бы с навигацией по разделам.
    /// Возвращает `true`, если порядок изменился.
    pub fn move_within_zone(&mut self, module: Module, delta: i32) -> bool {
        let order = match module.zone() {
            Zone::Left => &mut self.left,
            Zone::Center => &mut self.center,
            Zone::Right => &mut self.right,
        };
        let Some(index) = order.iter().position(|item| *item == module) else {
            return false;
        };
        let target = index as i32 + delta;
        if target < 0 || target >= order.len() as i32 {
            return false;
        }
        order.swap(index, target as usize);
        true
    }

    /// `Shift+↑`/`Shift+↓` двигают модуль внутри зоны.
    ///
    /// Отдельного запрета для `tray` здесь нет: он и так неподвижен, потому что
    /// левая зона состоит из одного элемента и перенос через границу запрещён.
    /// Если в левую зону добавят второй модуль, понадобится явное правило.
    pub fn move_selected(&mut self, module: Module, delta: i32) -> bool {
        self.move_within_zone(module, delta)
    }

    /// Плоский список для `settings.json`: зоны идут в порядке `Zone::ALL`,
    /// внутри зоны — как в модели.
    pub fn flatten(&self) -> Vec<String> {
        Zone::ALL
            .iter()
            .flat_map(|zone| self.zone(*zone))
            .map(|module| module.key().to_string())
            .collect()
    }

    /// Собирает `Settings` для панели. Тема сюда не попадает намеренно:
    /// панель определяет её по файлу шрифта, а не по `settings.json`.
    pub fn to_settings(&self) -> Settings {
        Settings {
            language: self.language,
            module_order: self.flatten(),
            height: self.height,
            tray: self.module_enabled(Module::Tray),
            weather: self.module_enabled(Module::Weather),
            webcam: self.module_enabled(Module::Webcam),
            clock: self.module_enabled(Module::Clock),
            recorder: self.module_enabled(Module::Recorder),
            battery: self.module_enabled(Module::Battery),
            system: self.module_enabled(Module::System),
            audio: self.module_enabled(Module::Audio),
            network: self.module_enabled(Module::Network),
            dnd: self.module_enabled(Module::Dnd),
            control_button: self.control_button,
            theme: self.theme.map(|theme| theme.key().to_string()),
            font_size: self.font_size,
            line_height: self.line_height,
            position: self.position,
            osd: self.osd,
            osd_duration_ms: self.osd_duration_ms,
            vpn_controller: super::settings::VPN_CONTROLLER_DEFAULT.to_string(),
            vpn_timeout_ms: super::settings::VPN_TIMEOUT_DEFAULT_MS,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            left: Zone::Left.default_modules(),
            center: Zone::Center.default_modules(),
            right: Zone::Right.default_modules(),
            modules: MODULE_KEYS
                .iter()
                .filter_map(|key| Module::from_key(key))
                .map(|module| (module, true))
                .collect(),
            height: 27,
            language: Language::Ru,
            theme: None,
            font_size: 13,
            line_height: 15,
            position: NotificationPosition::TopRight,
            control_button: true,
            osd: true,
            osd_duration_ms: OSD_DURATION_DEFAULT_MS,
            vpn_controller: VPN_CONTROLLER_DEFAULT.to_string(),
            vpn_timeout_ms: VPN_TIMEOUT_DEFAULT_MS,
        }
    }
}

impl From<&Settings> for Config {
    /// Разворачивает плоский `module_order` по зонам. Каждый модуль попадает
    /// ровно в свою зону, пропущенные дописываются в конец зоны — так модель
    /// всегда содержит все 10 модулей, даже если список пришёл урезанным.
    fn from(settings: &Settings) -> Self {
        let mut config = Config {
            height: clamp_height(settings.height as i64),
            language: settings.language,
            theme: settings.theme.as_deref().and_then(Theme::from_key),
            font_size: settings.font_size(),
            line_height: settings.line_height(),
            position: settings.position,
            osd: settings.osd,
            osd_duration_ms: settings.osd_duration_ms,
            ..Config::default()
        };
        for key in MODULE_KEYS {
            if let Some(module) = Module::from_key(key) {
                let on = settings.enabled(key);
                config.set_module(module, on);
            }
        }
        config.control_button = settings.control_button;
        let zones = split_by_zone(&settings.module_order);
        config.left = zones.0;
        config.center = zones.1;
        config.right = zones.2;
        config
    }
}

/// Раскладывает плоский список ключей по зонам, сохраняя взаимный порядок
/// внутри каждой зоны. Нормализация гарантирует, что список полон и без дублей.
fn split_by_zone(order: &[String]) -> (Vec<Module>, Vec<Module>, Vec<Module>) {
    let mut left = Vec::new();
    let mut center = Vec::new();
    let mut right = Vec::new();
    for key in super::settings::normalize_module_order(order) {
        let Some(module) = Module::from_key(&key) else {
            continue;
        };
        let list = match module.zone() {
            Zone::Left => &mut left,
            Zone::Center => &mut center,
            Zone::Right => &mut right,
        };
        if !list.contains(&module) {
            list.push(module);
        }
    }
    (left, center, right)
}

#[cfg(test)]
mod tests {
    use super::super::settings::{Language, Module, Settings, Zone};
    use super::{Config, Theme, clamp_height};

    fn config_with_order(keys: &[&str]) -> Config {
        let settings = Settings {
            module_order: keys.iter().map(|key| key.to_string()).collect(),
            ..Default::default()
        };
        Config::from(&settings)
    }

    #[test]
    fn zones_hold_only_their_own_modules() {
        let config = Config::default();

        assert_eq!(config.left, vec![Module::Tray]);
        assert_eq!(
            config.center,
            vec![Module::Weather, Module::Webcam, Module::Clock]
        );
        assert_eq!(config.right.len(), 6);
        assert!(config.zone(Zone::Left).contains(&Module::Tray));
        assert!(!config.zone(Zone::Right).contains(&Module::Tray));
        assert!(!config.zone(Zone::Center).contains(&Module::Audio));
    }

    #[test]
    fn moving_a_module_stays_inside_its_zone() {
        let mut config = Config::default();

        // Центр по умолчанию: weather, webcam, clock. Сдвиг clock на -1 ставит
        // его на место webcam, вытесняя webcam в конец зоны.
        assert!(config.move_within_zone(Module::Clock, -1));
        assert_eq!(
            config.center,
            vec![Module::Weather, Module::Clock, Module::Webcam]
        );
        assert_eq!(
            config.right.len(),
            6,
            "правая зона не должна потерять модуль"
        );
        assert!(config.flatten().contains(&"clock".to_string()));
    }

    #[test]
    fn moving_past_a_zone_edge_is_a_no_op() {
        let mut config = Config::default();
        let before = config.clone();

        assert!(!config.move_within_zone(Module::Weather, -1));
        assert!(!config.move_within_zone(Module::Dnd, 1));
        assert_eq!(config, before);
    }

    #[test]
    fn tray_cannot_be_moved_and_only_toggles() {
        let mut config = Config::default();

        assert_eq!(Module::Tray.zone(), Zone::Left);
        assert_eq!(config.zone(Zone::Left).len(), 1);
        for delta in [-1, 1] {
            assert!(!config.move_selected(Module::Tray, delta), "delta={delta}");
            assert!(
                !config.move_within_zone(Module::Tray, delta),
                "delta={delta}"
            );
        }
        assert_eq!(config.left, vec![Module::Tray]);

        config.toggle_module(Module::Tray);
        assert!(!config.module_enabled(Module::Tray));
        config.toggle_module(Module::Tray);
        assert!(config.module_enabled(Module::Tray));
        assert_eq!(
            config.left,
            vec![Module::Tray],
            "переключатель не двигает модуль"
        );
    }

    #[test]
    fn tray_is_pinned_while_the_left_zone_holds_one_module() {
        assert_eq!(
            Zone::Left.default_modules(),
            vec![Module::Tray],
            "расширение левой зоны потребует явного запрета для tray"
        );
    }

    #[test]
    fn flatten_round_trips_through_zones() {
        assert_eq!(
            Config::default().flatten(),
            super::super::settings::default_module_order()
        );
    }

    #[test]
    fn flatten_reflects_moves_inside_a_zone() {
        let mut config = Config::default();
        config.move_within_zone(Module::Dnd, -1);
        let order = config.flatten();

        // dnd теперь перед network, но всё ещё в правой зоне.
        let dnd = order.iter().position(|key| key == "dnd").unwrap();
        let network = order.iter().position(|key| key == "network").unwrap();
        assert!(dnd < network);
        assert_eq!(order.len(), super::super::settings::MODULE_KEYS.len());
    }

    #[test]
    fn config_from_settings_keeps_every_module_exactly_once() {
        let config = config_with_order(&["tray", "dnd", "audio", "weather"]);

        let flat = config.flatten();
        assert_eq!(flat.len(), super::super::settings::MODULE_KEYS.len());
        for key in super::super::settings::MODULE_KEYS {
            assert_eq!(flat.iter().filter(|item| *item == key).count(), 1);
        }
    }

    #[test]
    fn config_from_settings_drops_unknown_keys() {
        let config = config_with_order(&["nope", "tray", "height"]);

        assert_eq!(config.left, vec![Module::Tray]);
        assert!(!config.flatten().contains(&"nope".to_string()));
        assert!(!config.flatten().contains(&"height".to_string()));
    }

    #[test]
    fn config_from_settings_appends_modules_missing_from_the_list() {
        let config = config_with_order(&["tray", "clock"]);

        assert_eq!(config.center.len(), 3, "пропущенные модули дописываются");
        assert_eq!(config.right.len(), 6);
    }

    #[test]
    fn config_reads_module_flags_from_settings() {
        let settings = Settings {
            weather: false,
            height: 99,
            ..Default::default()
        };

        let config = Config::from(&settings);

        assert!(!config.module_enabled(Module::Weather));
        assert!(config.module_enabled(Module::Clock));
        assert_eq!(config.height, 48, "высота тоже нормализуется");
    }

    #[test]
    fn config_round_trips_through_settings_without_drift() {
        let mut config = Config::default();
        config.move_within_zone(Module::Audio, -1);
        config.height = 33;

        let restored = Config::from(&config.to_settings());

        assert_eq!(restored.flatten(), config.flatten());
        assert_eq!(restored.height, 33);
    }

    #[test]
    fn height_is_clamped_to_the_supported_range() {
        assert_eq!(clamp_height(1), 24);
        assert_eq!(clamp_height(27), 27);
        assert_eq!(clamp_height(999), 48);
    }

    #[test]
    fn theme_and_its_font_file_agree() {
        // Панель читает тему из файла шрифта, окно — из settings.json. Если
        // имена разойдутся, панель и окно покажут разные шрифты.
        assert_eq!(Theme::Normal.font(), "JetBrainsMono Nerd Font Propo");
        assert_eq!(Theme::Pixel.font(), "Minecraft Rus");
        for theme in [Theme::Normal, Theme::Pixel] {
            let settings = Settings {
                theme: Some(theme.key().to_string()),
                ..Default::default()
            };
            assert_eq!(Config::from(&settings).theme, Some(theme));
        }
    }

    #[test]
    fn config_reads_notifications_with_clamping() {
        let settings = Settings {
            font_size: 99,
            line_height: 1,
            position: super::super::settings::NotificationPosition::BottomLeft,
            ..Default::default()
        };

        let config = Config::from(&settings);

        assert_eq!(config.font_size, 18);
        assert_eq!(config.line_height, 14);
        assert_eq!(
            config.position,
            super::super::settings::NotificationPosition::BottomLeft
        );
    }

    #[test]
    fn language_survives_the_round_trip() {
        let settings = Settings {
            language: Language::En,
            ..Default::default()
        };

        let config = Config::from(&settings);

        assert_eq!(config.language, Language::En);
        assert_eq!(config.to_settings().language, Language::En);
    }
}

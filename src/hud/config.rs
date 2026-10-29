//! Порядок модулей панели по зонам.
//!
//! В `settings.json` порядок модулей хранится одним плоским списком, но панель
//! рисуется тремя зонами, и переставлять модули можно только внутри зоны.
//! Поэтому модель держит зоны явно, а `module_order` — развёрнутое представление
//! того же. Настройки применяются сразу, поэтому здесь нет черновика и флага
//! несохранённых изменений: значение в модели всегда совпадает с файлом.

use super::settings::{Module, Settings, Zone};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub left: Vec<Module>,
    pub center: Vec<Module>,
    pub right: Vec<Module>,
}

impl Config {
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

    /// Плоский список для `settings.json`: зоны идут в порядке `Zone::ALL`,
    /// внутри зоны — как в модели.
    pub fn flatten(&self) -> Vec<String> {
        Zone::ALL
            .iter()
            .flat_map(|zone| self.zone(*zone))
            .map(|module| module.key().to_string())
            .collect()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            left: Zone::Left.default_modules(),
            center: Zone::Center.default_modules(),
            right: Zone::Right.default_modules(),
        }
    }
}

impl From<&Settings> for Config {
    /// Разворачивает плоский `module_order` по зонам. Каждый модуль попадает
    /// ровно в свою зону, пропущенные дописываются в конец зоны — так модель
    /// всегда содержит все 10 модулей, даже если список пришёл урезанным.
    fn from(settings: &Settings) -> Self {
        let zones = split_by_zone(&settings.module_order);
        Self {
            left: zones.0,
            center: zones.1,
            right: zones.2,
        }
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
    use super::super::settings::{Module, Settings, Zone};
    use super::Config;

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
    fn tray_cannot_be_moved() {
        let mut config = Config::default();

        assert_eq!(Module::Tray.zone(), Zone::Left);
        assert_eq!(config.zone(Zone::Left).len(), 1);
        for delta in [-1, 1] {
            assert!(
                !config.move_within_zone(Module::Tray, delta),
                "delta={delta}"
            );
        }
        assert_eq!(config.left, vec![Module::Tray]);
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
}

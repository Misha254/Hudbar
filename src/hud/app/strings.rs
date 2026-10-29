//! Подписи панели и её мини-окон на языке из `settings.json`.
//!
//! Строки собраны в одном месте, потому что панель рисует их из нескольких
//! модулей (`panels`, `input`, `calendar`), а `is_hint` должен узнавать
//! служебные строки в обоих языках. Само дерево меню живёт отдельно: у него
//! свой слой подписей, и смешивать их значило бы тащить в панель весь словарь
//! меню.

use crate::hud::i18n::{pick, wind_unit};
use crate::hud::settings::Language;

pub fn sound(lang: Language) -> &'static str {
    pick(lang, "Звук", "Sound")
}

pub fn microphone(lang: Language) -> &'static str {
    pick(lang, "Микрофон", "Microphone")
}

/// Состояние выключенного радио: это подсказка, а не выбор.
pub fn off_state(lang: Language) -> &'static str {
    pick(lang, "Выключено", "Off")
}

pub fn search(lang: Language) -> &'static str {
    pick(lang, "Поиск…", "Search…")
}

pub fn no_networks(lang: Language) -> &'static str {
    pick(lang, "Нет сетей", "No networks")
}

pub fn no_devices(lang: Language) -> &'static str {
    pick(lang, "Нет устройств", "No devices")
}

pub fn no_menu(lang: Language) -> &'static str {
    pick(lang, "Нет меню", "No menu")
}

pub fn no_inputs(lang: Language) -> &'static str {
    pick(lang, "Нет входов", "No inputs")
}

pub fn no_outputs(lang: Language) -> &'static str {
    pick(lang, "Нет выходов", "No outputs")
}

pub fn wifi_error(lang: Language) -> &'static str {
    pick(lang, "Ошибка Wi-Fi", "Wi-Fi error")
}

pub fn bt_error(lang: Language) -> &'static str {
    pick(lang, "Ошибка Bluetooth", "Bluetooth error")
}

pub fn audio_error(lang: Language) -> &'static str {
    pick(lang, "Ошибка аудио", "Audio error")
}

pub fn connected(lang: Language) -> &'static str {
    pick(lang, "подключено", "connected")
}

pub fn muted(lang: Language) -> &'static str {
    pick(lang, "Заглушено", "Muted")
}

pub fn input_section(lang: Language) -> &'static str {
    pick(lang, "Вход", "Input")
}

pub fn output_section(lang: Language) -> &'static str {
    pick(lang, "Выход", "Output")
}

pub fn password_hint(lang: Language) -> &'static str {
    pick(
        lang,
        "Enter — подключить · Esc — назад",
        "Enter — connect · Esc — back",
    )
}

pub fn updating_title(lang: Language) -> &'static str {
    pick(lang, "Обновление…", "Updating…")
}

pub fn updating_footer(lang: Language) -> &'static str {
    pick(lang, "обновление…", "updating…")
}

pub fn weather_error(lang: Language) -> &'static str {
    pick(lang, "Ошибка погоды", "Weather error")
}

pub fn no_data(lang: Language) -> &'static str {
    pick(lang, "Нет данных", "No data")
}

pub fn feels(lang: Language, temp: i32) -> String {
    match lang {
        Language::Ru => format!("ощущается {temp}°"),
        Language::En => format!("feels like {temp}°"),
    }
}

pub fn humidity(lang: Language, percent: u8) -> String {
    match lang {
        Language::Ru => format!("влажность {percent}%"),
        Language::En => format!("humidity {percent}%"),
    }
}

pub fn wind(lang: Language, speed: f32) -> String {
    match lang {
        Language::Ru => format!("ветер {speed:.1} {}", wind_unit(lang)),
        Language::En => format!("wind {speed:.1} {}", wind_unit(lang)),
    }
}

pub fn hourly(lang: Language) -> &'static str {
    pick(lang, "По часам", "Hourly")
}

pub fn forecast(lang: Language) -> &'static str {
    pick(lang, "Прогноз", "Forecast")
}

pub fn offline_cache(lang: Language, updated: &str) -> String {
    match lang {
        Language::Ru => format!("офлайн · кэш {updated}"),
        Language::En => format!("offline · cache {updated}"),
    }
}

pub fn updated(lang: Language, updated: &str) -> String {
    match lang {
        Language::Ru => format!("обновлено {updated}"),
        Language::En => format!("updated {updated}"),
    }
}

/// Номер недели в углу календаря: «№» / «W».
pub fn week_number(lang: Language) -> &'static str {
    pick(lang, "№", "W")
}

/// Ошибка системного опроса в мини-окне панели. Панель складывает причины в
/// общее состояние без языка (опрос идёт в фоне), поэтому известные строки
/// переводятся здесь. Незнакомый текст остаётся как есть.
pub fn system_error(lang: Language, raw: &str) -> String {
    if lang == Language::Ru {
        return raw.to_string();
    }
    match raw {
        "nmcli недоступен" => "nmcli is unavailable",
        "bluetoothctl недоступен" => "bluetoothctl is unavailable",
        "bluetoothctl не отвечает" => "bluetoothctl does not respond",
        "pactl не вернул список выходов" => {
            "pactl did not return the output list"
        }
        "pactl не вернул список входов" => {
            "pactl did not return the input list"
        }
        "weather-fancy.py недоступен или вернул неверный JSON" => {
            "weather-fancy.py is unavailable or returned invalid JSON"
        }
        other => other,
    }
    .to_string()
}

/// Служебная строка списка: на неё нельзя навести/нажать, и подсветки у неё
/// нет. Проверяем точные подписи, а не префиксы: иначе сеть с именем
/// «Nothing Phone» в английском окне стала бы некликабельной.
pub fn is_hint(label: &str, lang: Language) -> bool {
    let hints: &[&str] = match lang {
        Language::Ru => &[
            search(Language::Ru),
            no_networks(Language::Ru),
            no_devices(Language::Ru),
            no_menu(Language::Ru),
            no_inputs(Language::Ru),
            no_outputs(Language::Ru),
            off_state(Language::Ru),
            wifi_error(Language::Ru),
            bt_error(Language::Ru),
            audio_error(Language::Ru),
        ],
        Language::En => &[
            search(Language::En),
            no_networks(Language::En),
            no_devices(Language::En),
            no_menu(Language::En),
            no_inputs(Language::En),
            no_outputs(Language::En),
            off_state(Language::En),
            wifi_error(Language::En),
            bt_error(Language::En),
            audio_error(Language::En),
        ],
    };
    hints.contains(&label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_have_both_languages() {
        assert_eq!(sound(Language::Ru), "Звук");
        assert_eq!(sound(Language::En), "Sound");
        assert_eq!(
            password_hint(Language::Ru),
            "Enter — подключить · Esc — назад"
        );
        assert_eq!(password_hint(Language::En), "Enter — connect · Esc — back");
    }

    #[test]
    fn weather_texts_are_formatted_per_language() {
        assert_eq!(feels(Language::Ru, 21), "ощущается 21°");
        assert_eq!(feels(Language::En, 21), "feels like 21°");
        assert_eq!(humidity(Language::Ru, 48), "влажность 48%");
        assert_eq!(humidity(Language::En, 48), "humidity 48%");
        assert_eq!(wind(Language::Ru, 3.2), "ветер 3.2 м/с");
        assert_eq!(wind(Language::En, 3.2), "wind 3.2 m/s");
        assert_eq!(offline_cache(Language::Ru, "12:00"), "офлайн · кэш 12:00");
        assert_eq!(
            offline_cache(Language::En, "12:00"),
            "offline · cache 12:00"
        );
        assert_eq!(updated(Language::Ru, "12:00"), "обновлено 12:00");
        assert_eq!(updated(Language::En, "12:00"), "updated 12:00");
    }

    /// Подсказки узнаются строго, а пользовательские имена — нет: сеть
    /// «Nothing Phone» должна оставаться кликабельной.
    #[test]
    fn hints_are_exact_labels_not_prefixes() {
        assert!(is_hint("Поиск…", Language::Ru));
        assert!(is_hint("Off", Language::En));
        assert!(!is_hint("Nothing Phone", Language::En));
        assert!(!is_hint("Нетфликс", Language::Ru));
        assert!(!is_hint("Дом", Language::Ru));
    }

    /// Причины системных сбоев приходят в общее состояние без языка и
    /// переводятся при отрисовке панели.
    #[test]
    fn system_errors_are_translated_for_the_panel() {
        assert_eq!(
            system_error(Language::En, "nmcli недоступен"),
            "nmcli is unavailable"
        );
        assert_eq!(
            system_error(Language::En, "pactl не вернул список входов"),
            "pactl did not return the input list"
        );
        assert_eq!(
            system_error(Language::Ru, "nmcli недоступен"),
            "nmcli недоступен"
        );
        assert_eq!(system_error(Language::En, "что-то своё"), "что-то своё");
    }
}

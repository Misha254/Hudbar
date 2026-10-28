//! Данные верхней полосы: батарея и часы.
//!
//! Часы и дату гритер берёт из `i18n` — там же, где панель, поэтому «Ср, 28 окт
//! 20:31» и «Среда, 28 октября 2026» на экране входа и на баре совпадают по
//! формату и языку.
//!
//! Батарея читается прямо из sysfs: та же папка, что у панели, но без её
//! потоков и DBus — гритеру нужно одно число на кадр, а не поток событий.

use std::path::Path;

use super::i18n;
use super::settings::Language;
use chrono::{DateTime, Local, Timelike};

/// Состояние батареи для полосы.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    /// Заряд в процентах, 0..=100.
    pub percent: u8,
    /// Питание подключено или батарея заряжается.
    pub charging: bool,
}

/// Каталог sysfs по умолчанию.
pub const POWER_SUPPLY: &str = "/sys/class/power_supply";

/// Батарея машины, если она есть.
pub fn battery() -> Option<Battery> {
    battery_from(Path::new(POWER_SUPPLY))
}

/// Чтение из явного каталога — чтобы тест проверял разбор без своей машины.
pub fn battery_from(dir: &Path) -> Option<Battery> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<Battery> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if read_trim(&path.join("type")) != "Battery" {
            continue;
        }
        let Ok(percent) = read_trim(&path.join("capacity")).parse::<u8>() else {
            continue;
        };
        let status = read_trim(&path.join("status"));
        let battery = Battery {
            percent: percent.min(100),
            charging: status == "Charging" || status == "Full",
        };
        // Первая найденная батарея — как у панели: несколько батарей
        // показываются одной, отдельной ячейки под каждую нет.
        best.get_or_insert(battery);
    }
    best
}

fn read_trim(path: &Path) -> String {
    std::fs::read_to_string(path)
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

/// Подпись батареи: проценты, а при зарядке — ещё и знак «+».
pub fn battery_label(battery: Battery) -> String {
    if battery.charging {
        format!("{}%+", battery.percent)
    } else {
        format!("{}%", battery.percent)
    }
}

/// Часы в верхней полосе: дата и время по локали (`i18n::clock_bar`).
pub fn strip_clock(lang: Language, now: DateTime<Local>) -> String {
    i18n::clock_bar(lang, now)
}

/// Крупные часы карточки: только время, часы и минуты.
pub fn big_clock(now: DateTime<Local>) -> String {
    format!("{:02}:{:02}", now.hour(), now.minute())
}

/// Дата под часами: «Среда, 28 октября 2026» / «Wednesday, October 28, 2026».
pub fn date_line(lang: Language, now: DateTime<Local>) -> String {
    i18n::date_long(lang, now.date_naive())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn machine(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hudbar-greeter-battery-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог");
        dir
    }

    fn battery_entry(dir: &Path, name: &str, percent: &str, status: &str) {
        let path = dir.join(name);
        std::fs::create_dir_all(&path).expect("каталог батареи");
        std::fs::write(path.join("type"), "Battery").expect("тип");
        std::fs::write(path.join("capacity"), percent).expect("заряд");
        std::fs::write(path.join("status"), status).expect("статус");
    }

    #[test]
    fn a_battery_is_read_from_sysfs() {
        let dir = machine("present");
        battery_entry(&dir, "BAT0", "84", "Discharging");
        assert_eq!(
            battery_from(&dir),
            Some(Battery {
                percent: 84,
                charging: false
            })
        );

        battery_entry(&dir, "BAT0", "84", "Charging");
        assert_eq!(
            battery_from(&dir),
            Some(Battery {
                percent: 84,
                charging: true
            })
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Нет батареи (десктоп) — `None`, а не ноль процентов: ноль на полосе
    /// читался бы как «разряжено».
    #[test]
    fn a_desktop_has_no_battery() {
        let dir = machine("empty");
        assert_eq!(battery_from(&dir), None);
        assert_eq!(battery_from(&dir.join("нет-такого")), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Мусор в capacity и чужие устройства (сеть, мышь) не считаются
    /// батареей.
    #[test]
    fn only_batteries_with_a_number_count() {
        let dir = machine("garbage");
        let other = dir.join("AC");
        std::fs::create_dir_all(&other).expect("каталог");
        std::fs::write(other.join("type"), "Mains").expect("тип");
        battery_entry(&dir, "BAT0", "не число", "Discharging");
        assert_eq!(battery_from(&dir), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_label_shows_charging() {
        assert_eq!(
            battery_label(Battery {
                percent: 12,
                charging: false
            }),
            "12%"
        );
        assert_eq!(
            battery_label(Battery {
                percent: 12,
                charging: true
            }),
            "12%+"
        );
    }

    /// Часы и дата — из `i18n`, а не из своей таблицы месяцев.
    #[test]
    fn the_clock_and_date_follow_the_language() {
        let now = Local
            .with_ymd_and_hms(2026, 10, 28, 20, 31, 0)
            .single()
            .expect("дата");
        assert_eq!(big_clock(now), "20:31");
        assert_eq!(strip_clock(Language::Ru, now), "Ср, 28 окт 20:31");
        assert_eq!(strip_clock(Language::En, now), "Wed, 28 Oct 20:31");
        assert_eq!(date_line(Language::Ru, now), "Среда, 28 октября 2026");
        assert_eq!(date_line(Language::En, now), "Wednesday, October 28, 2026");
    }
}

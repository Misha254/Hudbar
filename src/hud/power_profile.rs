//! Профиль энергосбережения CPU: чтение, подпись и переключение по кругу.
//!
//! Раньше это был `power-cycle.sh` на полсотни строк: он читал тот же файл
//! `/sys`, звал `gdbus`, а результат проверял десятью чтениями подряд.
//! Здесь то же самое на Rust и без оболочки.
//!
//! Файл в `/sys` принадлежит root, поэтому писать напрямую нельзя — профиль
//! меняется через `org.freedesktop.UPower.PowerProfiles`, который это делает
//! через power-profiles-shim. Имя сервиса и путь объекта — стандартные,
//! набор профилей тоже: `power-saver`, `balanced`, `performance`.

use std::time::Duration;

/// Что отдаёт ядро в `energy_performance_preference` для каждого профиля.
const EPP_FILE: &str = "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference";
/// Значение, которое ядро пишет вместо имени профиля.
const EPP_BY_PROFILE: [(&str, &str); 3] = [
    ("performance", "performance"),
    ("balanced", "balance_performance"),
    ("power-saver", "power"),
];

/// Профили в порядке перебора: тот же круг, что был в скрипте.
pub const PROFILES: [&str; 3] = ["performance", "balanced", "power-saver"];

/// Есть ли чем управлять: без файла и без сервиса PowerProfiles переключать
/// нечего, и пункт меню должен скрыться, а не показывать ошибку по `Enter`.
pub fn supported() -> bool {
    std::path::Path::new(EPP_FILE).exists() && service_present()
}

fn service_present() -> bool {
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    let Ok(conn) = dbus::blocking::Connection::new_system() else {
        return false;
    };
    let proxy = dbus::blocking::Proxy::new(
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
        Duration::from_millis(250),
        &conn,
    );
    // Читаем текущий профиль: сервиса нет — чтения не будет, и пункт меню
    // скроется заранее, вместо ошибки по `Enter`.
    proxy
        .get::<String>("org.freedesktop.UPower.PowerProfiles", "ActiveProfile")
        .is_ok()
}

/// Текущий профиль по тому, что ядро пишет в `energy_performance_preference`:
/// файл не читается — профиль неизвестен, а не «энергосбережение».
pub fn current() -> Option<&'static str> {
    let epp = std::fs::read_to_string(EPP_FILE).ok()?;
    let epp = epp.trim();
    EPP_BY_PROFILE
        .iter()
        .find(|(_, value)| *value == epp)
        .map(|(profile, _)| *profile)
}

/// Следующий профиль по кругу. Чистая функция: тестируется без `/sys`.
pub fn next(current: &str) -> &'static str {
    match current {
        "performance" => "power-saver",
        "power-saver" => "balanced",
        _ => "performance",
    }
}

/// Подпись профиля для меню и уведомления.
pub fn label(profile: &str, ru: bool) -> &'static str {
    match (profile, ru) {
        ("performance", true) => "Производительность",
        ("performance", false) => "Performance",
        ("balanced", true) => "Сбалансированный",
        ("balanced", false) => "Balanced",
        ("power-saver", true) => "Энергосбережение",
        ("power-saver", false) => "Power saver",
        (_, true) => "Неизвестно",
        (_, false) => "Unknown",
    }
}

/// Текущий профиль словами; без файла — «неизвестно», а не молчание.
pub fn current_label(ru: bool) -> &'static str {
    label(current().unwrap_or(""), ru)
}

/// Поставить профиль через шим и дождаться, пока ядро его примет.
pub fn set(profile: &str) -> Result<(), String> {
    if !PROFILES.contains(&profile) {
        return Err(format!("неизвестный профиль {profile}"));
    }
    let conn = dbus::blocking::Connection::new_system().map_err(|error| error.to_string())?;
    let proxy = dbus::blocking::Proxy::new(
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
        Duration::from_millis(500),
        &conn,
    );
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    proxy
        .set(
            "org.freedesktop.UPower.PowerProfiles",
            "ActiveProfile",
            profile,
        )
        .map_err(|error| format!("PowerProfiles: {error}"))?;
    // Шим отвечает до того, как перепишет файл в `/sys`; без проверки меню
    // показало бы новый профиль при старой частоте.
    for _ in 0..10 {
        if current() == Some(profile) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("профиль {profile} не применился"))
}

/// Переключить по кругу и вернуть новый профиль.
pub fn cycle() -> Result<&'static str, String> {
    let next = next(current().unwrap_or(""));
    set(next)?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cycle_covers_three_profiles_without_sticking() {
        let mut seen = vec![next("")];
        for _ in 0..6 {
            seen.push(next(seen.last().expect("профиль")));
        }
        assert_eq!(
            seen,
            [
                "performance",
                "power-saver",
                "balanced",
                "performance",
                "power-saver",
                "balanced",
                "performance"
            ],
            "круг как в скрипте: performance → power-saver → balanced"
        );
        assert!(
            !seen.windows(2).any(|pair| pair[0] == pair[1]),
            "профиль не должен повторяться подряд: {seen:?}"
        );
    }

    #[test]
    fn every_profile_maps_to_a_kernel_value() {
        for profile in PROFILES {
            let epp = EPP_BY_PROFILE
                .iter()
                .find(|(name, _)| *name == profile)
                .map(|(_, value)| *value)
                .unwrap_or_default();
            assert!(!epp.is_empty(), "нет значения EPP для {profile}");
        }
        assert_eq!(EPP_BY_PROFILE.len(), PROFILES.len());
        // Подпись без профиля — «неизвестно», а не молчание и не выдумка.
        assert_eq!(label("", true), "Неизвестно");
        assert_eq!(label("", false), "Unknown");
    }

    #[test]
    fn unknown_profiles_are_refused_before_dbus() {
        let error = set("turbo").expect_err("чужой профиль не принимаем");
        assert!(error.contains("turbo"), "{error}");
    }
}

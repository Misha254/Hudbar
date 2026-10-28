//! Общий слой подписей: язык, месяцы, дни недели, единицы и форматы.
//!
//! Модуль подключается и в панель (`crate::hud::i18n`), и в отдельные окна,
//! где `settings` лежит у корня крейта. Поэтому путь к языку — `super::settings`:
//! он одинаково означает `hud::settings` и корневой `settings`.
//!
//! Здесь живёт только то, что нужно нескольким компонентам сразу. Подписи
//! конкретного окна остаются рядом с ним: меню строит дерево на языке, панель
//! берёт подписи из `app::strings`, и общий словарь на тысячи строк сделал бы
//! обе стороны хуже.

use chrono::{DateTime, Datelike, Local, NaiveDate, Timelike};

use super::settings::Language;

/// `true` для русского языка: короткая форма для ветвлений в разметке.
pub fn ru(lang: Language) -> bool {
    matches!(lang, Language::Ru)
}

/// Пара «русский / английский» без повторения `match` на каждом вызове.
pub fn pick(lang: Language, ru_text: &'static str, en_text: &'static str) -> &'static str {
    if ru(lang) { ru_text } else { en_text }
}

const MONTHS_NOM_RU: [&str; 12] = [
    "Январь",
    "Февраль",
    "Март",
    "Апрель",
    "Май",
    "Июнь",
    "Июль",
    "Август",
    "Сентябрь",
    "Октябрь",
    "Ноябрь",
    "Декабрь",
];
const MONTHS_NOM_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTHS_GEN_RU: [&str; 12] = [
    "января",
    "февраля",
    "марта",
    "апреля",
    "мая",
    "июня",
    "июля",
    "августа",
    "сентября",
    "октября",
    "ноября",
    "декабря",
];
const MONTHS_SHORT_RU: [&str; 12] = [
    "янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек",
];
const MONTHS_SHORT_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS_FULL_RU: [&str; 7] = [
    "Понедельник",
    "Вторник",
    "Среда",
    "Четверг",
    "Пятница",
    "Суббота",
    "Воскресенье",
];
const WEEKDAYS_FULL_EN: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const WEEKDAYS_SHORT_RU: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];
const WEEKDAYS_SHORT_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Название месяца в именительном падеже: «Октябрь».
pub fn month_nom(lang: Language, month: u32) -> &'static str {
    let index = (month.clamp(1, 12) - 1) as usize;
    match lang {
        Language::Ru => MONTHS_NOM_RU[index],
        Language::En => MONTHS_NOM_EN[index],
    }
}

/// Месяц в родительном падеже для полной даты: «28 октября 2026».
pub fn month_gen(lang: Language, month: u32) -> &'static str {
    let index = (month.clamp(1, 12) - 1) as usize;
    match lang {
        Language::Ru => MONTHS_GEN_RU[index],
        Language::En => MONTHS_NOM_EN[index],
    }
}

/// Короткий месяц для часов на баре: «окт» / «Oct».
pub fn month_short(lang: Language, month: u32) -> &'static str {
    let index = (month.clamp(1, 12) - 1) as usize;
    match lang {
        Language::Ru => MONTHS_SHORT_RU[index],
        Language::En => MONTHS_SHORT_EN[index],
    }
}

/// Полное имя дня недели; индекс считается от понедельника.
pub fn weekday_full(lang: Language, index: usize) -> &'static str {
    let index = index % 7;
    match lang {
        Language::Ru => WEEKDAYS_FULL_RU[index],
        Language::En => WEEKDAYS_FULL_EN[index],
    }
}

/// Короткое имя дня недели: «Пн» / «Mon».
pub fn weekday_short(lang: Language, index: usize) -> &'static str {
    let index = index % 7;
    match lang {
        Language::Ru => WEEKDAYS_SHORT_RU[index],
        Language::En => WEEKDAYS_SHORT_EN[index],
    }
}

/// Полная дата календаря: «Понедельник, 28 октября 2026» /
/// «Monday, October 28, 2026».
pub fn date_long(lang: Language, date: NaiveDate) -> String {
    let weekday = weekday_full(lang, date.weekday().num_days_from_monday() as usize);
    match lang {
        Language::Ru => format!(
            "{weekday}, {} {} {}",
            date.day(),
            month_gen(lang, date.month()),
            date.year()
        ),
        Language::En => format!(
            "{weekday}, {} {}, {}",
            month_nom(lang, date.month()),
            date.day(),
            date.year()
        ),
    }
}

/// Заголовок месяца в календаре: «Октябрь 2026».
pub fn month_title(lang: Language, year: i32, month: u32) -> String {
    format!("{} {year}", month_nom(lang, month))
}

/// Короткая дата в прогнозе погоды: «18.10» / «10/18».
pub fn short_date(lang: Language, date: NaiveDate) -> String {
    match lang {
        Language::Ru => format!("{:02}.{:02}", date.day(), date.month()),
        Language::En => format!("{:02}/{:02}", date.month(), date.day()),
    }
}

/// Часы на баре: «Вт, 28 окт 20:31» / «Tue, 28 Oct 20:31».
pub fn clock_bar(lang: Language, now: DateTime<Local>) -> String {
    format!(
        "{}, {:02} {} {:02}:{:02}",
        weekday_short(lang, now.weekday().num_days_from_monday() as usize),
        now.day(),
        month_short(lang, now.month()),
        now.hour(),
        now.minute()
    )
}

/// Остаток времени батареи: «2ч15м» / «2h15m», без часов — «15м» / «15m».
pub fn duration_hm(lang: Language, hours: u64, minutes: u64) -> String {
    match lang {
        Language::Ru if hours > 0 => format!("{hours}ч{minutes:02}м"),
        Language::Ru => format!("{minutes}м"),
        Language::En if hours > 0 => format!("{hours}h{minutes:02}m"),
        Language::En => format!("{minutes}m"),
    }
}

/// Аптайм без секунд: «10 д 11 ч» / «10 d 11 h», «1 м» / «1 m».
pub fn uptime(lang: Language, seconds: Option<f64>) -> Option<String> {
    let seconds = seconds?;
    let minutes = (seconds / 60.0).floor() as u64;
    if minutes == 0 {
        return None;
    }
    let hours = minutes / 60;
    let days = hours / 24;
    let (day, hour, minute) = match lang {
        Language::Ru => ("д", "ч", "м"),
        Language::En => ("d", "h", "m"),
    };
    let mut units: Vec<String> = Vec::new();
    if days > 0 {
        units.push(format!("{days} {day}"));
    }
    let hours = hours % 24;
    if hours > 0 {
        units.push(format!("{hours} {hour}"));
    }
    let minutes = minutes % 60;
    if minutes > 0 || units.is_empty() {
        units.push(format!("{minutes} {minute}"));
    }
    Some(units.join(" "))
}

/// Объём памяти: «15.6 ГиБ» / «15.6 GiB».
pub fn gib(lang: Language, value: f64) -> String {
    match lang {
        Language::Ru => format!("{value:.1} ГиБ"),
        Language::En => format!("{value:.1} GiB"),
    }
}

/// Единица скорости ветра для карточки погоды: «м/с» / «m/s».
pub fn wind_unit(lang: Language) -> &'static str {
    pick(lang, "м/с", "m/s")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_follow_the_language_and_case() {
        assert_eq!(month_nom(Language::Ru, 10), "Октябрь");
        assert_eq!(month_nom(Language::En, 10), "October");
        assert_eq!(month_gen(Language::Ru, 10), "октября");
        assert_eq!(month_gen(Language::En, 10), "October");
        assert_eq!(month_short(Language::Ru, 10), "окт");
        assert_eq!(month_short(Language::En, 10), "Oct");
        // Номер месяца за границей файла не должен паниковать.
        assert_eq!(month_nom(Language::Ru, 0), "Январь");
        assert_eq!(month_nom(Language::Ru, 13), "Декабрь");
    }

    #[test]
    fn dates_use_the_natural_order_of_the_language() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 28).expect("дата");
        assert_eq!(date_long(Language::Ru, date), "Среда, 28 октября 2026");
        assert_eq!(date_long(Language::En, date), "Wednesday, October 28, 2026");
        assert_eq!(month_title(Language::Ru, 2026, 10), "Октябрь 2026");
        assert_eq!(month_title(Language::En, 2026, 10), "October 2026");
        assert_eq!(short_date(Language::Ru, date), "28.10");
        assert_eq!(short_date(Language::En, date), "10/28");
    }

    #[test]
    fn weekday_names_are_short_and_full() {
        assert_eq!(weekday_full(Language::Ru, 0), "Понедельник");
        assert_eq!(weekday_full(Language::En, 0), "Monday");
        assert_eq!(weekday_short(Language::Ru, 0), "Пн");
        assert_eq!(weekday_short(Language::En, 0), "Mon");
    }

    #[test]
    fn durations_and_units_use_language_letters() {
        assert_eq!(duration_hm(Language::Ru, 2, 15), "2ч15м");
        assert_eq!(duration_hm(Language::En, 2, 15), "2h15m");
        assert_eq!(duration_hm(Language::Ru, 0, 7), "7м");
        assert_eq!(duration_hm(Language::En, 0, 7), "7m");
        assert_eq!(
            uptime(Language::Ru, Some(90_000.0)).as_deref(),
            Some("1 д 1 ч")
        );
        assert_eq!(
            uptime(Language::En, Some(90_000.0)).as_deref(),
            Some("1 d 1 h")
        );
        assert_eq!(uptime(Language::Ru, None), None);
        assert_eq!(gib(Language::Ru, 15.6), "15.6 ГиБ");
        assert_eq!(gib(Language::En, 15.6), "15.6 GiB");
        assert_eq!(wind_unit(Language::Ru), "м/с");
        assert_eq!(wind_unit(Language::En), "m/s");
    }

    #[test]
    fn bar_clock_is_localized() {
        use chrono::TimeZone;
        let now = Local.with_ymd_and_hms(2026, 10, 28, 20, 31, 0).unwrap();
        assert_eq!(clock_bar(Language::Ru, now), "Ср, 28 окт 20:31");
        assert_eq!(clock_bar(Language::En, now), "Wed, 28 Oct 20:31");
    }
}

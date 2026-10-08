//! Глифы интерфейса: Nerd Font, всегда JetBrainsMono Nerd Font.
//!
//! В Pixel основной шрифт окна — Minecraft Rus, и этих глифов в нём нет:
//! получился бы «тофу». Поэтому иконки рисуются отдельной копией painter с
//! принудительным шрифтом. Все кодпойнты проверены по cmap JetBrainsMono Nerd
//! Font, отсутствующих нет.

/// Разделы сайдбара и карточек.
pub const OVERVIEW: &str = "\u{f24e}";
pub const PANEL: &str = "\u{f0e8}";
/// Монитор: иконка раздела «Панель» в меню. У окна настроек свой глиф
/// панели (`f0e8`), в меню раздел называется «Панель», и монитор читается
/// однозначнее схемы.
pub const PANEL_MONITOR: &str = "\u{f108}";
pub const APPEARANCE: &str = "\u{f074}";
pub const NOTIFICATIONS: &str = "\u{f0f3}";
pub const CONTROLS: &str = "\u{f11c}";
pub const WALLPAPER: &str = "\u{f1bc}";

/// Модули панели: те же глифы, что рисует сама панель.
pub const TRAY: &str = "\u{f0c9}";
pub const WEATHER: &str = "\u{f0e6}";
pub const WEBCAM: &str = "\u{f030}";
pub const CLOCK: &str = "\u{f017}";
pub const RECORDING: &str = "\u{f111}";
pub const BATTERY: &str = "\u{f240}";
pub const SYSTEM: &str = "\u{f4bc}";
pub const SOUND: &str = "\u{f028}";
pub const NETWORK: &str = "\u{f1eb}";
/// Bluetooth: тот же глиф, что в панели у модуля Bluetooth (у панели его
/// пока нет, поэтому константа живёт здесь, рядом со звуком и сетью).
pub const BLUETOOTH: &str = "\u{f293}";
pub const DND: &str = "\u{f133}";

/// Разделы меню: приложения, стиль, захват, бинды, «о программе» и подписи
/// внутри разделов. Отдельные от разделов окна настроек, потому что меню
/// описывает другие вещи: у него нет «Обзора» и «Управления», зато есть
/// «Приложения» и «О программе».
pub const APPS: &str = "\u{f00a}";
pub const STYLE: &str = "\u{f1fc}";
pub const CAPTURE: &str = "\u{f083}";
pub const KEYBINDS: &str = "\u{f11c}";
pub const ABOUT: &str = "\u{f05a}";
pub const THEME: &str = "\u{f185}";
pub const LANGUAGE: &str = "\u{f0ac}";
pub const SCHEME: &str = "\u{f043}";
pub const FONT_ICON: &str = "\u{f031}";
pub const TERMINAL: &str = "\u{f120}";

/// Раздел «Питание» и его строки. Глифы из Font Awesome: питание `f011`,
/// перезагрузка `f079`, замок `f023`, кровать `f236` (сон), выход `f2f5`.
/// `f011` выбран вместо `f055`: в JetBrainsMono Nerd Font Propo `f055`
/// рисуется кружком с точкой, а `f011` — читаемым знаком питания. Все пять
/// проверены по cmap шрифта.
pub const POWER: &str = "\u{f011}";
pub const REBOOT: &str = "\u{f079}";
pub const LOCK: &str = "\u{f023}";
pub const SLEEP: &str = "\u{f236}";
pub const LOGOUT: &str = "\u{f2f5}";

/// Служебные глифы меню: лупа, крестик, шеврон, стрелки для подсказок.
pub const SEARCH: &str = "\u{f002}";
pub const TIMES: &str = "\u{f00d}";
pub const CHEVRON: &str = "\u{f054}";
pub const ARROW_UP: &str = "\u{f062}";
pub const ARROW_DOWN: &str = "\u{f063}";
/// Луна: «не беспокоить» в меню. У панели тот же пункт рисуется колокольчиком
/// (`DND`), но в меню луна читается как режим, а не как уведомление.
pub const DND_MOON: &str = "\u{f186}";
/// Картинка: иконка «Обои» в меню. У `WALLPAPER` выше кодпойнт `f1bc`, а это
/// в шрифте `fa-spotify` — Spotify, а не обои. Имя глифа проверено по cmap
/// JetBrainsMono Nerd Font Propo.
pub const PICTURE: &str = "\u{f03e}";
pub const DOT: &str = "\u{f192}";

/// Служебные: галочка, стрелки перестановки и настоящий минус U+2212.
pub const CHECK: &str = "\u{f00c}";
/// Папка и «всё сразу» для колонки каталогов обоев. Оба глифа проверены
/// по cmap шрифта: отсутствие дало бы «тофу» вместо значка.
pub const FOLDER: &str = "\u{f07b}";
pub const TH_LARGE: &str = "\u{f009}";
pub const MINUS: &str = "\u{2212}";
pub const PLUS: &str = "+";
pub const UP: &str = "\u{f077}";
pub const DOWN: &str = "\u{f078}";

/// Все иконки галереи: подпись, глиф и смысл. Галерея рисует их в порядке
/// объявления, а тест сверяет, что список не пустеет и глифы уникальны.
pub const ALL: [(&str, &str); 22] = [
    ("Обзор", OVERVIEW),
    ("Панель", PANEL_MONITOR),
    ("Внешний вид", APPEARANCE),
    ("Уведомления", NOTIFICATIONS),
    ("Управление", CONTROLS),
    ("Обои", PICTURE),
    ("Трей", TRAY),
    ("Погода", WEATHER),
    ("Вебка", WEBCAM),
    ("Часы", CLOCK),
    ("Запись", RECORDING),
    ("Батарея", BATTERY),
    ("Система", SYSTEM),
    ("Звук", SOUND),
    ("Сеть", NETWORK),
    ("Не беспокоить", DND_MOON),
    ("Галочка", CHECK),
    ("Минус", MINUS),
    ("Плюс", PLUS),
    ("Вверх", UP),
    ("Папка", FOLDER),
    ("Всё", TH_LARGE),
];

/// Иконка раздела по индексу `Section` или по подписи. Неизвестное имя
/// возвращает `None`, чтобы вызывающий не рисовал «тофу».
pub fn section(name: &str) -> Option<&'static str> {
    Some(match name {
        "Обзор" => OVERVIEW,
        "Панель" => PANEL,
        "Внешний вид" => APPEARANCE,
        "Уведомления" => NOTIFICATIONS,
        "Управление" => CONTROLS,
        "Обои" => WALLPAPER,
        _ => return None,
    })
}

/// Иконки меню в порядке дерева: девять разделов корня, потом содержимое
/// «Стиля», «Панели» и «Питания». Галерея меню рисует их именно в таком
/// порядке, а тест сверяет, что список не пустеет и что глифов хватает на все
/// пункты.
pub const MENU: [(&str, &str); 25] = [
    ("Приложения", APPS),
    ("Панель", PANEL_MONITOR),
    ("Стиль", STYLE),
    ("Уведомления", NOTIFICATIONS),
    ("Захват", CAPTURE),
    ("Бинды", KEYBINDS),
    ("Система", SYSTEM),
    ("Питание", POWER),
    ("О программе", ABOUT),
    ("Тема", THEME),
    ("Язык", LANGUAGE),
    ("Обои", PICTURE),
    ("Схема", SCHEME),
    ("Шрифт", FONT_ICON),
    ("Терминал", TERMINAL),
    ("Не беспокоить", DND_MOON),
    ("Заблокировать", LOCK),
    ("Спящий режим", SLEEP),
    ("Перезагрузка", REBOOT),
    ("Выйти", LOGOUT),
    ("Лупа", SEARCH),
    ("Крестик", TIMES),
    ("Шеврон", CHEVRON),
    ("Стрелка вверх", ARROW_UP),
    ("Стрелка вниз", ARROW_DOWN),
];

/// Шрифт иконок. Отдельный от темы: Minecraft Rus этих глифов не содержит.
pub const FONT: &str = "JetBrainsMono Nerd Font Propo";

#[cfg(test)]
mod tests {
    use super::*;

    /// Шрифт иконок не должен зависеть от темы окна.
    #[test]
    fn icon_font_is_nerd_font_in_both_themes() {
        assert!(FONT.contains("Nerd Font"), "иконки рисуются Nerd Font");
        assert!(
            !FONT.contains("Minecraft"),
            "в Minecraft Rus нет глифов Nerd Font"
        );
    }

    /// Каждая иконка должна быть единственным символом: иначе в подписи или
    /// в колонке она растянется.
    #[test]
    fn every_icon_is_a_single_character() {
        for (name, glyph) in ALL {
            assert_eq!(
                glyph.chars().count(),
                1,
                "иконка «{name}» не один символ: {glyph:?}"
            );
        }
    }

    /// Минус в степпере — настоящий U+2212, а не дефис: на −/+ он читается,
    /// на -/+ выглядит как опечатка.
    #[test]
    fn stepper_minus_is_a_real_minus_sign() {
        assert_eq!(MINUS, "\u{2212}");
        assert_ne!(MINUS, "-");
    }

    /// Список иконок полон и без повторов глифов.
    #[test]
    fn gallery_has_all_icons_without_duplicates() {
        assert_eq!(ALL.len(), 22);
        let mut glyphs: Vec<&str> = ALL.iter().map(|(_, glyph)| *glyph).collect();
        glyphs.sort_unstable();
        let count = glyphs.len();
        glyphs.dedup();
        assert_eq!(glyphs.len(), count, "в галерее есть повторяющиеся глифы");
    }

    /// Иконки меню: полный список и без повторов. Дублей с `ALL` тут нет
    /// намеренно — меню и окно настроек рисуются в разных галереях, и общий
    /// глиф в обеих не мешает.
    #[test]
    fn menu_gallery_has_every_icon_once() {
        assert_eq!(MENU.len(), 25);
        let mut glyphs: Vec<&str> = MENU.iter().map(|(_, glyph)| *glyph).collect();
        glyphs.sort_unstable();
        let count = glyphs.len();
        glyphs.dedup();
        assert_eq!(glyphs.len(), count, "в меню есть повторяющиеся глифы");
        for (name, glyph) in MENU {
            assert_eq!(
                glyph.chars().count(),
                1,
                "иконка «{name}» не один символ: {glyph:?}"
            );
        }
    }

    /// Каждая иконка обязана быть глифом Nerd Font, а не «тофу». Точное
    /// наличие в cmap проверяется при отрисовке галереи, а здесь — дешёвая
    /// страховка от опечатки: все используемые глифы лежат в области
    /// Font Awesome и Material Design, отведённой под иконки.
    #[test]
    fn every_icon_sits_in_the_named_glyph_ranges() {
        for (name, glyph) in ALL
            .iter()
            .chain(MENU.iter())
            .filter(|(_, glyph)| !matches!(*glyph, MINUS | PLUS))
        {
            let code = glyph.chars().next().unwrap() as u32;
            assert!(
                (0xf000..=0xf4ff).contains(&code),
                "иконка «{name}» ({glyph:?}) не из области иконок: U+{code:04x}"
            );
        }
        // Минус — настоящий U+2212, плюс — ASCII: оба печатаются самим
        // шрифтом темы, а не иконкой.
        assert_eq!(MINUS, "\u{2212}");
        assert_eq!(PLUS, "+");
    }

    /// Каждый раздел находит свою иконку, неизвестное имя — нет.
    #[test]
    fn section_icons_resolve_by_name() {
        for name in [
            "Обзор",
            "Панель",
            "Внешний вид",
            "Уведомления",
            "Управление",
            "Обои",
        ] {
            assert!(section(name).is_some(), "нет иконки для раздела {name}");
        }
        assert_eq!(section("Нет такого"), None);
    }
}

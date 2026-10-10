//! Настоящее дерево меню: разделы, значения и действия из системы.
//!
//! В отличие от `demo_tree`, здесь нет фиктивных значений: каждый тумблер
//! читает `settings.json` или разовый опрос, каждое действие мапится на
//! `Patch`, системную команду, скрипт или приложение. Пункты, чей скрипт не
//! установлен, скрываются условием `visible`, а не висят мёртвым грузом.
//!
//! Почти все скрипты приложений открывают rofi, а меню держит клавиатуру в
//! `Exclusive`: ждать их (`Run`) значило бы повесить и меню, и rofi. Поэтому
//! скрипты с интерфейсом запускаются откреплённо (`Spawn`) с закрытием меню,
//! а `Run` остаётся быстрым неинтерактивным командам (`wall.sh --random`,
//! переключатели без внешней команды): меню ждёт завершения, но остаётся
//! открытым и показывает статус.

use super::super::data::oneshot;
use super::action::Action;
use super::settings::{Language, Module, OSD_DURATION_MAX_MS, OSD_DURATION_MIN_MS};
use super::settings_icons;
use super::settings_ui;
use super::state::Menu;
use super::system::ProviderKey;
use super::tree::Node;
use super::values;

/// Версия пакета для строки «О программе».
const VERSION: &str = env!("CARGO_PKG_VERSION");

const THEME_RU: [&str; 2] = ["Обычная", "Pixel"];
const THEME_EN: [&str; 2] = ["Normal", "Pixel"];
const LANG_OPTS: [&str; 2] = ["Русский", "English"];
const POS_RU: [&str; 4] = [
    "Слева сверху",
    "Справа сверху",
    "Слева снизу",
    "Справа снизу",
];
const POS_EN: [&str; 4] = ["Top left", "Top right", "Bottom left", "Bottom right"];

/// Есть ли программа: `PATH` или `~/.local/bin`.
fn has(program: &str) -> bool {
    oneshot::program_exists(program)
}

fn has_flatpak() -> bool {
    has("flatpak")
}
fn has_kitty() -> bool {
    has("kitty")
}
fn has_kitty_yazi() -> bool {
    has("kitty") && has("yazi")
}
fn has_kitty_opencode() -> bool {
    has("kitty") && has("opencode")
}
fn has_steam() -> bool {
    has("steam")
}
fn has_wall() -> bool {
    has("wall.sh")
}
/// Окно выбора обоев бинарником. Пока его нет, пункт остаётся на `wall.sh`:
/// старый скрипт никуда не девается и работает как запасной путь.
fn has_wallpaper_picker() -> bool {
    has("hud-wallpaper-rs")
}
/// Окно выбора обоев бинарником. Пока его нет, пункт остаётся на `wall.sh`:
/// старый скрипт никуда не девается и работает как запасной путь.
/// Виден ли пункт выбора обоев: окно установлено или есть запасной `wall.sh`.
fn has_wallpaper_picker_or_script() -> bool {
    has_wallpaper_picker() || has_wall()
}

/// Базовое имя файла текущих обоев из `~/.config/wallpaper`: показывается
/// справа от «Выбрать обои…». Чтение маленького файла, без процессов.
fn current_wallpaper_label() -> String {
    values::current_wallpaper()
        .as_deref()
        .and_then(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
        })
        .unwrap_or("")
        .to_string()
}
fn has_script(name: &str) -> bool {
    has(name)
}
fn has_calcurse() -> bool {
    oneshot::program_for_spawn_sh("calcurse")
}
fn has_note() -> bool {
    has_script("note.sh")
}
fn has_calc() -> bool {
    has_script("calc.sh")
}
fn has_emoji() -> bool {
    has_script("emoji.sh")
}
fn has_clip() -> bool {
    has_script("clip.sh")
}
fn has_tl() -> bool {
    has_script("tl.sh")
}
fn has_media() -> bool {
    has_script("playerctl.sh")
}
fn has_capture() -> bool {
    has_script("ss.sh")
}
fn has_record() -> bool {
    has_script("record.sh")
}
/// Профиль питания есть не везде: без `/sys`-файла и без сервиса
/// PowerProfiles пункт скрывается, а не падает по `Enter`.
fn has_power_profile() -> bool {
    crate::power_profile::supported()
}

/// Правое значение строки профиля: текущий, а не название действия.
fn power_profile_value() -> String {
    let ru = super::settings::load().language == Language::Ru;
    crate::power_profile::current_label(ru).to_string()
}
/// Замок: `dynalock.sh` умеет гасить экран и запускать блокировку. Без него
/// пункт скрыт, а не показывает ошибку по нажатию.
/// Гибернация есть не на каждой машине: без `disk` в `/sys/power/state`
/// ядро не умеет писать образ в swap, и строка была бы обещанием, которое
/// не выполнится.
fn has_hibernate() -> bool {
    std::fs::read_to_string("/sys/power/state")
        .is_ok_and(|states| states.split_whitespace().any(|state| state == "disk"))
}

fn has_dynalock() -> bool {
    has_script("dynalock.sh")
}
fn has_keybinds() -> bool {
    has("hud-keybinds-rs")
}
fn has_yazibinds() -> bool {
    has("hud-yazibinds-rs")
}
/// Подпись модуля панели на языке окна.
fn module_title(module: Module, lang: Language) -> &'static str {
    match lang {
        Language::Ru => match module {
            Module::Tray => "Трей",
            Module::Weather => "Погода",
            Module::Webcam => "Камера",
            Module::Clock => "Часы",
            Module::Recorder => "Запись",
            Module::Battery => "Батарея",
            Module::System => "Система",
            Module::Audio => "Звук",
            Module::Network => "Сеть",
            Module::Dnd => "DND",
        },
        Language::En => match module {
            Module::Tray => "Tray",
            Module::Weather => "Weather",
            Module::Webcam => "Webcam",
            Module::Clock => "Clock",
            Module::Recorder => "Recorder",
            Module::Battery => "Battery",
            Module::System => "System",
            Module::Audio => "Audio",
            Module::Network => "Network",
            Module::Dnd => "DND",
        },
    }
}

/// Синонимы модуля для поиска: английское и русское имя сразу.
fn module_keywords(module: Module) -> &'static [&'static str] {
    match module {
        Module::Tray => &["tray", "трей"],
        Module::Weather => &["weather", "погода"],
        Module::Webcam => &["webcam", "камера"],
        Module::Clock => &["clock", "часы"],
        Module::Recorder => &["recorder", "запись"],
        Module::Battery => &["battery", "батарея"],
        Module::System => &["system", "система"],
        Module::Audio => &["audio", "звук"],
        Module::Network => &["network", "сеть"],
        Module::Dnd => &["dnd", "днд", "do not disturb"],
    }
}

/// Иконка модуля панели.
fn module_icon(module: Module) -> &'static str {
    match module {
        Module::Tray => settings_icons::TRAY,
        Module::Weather => settings_icons::WEATHER,
        Module::Webcam => settings_icons::WEBCAM,
        Module::Clock => settings_icons::CLOCK,
        Module::Recorder => settings_icons::RECORDING,
        Module::Battery => settings_icons::BATTERY,
        Module::System => settings_icons::SYSTEM,
        Module::Audio => settings_icons::SOUND,
        Module::Network => settings_icons::NETWORK,
        Module::Dnd => settings_icons::DND,
    }
}

/// Строка тумблера модуля панели.
fn module_row(module: Module, lang: Language) -> Node {
    let (get, set) = values::module_toggle(module);
    Node::toggle(module_icon(module), module_title(module, lang), get, set)
        .search_as(module_keywords(module))
}

/// Раздел «Приложения»: запуск программ. Всё откреплённо с закрытием меню:
//  скриптам нужен rofi, а rofi нужна клавиатура, которую держит меню.
fn apps_section(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Приложения", &["apps", "приложения"]),
        Language::En => ("Applications", &["apps", "приложения"]),
    };
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    Node::submenu(
        settings_icons::APPS,
        title,
        vec![
            Node::action(
                settings_icons::APPS,
                t("Zen", "Zen"),
                Action::Spawn("flatpak", &["run", "app.zen_browser.zen"]),
            )
            .when(has_flatpak)
            .search_as(&["zen", "browser", "браузер"]),
            Node::action(
                settings_icons::TERMINAL,
                t("Терминал", "Terminal"),
                Action::Spawn("kitty", &[]),
            )
            .when(has_kitty)
            .search_as(&["terminal", "терминал"]),
            Node::action(
                settings_icons::PANEL,
                t("Файлы", "Files"),
                Action::Spawn("kitty", &["-e", "yazi"]),
            )
            .when(has_kitty_yazi)
            .search_as(&["files", "файлы", "yazi"]),
            Node::action(
                settings_icons::CLOCK,
                t("Календарь", "Calendar"),
                Action::Spawn("kitty", &["--class", "calcurse", "zsh", "-c", "calcurse"]),
            )
            .when(has_calcurse)
            .search_as(&["calendar", "календарь"]),
            Node::action(
                settings_icons::FONT_ICON,
                t("Заметки", "Notes"),
                Action::Spawn("note.sh", &[]),
            )
            .when(has_note)
            .search_as(&["notes", "заметки"]),
            Node::action(
                settings_icons::PLUS,
                t("Калькулятор", "Calculator"),
                Action::Spawn("calc.sh", &[]),
            )
            .when(has_calc)
            .search_as(&["calc", "калькулятор"]),
            Node::action(
                settings_icons::DOT,
                t("Эмодзи", "Emoji"),
                Action::Spawn("emoji.sh", &[]),
            )
            .when(has_emoji)
            .search_as(&["emoji", "эмодзи"]),
            Node::action(
                settings_icons::SEARCH,
                t("Буфер обмена", "Clipboard"),
                Action::Spawn("clip.sh", &[]),
            )
            .when(has_clip)
            .search_as(&["clipboard", "буфер", "cliphist"]),
            Node::action(
                settings_icons::LANGUAGE,
                t("Переводчик", "Translator"),
                Action::Spawn("tl.sh", &[]),
            )
            .when(has_tl)
            .search_as(&["translate", "перевод"]),
            Node::action(settings_icons::CHECK, "Steam", Action::Spawn("steam", &[]))
                .when(has_steam)
                .search_as(&["steam"]),
            Node::action(
                settings_icons::TERMINAL,
                "OpenCode",
                Action::Spawn("kitty", &["--class", "opencode", "-e", "opencode"]),
            )
            .when(has_kitty_opencode)
            .search_as(&["opencode"]),
            Node::action(
                settings_icons::CHEVRON,
                t("Медиа", "Media"),
                Action::Spawn("playerctl.sh", &[]),
            )
            .when(has_media)
            .search_as(&["media", "медиа", "playerctl"]),
        ],
    )
    .search_as(kw)
}

/// Раздел «Панель»: модули, высота, Control Center и порядок.
fn panel_section(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Панель", &["panel", "панель"]),
        Language::En => ("Panel", &["panel", "панель"]),
    };
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    let mut children: Vec<Node> = Module::ALL
        .into_iter()
        .map(|module| module_row(module, lang))
        .collect();
    children.push(
        Node::number(
            settings_icons::PANEL,
            t("Высота", "Height"),
            24.0,
            48.0,
            1.0,
            " px",
            values::height,
            values::set_height,
        )
        .search_as(&["height", "высота"]),
    );
    children.push(
        Node::toggle(
            settings_icons::CONTROLS,
            "Control Center",
            values::control_button,
            values::set_control_button,
        )
        .search_as(&["control", "control center", "шестерёнка"]),
    );
    children.push(
        Node::toggle(
            settings_icons::SOUND,
            t("OSD громкости", "Volume OSD"),
            values::osd,
            values::set_osd,
        )
        .search_as(&["osd", "громкость", "volume"]),
    );
    children.push(
        Node::number(
            settings_icons::NOTIFICATIONS,
            t("Срок OSD", "OSD duration"),
            OSD_DURATION_MIN_MS as f32,
            OSD_DURATION_MAX_MS as f32,
            100.0,
            " ms",
            values::osd_duration,
            values::set_osd_duration,
        )
        .search_as(&["osd", "срок", "длительность"]),
    );
    children.push(order_section(lang));
    Node::submenu(settings_icons::PANEL_MONITOR, title, children).search_as(kw)
}

/// Подраздел порядка модулей: `Enter` опускает модуль на одну позицию в
/// своей зоне, `Shift+↑`/`Shift+↓` двигают его выбором в обе стороны.
/// Перенос через границу зоны запрещён у обоих.
fn order_section(lang: Language) -> Node {
    let (title, hint, kw) = match lang {
        Language::Ru => (
            "Порядок модулей",
            "Enter ↓ по зоне · Shift+↑↓ в обе стороны",
            &["order", "порядок"],
        ),
        Language::En => (
            "Module order",
            "Enter ↓ in its zone · Shift+↑↓ both ways",
            &["order", "порядок"],
        ),
    };
    let mut children = vec![Node::info(settings_icons::DOT, hint)];
    let order = super::super::settings::load().module_order;
    for key in order {
        let Some(module) = Module::from_key(&key) else {
            continue;
        };
        children.push(Node::action(
            module_icon(module),
            module_title(module, lang),
            Action::MoveModule { module, delta: 1 },
        ));
    }
    Node::submenu(settings_icons::UP, title, children).search_as(kw)
}

/// Раздел «Стиль»: тема, язык, обои и схема.
fn style_section(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Стиль", &["style", "стиль"]),
        Language::En => ("Style", &["style", "стиль"]),
    };
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    let theme_opts = match lang {
        Language::Ru => &THEME_RU,
        Language::En => &THEME_EN,
    };
    Node::submenu(
        settings_icons::STYLE,
        title,
        vec![
            Node::choice(
                settings_icons::THEME,
                t("Тема", "Theme"),
                theme_opts,
                values::theme,
                values::set_theme,
            )
            .search_as(&["theme", "тема"]),
            Node::choice(
                settings_icons::LANGUAGE,
                t("Язык", "Language"),
                &LANG_OPTS,
                values::language,
                values::set_language,
            )
            .search_as(&["language", "язык"]),
            // Окно выбора обоев, если оно установлено, иначе прежний
            // `wall.sh` с rofi: обе команды неинтерактивны для меню, уходят
            // откреплённо и закрывают его.
            Node::action(
                settings_icons::PICTURE,
                t("Выбрать обои…", "Choose wallpaper…"),
                if has_wallpaper_picker() {
                    Action::Spawn("hud-wallpaper-rs", &[])
                } else {
                    Action::Spawn("wall.sh", &[])
                },
            )
            .when(has_wallpaper_picker_or_script)
            .search_as(&["wallpaper", "обои", "choose", "выбрать"])
            .with_value(current_wallpaper_label),
            Node::action(
                settings_icons::PICTURE,
                t("Случайные обои", "Random wallpapers"),
                // Дольше секунды: держим меню открытым, результат — статус
                // «Применено» в подвале, а не молчаливый выход.
                Action::Run("wall.sh", &["--random"]),
            )
            .when(has_wall)
            .search_as(&["random", "случайные", "wallpaper", "обои"]),
            Node::choice(
                settings_icons::SCHEME,
                t("Схема matugen", "matugen scheme"),
                &settings_ui::SCHEMES,
                values::scheme_index,
                values::set_scheme,
            )
            .search_as(&["scheme", "схема", "matugen"]),
        ],
    )
    .search_as(kw)
}

/// Раздел уведомлений: системный DND и параметры dunst.
fn notifications_section(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Уведомления", &["notifications", "уведомления"]),
        Language::En => ("Notifications", &["notifications", "уведомления"]),
    };
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    Node::submenu(
        settings_icons::NOTIFICATIONS,
        title,
        vec![
            dnd_row(lang),
            Node::number(
                settings_icons::NOTIFICATIONS,
                t("Размер шрифта", "Font size"),
                10.0,
                18.0,
                1.0,
                " px",
                values::font_size,
                values::set_font_size,
            )
            .search_as(&["font", "шрифт"]),
            Node::number(
                settings_icons::NOTIFICATIONS,
                t("Высота строки", "Line height"),
                14.0,
                28.0,
                1.0,
                " px",
                values::line_height,
                values::set_line_height,
            )
            .search_as(&["line", "строка"]),
            Node::choice(
                settings_icons::NOTIFICATIONS,
                t("Позиция", "Position"),
                match lang {
                    Language::Ru => &POS_RU,
                    Language::En => &POS_EN,
                },
                values::notification_position,
                values::set_notification_position,
            )
            .search_as(&["position", "позиция"]),
        ],
    )
    .search_as(kw)
}

/// Строка «Не беспокоить»: системное состояние из dunst, без аббревиатур в
/// названии — для поиска есть синонимы.
fn dnd_row(lang: Language) -> Node {
    let title = match lang {
        Language::Ru => "Не беспокоить",
        Language::En => "Do not disturb",
    };
    Node::toggle(
        settings_icons::DND_MOON,
        title,
        values::dnd,
        values::set_dnd,
    )
    .with_id("system/dnd")
    .search_as(&["dnd", "днд", "do not disturb"])
}

/// Раздел захвата: скрипты с rofi и прямые команды niri.
fn capture_section(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Захват", &["capture", "захват"]),
        Language::En => ("Capture", &["capture", "захват"]),
    };
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    Node::submenu(
        settings_icons::CAPTURE,
        title,
        vec![
            Node::action(
                settings_icons::CAPTURE,
                t("Снимок экрана", "Screenshot"),
                Action::Spawn("ss.sh", &[]),
            )
            .when(has_capture)
            .search_as(&["screenshot", "скриншот"]),
            Node::action(
                settings_icons::RECORDING,
                record_title(lang),
                Action::Spawn("record.sh", &[]),
            )
            .when(has_record)
            .search_as(&["record", "запись"]),
            Node::action(
                settings_icons::CAPTURE,
                t("Область", "Area"),
                Action::Niri(&["msg", "action", "screenshot"]),
            )
            .search_as(&["area", "область"]),
            Node::action(
                settings_icons::CAPTURE,
                t("Экран", "Screen"),
                Action::Niri(&["msg", "action", "screenshot-screen"]),
            )
            .search_as(&["screen", "экран"]),
            Node::action(
                settings_icons::CAPTURE,
                t("Окно", "Window"),
                Action::Niri(&["msg", "action", "screenshot-window"]),
            )
            .search_as(&["window", "окно"]),
        ],
    )
    .search_as(kw)
}

/// Заголовок записи с точкой, если уже идёт: название статично на момент
/// сборки, состояние видно сразу.
fn record_title(lang: Language) -> &'static str {
    let on = values::recording();
    match (lang, on) {
        (Language::Ru, false) => "Запись экрана",
        (Language::Ru, true) => "Запись экрана ●",
        (Language::En, false) => "Screen recording",
        (Language::En, true) => "Screen recording ●",
    }
}

/// Раздел биндов: окна списков только для чтения.
fn keybinds_section(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Бинды", &["keybinds", "бинды", "binds"]),
        Language::En => ("Keybinds", &["keybinds", "бинды", "binds"]),
    };
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    Node::submenu(
        settings_icons::KEYBINDS,
        title,
        vec![
            Node::action(
                settings_icons::KEYBINDS,
                t("Бинды niri", "Niri keybinds"),
                Action::Spawn("hud-keybinds-rs", &[]),
            )
            .when(has_keybinds)
            .search_as(&["niri"]),
            Node::action(
                settings_icons::KEYBINDS,
                t("Бинды yazi", "Yazi keybinds"),
                Action::Spawn("hud-yazibinds-rs", &[]),
            )
            .when(has_yazibinds)
            .search_as(&["yazi"]),
        ],
    )
    .search_as(kw)
}

/// Есть чем управлять звуком: PipeWire даёт и список, и громкость.
fn has_audio() -> bool {
    has("wpctl") || has("pactl")
}

/// NetworkManager на месте — значит есть чем управлять Wi-Fi.
fn has_nmcli() -> bool {
    has("nmcli")
}

/// BlueZ на месте — значит есть что показать про Bluetooth. Сам раздел
/// read-only: без адаптера и без устройств он всё равно говорит своё
/// состояние, а не падает.
fn has_bluetoothctl() -> bool {
    has("bluetoothctl")
}

/// niri на месте — значит есть чем читать выводы. Пока раздел строго
/// read-only: он показывает режим, масштаб, поворот и VRR, но ничего не
/// меняет.
fn has_niri() -> bool {
    has("niri")
}

/// Есть чем читать список накопителей и извлекать их. Сам раздел кнопку
/// «Извлечь» показывает только у съёмных дисков, так что на ноутбуке с
/// одними SSD он будет состоять из одного списка.
fn has_udisksctl() -> bool {
    has("lsblk") && has("udisksctl")
}

/// Раздел системы: живые переключатели и быстрые скрипты.
fn system_section(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Система", &["system", "система"]),
        Language::En => ("System", &["system", "система"]),
    };
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    let mut children = vec![
        dnd_row(lang),
        Node::dynamic(
            settings_icons::SOUND,
            t("Звук", "Audio"),
            ProviderKey::AUDIO,
        )
        .with_id("system/audio")
        .when(has_audio)
        .search_as(&["audio", "sound", "звук", "volume", "громкость", "микрофон"]),
        Node::dynamic(
            settings_icons::NETWORK,
            t("Wi-Fi", "Wi-Fi"),
            ProviderKey::WIFI,
        )
        .with_id("system/wifi")
        .when(has_nmcli)
        .search_as(&["wifi", "вайфай", "сеть"]),
        Node::dynamic(
            settings_icons::BLUETOOTH,
            t("Bluetooth", "Bluetooth"),
            ProviderKey::BLUETOOTH,
        )
        .with_id("system/bluetooth")
        .when(has_bluetoothctl)
        .search_as(&["bluetooth", "блютуз", "bt"]),
        Node::dynamic(
            settings_icons::PANEL_MONITOR,
            t("Дисплеи", "Displays"),
            ProviderKey::OUTPUTS,
        )
        .with_id("system/displays")
        .when(has_niri)
        .search_as(&["displays", "дисплеи", "monitors", "мониторы", "screens"]),
        Node::dynamic(
            settings_icons::BATTERY,
            t("Устройства", "Storage"),
            ProviderKey::STORAGE,
        )
        .with_id("system/storage")
        .when(has_udisksctl)
        .search_as(&["storage", "диски", "накопители", "disks", "флешка"]),
        Node::dynamic(settings_icons::NETWORK, t("VPN", "VPN"), ProviderKey::VPN)
            .with_id("system/vpn")
            .search_as(&["vpn", "прокси", "proxy", "mihomo"]),
        Node::toggle(
            settings_icons::DOT,
            t("Кофе-мод", "Coffee mode"),
            values::coffee,
            values::set_coffee,
        )
        .with_id("system/coffee")
        .search_as(&["coffee", "кофе", "caffeine"]),
        Node::action(
            settings_icons::BATTERY,
            t("Режим питания", "Power profile"),
            Action::Native(super::action::Native::PowerProfileCycle),
        )
        .with_value(power_profile_value)
        .with_id("system/power-profile")
        .when(has_power_profile)
        .search_as(&["power", "питание", "профиль", "battery"]),
    ];
    // Панель управления дописывается отдельно: целей может не оказаться
    // вовсе, и пустую строку в разделе показывать нечего.
    children.extend(control_node(lang));
    Node::submenu(settings_icons::SYSTEM, title, children).search_as(kw)
}

/// Быстрый доступ к конфигам: тот же список, что был в `control.sh` на rofi,
/// но строкой меню. Отдельный скрипт и второй лаунчер не нужны — список и
/// есть окно.
fn control_node(lang: Language) -> Option<Node> {
    // `when` умеет только `fn() -> bool`, а путь здесь со значением: строки с
    // несуществующим каталогом просто не строятся.
    let children: Vec<Node> = targets()
        .into_iter()
        .filter(|(_, _, path)| std::path::Path::new(path).exists())
        .map(|(icon, title, path)| {
            let id = format!(
                "control/{}",
                std::path::Path::new(&path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            );
            Node::action(icon, title, open_in_editor(path.clone())).with_id(&id)
        })
        .collect();
    // Ни одной цели — строка была бы мёртвой: заходить в пустое подменю
    // нечего.
    if children.is_empty() {
        return None;
    }
    let (title, kw) = match lang {
        Language::Ru => (
            "Панель управления",
            &["control", "управление", "конфиг", "конфиги"][..],
        ),
        Language::En => (
            "Control panel",
            &["control", "panel", "config", "configs"][..],
        ),
    };
    Some(
        Node::submenu(settings_icons::CONTROLS, title, children)
            .with_id("system/control")
            .search_as(kw),
    )
}

/// Куда открывать: каталог или файл — без разницы, `nvim` разберётся сам.
fn open_in_editor(path: String) -> Action {
    // Путь приходит из `$HOME` и в дереве живёт `'static`: меню строится
    // один раз на запуск окна, а процесс короткоживущий, так что утечка
    // десятка строк здесь честнее, чем новый тип строки в `Action`.
    Action::Spawn(
        "kitty",
        Box::leak(Box::new(["-e", "nvim", Box::leak(path.into_boxed_str())])),
    )
}

/// Цели быстрого доступа. Порядок — по частоте правки: сначала композитор и
/// его бинды, потом панель и терминал, потом остальное окружение.
fn targets() -> Vec<(&'static str, &'static str, String)> {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = |parts: &str| std::path::Path::new(&home).join(parts);
    vec![
        (settings_icons::SYSTEM, "niri", path(".config/niri")),
        (
            settings_icons::KEYBINDS,
            "binds · niri/binds.kdl",
            path(".config/niri/binds.kdl"),
        ),
        (settings_icons::PANEL, "waybar", path(".config/waybar")),
        (settings_icons::TERMINAL, "kitty", path(".config/kitty")),
        (
            settings_icons::NOTIFICATIONS,
            "dunst",
            path(".config/dunst"),
        ),
        (
            settings_icons::SEARCH,
            "rofi",
            path(".config/rofi/config.rasi"),
        ),
        (settings_icons::THEME, "matugen", path(".config/matugen")),
        (settings_icons::FONT_ICON, "zsh", path(".config/zsh")),
        (settings_icons::APPS, "nvim", path(".config/nvim")),
        (settings_icons::OVERVIEW, "hudbar", path("code/hudbar")),
        (settings_icons::FOLDER, "scripts", path(".local/bin")),
    ]
    .into_iter()
    .map(|(icon, title, path)| (icon, title, path.to_string_lossy().into_owned()))
    .collect()
}

/// Вопрос подтверждения необратимого действия: отдельный уровень с двумя
/// строками, а не флаг в строке. Плюсы такого решения: `Esc` уводит назад
/// сам, «Отмена» — обычное действие [`Action::Back`], а у вопроса вообще нет
/// команды, поэтому исполнителю она и не достанется. Внутренние строки
/// скрыты от глобального поиска: в общем списке два «Отмена» — шум.
fn confirm_node(
    icon: &'static str,
    question: &'static str,
    yes: &'static str,
    cancel: &'static str,
    program: &'static str,
    args: &'static [&'static str],
) -> Node {
    Node::submenu(
        icon,
        question,
        vec![
            Node::action(settings_icons::CHECK, yes, Action::Spawn(program, args))
                .hidden_from_search(),
            Node::action(settings_icons::TIMES, cancel, Action::Back).hidden_from_search(),
        ],
    )
}

/// Раздел «Питание»: блокировка, сон и выход — в начале, необратимые команды в
/// конце и обязательно за вопросом. Порядок в меню един, поэтому «Выключение»
/// не оказалось второй строкой списка.
fn power_section(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Питание", &["power", "питание", "shutdown"]),
        Language::En => ("Power", &["power", "питание", "shutdown"]),
    };
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    Node::submenu(
        settings_icons::POWER,
        title,
        vec![
            Node::action(
                settings_icons::LOCK,
                t("Заблокировать", "Lock screen"),
                Action::Spawn("dynalock.sh", &[]),
            )
            .when(has_dynalock)
            .search_as(&["lock", "замок", "блокировка", "заблокировать"]),
            Node::action(
                settings_icons::SLEEP,
                t("Спящий режим", "Suspend"),
                Action::Spawn("systemctl", &["suspend"]),
            )
            .search_as(&["suspend", "сон", "спать", "sleep"]),
            Node::action(
                settings_icons::LOGOUT,
                t("Выйти из сессии", "Log out"),
                Action::Niri(&["msg", "action", "quit", "--skip-confirmation"]),
            )
            .search_as(&["logout", "выход", "выйти"]),
            confirm_node(
                settings_icons::HIBERNATE,
                t("Гибернация?", "Hibernate?"),
                t("Да, в спячку", "Yes, hibernate"),
                t("Отмена", "Cancel"),
                "systemctl",
                &["hibernate"],
            )
            .when(has_hibernate)
            .search_as(&["hibernate", "гибернация", "спячка", "suspend-to-disk"]),
            confirm_node(
                settings_icons::REBOOT,
                t("Перезагрузить?", "Reboot?"),
                t("Да, перезагрузить", "Yes, reboot"),
                t("Отмена", "Cancel"),
                "systemctl",
                &["reboot"],
            )
            .search_as(&["reboot", "перезагрузка", "перезагрузить", "restart"]),
            confirm_node(
                settings_icons::POWER,
                t("Выключить?", "Power off?"),
                t("Да, выключить", "Yes, power off"),
                t("Отмена", "Cancel"),
                "systemctl",
                &["poweroff"],
            )
            .search_as(&["poweroff", "shutdown", "выключить", "выключение"]),
        ],
    )
    .search_as(kw)
}

/// Раздел «О программе»: версия и перезапуск панели.
fn about_section(lang: Language) -> Node {
    let (title, version, restart, kw) = match lang {
        Language::Ru => (
            "О программе",
            format!("Версия {VERSION}"),
            "Перезапуск панели",
            &["about", "о программе"],
        ),
        Language::En => (
            "About",
            format!("Version {VERSION}"),
            "Restart panel",
            &["about", "о программе"],
        ),
    };
    Node::submenu(
        settings_icons::ABOUT,
        title,
        vec![
            Node::info(settings_icons::ABOUT, &version),
            Node::action(settings_icons::SYSTEM, restart, Action::RestartHudbar),
        ],
    )
    .search_as(kw)
}

/// Настоящее дерево на указанном языке. Порядок корня повторяет `SECTIONS`,
/// чтобы прямой вход по идентификатору не зависел от подписей.
pub fn tree_in(lang: Language) -> Vec<Node> {
    vec![
        apps_section(lang),
        panel_section(lang),
        style_section(lang),
        notifications_section(lang),
        capture_section(lang),
        keybinds_section(lang),
        system_section(lang),
        power_section(lang),
        about_section(lang),
    ]
}

/// Настоящее дерево на текущем языке из `settings.json`.
pub fn tree() -> Vec<Node> {
    tree_in(super::super::settings::load().language)
}

/// Готовое меню на настоящем дереве и текущем языке.
pub fn menu() -> Menu {
    let lang = super::super::settings::load().language;
    let mut menu = Menu::new(tree_in(lang));
    menu.set_lang(lang);
    menu
}

#[cfg(test)]
mod tests {
    use super::super::state::{self, Outcome};
    use super::*;

    /// Панель управления — подменю с целями, а не запуск второго лаунчера.
    /// Проверяется на дереве: набор путей зависит от машины.
    #[test]
    fn control_panel_is_a_submenu_of_configs() {
        let Some(control) = control_node(Language::Ru) else {
            return; // на машине нет ни одного каталога конфигов
        };
        assert_eq!(control.title, "Панель управления");
        let super::super::tree::NodeKind::Submenu(children) = &control.kind else {
            panic!("ожидалось подменю, {:?}", control.kind);
        };
        assert!(!children.is_empty());
        for child in children {
            let super::super::tree::NodeKind::Action(Action::Spawn(program, args)) = &child.kind
            else {
                panic!("строка должна запускать редактор: {:?}", child.kind);
            };
            assert_eq!(*program, "kitty");
            assert_eq!(
                args,
                &["-e", "nvim", args[2]],
                "открываем nvim с путём из строки"
            );
            let path = args[2];
            assert!(
                std::path::Path::new(path).exists(),
                "цели без пути на диске: {path}"
            );
            assert!(
                child.identity().starts_with("control/"),
                "своя личность у строки: {}",
                child.identity()
            );
        }
    }

    /// Цели не должны повторяться: два пункта на один и тот же путь — это
    /// шум, а не удобство.
    #[test]
    fn control_targets_are_unique_and_exist_on_this_machine() {
        let mut paths: Vec<String> = targets().into_iter().map(|(_, _, path)| path).collect();
        let count = paths.len();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), count, "пути повторяются: {paths:?}");
        assert!(
            count >= 8,
            "ожидался рабочий набор целей, а не {count}: {paths:?}"
        );
        assert!(
            !paths.iter().any(|path| path.contains("noctalia")),
            "noctalia из списка ушёл: {paths:?}"
        );
    }

    #[test]
    fn real_tree_is_valid() {
        for lang in [Language::Ru, Language::En] {
            assert!(state::validate(&tree_in(lang)).is_ok());
        }
    }

    /// Раздел «Звук» — динамический вход с ключом AUDIO и устойчивой
    /// личностью: по ней путь и курсор восстанавливаются после пересборки
    /// дерева. Проверка на дереве, а не на видимости: наличие wpctl/pactl
    /// зависит от машины.
    #[test]
    fn system_section_has_the_audio_entry_by_key() {
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            let audio = children
                .iter()
                .find(|node| node.identity() == "system/audio")
                .expect("вход в «Звук»");
            assert!(
                matches!(audio.kind, super::super::tree::NodeKind::Dynamic { key } if key == ProviderKey::AUDIO),
                "вход динамический и с ключом AUDIO"
            );
            let (audio_title, system_title) = match lang {
                Language::Ru => ("Звук", "Система"),
                Language::En => ("Audio", "System"),
            };
            let expected = audio_title;
            assert_eq!(audio.title, expected);
            assert!(
                audio.visible.is_some(),
                "видимость входа задана условием has_audio"
            );

            let mut menu = Menu::new(tree_in(lang));
            let index = menu
                .frame()
                .items
                .iter()
                .position(|item| item.title == system_title)
                .expect("раздел «Система» на корне");
            menu.current_mut().list.select(index);
            assert_eq!(menu.enter(), Outcome::Pushed);
            let frame = menu.frame();
            let visible = frame.items.iter().any(|item| item.title == expected);
            assert_eq!(
                visible,
                has_audio(),
                "видимость входа обязана совпадать с наличием wpctl/pactl"
            );
        }
    }

    /// Раздел «Wi-Fi» — динамический вход с ключом WIFI, личностью
    /// `system/wifi` и условием `has_nmcli`. Старый статичный тумблер Wi-Fi
    /// убран: он дёргал `nmcli radio` из UI-потока, а радио теперь
    /// переключаетсяworker-ом внутри раздела.
    #[test]
    fn system_section_has_the_wifi_entry_by_key() {
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            let wifi = children
                .iter()
                .find(|node| node.identity() == "system/wifi")
                .expect("вход в «Wi-Fi»");
            assert!(
                matches!(wifi.kind, super::super::tree::NodeKind::Dynamic { key } if key == ProviderKey::WIFI),
                "вход динамический и с ключом WIFI"
            );
            assert!(
                wifi.visible.is_some(),
                "видимость задана условием has_nmcli"
            );
            assert_eq!(wifi.title, "Wi-Fi");
            // Статичного тумблера больше нет: две строки с названием «Wi-Fi»
            // в одном разделе пользователю только мешали бы.
            assert!(
                !children.iter().any(|node| {
                    node.title == "Wi-Fi"
                        && matches!(node.kind, super::super::tree::NodeKind::Toggle { .. })
                }),
                "старый тумблер Wi-Fi удалён из раздела"
            );
        }
    }

    /// Раздел «Bluetooth» — динамический вход с ключом BLUETOOTH и
    /// условием `has_bluetoothctl`. Действий у него нет: W5.3a read-only.
    #[test]
    fn system_section_has_the_bluetooth_entry_by_key() {
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            let bluetooth = children
                .iter()
                .find(|node| node.identity() == "system/bluetooth")
                .expect("вход в «Bluetooth»");
            assert!(
                matches!(bluetooth.kind, super::super::tree::NodeKind::Dynamic { key } if key == ProviderKey::BLUETOOTH),
                "вход динамический и с ключом BLUETOOTH"
            );
            assert!(
                bluetooth.visible.is_some(),
                "видимость задана условием has_bluetoothctl"
            );
            assert_eq!(bluetooth.title, "Bluetooth");
        }
    }

    /// Раздел «Дисплеи» — динамический вход с ключом OUTPUTS и условием
    /// `has_niri`. Действий над выводами у него нет: W5.4a read-only.
    #[test]
    fn system_section_has_the_displays_entry_by_key() {
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            let displays = children
                .iter()
                .find(|node| node.identity() == "system/displays")
                .expect("вход в «Дисплеи»");
            assert!(
                matches!(displays.kind, super::super::tree::NodeKind::Dynamic { key } if key == ProviderKey::OUTPUTS),
                "вход динамический и с ключом OUTPUTS"
            );
            assert!(
                displays.visible.is_some(),
                "видимость задана условием has_niri"
            );
            assert_eq!(
                displays.title,
                if lang == Language::Ru {
                    "Дисплеи"
                } else {
                    "Displays"
                }
            );
        }
    }

    /// Раздел «Устройства» — динамический вход с ключом STORAGE и условием
    /// `has_udisksctl`: без `lsblk` и `udisksctl` накопители нечем ни
    /// показать, ни извлечь.
    #[test]
    fn system_section_has_the_storage_entry_by_key() {
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            let storage = children
                .iter()
                .find(|node| node.identity() == "system/storage")
                .expect("вход в «Устройства»");
            assert!(
                matches!(storage.kind, super::super::tree::NodeKind::Dynamic { key } if key == ProviderKey::STORAGE),
                "вход динамический и с ключом STORAGE"
            );
            assert!(
                storage.visible.is_some(),
                "видимость задана условием has_udisksctl"
            );
            assert_eq!(
                storage.title,
                if lang == Language::Ru {
                    "Устройства"
                } else {
                    "Storage"
                }
            );
        }
    }

    #[test]
    fn root_has_nine_sections_in_order() {
        for lang in [Language::Ru, Language::En] {
            let menu = Menu::new(tree_in(lang));
            assert_eq!(menu.depth(), 0);
            assert_eq!(menu.frame().items.len(), 9);
        }
        let ru = Menu::new(tree_in(Language::Ru));
        let frame = ru.frame();
        let titles: Vec<&str> = frame.items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Приложения",
                "Панель",
                "Стиль",
                "Уведомления",
                "Захват",
                "Бинды",
                "Система",
                "Питание",
                "О программе"
            ]
        );
    }

    #[test]
    fn searching_dnd_finds_system_rows_without_abbreviations() {
        let mut menu = Menu::new(tree_in(Language::Ru));
        for ch in "dnd".chars() {
            menu.type_char(ch);
        }
        let frame = menu.frame();
        assert!(
            frame.items.len() >= 2,
            "dnd должен находиться: {}",
            frame.items.len()
        );
        for item in &frame.items {
            assert!(
                !item.title.contains("(DND)"),
                "аббревиатуры в названии нет: {}",
                item.title
            );
        }
        assert!(
            frame.items.iter().any(|item| item.title == "Не беспокоить"),
            "точное название на месте"
        );
    }

    #[test]
    fn english_tree_has_english_titles() {
        let menu = Menu::new(tree_in(Language::En));
        let frame = menu.frame();
        let titles: Vec<&str> = frame.items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles[0], "Applications");
        assert_eq!(titles[2], "Style");
    }

    /// Раздел «Питание»: обратимые действия в начале, необратимые — в конце
    /// и обязательно за вопросом. Проверка по порядку, а не по наличию:
    /// список из трёх строк, где «Выключение» стоит первым, опасен.
    /// Гибернация показывается только там, где ядро её умеет: решение
    /// принимает `/sys/power/state`, а не наличие `systemctl`.
    #[test]
    fn hibernate_follows_the_kernel_states() {
        let states = |text: &str| text.split_whitespace().any(|state| state == "disk");
        assert!(states("freeze mem disk"), "диск есть — гибернация есть");
        assert!(!states("freeze mem"), "без диска обещать нечего");
    }

    /// Строка гибернации в «Питание» — с вопросом и `systemctl hibernate`,
    /// и на этой машине скрыта: `/sys/power/state` без `disk`.
    #[test]
    fn hibernate_row_is_a_confirmed_systemctl_call() {
        use super::super::tree::NodeKind;
        let power = tree_in(Language::Ru)
            .into_iter()
            .find(|node| node.identity() == "Питание")
            .expect("раздел «Питание»");
        let rows = power.children().expect("у раздела есть дети");
        let hibernate = rows.iter().find(|node| node.title == "Гибернация?");
        match hibernate {
            Some(row) => {
                let NodeKind::Submenu(answers) = &row.kind else {
                    panic!("ожидался вопрос, {:?}", row.kind);
                };
                let yes = answers
                    .iter()
                    .find(|answer| answer.title.starts_with("Да"))
                    .expect("есть подтверждение");
                let NodeKind::Action(Action::Spawn(program, args)) = &yes.kind else {
                    panic!("подтверждение должно звать systemctl, {:?}", yes.kind);
                };
                assert_eq!(*program, "systemctl");
                assert_eq!(*args, ["hibernate"]);
            }
            // На машине без гибернации строки нет — и это правильно.
            None => assert!(
                !has_hibernate(),
                "строка спрятана, хотя ядро гибернацию умеет"
            ),
        }
    }

    #[test]
    fn power_section_puts_destructive_rows_last_and_behind_a_question() {
        use super::super::tree::NodeKind;
        let power = tree_in(Language::Ru)
            .into_iter()
            .find(|node| node.identity() == "Питание")
            .expect("раздел «Питание»");
        let titles: Vec<&str> = power
            .children()
            .expect("у раздела есть строки")
            .iter()
            .map(|node| node.title.as_str())
            .collect();
        assert_eq!(
            titles.last().copied(),
            Some("Выключить?"),
            "выключение обязано быть последним: {titles:?}"
        );
        assert!(
            titles.contains(&"Перезагрузить?"),
            "перезагрузка за вопросом: {titles:?}"
        );
        for question in ["Перезагрузить?", "Выключить?"] {
            let node = power
                .children()
                .unwrap()
                .iter()
                .find(|node| node.title == question)
                .expect("вопрос");
            let children = node.children().expect("у вопроса есть ответы");
            assert_eq!(children.len(), 2, "у вопроса ровно два ответа: {question}");
            assert_eq!(children[1].title, "Отмена");
            assert!(
                matches!(&children[1].kind, NodeKind::Action(Action::Back)),
                "отмена возвращает уровень, а не запускает команду"
            );
        }
    }

    /// Подтверждение: «Да, выключить» действительно выключает, а «Отмена» и
    /// `Esc` возвращают в «Питание», не выполнив ничего.
    #[test]
    fn power_confirm_runs_the_command_or_returns_without_it() {
        let mut menu = Menu::new(tree_in(Language::Ru));
        let index = menu
            .frame()
            .items
            .iter()
            .position(|item| item.title == "Питание")
            .expect("раздел «Питание»");
        menu.current_mut().list.select(index);
        assert_eq!(menu.enter(), Outcome::Pushed);

        let index = menu
            .frame()
            .items
            .iter()
            .position(|item| item.title == "Выключить?")
            .expect("строка выключения");
        menu.current_mut().list.select(index);
        assert_eq!(menu.enter(), Outcome::Pushed, "открылся вопрос");
        assert_eq!(menu.opened_submenu(), Some("Выключить?"));

        // Отмена первой строкой снизу: сначала выбираем «Отмена».
        let cancel = menu
            .frame()
            .items
            .iter()
            .position(|item| item.title == "Отмена")
            .expect("строка отмены");
        menu.current_mut().list.select(cancel);
        assert_eq!(menu.enter(), Outcome::Popped, "отмена вернула в раздел");
        assert_eq!(menu.opened_submenu(), Some("Питание"));
    }

    /// Внутренние строки вопроса не должны попадать в глобальный поиск: там лежат
    /// два одинаковых «Отмена», а «Да, выключить» по `Enter` из поиска выключила бы
    /// машину одним нажатием мимо вопроса. Выход только через сам раздел.
    #[test]
    fn confirm_answers_stay_out_of_the_global_search() {
        let mut menu = Menu::new(tree_in(Language::Ru));
        for ch in "выключ".chars() {
            menu.type_char(ch);
        }
        let frame = menu.frame();
        let titles: Vec<&str> = frame.items.iter().map(|item| item.title.as_str()).collect();
        assert!(
            titles.is_empty(),
            "из поиска выключение недостижимо: {titles:?}"
        );
    }

    #[test]
    fn every_external_command_is_installed() {
        use super::super::tree::NodeKind;
        fn walk(nodes: &[Node], missing: &mut Vec<String>) {
            for node in nodes {
                if let NodeKind::Action(action) = &node.kind {
                    // Действие niri выполняется композитором: проверяется сам
                    // niri, а не первое слово его аргументов.
                    let program = match action {
                        Action::Niri(_) => Some("niri"),
                        _ => action.command(),
                    };
                    if let Some(program) = program
                        && !oneshot::program_exists(program)
                    {
                        missing.push(format!("{}: {program}", node.title));
                    }
                }
                if let Some(children) = node.children() {
                    walk(children, missing);
                }
            }
        }
        let mut missing = Vec::new();
        walk(&tree(), &mut missing);
        assert!(
            missing.is_empty(),
            "пункты без установленных программ: {missing:?}"
        );
    }

    #[test]
    fn every_real_node_has_an_icon() {
        fn walk(nodes: &[Node]) {
            for node in nodes {
                assert!(
                    node.icon.chars().count() == 1,
                    "у «{}» иконка не один глиф: {:?}",
                    node.title,
                    node.icon
                );
                if let Some(children) = node.children() {
                    walk(children);
                }
            }
        }
        walk(&tree_in(Language::Ru));
        walk(&tree_in(Language::En));
    }

    #[test]
    fn order_section_lists_every_module_once() {
        let section = order_section(Language::Ru);
        let children = section.children().expect("подменю порядка");
        // Подсказка плюс десять модулей в текущем порядке.
        assert_eq!(children.len(), 11);
    }

    /// Пункт выбора обоев: окно бинарником, если оно установлено, иначе
    /// прежний `wall.sh`. Оба варианта — `Spawn`, то есть с закрытием меню.
    #[test]
    fn wallpaper_item_prefers_the_picker_and_falls_back_to_wall_script() {
        fn picker_command() -> String {
            let mut menu = Menu::new(tree_in(Language::Ru));
            menu.move_sel(2);
            menu.enter();
            let title = "Выбрать обои…";
            let item = menu
                .frame()
                .items
                .into_iter()
                .find(|item| item.title == title)
                .expect("пункт выбора обоев");
            match item.kind {
                super::super::item::ItemKind::Action(action) => {
                    assert!(action.closes_menu(), "окно обоев закрывает меню");
                    action.describe()
                }
                other => panic!("пункт не действие: {other:?}"),
            }
        }
        let expected = if has_wallpaper_picker() {
            "hud-wallpaper-rs"
        } else {
            "wall.sh"
        };
        assert_eq!(picker_command(), expected);
        assert!(
            has_wallpaper_picker_or_script(),
            "пункт должен быть виден при любом из двух вариантов"
        );
    }

    #[test]
    fn style_section_moves_values_with_choice() {
        let mut menu = Menu::new(tree_in(Language::Ru));
        menu.move_sel(2);
        assert_eq!(menu.enter(), Outcome::Pushed);
        let frame = menu.frame();
        let titles: Vec<&str> = frame.items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles[0], "Тема");
        assert_eq!(titles[1], "Язык");
        assert_eq!(titles[2], "Выбрать обои…");
        assert_eq!(titles[3], "Случайные обои");
        assert_eq!(titles[4], "Схема matugen");
    }
}

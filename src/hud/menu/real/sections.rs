use super::*;

/// Строка языка на корне. Язык — настройка всего интерфейса, а не деталь
/// оформления: в «Стиле» её приходилось искать, теперь она на виду.
pub(crate) fn language_row(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Язык", &["language", "язык"][..]),
        Language::En => ("Language", &["language", "язык"][..]),
    };
    Node::choice(
        settings_icons::LANGUAGE,
        title,
        &LANG_OPTS,
        values::language,
        values::set_language,
    )
    .with_id("language")
    .search_as(kw)
}
/// Строка тумблера модуля панели.
pub(crate) fn module_row(module: Module, lang: Language) -> Node {
    let (get, set) = values::module_toggle(module);
    Node::toggle(module_icon(module), module_title(module, lang), get, set)
        .search_as(module_keywords(module))
}

/// Раздел «Приложения»: запуск программ. Всё откреплённо с закрытием меню:
//  скриптам нужен rofi, а rofi нужна клавиатура, которую держит меню.
pub(crate) fn apps_section(lang: Language) -> Node {
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

/// Раздел «Панель». В группах: сам экран держит пять входов, а не 17 строк.
/// Поиск по-прежнему находит конкретные строки — листья пусть и на глубине
/// два, но в индексе.
pub(crate) fn panel_section(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Панель", &["panel", "панель"]),
        Language::En => ("Panel", &["panel", "панель"]),
    };
    let children = vec![
        modules_group(lang),
        look_group(lang),
        osd_group(lang),
        animations_group(lang),
        order_section(lang),
    ];
    Node::submenu(settings_icons::PANEL_MONITOR, title, children).search_as(kw)
}

/// Группа тумблеров модулей: что вообще живёт на панели.
pub(crate) fn modules_group(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Модули", &["modules", "модули"]),
        Language::En => ("Modules", &["modules", "модули"]),
    };
    let children: Vec<Node> = Module::ALL
        .into_iter()
        .map(|module| module_row(module, lang))
        .collect();
    Node::submenu(settings_icons::TH_LARGE, title, children).search_as(kw)
}

/// Группа вида панели: высота панели.
pub(crate) fn look_group(lang: Language) -> Node {
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    let (title, kw) = match lang {
        Language::Ru => ("Вид панели", &["look", "вид"]),
        Language::En => ("Bar look", &["look", "вид"]),
    };
    Node::submenu(
        settings_icons::PANEL,
        title,
        vec![
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
        ],
    )
    .search_as(kw)
}

/// Группа OSD: поведение и срок сдвоены, настройка одна без соседа выглядела
/// как забытое.
pub(crate) fn osd_group(lang: Language) -> Node {
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    let (title, kw) = match lang {
        Language::Ru => ("OSD", &["osd", "осд"]),
        Language::En => ("OSD", &["osd", "осд"]),
    };
    Node::submenu(
        settings_icons::NOTIFICATIONS,
        title,
        vec![
            Node::toggle(
                settings_icons::SOUND,
                t("OSD громкости", "Volume OSD"),
                values::osd,
                values::set_osd,
            )
            .search_as(&["osd", "громкость", "volume"]),
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
        ],
    )
    .search_as(kw)
}

/// Группа анимаций: панель читает файл на каждом тике и применяет сразу,
/// окна берут настройки при старте — это сказано в подписи строки.
pub(crate) fn animations_group(lang: Language) -> Node {
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    let (title, kw) = match lang {
        Language::Ru => ("Анимации", &["animations", "анимации"]),
        Language::En => ("Animations", &["animations", "анимации"]),
    };
    Node::submenu(
        settings_icons::APPEARANCE,
        title,
        vec![
            Node::toggle(
                settings_icons::APPEARANCE,
                t("Анимации", "Animations"),
                values::animations,
                values::set_animations,
            )
            .search_as(&["animation", "animations", "анимации"]),
            Node::number(
                settings_icons::STYLE,
                t(
                    "Скорость анимаций (окна — при запуске)",
                    "Animation speed (windows on next start)",
                ),
                ANIMATION_SPEED_MIN,
                ANIMATION_SPEED_MAX,
                0.25,
                "×",
                values::animation_speed,
                values::set_animation_speed,
            )
            .search_as(&["animation", "speed", "анимации", "скорость"]),
        ],
    )
    .search_as(kw)
}

/// Подраздел порядка модулей: `Enter` опускает модуль на одну позицию в
/// своей зоне, `Shift+↑`/`Shift+↓` двигают его выбором в обе стороны.
/// Перенос через границу зоны запрещён у обоих.
pub(crate) fn order_section(lang: Language) -> Node {
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
    let order = super::super::super::settings::load().module_order;
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

/// Раздел «Стиль»: обои и схема matugen. Язык переехал на корень.
pub(crate) fn style_section(lang: Language) -> Node {
    style_section_with(lang, WallpaperTools::detect())
}

/// Раздел стиля. Обойные программы приходят аргументом: настоящие берутся из
/// `PATH`, тесты подают свои, поэтому обе ветки видны без установленных
/// программ.
pub(crate) fn style_section_with(lang: Language, wallpaper: WallpaperTools) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Стиль", &["style", "стиль"]),
        Language::En => ("Style", &["style", "стиль"]),
    };
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    let mut children = Vec::new();
    if wallpaper.chooses() {
        children.push(wallpaper_row(lang, wallpaper.picker));
    }
    if wallpaper.script {
        children.push(random_wallpapers_row(lang));
    }
    children.push(
        Node::choice(
            settings_icons::SCHEME,
            t("Схема matugen", "matugen scheme"),
            &schemes::SCHEMES,
            values::scheme_index,
            values::set_scheme,
        )
        .search_as(&["scheme", "схема", "matugen"]),
    );
    Node::submenu(settings_icons::STYLE, title, children).search_as(kw)
}

/// Окно выбора обоев, если оно установлено, иначе прежний `wall.sh` с rofi:
/// обе команды неинтерактивны для меню, уходят откреплённо и закрывают его.
pub(crate) fn wallpaper_row(lang: Language, picker: bool) -> Node {
    let title = match lang {
        Language::Ru => "Выбрать обои…",
        Language::En => "Choose wallpaper…",
    };
    Node::action(
        settings_icons::PICTURE,
        title,
        if picker {
            Action::Spawn("hud-wallpaper-rs", &[])
        } else {
            Action::Spawn("wall.sh", &[])
        },
    )
    .search_as(&["wallpaper", "обои", "choose", "выбрать"])
    .with_value(current_wallpaper_label)
}

/// Случайные обои: работу делает запасной `wall.sh`, без него строки нет.
pub(crate) fn random_wallpapers_row(lang: Language) -> Node {
    let title = match lang {
        Language::Ru => "Случайные обои",
        Language::En => "Random wallpapers",
    };
    Node::action(
        settings_icons::PICTURE,
        title,
        // Дольше секунды: держим меню открытым, результат — статус
        // «Применено» в подвале, а не молчаливый выход.
        Action::Run("wall.sh", &["--random"]),
    )
    .search_as(&["random", "случайные", "wallpaper", "обои"])
}

/// Раздел уведомлений: системный DND и параметры dunst.
pub(crate) fn notifications_section(lang: Language) -> Node {
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
pub(crate) fn dnd_row(lang: Language) -> Node {
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
pub(crate) fn capture_section(lang: Language) -> Node {
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
pub(crate) fn record_title(lang: Language) -> &'static str {
    let on = values::recording();
    match (lang, on) {
        (Language::Ru, false) => "Запись экрана",
        (Language::Ru, true) => "Запись экрана ●",
        (Language::En, false) => "Screen recording",
        (Language::En, true) => "Screen recording ●",
    }
}

/// Раздел биндов: окна списков только для чтения.
pub(crate) fn keybinds_section(lang: Language) -> Node {
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
pub(crate) fn has_audio() -> bool {
    has("wpctl") || has("pactl")
}

/// NetworkManager на месте — значит есть чем управлять Wi-Fi.
pub(crate) fn has_nmcli() -> bool {
    has("nmcli")
}

/// BlueZ на месте — значит есть что показать про Bluetooth. Сам раздел
/// read-only: без адаптера и без устройств он всё равно говорит своё
/// состояние, а не падает.
pub(crate) fn has_bluetoothctl() -> bool {
    has("bluetoothctl")
}

/// niri на месте — значит есть чем читать выводы. Пока раздел строго
/// read-only: он показывает режим, масштаб, поворот и VRR, но ничего не
/// меняет.
pub(crate) fn has_niri() -> bool {
    has("niri")
}

/// Есть чем читать список накопителей и извлекать их. Сам раздел кнопку
/// «Извлечь» показывает только у съёмных дисков, так что на ноутбуке с
/// одними SSD он будет состоять из одного списка.
pub(crate) fn has_udisksctl() -> bool {
    has("lsblk") && has("udisksctl")
}

/// niri или udisks гарантируют хотя бы одну строку в «Устройствах»: сама
/// группа без содержимого не показывается вовсе.
pub(crate) fn has_devices_menu() -> bool {
    has_niri() || has_udisksctl()
}

/// Раздел системы: живые переключатели и быстрые скрипты.
/// «Питание» в «Системе»: простой экрана, сон ноутбука и профиль питания
/// одной строкой. Три отдельных пункта подряд занимали половину уровня и
/// читались как разрозненные настройки, хотя все они про энергию.
pub(crate) fn power_group(lang: Language) -> Node {
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    Node::submenu(
        settings_icons::BATTERY,
        t("Питание", "Power"),
        vec![
            Node::number(
                settings_icons::PANEL_MONITOR,
                t("Экран гаснет", "Screen off"),
                (IDLE_SCREEN_OFF_MIN_SECONDS / 60) as f32,
                (IDLE_SCREEN_OFF_MAX_SECONDS / 60) as f32,
                1.0,
                t(" мин", " min"),
                values::idle_screen_off,
                values::set_idle_screen_off,
            )
            .with_id("system/idle-screen-off")
            .search_as(&["screen", "idle", "экран", "гаснет", "простой"]),
            Node::number(
                settings_icons::SLEEP,
                t("Сон ноутбука", "Suspend after"),
                (IDLE_SUSPEND_MIN_SECONDS / 60) as f32,
                (IDLE_SUSPEND_MAX_SECONDS / 60) as f32,
                1.0,
                t(" мин", " min"),
                values::idle_suspend,
                values::set_idle_suspend,
            )
            .with_id("system/idle-suspend")
            .search_as(&["sleep", "suspend", "сон", "спать", "простой"]),
            Node::action(
                settings_icons::BATTERY,
                t("Режим питания", "Power profile"),
                Action::Native(super::super::action::Native::PowerProfileCycle),
            )
            .with_value(power_profile_value)
            .with_id("system/power-profile")
            .when(has_power_profile)
            .search_as(&["power", "питание", "профиль", "battery"]),
        ],
    )
    .with_id("system/power")
    .search_as(&[
        "power",
        "питание",
        "энергия",
        "idle",
        "простой",
        "сон",
        "экран",
    ])
}

pub(crate) fn system_section(lang: Language) -> Node {
    let (title, kw) = match lang {
        Language::Ru => ("Система", &["system", "система"]),
        Language::En => ("System", &["system", "система"]),
    };
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    let mut children = vec![
        Node::dynamic(
            settings_icons::SOUND,
            t("Звук", "Audio"),
            ProviderKey::AUDIO,
        )
        .with_id("system/audio")
        .when(has_audio)
        .search_as(&["audio", "sound", "звук", "volume", "громкость", "микрофон"]),
        connections_group(lang),
        devices_group(lang),
        Node::toggle(
            settings_icons::DOT,
            t("Кофе-мод", "Coffee mode"),
            values::coffee,
            values::set_coffee,
        )
        .with_id("system/coffee")
        .search_as(&["coffee", "кофе", "caffeine"]),
        power_group(lang),
        updates_group(lang),
    ];
    // «Конфиги» дописываются отдельно: целей может не оказаться вовсе, и
    // пустую строку в разделе показывать нечего. «О системе» замыкает
    // список — паспорт в конце документа.
    children.extend(control_node(lang));
    children.push(super::super::sysinfo::section(lang));
    Node::submenu(settings_icons::SYSTEM, title, children).search_as(kw)
}

/// Группа обновления системы: команды запускаются в отдельном окне kitty,
/// чтобы подтверждения и вывод pacman/AUR/Flatpak были видны, а меню не
/// запирало клавиатуру на время долгой команды.
pub(crate) fn updates_group(lang: Language) -> Node {
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    let title = match lang {
        Language::Ru => "Обновление системы",
        Language::En => "System update",
    };
    Node::submenu(
        settings_icons::ARROW_UP,
        title,
        vec![
            Node::action(
                settings_icons::SYSTEM,
                t("Обновить всё", "Update everything"),
                Action::Spawn(
                    "kitty",
                    &["--class", "hud-update", "-e", "hud-update", "all"],
                ),
            )
            .with_id("system/updates/all"),
            Node::action(
                settings_icons::TH_LARGE,
                t("Только pacman", "Pacman only"),
                Action::Spawn(
                    "kitty",
                    &["--class", "hud-update", "-e", "hud-update", "pacman"],
                ),
            )
            .with_id("system/updates/pacman"),
            Node::action(
                settings_icons::APPS,
                t("Только AUR", "AUR only"),
                Action::Spawn(
                    "kitty",
                    &["--class", "hud-update", "-e", "hud-update", "aur"],
                ),
            )
            .with_id("system/updates/aur")
            .when(has_paru_or_yay),
            Node::action(
                settings_icons::APPS,
                t("Только Flatpak", "Flatpak only"),
                Action::Spawn(
                    "kitty",
                    &["--class", "hud-update", "-e", "hud-update", "flatpak"],
                ),
            )
            .with_id("system/updates/flatpak")
            .when(has_flatpak),
            Node::action(
                settings_icons::SEARCH,
                t("Проверить обновления", "Check for updates"),
                Action::Spawn(
                    "kitty",
                    &["--class", "hud-update", "-e", "hud-update", "check"],
                ),
            )
            .with_id("system/updates/check"),
        ],
    )
    .with_id("system/updates")
    .when(has_kitty)
    .search_as(&[
        "update",
        "upgrade",
        "pacman",
        "aur",
        "flatpak",
        "обновление",
        "обновить",
        "пакеты",
    ])
}

/// Группа связи: Wi-Fi и Bluetooth собраны под одним входом, чтобы
/// раздел «Система» не превратился в простыню.
pub(crate) fn connections_group(lang: Language) -> Node {
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    let (title, kw) = match lang {
        Language::Ru => ("Связь", &["connection", "связь", "сеть"]),
        Language::En => ("Connections", &["connection", "связь", "сеть"]),
    };
    Node::submenu(
        settings_icons::NETWORK,
        title,
        vec![
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
        ],
    )
    .search_as(kw)
}

/// Группа устройств: дисплеи и накопители. niri или udisks гарантируют хотя
/// бы одну строку — без содержимого группа не показывается вовсе.
pub(crate) fn devices_group(lang: Language) -> Node {
    let t = |ru: &'static str, en: &'static str| match lang {
        Language::Ru => ru,
        Language::En => en,
    };
    let (title, kw) = match lang {
        Language::Ru => ("Устройства", &["devices", "устройства"]),
        Language::En => ("Devices", &["devices", "устройства"]),
    };
    Node::submenu(
        settings_icons::PANEL_MONITOR,
        title,
        vec![
            Node::dynamic(
                settings_icons::PANEL_MONITOR,
                t("Дисплеи", "Displays"),
                ProviderKey::OUTPUTS,
            )
            .with_id("system/displays")
            .when(has_niri)
            .search_as(&["displays", "дисплеи", "monitors", "мониторы", "screens"]),
            Node::dynamic(
                settings_icons::HARD_DRIVE,
                t("Накопители", "Drives"),
                ProviderKey::STORAGE,
            )
            .with_id("system/storage")
            .when(has_udisksctl)
            .search_as(&["storage", "диски", "накопители", "ssd", "nvme", "флешка"]),
        ],
    )
    .when(has_devices_menu)
    .search_as(kw)
}

/// Быстрый доступ к конфигам: тот же список, что был в `control.sh` на rofi,
/// но строкой меню. Отдельный скрипт и второй лаунчер не нужны — список и
/// есть окно.
pub(crate) fn control_node(lang: Language) -> Option<Node> {
    control_node_with(lang, targets())
}

/// Строки подменю из готового списка целей. Список приходит снаружи, потому
/// что настоящие цели зависят от того, какие конфиги лежат в `$HOME`: тест на
/// чистой машине иначе не проверял бы ничего.
pub(crate) fn control_node_with(
    lang: Language,
    targets: Vec<(&'static str, &'static str, String)>,
) -> Option<Node> {
    // `when` умеет только `fn() -> bool`, а путь здесь со значением: строки с
    // несуществующим каталогом просто не строятся.
    let children: Vec<Node> = targets
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
            "Конфиги",
            &["control", "config", "управление", "конфиг", "конфиги"][..],
        ),
        Language::En => (
            "Configs",
            &["control", "config", "configs", "управление", "конфиги"][..],
        ),
    };
    Some(
        Node::submenu(settings_icons::CONTROLS, title, children)
            .with_id("system/control")
            .search_as(kw),
    )
}

/// Раздел «Питание»: блокировка, сон и выход — в начале, необратимые команды в
/// конце и обязательно за вопросом. Порядок в меню един, поэтому «Выключение»
/// не оказалось второй строкой списка.
pub(crate) fn power_section(lang: Language) -> Node {
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

/// Раздел «Hudbar»: версия и перезапуск панели. Название без кириллической
/// «О»: в шрифте меню она читалась как ноль («0 программе»).
pub(crate) fn about_section(lang: Language) -> Node {
    let (version_title, restart, kw) = match lang {
        Language::Ru => ("Версия", "Перезапуск панели", &["about", "о программе"][..]),
        Language::En => ("Version", "Restart panel", &["about", "о программе"][..]),
    };
    Node::submenu(
        settings_icons::ABOUT,
        "Hudbar",
        vec![
            Node::info_value(version_title, VERSION),
            Node::action(settings_icons::SYSTEM, restart, Action::RestartHudbar),
        ],
    )
    .with_value(|| VERSION.to_string())
    .search_as(kw)
}

/// «Питание» не показывается в корне: у него есть бинд `super+P`, который
/// открывает раздел напрямую (`hud-menu power`). Узел остаётся в дереве, чтобы
/// этот вход и снимки работали; `visible` убирает его из списка и поиска.
pub(crate) fn power_hidden() -> bool {
    false
}

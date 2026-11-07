use super::*;

pub(crate) fn ru(lang: Language) -> bool {
    lang == Language::Ru
}

/// Статусная строка раздела: сколько выводов и с чем разбирались.
pub(crate) fn status_row(lang: Language, snapshot: &DisplaysSnapshot) -> Node {
    let id = "displays/status";
    let text = match &snapshot.outputs {
        Block::Missing => {
            return Node::info(
                settings_icons::PANEL_MONITOR,
                if ru(lang) {
                    "niri нет"
                } else {
                    "niri is missing"
                },
            )
            .with_id(id);
        }
        Block::Failed(reason) => {
            let reason = provider_text(lang, reason);
            return Node::info(
                settings_icons::PANEL_MONITOR,
                &if ru(lang) {
                    format!("Дисплеи: {reason}")
                } else {
                    format!("Displays: {reason}")
                },
            )
            .with_id(id);
        }
        Block::Ready(list) if list.is_empty() => {
            return Node::info(
                settings_icons::PANEL_MONITOR,
                if ru(lang) {
                    "Нет дисплеев"
                } else {
                    "No displays"
                },
            )
            .with_id(id);
        }
        Block::Ready(list) => {
            format!("{}: {}", label(lang, "Дисплеев", "Displays"), list.len())
        }
    };
    Node::info(settings_icons::PANEL_MONITOR, &text).with_id(id)
}

/// Подменю пресетов. Показывается не всегда: пресет про внешний вывод без
/// единого внешнего вывода — мёртвая строка, поэтому при одном `eDP-*` в
/// списке остаются только те пресеты, которые что-то меняют и на ноутбуке.
pub(crate) fn preset_rows(outputs: &[Display], lang: Language) -> Option<Node> {
    let has_external = outputs.iter().any(|output| !is_internal(output));
    let presets: Vec<Node> = Preset::ALL
        .into_iter()
        .filter_map(|preset| {
            // Прессет про внешний вывод на ноутбуке без внешнего вывода
            // не делает ничего: прячем его, а не показываем в заглушку.
            if !has_external && preset != Preset::LaptopOnly {
                return None;
            }
            let action = preset.action(outputs)?;
            let title = preset.title(lang);
            Some(
                Node::action(settings_icons::CHECK, title, action)
                    .with_id(&format!("displays/preset/{preset:?}")),
            )
        })
        .collect();
    if presets.is_empty() {
        return None;
    }
    Some(
        Node::submenu(
            settings_icons::CHECK,
            label(lang, "Раскладка", "Layout"),
            presets,
        )
        .with_id("displays/presets"),
    )
}

/// Подпись вывода в списке: имя и человеческий текущий режим, когда он есть.
pub(crate) fn display_title(lang: Language, display: &Display) -> String {
    if is_internal(display) {
        format!(
            "{}{}",
            display.name,
            label(lang, " — встроенный", " — built-in")
        )
    } else {
        display.name.clone()
    }
}

pub(crate) fn display_value(lang: Language, display: &Display) -> String {
    display
        .current()
        .map(|mode| mode.human_label(lang))
        .unwrap_or_else(|| label(lang, "выключен", "off").to_string())
}

/// Убирает из производителя только юридические хвосты. Правило простое и
/// безопасное: удаляются последние слова `CO`, `LTD`, `INC`, `CORP`,
/// `CORPORATION`, `LIMITED` без учёта регистра и точки; как только встретилось
/// другое слово, разбор прекращён. Слова `TECHNOLOGY` и `TECHNOLOGIES` —
/// часть названия производителя, поэтому остаются.
pub(crate) fn clean_make(make: &str) -> String {
    let mut parts: Vec<&str> = make.split_whitespace().collect();
    while let Some(last) = parts.last() {
        let token = last.trim_end_matches('.').to_ascii_lowercase();
        let legal = matches!(
            token.as_str(),
            "co" | "ltd" | "inc" | "corp" | "corporation" | "limited"
        );
        if legal {
            parts.pop();
        } else {
            break;
        }
    }
    parts.join(" ")
}

/// Сырой код модели из EDID (`0x004D`): человеку он ничего не говорит и в
/// строке «Модель» не показывается.
pub(crate) fn is_raw_model_code(model: &str) -> bool {
    let Some(code) = model
        .strip_prefix("0x")
        .or_else(|| model.strip_prefix("0X"))
    else {
        return false;
    };
    !code.is_empty() && code.chars().all(|ch| ch.is_ascii_hexdigit())
}

/// Пояснение поворота для строки «Логический». `None` — обычный вывод, про
/// который в короткой строке говорить нечего.
pub(crate) fn rotation_note(lang: Language, raw: &str) -> Option<String> {
    let transform = Transform::parse(raw)?;
    let note = match transform {
        Transform::Normal => return None,
        Transform::Rotate90 => label(lang, "поворот 90°", "rotated 90°"),
        Transform::Rotate180 => label(lang, "поворот 180°", "rotated 180°"),
        Transform::Rotate270 => label(lang, "поворот 270°", "rotated 270°"),
        Transform::Flipped => label(lang, "зеркало", "mirrored"),
        Transform::Flipped90 => label(lang, "зеркало 90°", "mirrored 90°"),
        Transform::Flipped180 => label(lang, "зеркало 180°", "mirrored 180°"),
        Transform::Flipped270 => label(lang, "зеркало 270°", "mirrored 270°"),
    };
    Some(note.to_string())
}

/// Короткая строка логического размера: без позиции (для неё есть отдельный
/// пункт) и без «без поворота»; поворот добавляется только когда он не Normal.
pub(crate) fn logical_text(lang: Language, logical: &DisplayLogical) -> String {
    let size = format!("{}×{}", logical.width, logical.height);
    let scale = format_scale(logical.scale);
    let mut text = if ru(lang) {
        format!("Логический: {size} · ×{scale}")
    } else {
        format!("Logical: {size} · ×{scale}")
    };
    if let Some(note) = rotation_note(lang, &logical.transform) {
        text.push_str(" · ");
        text.push_str(&note);
    }
    text
}

/// Подменю одного вывода: модель, режим, логика, VRR, физический размер.
pub(crate) fn display_submenu(lang: Language, display: &Display, on_count: usize) -> Node {
    let id = display.row_id();
    let mut information = Vec::new();

    let make = clean_make(&display.make);
    let model = [make.as_str(), display.model.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty() && !is_raw_model_code(part))
        .collect::<Vec<_>>()
        .join(" ");
    if !model.is_empty() {
        information.push(
            Node::info_value(label(lang, "Модель", "Model"), &model)
                .with_id(&format!("{id}/info/model")),
        );
    }

    if let Some(mode) = display.current() {
        information.push(
            Node::info_value(
                label(lang, "Разрешение", "Resolution"),
                &format!("{}×{}", mode.width, mode.height),
            )
            .with_id(&format!("{id}/info/resolution")),
        );
        information.push(
            Node::info_value(
                label(lang, "Частота", "Refresh rate"),
                &format!(
                    "{} {}",
                    format_refresh(mode.refresh_mhz),
                    label(lang, "Гц", "Hz")
                ),
            )
            .with_id(&format!("{id}/info/refresh")),
        );
    }

    if let Some(logical) = &display.logical {
        let value = format!(
            "{} ({} {}×{})",
            format_scale_percent(logical.scale),
            label(lang, "логический", "logical"),
            logical.width,
            logical.height
        );
        information.push(
            Node::info_value(label(lang, "Масштаб", "Scale"), &value)
                .with_id(&format!("{id}/info/scale")),
        );
    }
    if let Some((width, height)) = display.physical_size_mm {
        information.push(
            Node::info_value(
                label(lang, "Размер", "Size"),
                &format!("{width}×{height} {}", label(lang, "мм", "mm")),
            )
            .with_id(&format!("{id}/info/physical")),
        );
    }
    let vrr_status = if !display.vrr_supported {
        if ru(lang) {
            "не поддерживается"
        } else {
            "not supported"
        }
    } else if display.vrr_enabled {
        label(lang, "включён", "on")
    } else if ru(lang) {
        "выключен"
    } else {
        "off"
    };
    let mut vrr = Node::info_value("VRR", vrr_status).with_id(&format!("{id}/info/vrr"));
    vrr.muted = !display.vrr_supported;
    information.push(vrr);

    let mut children = vec![
        Node::header(label(lang, "Информация", "Information")).with_id(&format!("{id}/group/info")),
    ];
    children.extend(information);
    if display.is_on() {
        children.push(
            Node::header(label(lang, "Настройки", "Settings"))
                .with_id(&format!("{id}/group/settings")),
        );
        children.extend(control_rows(lang, display));
        // У действий нет отдельной шапки: выключение вывода — такой же пункт
        // настроек вывода, а лишняя группа дробила уровень и заставляла его
        // прокручиваться.
        if on_count == 1 {
            // Keep this as an action (rather than an informational row): the
            // disabled action still occupies the same stable slot and is
            // counted by callers enumerating display controls.
            children.push(
                Node::action(
                    settings_icons::POWER,
                    label(lang, "Выключить вывод", "Turn off display"),
                    power_action(&display.name, false),
                )
                .with_id(&format!("{id}/power"))
                .with_disabled(),
            );
        } else {
            let question = format!("{} {}?", label(lang, "Выключить", "Turn off"), display.name);
            let confirm = super::super::real::confirm_actions(
                settings_icons::POWER,
                &question,
                Node::action(
                    settings_icons::CHECK,
                    label(lang, "Да, выключить", "Yes, turn off"),
                    power_action(&display.name, false),
                )
                .with_danger(),
                Node::action(
                    settings_icons::TIMES,
                    label(lang, "Отмена", "Cancel"),
                    Action::Back,
                ),
            )
            .with_id(&format!("{id}/power"));
            children.push(confirm);
            children.push(
                Node::info(
                    "",
                    label(
                        lang,
                        "Изображение на этом выводе пропадёт",
                        "The image on this display will disappear",
                    ),
                )
                .with_id(&format!("{id}/power-warning"))
                .hidden_from_search(),
            );
        }
    } else {
        children.push(
            Node::action(
                settings_icons::POWER,
                label(lang, "Включить вывод", "Turn on display"),
                power_action(&display.name, true),
            )
            .with_id(&format!("{id}/power")),
        );
    }
    let title = display_title(lang, display);
    Node::submenu(settings_icons::PANEL_MONITOR, &title, children)
        .with_id(&id)
        .with_crumb(&display.name)
        .with_text(&display_value(lang, display))
        .hidden_from_search()
}

/// Действия над выводом: питание, режим, масштаб, поворот, VRR, позиция.
///
/// Всё уходит через `niri msg output`, а niri сам пишет в `--help`, что эти
/// изменения временные и в конфиг не попадают. Записи в
/// `~/.config/niri/outputs.kdl` здесь нет намеренно: это отдельное решение
/// с бэкапом файла, а не побочный эффект нажатия на режим.
pub(crate) fn control_rows(lang: Language, display: &Display) -> Vec<Node> {
    if !name_is_safe(&display.name) {
        return Vec::new();
    }
    let id = display.row_id();
    let name = display.name.as_str();
    let mut rows = Vec::new();

    if !display.modes.is_empty() {
        let current = display.current().map(mode_argument);
        let modes: Vec<Node> = display
            .modes
            .iter()
            .map(|mode| {
                let argument = mode_argument(mode);
                let is_current = current.as_deref() == Some(argument.as_str());
                let mut title = mode.human_label(lang);
                if is_current {
                    title.push_str(label(lang, " · текущий", " · current"));
                }
                if mode.is_preferred {
                    title.push_str(label(lang, " · предпочтительный", " · preferred"));
                }
                Node::action(settings_icons::TACHOMETER, &title, mode_action(name, mode))
                    .with_id(&format!("{id}/mode/{argument}"))
            })
            .collect();
        rows.push(
            Node::submenu(
                settings_icons::TACHOMETER,
                label(lang, "Режим", "Mode"),
                modes,
            )
            .with_id(&format!("{id}/modes")),
        );
    }

    let current_scale = display.logical.as_ref().map(|logical| logical.scale);
    let scales: Vec<Node> = SCALE_CHOICES
        .into_iter()
        .map(|scale| {
            let is_current = current_scale.is_some_and(|current| current == scale);
            let mut title = format_scale(scale);
            if is_current {
                title.push_str(label(lang, " · текущий", " · current"));
            }
            Node::action(settings_icons::ZOOM_IN, &title, scale_action(name, scale))
                .with_id(&format!("{id}/scale/{title}"))
        })
        .collect();
    rows.push(
        Node::submenu(
            settings_icons::ZOOM_IN,
            label(lang, "Масштаб", "Scale"),
            scales,
        )
        .with_id(&format!("{id}/scales")),
    );

    let current_transform = display
        .logical
        .as_ref()
        .and_then(|logical| Transform::parse(&logical.transform));
    let transforms: Vec<Node> = Transform::ALL
        .into_iter()
        .map(|transform| {
            let is_current = current_transform == Some(transform);
            let mut title = transform.title(lang).to_string();
            if is_current {
                title.push_str(label(lang, " · текущий", " · current"));
            }
            Node::action(
                settings_icons::ROTATE,
                &title,
                transform_action(name, transform),
            )
            .with_id(&format!("{id}/transform/{transform:?}"))
        })
        .collect();
    rows.push(
        Node::submenu(
            settings_icons::ROTATE,
            label(lang, "Поворот", "Rotation"),
            transforms,
        )
        .with_id(&format!("{id}/transforms")),
    );

    // VRR предлагается только там, где вывод его вообще умеет: у ноутбучной
    // панели `vrr_supported: false`, и кнопка была бы заведомым отказом.
    if display.vrr_supported {
        rows.push(
            Node::action(
                settings_icons::DOT,
                if display.vrr_enabled {
                    label(lang, "VRR: выключить", "VRR: off")
                } else {
                    label(lang, "VRR: включить", "VRR: on")
                },
                vrr_action(name, !display.vrr_enabled),
            )
            .with_id(&format!("{id}/vrr")),
        );
    }

    let mut position = vec![
        Node::action(
            settings_icons::CROSSHAIR,
            label(lang, "Автоматически", "Automatic"),
            position_auto_action(name),
        )
        .with_id(&format!("{id}/position/auto")),
    ];
    for direction in [
        (Nudge::Left, label(lang, "Левее", "Left")),
        (Nudge::Right, label(lang, "Правее", "Right")),
        (Nudge::Up, label(lang, "Выше", "Up")),
        (Nudge::Down, label(lang, "Ниже", "Down")),
    ] {
        let Some((x, y)) = display.nudged(direction.0) else {
            continue;
        };
        position.push(
            Node::action(
                settings_icons::CROSSHAIR,
                direction.1,
                position_action(name, x, y),
            )
            .with_id(&format!("{id}/position/{direction:?}")),
        );
    }
    rows.push(
        Node::submenu(
            settings_icons::CROSSHAIR,
            label(lang, "Позиция", "Position"),
            position,
        )
        .with_id(&format!("{id}/positions")),
    );

    rows
}

/// Подпись сразу на двух языках: строки действий короткие, и переводить их
/// дважды в коде — значит разойтись.
pub(crate) fn label<'a>(lang: Language, ru: &'a str, en: &'a str) -> &'a str {
    if lang == Language::Ru { ru } else { en }
}

/// Строки раздела «Дисплеи» из готового снимка.
pub fn build_nodes(snapshot: &DisplaysSnapshot, lang: Language) -> Vec<Node> {
    let mut rows = Vec::new();
    if let Block::Ready(list) = &snapshot.outputs {
        if list.is_empty() {
            rows.push(status_row(lang, snapshot));
        } else {
            let connected = list.len();
            let value = if ru(lang) {
                format!(
                    "{connected} {}",
                    if connected == 1 {
                        "подключён"
                    } else {
                        "подключено"
                    }
                )
            } else {
                format!("{connected} connected")
            };
            rows.push(
                Node::header(label(lang, "Дисплеи", "Displays"))
                    .with_text(&value)
                    .with_id("displays/header"),
            );
        }
    } else {
        rows.push(status_row(lang, snapshot));
    }
    if let Block::Ready(list) = &snapshot.outputs
        && !list.is_empty()
    {
        if let Some(presets) = preset_rows(list, lang) {
            rows.push(presets);
        }
        let on_count = list.iter().filter(|display| display.is_on()).count();
        rows.extend(
            list.iter()
                .map(|display| display_submenu(lang, display, on_count)),
        );
    }
    rows.push(
        Node::action(
            settings_icons::REFRESH,
            if ru(lang) {
                "Обновить"
            } else {
                "Refresh"
            },
            Action::RefreshDynamic(ProviderKey::OUTPUTS),
        )
        .with_id("displays/refresh"),
    );
    rows
}

use super::*;

pub(crate) fn ru(lang: Language) -> bool {
    lang == Language::Ru
}

/// Отметка типа защиты: замок для закрытой сети, точка для открытой.
pub(crate) fn security_mark(security: Security) -> &'static str {
    match security {
        Security::Open => "открытая",
        Security::Secured => "защищённая",
    }
}

pub(crate) fn net_row_id(net: &WifiNet) -> String {
    net.row_id()
}

/// Строка сети: подпись, личность и действие по её состоянию.
pub(crate) fn net_row(lang: Language, net: &WifiNet, active_uuid: Option<&str>) -> Node {
    let mut title = format!(
        "{} · {}% · {}",
        net.ssid,
        net.signal,
        security_mark(net.security)
    );
    if net.in_use {
        title.push_str(if ru(lang) {
            " · текущая"
        } else {
            " · current"
        });
    }
    let id = net_row_id(net);
    if net.in_use {
        // Отключиться можно только когда известен UUID профиля: имя из эфира
        // в `nmcli connection down` не годится.
        return match active_uuid {
            Some(uuid) => {
                Node::action(settings_icons::MINUS, &title, disconnect_action(uuid)).with_id(&id)
            }
            None => Node::info(settings_icons::MINUS, &title).with_id(&id),
        };
    }
    if let Some(uuid) = &net.saved_uuid {
        return Node::action(settings_icons::NETWORK, &title, connect_saved_action(uuid))
            .with_id(&id);
    }
    match net.security {
        // Открытая сеть без профиля подключается как есть: пароль не нужен,
        // UUID нет — по имени, отдельным argv-элементом.
        Security::Open => Node::action(
            settings_icons::NETWORK,
            &title,
            connect_open_action(&net.ssid),
        )
        .with_id(&id),
        // Защищённая без профиля: пароля ещё нет, поэтому строка не
        // подключает, а открывает ввод. Команда соберётся только после
        // `Enter` в поле пароля.
        Security::Secured => Node::action(
            settings_icons::DOT,
            &title,
            Action::SecretInput(SecretTarget::wifi(&net.ssid, &id)),
        )
        .with_id(&id),
    }
}

/// Статусная строка: «Wi-Fi: вкл · <сеть|не подключено>». Сама строка —
/// переключатель радио (`Enter` гасит и включает), поэтому отдельной строки
/// «Выключить Wi-Fi» под ней нет: она повторяла то же самое вторым способом.
pub(crate) fn status_row(lang: Language, snapshot: &WifiSnapshot) -> Node {
    let label = match (&snapshot.radio, &snapshot.device) {
        (Block::Missing, _) | (_, Block::Missing) => {
            return Node::info(
                settings_icons::DOT,
                if ru(lang) {
                    "Wi-Fi: нет nmcli"
                } else {
                    "Wi-Fi: no nmcli"
                },
            );
        }
        (_, Block::Ready(device)) if device.name.is_empty() => {
            return Node::info(
                settings_icons::NETWORK,
                if ru(lang) {
                    "Нет Wi-Fi адаптера"
                } else {
                    "No Wi-Fi adapter"
                },
            );
        }
        _ => "Wi-Fi",
    };
    let radio = match &snapshot.radio {
        Block::Ready(true) => {
            if ru(lang) {
                "вкл"
            } else {
                "on"
            }
        }
        Block::Ready(false) => {
            if ru(lang) {
                "выкл"
            } else {
                "off"
            }
        }
        // Ошибка чтения — это «не знаю», а не «выкл»: техническая неудача
        // не должна выглядеть выключенным радио. `Missing` сюда не доходит
        // (выше — ранний возврат «нет nmcli»), но если дойдёт, это тоже не
        // «выкл».
        Block::Failed(_) | Block::Missing => {
            if ru(lang) {
                "неизвестно"
            } else {
                "unknown"
            }
        }
    };
    let link = match snapshot.active.as_ref() {
        Some(active) => active.name.clone(),
        None => {
            if ru(lang) {
                "не подключено".to_string()
            } else {
                "not connected".to_string()
            }
        }
    };
    match snapshot.radio.ready().copied() {
        // Состояние известно — строка переключает радио. Неизвестно или
        // нет адаптера (выше — ранние возвраты) — оставляем справкой.
        Some(on) => Node::action(
            settings_icons::NETWORK,
            &format!("{label}: {radio} · {link}"),
            radio_action(!on),
        )
        .with_id("wifi/status"),
        None => Node::info(
            settings_icons::NETWORK,
            &format!("{label}: {radio} · {link}"),
        )
        .with_id("wifi/status"),
    }
}

/// Подменю со всеми доступными сетями.
pub(crate) fn all_networks_submenu(
    lang: Language,
    snapshot: &WifiSnapshot,
    active_uuid: Option<&str>,
) -> Option<Node> {
    let networks = snapshot.networks.ready()?;
    if networks.is_empty() {
        return None;
    }
    let children: Vec<Node> = networks
        .iter()
        .map(|net| net_row(lang, net, active_uuid))
        .collect();
    let title = if ru(lang) {
        format!("Доступные сети ({})", networks.len())
    } else {
        format!("Available networks ({})", networks.len())
    };
    Some(Node::submenu(settings_icons::NETWORK, &title, children).with_id("wifi/networks"))
}

/// Строки раздела «Wi-Fi» из готового снимка.
pub fn build_nodes(snapshot: &WifiSnapshot, lang: Language) -> Vec<Node> {
    let mut rows = vec![status_row(lang, snapshot)];
    if snapshot.has_no_device() {
        return rows;
    }
    // Радио выключено — список сетей не показываем: nmcli его всё равно не
    // отдаёт, а пустой уровень выглядит поломкой.
    let radio_on = snapshot.radio.ready().copied().unwrap_or(true);
    if radio_on
        && let Some(networks) = snapshot.networks.ready()
        && !networks.is_empty()
    {
        let active_uuid = snapshot
            .active
            .as_ref()
            .and_then(|active| active.uuid.as_deref());
        // Сети живут в подменю, а не в корне раздела: в корне статусная
        // строка («Wi-Fi: вкл · сеть») сливалась с первой точкой из списка —
        // отличать их приходилось по иконке. Так же устроен Bluetooth.
        if let Some(submenu) = all_networks_submenu(lang, snapshot, active_uuid) {
            rows.push(submenu);
        }
    }
    if let Some(error) = error_row(lang, snapshot) {
        rows.push(error);
    }
    rows.push(
        Node::action(
            settings_icons::UP,
            if ru(lang) {
                "Обновить"
            } else {
                "Refresh"
            },
            rescan_action(),
        )
        .with_id("wifi/refresh"),
    );
    rows
}

/// Недоступный блок как строка: пустой раздел не молчит.
pub(crate) fn error_row(lang: Language, snapshot: &WifiSnapshot) -> Option<Node> {
    let reason = match (&snapshot.radio, &snapshot.device, &snapshot.networks) {
        (Block::Failed(reason), _, _) => ("nmcli radio", reason),
        (_, Block::Failed(reason), _) => ("nmcli device", reason),
        (_, _, Block::Failed(reason)) => ("nmcli list", reason),
        _ => return None,
    };
    Some(
        Node::info(
            settings_icons::DOT,
            &format!(
                "{}: {}",
                reason.0,
                first_line(&provider_text(lang, reason.1))
            ),
        )
        .with_id("wifi/error"),
    )
}

/// NetworkManager отвечает на частый пересканирование не ошибкой, а отказом по
/// rate limit. Это не сбой раздела: список сетей и так актуален, поэтому
/// вызывающий показывает предупреждение, а не ошибку, и всё равно
/// перезапрашивает снимок.
pub fn is_rate_limited(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("too frequent") || lower.contains("слишком часто")
}

pub(crate) fn first_line(reason: &str) -> String {
    reason.lines().next().unwrap_or(reason).trim().to_string()
}

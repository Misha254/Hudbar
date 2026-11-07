use super::*;

pub(crate) fn ru(lang: Language) -> bool {
    lang == Language::Ru
}

/// Статусная строка раздела: «Bluetooth: вкл · <имя>». Сама строка —
/// переключатель питания (`Enter` гасит и включает), поэтому отдельной строки
/// «Выключить Bluetooth» под ней нет: она повторяла то же самое вторым
/// способом.
pub(crate) fn status_row(lang: Language, snapshot: &BluetoothSnapshot) -> Node {
    let title = match &snapshot.adapter {
        Block::Missing => {
            return Node::info(
                settings_icons::DOT,
                if ru(lang) {
                    "Bluetooth недоступен"
                } else {
                    "Bluetooth is unavailable"
                },
            )
            .with_id("bluetooth/status");
        }
        Block::Ready(adapter) if adapter.address.is_empty() => {
            return Node::info(
                settings_icons::BLUETOOTH,
                if ru(lang) {
                    "Нет Bluetooth адаптера"
                } else {
                    "No Bluetooth adapter"
                },
            )
            .with_id("bluetooth/status");
        }
        Block::Ready(adapter) => match adapter.powered {
            Some(true) => {
                if ru(lang) {
                    "Bluetooth: вкл"
                } else {
                    "Bluetooth: on"
                }
            }
            Some(false) => {
                if ru(lang) {
                    "Bluetooth: выкл"
                } else {
                    "Bluetooth: off"
                }
            }
            // Поля `Powered` просто не было в выводе: «выключен» тут был бы
            // выдумкой.
            None => {
                if ru(lang) {
                    "Bluetooth: состояние неизвестно"
                } else {
                    "Bluetooth: state unknown"
                }
            }
        },
        Block::Failed(_) => {
            return Node::info(
                settings_icons::DOT,
                if ru(lang) {
                    "Bluetooth: состояние неизвестно"
                } else {
                    "Bluetooth: state unknown"
                },
            )
            .with_id("bluetooth/status");
        }
    };
    let name = snapshot
        .adapter
        .ready()
        .and_then(|adapter| adapter.name.clone())
        .filter(|name| !name.trim().is_empty());
    let text = match name {
        Some(name) => format!("{title} · {name}"),
        None => title.to_string(),
    };
    let powered = snapshot.adapter.ready().and_then(|adapter| adapter.powered);
    match powered {
        // Состояние известно — строка переключает питание. Неизвестно (или
        // нет адаптера, это выше — ранние возвраты) — оставляем справкой:
        // действовать по неизвестному состоянию нельзя.
        Some(on) => Node::action(settings_icons::BLUETOOTH, &text, power_action(!on))
            .with_id("bluetooth/status"),
        None => Node::info(settings_icons::BLUETOOTH, &text).with_id("bluetooth/status"),
    }
}

/// Подпись устройства: имя плюс «· подключено», когда точно подключено.
/// Неизвестное состояние подписи не меняет: «подключено?» — это уже вопрос,
/// а не подпись.
pub(crate) fn device_title(lang: Language, device: &BluetoothDevice) -> String {
    let mut title = device.title();
    if device.connected == Some(true) {
        title.push_str(if ru(lang) {
            " · подключено"
        } else {
            " · connected"
        });
    }
    title
}

/// Строки раздела «Bluetooth» из готового снимка.
pub fn build_nodes(snapshot: &BluetoothSnapshot, lang: Language) -> Vec<Node> {
    let mut rows = vec![status_row(lang, snapshot)];
    if snapshot.adapter.failure().is_some() {
        rows.push(
            Node::info(
                settings_icons::DOT,
                if ru(lang) {
                    "Состояние адаптера недоступно"
                } else {
                    "Adapter state is unavailable"
                },
            )
            .with_id("bluetooth/adapter-error"),
        );
    }
    // Строка поиска — только когда адаптер реально есть: `scan` по
    // несуществующему контроллеру заведомо провал, а строка статуса уже
    // объяснила, что адаптера нет.
    if snapshot
        .adapter
        .ready()
        .is_some_and(|adapter| !adapter.address.is_empty())
    {
        rows.push(scan_row(lang, snapshot));
    }
    if let Some(row) = block_failure_row(
        lang,
        "Сопряжённые: список не прочитался",
        "Paired: the list was not read",
        "bluetooth/paired-error",
        &snapshot.paired,
    ) {
        rows.push(row);
    }
    rows.push(device_list(
        lang,
        "Сопряжённые",
        "Paired",
        "Нет устройств",
        "No devices",
        snapshot.paired_sorted(),
        &snapshot.infos,
    ));
    if let Some(row) = block_failure_row(
        lang,
        "Найденные: список не прочитался",
        "Found: the list was not read",
        "bluetooth/known-error",
        &snapshot.known,
    ) {
        rows.push(row);
    }
    rows.push(device_list(
        lang,
        "Найденные",
        "Found",
        "Нет устройств",
        "No devices",
        snapshot.discovered_sorted(),
        &snapshot.infos,
    ));
    rows.push(
        Node::action(
            settings_icons::UP,
            if ru(lang) {
                "Обновить"
            } else {
                "Refresh"
            },
            super::super::action::Action::RefreshDynamic(ProviderKey::BLUETOOTH),
        )
        .with_id("bluetooth/refresh"),
    );
    rows
}

/// Строка поиска: запускает сканирование на [`SCAN_SECONDS`] секунд и после
/// него обновляет список. Пока сканирование идёт, строка занята подписью
/// «выполняется…», поэтому клик не выглядит холостым.
pub(crate) fn scan_row(lang: Language, snapshot: &BluetoothSnapshot) -> Node {
    let discovering = snapshot
        .adapter
        .ready()
        .and_then(|adapter| adapter.discovering);
    let title = match discovering {
        Some(true) => {
            if ru(lang) {
                "Поиск: идёт"
            } else {
                "Scan: running"
            }
        }
        Some(false) => {
            if ru(lang) {
                "Начать поиск"
            } else {
                "Start scan"
            }
        }
        // Строки `Discovering` просто не было в выводе: «поиск не идёт» тут
        // был бы выдумкой, как и «выключен» у питания.
        None => {
            if ru(lang) {
                "Поиск: состояние неизвестно"
            } else {
                "Scan: state unknown"
            }
        }
    };
    match discovering {
        Some(false) => {
            Node::action(settings_icons::SEARCH, title, scan_action()).with_id("bluetooth/scan")
        }
        // Идёт чужое сканирование или состояние неизвестно — запускать второй
        // процесс нельзя, а останавливать чужой HUDbar не вправе.
        Some(true) | None => Node::info(settings_icons::SEARCH, title).with_id("bluetooth/scan"),
    }
}

/// Подменю одного устройства: первая строка — главное действие (подключить/
/// отключить/сопрячь), дальше доверие и удаление, потом статус. Личность
/// подменю — MAC, а не имя: два одинаковых AirPods остаются разными.
pub(crate) fn device_row(
    lang: Language,
    device: &BluetoothDevice,
    details: Option<&DeviceDetails>,
) -> Node {
    Node::submenu(
        settings_icons::BLUETOOTH,
        &device_title(lang, device),
        device_children(lang, device, details),
    )
    .with_id(&device.row_id())
}

/// Строки внутри подменю устройства. Первичное действие сверху — самое
/// частое: подключён → «Отключить», сопряжён но не подключён → «Подключить»,
/// известен BlueZ, но не сопряжён → «Сопрячь».
///
/// Неизвестное состояние первичного действия не даёт: вместо
/// Pair/Connect/Remove — строка «состояние неизвестно». Действие по
/// недостоверному состоянию (сопрячь то, что, возможно, уже сопряжено)
/// хуже его отсутствия. Trust/Remove/info — только при достоверно
/// известном сопряжении или подключении.
pub(crate) fn device_children(
    lang: Language,
    device: &BluetoothDevice,
    details: Option<&DeviceDetails>,
) -> Vec<Node> {
    let mut rows = Vec::new();
    match (device.paired, device.connected) {
        (_, Some(true)) => {
            rows.push(
                Node::action(
                    settings_icons::MINUS,
                    if ru(lang) {
                        "Отключить"
                    } else {
                        "Disconnect"
                    },
                    disconnect_action(&device.address),
                )
                .with_id(&format!("{}/disconnect", device.row_id())),
            );
        }
        (Some(true), Some(false)) => {
            rows.push(
                Node::action(
                    settings_icons::NETWORK,
                    if ru(lang) {
                        "Подключить"
                    } else {
                        "Connect"
                    },
                    connect_action(&device.address),
                )
                .with_id(&format!("{}/connect", device.row_id())),
            );
        }
        (Some(false), _) => {
            rows.push(
                Node::action(
                    settings_icons::PLUS,
                    if ru(lang) { "Сопрячь" } else { "Pair" },
                    pair_action(&device.address),
                )
                .with_id(&format!("{}/pair", device.row_id())),
            );
        }
        // `paired` неизвестно, либо сопряжено, но `connected` неизвестно:
        // гадать, что делать с устройством, нельзя.
        _ => {
            rows.push(
                Node::info(
                    settings_icons::DOT,
                    if ru(lang) {
                        "Состояние неизвестно: список не прочитался"
                    } else {
                        "State unknown: the list was not read"
                    },
                )
                .with_id(&format!("{}/unknown", device.row_id())),
            );
        }
    }

    if device.paired == Some(true) || device.connected == Some(true) {
        let trusted = details.and_then(|detail| detail.trusted);
        if trusted == Some(true) {
            rows.push(
                Node::action(
                    settings_icons::DOT,
                    if ru(lang) {
                        "Не доверять"
                    } else {
                        "Untrust"
                    },
                    trust_action(&device.address, false),
                )
                .with_id(&format!("{}/untrust", device.row_id())),
            );
        } else {
            rows.push(
                Node::action(
                    settings_icons::DOT,
                    if ru(lang) {
                        "Доверять"
                    } else {
                        "Trust"
                    },
                    trust_action(&device.address, true),
                )
                .with_id(&format!("{}/trust", device.row_id())),
            );
        }
        rows.push(
            Node::action(
                settings_icons::MINUS,
                if ru(lang) { "Удалить" } else { "Remove" },
                remove_action(&device.address),
            )
            .with_id(&format!("{}/remove", device.row_id())),
        );
        rows.push(info_line(
            &device.row_id(),
            "trusted",
            match details.and_then(|detail| detail.trusted) {
                Some(true) => {
                    if ru(lang) {
                        "Доверено: да".to_string()
                    } else {
                        "Trusted: yes".to_string()
                    }
                }
                Some(false) => {
                    if ru(lang) {
                        "Доверено: нет".to_string()
                    } else {
                        "Trusted: no".to_string()
                    }
                }
                None => {
                    if ru(lang) {
                        "Доверено: —".to_string()
                    } else {
                        "Trusted: —".to_string()
                    }
                }
            },
        ));
        rows.push(info_line(
            &device.row_id(),
            "rssi",
            match details.and_then(|detail| detail.rssi) {
                Some(rssi) => format!("RSSI: {rssi} dBm"),
                None => "RSSI: —".to_string(),
            },
        ));
        rows.push(info_line(
            &device.row_id(),
            "battery",
            match details.and_then(|detail| detail.battery) {
                Some(battery) => {
                    if ru(lang) {
                        format!("Батарея: {battery}%")
                    } else {
                        format!("Battery: {battery}%")
                    }
                }
                None => {
                    if ru(lang) {
                        "Батарея: —".to_string()
                    } else {
                        "Battery: —".to_string()
                    }
                }
            },
        ));
        rows.push(info_line(
            &device.row_id(),
            "uuids",
            match details {
                Some(detail) if detail.uuids > 0 => {
                    if ru(lang) {
                        format!("Служб: {}", detail.uuids)
                    } else {
                        format!("Services: {}", detail.uuids)
                    }
                }
                _ => {
                    if ru(lang) {
                        "Служб: —".to_string()
                    } else {
                        "Services: —".to_string()
                    }
                }
            },
        ));
    }
    rows
}

/// Статусная строка внутри подменю устройства: обычная `Info`, лично по
/// устройству и по ключу поля.
pub(crate) fn info_line(device_id: &str, key: &str, text: String) -> Node {
    let (title, value) = text.split_once(':').unwrap_or(("", text.as_str()));
    Node::info_value(title.trim(), value.trim()).with_id(&format!("{device_id}/info/{key}"))
}

/// Строка о непрочитанном списке: пустой подменю «Нет устройств» молчит о
/// причине, а `Failed` — это не «пусто», а «не знаю». Пусто/успех — строки
/// нет, чтобы не шуметь.
pub(crate) fn block_failure_row<T>(
    lang: Language,
    ru_text: &str,
    en_text: &str,
    id: &str,
    block: &Block<T>,
) -> Option<Node> {
    match block {
        Block::Failed(_) => Some(
            Node::info(
                settings_icons::DOT,
                if ru(lang) { ru_text } else { en_text },
            )
            .with_id(id),
        ),
        _ => None,
    }
}

/// Подменю со списком устройств. Длинный список обрезается до
/// [`MAX_VISIBLE_DEVICES`], а остальное уходит в подменю «Все»: терять
/// устройства молча нельзя, а поиск меню по динамическим строкам не ходит.
pub(crate) fn device_list(
    lang: Language,
    ru_title: &'static str,
    en_title: &'static str,
    ru_empty: &'static str,
    en_empty: &'static str,
    devices: Vec<BluetoothDevice>,
    infos: &HashMap<String, DeviceDetails>,
) -> Node {
    let title = if ru(lang) { ru_title } else { en_title };
    let empty = if ru(lang) { ru_empty } else { en_empty };
    let children: Vec<Node> = if devices.is_empty() {
        vec![Node::info(settings_icons::DOT, empty)]
    } else {
        let mut rows: Vec<Node> = devices
            .iter()
            .take(MAX_VISIBLE_DEVICES)
            .map(|device| device_row(lang, device, infos.get(&device.address)))
            .collect();
        if devices.len() > MAX_VISIBLE_DEVICES {
            let rest: Vec<Node> = devices[MAX_VISIBLE_DEVICES..]
                .iter()
                .map(|device| device_row(lang, device, infos.get(&device.address)))
                .collect();
            let all = if ru(lang) {
                format!("Все ({})", devices.len())
            } else {
                format!("All ({})", devices.len())
            };
            rows.push(
                Node::submenu(settings_icons::DOT, &all, rest)
                    .with_id(&format!("bluetooth/all/{title}")),
            );
        }
        rows
    };
    Node::submenu(settings_icons::BLUETOOTH, title, children)
        .with_id(&format!("bluetooth/list/{title}"))
}

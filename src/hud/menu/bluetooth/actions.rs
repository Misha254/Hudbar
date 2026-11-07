use super::*;

pub fn power_action(on: bool) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("bluetoothctl")
            .arg("power")
            .arg(if on { "on" } else { "off" })
            .with_timeout(DEVICE_TIMEOUT),
        refresh: ProviderKey::BLUETOOTH,
    }
}

/// Начать поиск устройств. Однократное сканирование на [`SCAN_SECONDS`]
/// секунд: `bluetoothctl scan on` в неинтерактивном режиме завершается сразу,
/// и поиск гаснет вместе с процессом. `--timeout` держит его живым, а
/// `RefreshAndRun` обновляет список уже после сканирования.
pub fn scan_action() -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("bluetoothctl")
            .arg("--timeout")
            .arg(&SCAN_SECONDS.to_string())
            .arg("scan")
            .arg("on")
            .with_timeout(SCAN_TIMEOUT),
        refresh: ProviderKey::BLUETOOTH,
    }
}

/// Подключить устройство: `bluetoothctl connect <MAC>`.
pub fn connect_action(address: &str) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("bluetoothctl")
            .arg("connect")
            .arg(address)
            .with_timeout(DEVICE_TIMEOUT),
        refresh: ProviderKey::BLUETOOTH,
    }
}

/// Отключить устройство: `bluetoothctl disconnect <MAC>`.
pub fn disconnect_action(address: &str) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("bluetoothctl")
            .arg("disconnect")
            .arg(address)
            .with_timeout(DEVICE_TIMEOUT),
        refresh: ProviderKey::BLUETOOTH,
    }
}

/// Сопрячь устройство: `bluetoothctl --agent NoInputNoOutput pair <MAC>`.
///
/// Временный агент нужен: без него `pair` не срабатывает на «Just Works»-
/// устройствах, где PIN не запрашивается, а агент desktop-сессии не
/// зарегистрирован. Для PIN-устройств pair упадёт явной ошибкой, а не
/// повисит на запросе ввода.
pub fn pair_action(address: &str) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("bluetoothctl")
            .arg("--agent")
            .arg("NoInputNoOutput")
            .arg("pair")
            .arg(address)
            .with_timeout(PAIR_TIMEOUT),
        refresh: ProviderKey::BLUETOOTH,
    }
}

/// Удалить устройство: `bluetoothctl remove <MAC>`. Удаление разрывает и
/// связь, и доверие — то же, что unpair + untrust в один шаг.
pub fn remove_action(address: &str) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("bluetoothctl")
            .arg("remove")
            .arg(address)
            .with_timeout(DEVICE_TIMEOUT),
        refresh: ProviderKey::BLUETOOTH,
    }
}

/// Доверять или не доверять: `bluetoothctl trust|untrust <MAC>`.
pub fn trust_action(address: &str, trust: bool) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("bluetoothctl")
            .arg(if trust { "trust" } else { "untrust" })
            .arg(address)
            .with_timeout(DEVICE_TIMEOUT),
        refresh: ProviderKey::BLUETOOTH,
    }
}

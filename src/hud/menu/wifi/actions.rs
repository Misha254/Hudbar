use super::*;

/// Радио вкл/выкл: `nmcli radio wifi on|off`.
pub fn radio_action(on: bool) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("nmcli")
            .arg("radio")
            .arg("wifi")
            .arg(if on { "on" } else { "off" }),
        refresh: ProviderKey::WIFI,
    }
}

/// Отключиться: `nmcli connection down uuid <UUID>`.
pub fn disconnect_action(uuid: &str) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("nmcli")
            .arg("connection")
            .arg("down")
            .arg("uuid")
            .arg(uuid),
        refresh: ProviderKey::WIFI,
    }
}

/// Включить сохранённую сеть: `nmcli connection up uuid <UUID>`.
/// Им же подключается панель бара, поэтому команда собрана отдельно.
pub fn connect_saved_spec(uuid: &str) -> CommandSpec {
    CommandSpec::new("nmcli")
        .arg("connection")
        .arg("up")
        .arg("uuid")
        .arg(uuid)
        .with_timeout(CONNECT_TIMEOUT)
}

/// Включить сохранённую сеть (действие меню).
pub fn connect_saved_action(uuid: &str) -> Action {
    Action::RefreshAndRun {
        command: connect_saved_spec(uuid),
        refresh: ProviderKey::WIFI,
    }
}

/// Подключиться к открытой сети без профиля: `nmcli device wifi connect <SSID>`.
/// Единственное место, где SSID из эфира попадает в argv: имя сети может
/// начинаться с `-`, и такое nmcli принял бы за опцию, поэтому перед таким
/// SSID ставится `--`.
pub fn connect_open_action(ssid: &str) -> Action {
    let mut command = CommandSpec::new("nmcli")
        .arg("device")
        .arg("wifi")
        .arg("connect");
    if ssid.starts_with('-') {
        command = command.arg("--");
    }
    Action::RefreshAndRun {
        command: command.arg(ssid).with_timeout(CONNECT_TIMEOUT),
        refresh: ProviderKey::WIFI,
    }
}

/// Подключение к защищённой сети с паролем:
/// `nmcli --ask device wifi connect <SSID>`, пароль — по stdin.
///
/// Пароля нет ни в `argv` (а значит, и в `ps` с `/proc/<pid>/cmdline`), ни в
/// журнале: `--ask` разрешает nmcli спросить недостающий секрет, а спрашивает
/// он через readline поверх stdin (`nmc_readline_echo` в NetworkManager), так
/// что хватает pipe — pty не нужен.
///
/// Отсюда следствие для вызывающего: `stderr` этой команды нельзя писать в
/// журнал как есть — nmcli умеет вернуть в сообщении сам пароль
/// (`'%s' is not valid WPA PSK`). Строка журнала у описания своя,
/// с `<stdin redacted>`.
///
/// Правило для SSID с ведущим дефисом — то же, что у открытой сети в W5.2a.
pub fn connect_with_password(ssid: &str, password: &str) -> CommandSpec {
    let mut command = CommandSpec::new("nmcli")
        .arg("--ask")
        .arg("device")
        .arg("wifi")
        .arg("connect");
    if ssid.starts_with('-') {
        command = command.arg("--");
    }
    command
        .arg(ssid)
        .stdin(SecretData::from_str(password))
        .with_timeout(CONNECT_TIMEOUT)
}

/// Обновление с пересканированием: `nmcli device wifi rescan`, затем refresh.
pub fn rescan_action() -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("nmcli")
            .arg("device")
            .arg("wifi")
            .arg("rescan")
            .with_timeout(CONNECT_TIMEOUT),
        refresh: ProviderKey::WIFI,
    }
}

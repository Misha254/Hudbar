//! Динамический раздел «Bluetooth»: провайдер поверх `bluetoothctl`.
//!
//! W5.3b: раздел умеет действия над адаптером и устройствами. Адаптер —
//! включение/выключение, поиск; устройства — подключить/отключить,
//! доверять/не доверять, удалить. Сопряжённые и найденные — два отдельных
//! списка.
//!
//! Четыре вызова на одном обновлении — и ни одного `info <MAC>`:
//!
//! | команда | что берём |
//! |---|---|
//! | `bluetoothctl show` | адрес контроллера, имя, `Powered`, `Discovering` |
//! | `bluetoothctl devices Paired` | список сопряжённых |
//! | `bluetoothctl devices Connected` | список подключённых |
//! | `bluetoothctl devices` | все известные устройства (то, что BlueZ уже знает) |
//!
//! Почему именно так. `bluetoothctl paired-devices` в bluez 5.87 не существует
//! (`Invalid command in menu main`), а фильтр `devices Paired` есть в этой же
//! версии и отдаёт тот же список одним вызовом. Состояние `Connected` в общем
//! выводе `devices` не печатается, поэтому оно берётся вторым фильтром того же
//! списка. Сканирование не запускается автоматически: `scan on`
//! меняет состояние адаптера, «Найденные» — это то, что BlueZ уже знает.
//!
//! `info <MAC>` обычный refresh не делает вовсе: один вызов на устройство —
//! это N+1, растущий с числом гаджетов. Trusted/RSSI/Battery/Службы поэтому
//! показываются как «—», пока не появится on-demand чтение выбранного
//! устройства отдельным запросом.
//!
//! Разбор локальный и построчный: строка `Device AA:BB:CC:DD:EE:01 Имя`
//! делится по первому пробелу после MAC, а имя берётся целиком — пробелы,
//! двоеточия, обратные слэши, кавычки и юникод в именах разрешены. Личность
//! устройства — только MAC: имя у AirPods одинаковое у всех, а адрес разный.
//! Действия идут через `CommandSpec` (argv, без shell), занятая строка — тем
//! же `CommandJob.row`, что и в Wi-Fi.

use super::action::Action;
use super::settings::Language;
use super::settings_icons;
use super::system::{
    CommandError, CommandRunner, CommandSpec, ProviderError, ProviderKey, SystemCommandRunner,
    SystemProvider,
};
use super::tree::Node;
use std::collections::HashMap;
use std::time::Duration;

/// Сколько устройств показывать в каждом списке: больше не влезет в карточку,
/// а найти можно будет поиском и разделом «Все».
pub const MAX_VISIBLE_DEVICES: usize = 12;

/// Операция над устройством: `connect`/`disconnect`/`power`/`scan`/`trust`/
/// `remove` могут ждать ответа гаджета, односекундный таймаут `oneshot` тут
/// не годится.
const DEVICE_TIMEOUT: Duration = Duration::from_secs(8);
/// Pairing ждёт регистрации агента и, возможно, PIN: даём больше, а не
/// держим меню.
const PAIR_TIMEOUT: Duration = Duration::from_secs(20);

/// Состояние одного блока раздела. Отсутствие поля — не «нет», а «не знаю»:
/// `Missing` значит «инструмента нет», `Failed` — «команда ответила не тем».
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block<T> {
    /// Данные есть.
    Ready(T),
    /// Инструмента нет: блок пуст, раздел в целом жив.
    Missing,
    /// Инструмент ответил, но данных из него не получилось.
    Failed(String),
}

impl<T> Default for Block<T> {
    /// Пустой снимок без единого вызова: блок «инструмента нет». Так удобно
    /// собирать снимок в тестах и не выдумывать четвёртое состояние.
    fn default() -> Self {
        Block::Missing
    }
}

impl<T> Block<T> {
    /// Данные блока, если они есть.
    pub fn ready(&self) -> Option<&T> {
        match self {
            Block::Ready(value) => Some(value),
            _ => None,
        }
    }

    /// Блок пуст по-настоящему: инструмента нет или данных не добыть.
    pub fn is_empty(&self) -> bool {
        !matches!(self, Block::Ready(_))
    }

    /// Есть ли блок, который разочаровал: по нему рисуется причина.
    pub fn failure(&self) -> Option<&str> {
        match self {
            Block::Failed(reason) => Some(reason),
            _ => None,
        }
    }
}

/// Bluetooth-адаптер: то, что говорит `bluetoothctl show`.
///
/// Поля `Option` не из вежливости: у другой версии bluez может не печатать
/// `Discovering`, и «нет строки» — это не то же самое, что «выключено».
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BluetoothAdapter {
    /// MAC контроллера: `AA:BB:CC:DD:EE:FF` в верхнем регистре.
    pub address: String,
    /// Имя или псевдоним контроллера.
    pub name: Option<String>,
    /// Включён ли радиомодуль (`Powered: yes|no`). `None` — строки не было.
    pub powered: Option<bool>,
    /// Идёт ли сканирование (`Discovering: yes|no`). `None` — строки не было.
    pub discovering: Option<bool>,
}

/// Устройство Bluetooth. `paired` и `connected` — `Option`: если список
/// `devices Paired`/`devices Connected` прочитать не удалось, состояние
/// остаётся `None` («не знаю»), а не превращается в `false`. По `None` меню
/// не предлагает Pair/Connect/Remove: действие по недостоверному состоянию
/// хуже его отсутствия.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BluetoothDevice {
    /// MAC в нормализованном виде: единственная личность устройства.
    pub address: String,
    /// Имя устройства. Пусто, если BlueZ его не знает: покажется MAC.
    pub name: String,
    /// Сопряжено ли устройство: сохранено в BlueZ. `None` — список
    /// сопряжённых не прочитался.
    pub paired: Option<bool>,
    /// Подключено ли устройство прямо сейчас. `None` — список подключённых
    /// не прочитался.
    pub connected: Option<bool>,
}

impl BluetoothDevice {
    /// Что показать в списке: имя, а без имени — MAC. Имя может начинаться с
    /// дефиса или содержать двоеточие: это текст на экране, в команды он не
    /// попадает, потому что действий у раздела нет.
    pub fn title(&self) -> String {
        if self.name.trim().is_empty() {
            self.address.clone()
        } else {
            self.name.trim().to_string()
        }
    }

    /// Личность строки: MAC, а не имя. Два одинаковых AirPods остаются
    /// разными строками.
    pub fn row_id(&self) -> String {
        device_row_id(&self.address)
    }
}

/// Личность строки устройства.
pub fn device_row_id(address: &str) -> String {
    format!("bluetooth/device/{address}")
}

/// Статус одного устройства из `bluetoothctl info <MAC>`. Поля `Option`:
/// устройство вне эфира не отдаёт RSSI и батарею, а BlueZ может не писать
/// часть полей — это «не знаю», а не «нет».
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceDetails {
    /// Доверяет ли устройство этому адаптеру (`Trusted: yes|no`).
    pub trusted: Option<bool>,
    /// Уровень сигнала (`RSSI: -62`): есть только у подключённого.
    pub rssi: Option<i32>,
    /// Заряд (`Battery: 84%`): есть, только если сам гаджет его отдаёт.
    pub battery: Option<u8>,
    /// Число служб (строк `UUID:`) — чем их больше, тем «жирнее» профиль.
    pub uuids: usize,
}

/// Разбор `bluetoothctl info <MAC>`. Устройство вне эфира не отдаёт RSSI и
/// батарею — поля остаются `None`.
pub fn parse_device_info(text: &str) -> DeviceDetails {
    let mut details = DeviceDetails::default();
    for line in text.lines() {
        let trimmed = line.trim();
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key {
            "Trusted" => details.trusted = parse_yes_no(value),
            "RSSI" => details.rssi = value.parse().ok(),
            "Battery" | "Percentage" => {
                // "Battery: 84%" или вложенный "Percentage: 84".
                let digits: String = value.chars().filter(|ch| ch.is_ascii_digit()).collect();
                details.battery = digits.parse().ok();
            }
            "UUID" => details.uuids += 1,
            _ => {}
        }
    }
    details
}

/// Типизированный снимок раздела «Bluetooth».
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BluetoothSnapshot {
    /// Адаптер: `Ready` без адреса — адаптера нет, `Missing` — нет
    /// `bluetoothctl`, `Failed` — команда не ответила.
    pub adapter: Block<BluetoothAdapter>,
    /// Устройства, известные BlueZ (вывод `bluetoothctl devices`).
    pub known: Block<Vec<BluetoothDevice>>,
    /// Сопряжённые устройства: есть и те, кого сейчас нет в эфире.
    pub paired: Block<Vec<BluetoothDevice>>,
    /// Подключённые прямо сейчас.
    pub connected: Block<Vec<String>>,
    /// Статус из `info <MAC>`: по MAC. Обычный refresh карту не заполняет —
    /// это был бы N+1 вызов на устройство. Поле — кэш для будущего on-demand
    /// чтения выбранного устройства; пока оно пусто, строки Trusted/RSSI/
    /// Батарея показывают «—».
    pub infos: HashMap<String, DeviceDetails>,
}

impl BluetoothSnapshot {
    /// Сопряжённые устройства в порядке показа: подключённые первыми, потом по
    /// имени, потом по MAC — чтобы порядок был предсказуемым и не зависел от
    /// локали.
    pub fn paired_sorted(&self) -> Vec<BluetoothDevice> {
        let mut list = self.paired.ready().cloned().unwrap_or_default();
        sort_devices(&mut list);
        list
    }

    /// Найденные: всё известное BlueZ, кроме точно сопряжённых — те показаны
    /// в своём списке, дублировать их во втором бессмысленно. Устройства с
    /// неизвестным `paired` остаются здесь: это честно («вижу, но не знаю,
    /// сопряжено ли»), а Pair им не предлагается — см. `device_children`.
    pub fn discovered_sorted(&self) -> Vec<BluetoothDevice> {
        let mut list = self.known.ready().cloned().unwrap_or_default();
        list.retain(|device| device.paired != Some(true));
        sort_devices(&mut list);
        list
    }

    /// Есть ли хоть что-то, что можно показать: адаптер или хоть одно
    /// устройство. Если нет — разделу есть что сказать, но нечего перечислять.
    pub fn is_empty(&self) -> bool {
        self.adapter.is_empty() && self.paired.is_empty() && self.known.is_empty()
    }
}

fn sort_devices(list: &mut [BluetoothDevice]) {
    list.sort_by(|a, b| {
        // `Option<bool>`: `None < Some(false) < Some(true)`. Подключённые
        // первыми, затем точно неподключённые, затем неизвестные — в самом
        // низу, чтобы не вводить в заблуждение.
        b.connected
            .cmp(&a.connected)
            .then_with(|| a.title().to_lowercase().cmp(&b.title().to_lowercase()))
            .then_with(|| a.address.cmp(&b.address))
    });
}

/// Провайдер раздела «Bluetooth».
pub struct BluetoothProvider;

impl SystemProvider for BluetoothProvider {
    type Snapshot = BluetoothSnapshot;

    fn key(&self) -> ProviderKey {
        ProviderKey::BLUETOOTH
    }

    fn fetch(&self, runner: &dyn CommandRunner) -> Result<Self::Snapshot, ProviderError> {
        // `show` и проверка инструмента — один и тот же вызов: без него раздел
        // был бы пятью вызовами вместо четырёх. Отсутствие самого
        // `bluetoothctl` — единственная причина скрыть раздел целиком;
        // сбой `show` оставляет список устройств в покое.
        let adapter = adapter(runner);
        if matches!(adapter, Block::Missing) {
            return Err(ProviderError::Unavailable("нет bluetoothctl".to_string()));
        }
        let connected = addresses(runner, "Connected");
        let known = devices(runner, "");
        let paired = devices(runner, "Paired");

        // Подключённость приходит отдельным списком MAC: в общем выводе её
        // нет. Если список не прочитался, состояние — `None`, а не `false`:
        // считать всё неподключённым было бы выдумкой.
        let connected_addresses: Option<Vec<String>> = connected.ready().cloned();
        // Сопряжённость приезжает отдельным списком: устройство из общего
        // списка тоже сопряжено, просто об этом не сказано в его строке. Без
        // этой пометки оно дублировалось бы ещё и в «Найденных». Непрочитанный
        // список — `None`, а не «все несопряжены».
        let paired_addresses: Option<Vec<String>> = paired
            .ready()
            .map(|list| list.iter().map(|device| device.address.clone()).collect());
        let mut known = known;
        if let Block::Ready(list) = &mut known {
            for device in list.iter_mut() {
                device.connected = connected_addresses
                    .as_ref()
                    .map(|addresses| addresses.contains(&device.address));
                device.paired = paired_addresses
                    .as_ref()
                    .map(|addresses| addresses.contains(&device.address));
            }
        }
        let mut paired = paired;
        if let Block::Ready(list) = &mut paired {
            for device in list.iter_mut() {
                device.paired = Some(true);
                device.connected = connected_addresses
                    .as_ref()
                    .map(|addresses| addresses.contains(&device.address));
            }
        }
        Ok(BluetoothSnapshot {
            adapter,
            known,
            paired,
            connected,
            infos: HashMap::new(),
        })
    }

    fn nodes(&self, snapshot: &Self::Snapshot, lang: Language) -> Vec<Node> {
        build_nodes(snapshot, lang)
    }
}

/// Fetch + nodes одним вызовом: так провайдера зовёт цикл окна.
pub fn fetch_nodes(lang: Language) -> Result<Vec<Node>, String> {
    let provider = BluetoothProvider;
    let snapshot = provider
        .fetch(&SystemCommandRunner::default())
        .map_err(|error| error.message())?;
    Ok(provider.nodes(&snapshot, lang))
}

// ── Действия над адаптером и устройствами ────────────────────────────────
//
// Все идут через `CommandSpec` (argv, без shell) и `RefreshAndRun`: после
// успеха слот BLUETOOTH обновляется, строка занята до ответа worker-а.
// Никаких `sh -c`, никаких статических команд в обход runner-а.

/// Включить или выключить радио адаптера: `bluetoothctl power on|off`.
pub fn power_action(on: bool) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("bluetoothctl")
            .arg("power")
            .arg(if on { "on" } else { "off" })
            .with_timeout(DEVICE_TIMEOUT),
        refresh: ProviderKey::BLUETOOTH,
    }
}

/// Начать или остановить поиск устройств: `bluetoothctl scan on|off`.
pub fn scan_action(on: bool) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("bluetoothctl")
            .arg("scan")
            .arg(if on { "on" } else { "off" })
            .with_timeout(DEVICE_TIMEOUT),
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

/// Блок команды: успех — данные, отсутствие инструмента — `Missing`, сбой —
/// `Failed` с безопасным сообщением вместо сырого stderr.
fn text_block<T, F: FnOnce(&str) -> Option<T>>(
    runner: &dyn CommandRunner,
    spec: &CommandSpec,
    parse: F,
) -> Block<T> {
    match runner.run(spec) {
        Err(CommandError::Spawn(_)) => Block::Missing,
        Err(error) => Block::Failed(error.to_string()),
        Ok(output) if output.success() => match parse(&output.stdout) {
            Some(value) => Block::Ready(value),
            None => Block::Failed(format!("{}: не разобрано", spec.program())),
        },
        Ok(output) => Block::Failed(failure(&output.stderr)),
    }
}

/// Причина сбоя: первая строка stderr или код возврата. Сырой вывод в меню не
/// идёт: он не для человека.
fn failure(stderr: &str) -> String {
    let line = stderr.lines().map(str::trim).find(|line| !line.is_empty());
    match line {
        Some(text) => text.to_string(),
        None => "bluetoothctl: команда не сработала".to_string(),
    }
}

/// Адаптер из `bluetoothctl show`.
fn adapter(runner: &dyn CommandRunner) -> Block<BluetoothAdapter> {
    let spec = CommandSpec::new("bluetoothctl").arg("show");
    text_block(runner, &spec, |text| Some(parse_adapter(text)))
}

/// Разбор `bluetoothctl show`. Строк-«полей» может не быть вовсе: адаптер,
/// у которого BlueZ не печатает `Powered`, — это `powered: None`, а не
/// «выключен».
pub fn parse_adapter(text: &str) -> BluetoothAdapter {
    let mut adapter = BluetoothAdapter::default();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Controller ") {
            let (address, _name) = split_address(rest);
            adapter.address = address;
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key {
            "Name" | "Alias" => {
                if adapter.name.is_none() && !value.is_empty() {
                    adapter.name = Some(value.to_string());
                }
            }
            "Powered" => adapter.powered = parse_yes_no(value),
            "Discovering" => adapter.discovering = parse_yes_no(value),
            _ => {}
        }
    }
    adapter
}

/// `yes`/`no` в любом регистре. Мусор — это `None`, а не `false`.
fn parse_yes_no(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

/// Список MAC из `bluetoothctl devices <filter>`.
fn addresses(runner: &dyn CommandRunner, filter: &str) -> Block<Vec<String>> {
    let mut spec = CommandSpec::new("bluetoothctl").arg("devices");
    if !filter.is_empty() {
        spec = spec.arg(filter);
    }
    text_block(runner, &spec, |text| {
        Some(
            parse_devices(text)
                .into_iter()
                .map(|device| device.address)
                .collect(),
        )
    })
}

/// Список устройств из `bluetoothctl devices <filter>`.
fn devices(runner: &dyn CommandRunner, filter: &str) -> Block<Vec<BluetoothDevice>> {
    let mut spec = CommandSpec::new("bluetoothctl").arg("devices");
    if !filter.is_empty() {
        spec = spec.arg(filter);
    }
    text_block(runner, &spec, |text| Some(parse_devices(text)))
}

/// Разбор `bluetoothctl devices`: строка `Device <MAC> <имя…>`.
///
/// Имя берётся целиком после MAC, поэтому пробелы, двоеточия, обратные
/// слэши, кавычки и юникод остаются как есть. Строки без `Device`, без
/// похожего на MAC адреса и пустые строки молча пропускаются.
pub fn parse_devices(text: &str) -> Vec<BluetoothDevice> {
    let mut list: Vec<BluetoothDevice> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("Device ") else {
            continue;
        };
        let (address, name) = split_address(rest);
        if address.is_empty() {
            continue;
        }
        match list.iter_mut().find(|device| device.address == address) {
            // Один MAC дважды: состояние объединяется, а не теряется.
            Some(kept) => {
                if kept.name.trim().is_empty() && !name.trim().is_empty() {
                    kept.name = name;
                }
            }
            None => list.push(BluetoothDevice {
                address,
                name,
                paired: None,
                connected: None,
            }),
        }
    }
    list
}

/// MAC из начала строки и всё остальное как имя. MAC нормализуется в верхний
/// регистр с двоеточиями — форма, в которой его же показывает BlueZ.
fn split_address(text: &str) -> (String, String) {
    let trimmed = text.trim_start();
    let head: String = trimmed
        .chars()
        .take_while(|ch| *ch != ' ' && *ch != '\t')
        .collect();
    let Some(normalized) = normalize_address(&head) else {
        return (String::new(), trimmed.to_string());
    };
    let name = trimmed[head.len()..].trim().to_string();
    (normalized, name)
}

/// Проверка и нормализация MAC: ровно шесть пар шестнадцатеричных цифр.
pub fn normalize_address(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut out = Vec::with_capacity(6);
    for part in parts {
        if part.len() != 2 || !part.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return None;
        }
        out.push(part.to_ascii_uppercase());
    }
    Some(out.join(":"))
}

fn ru(lang: Language) -> bool {
    lang == Language::Ru
}

/// Статусная строка раздела: «Bluetooth: вкл · <имя>».
fn status_row(lang: Language, snapshot: &BluetoothSnapshot) -> Node {
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
    Node::info(settings_icons::BLUETOOTH, &text).with_id("bluetooth/status")
}

/// Подпись устройства: имя плюс «· подключено», когда точно подключено.
/// Неизвестное состояние подписи не меняет: «подключено?» — это уже вопрос,
/// а не подпись.
fn device_title(lang: Language, device: &BluetoothDevice) -> String {
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
    // Действия над адаптером — только когда он реально есть: power/scan по
    // несуществующему контроллеру заведомо провал, а строка статуса уже
    // объяснила, что адаптера нет.
    if let Some(adapter) = snapshot
        .adapter
        .ready()
        .filter(|adapter| !adapter.address.is_empty())
    {
        if let Some(power) = power_row(lang, adapter) {
            rows.push(power);
        }
        rows.push(
            Node::action(
                settings_icons::NETWORK,
                if ru(lang) {
                    "Начать поиск"
                } else {
                    "Start scanning"
                },
                scan_action(true),
            )
            .with_id("bluetooth/scan/on"),
        );
        rows.push(
            Node::action(
                settings_icons::MINUS,
                if ru(lang) {
                    "Остановить поиск"
                } else {
                    "Stop scanning"
                },
                scan_action(false),
            )
            .with_id("bluetooth/scan/off"),
        );
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
            super::action::Action::RefreshDynamic(ProviderKey::BLUETOOTH),
        )
        .with_id("bluetooth/refresh"),
    );
    rows
}

/// Кнопка включения/выключения радио.
/// `Some(true)` — включён, предлагаем выключить; `Some(false)` — выключен,
/// предлагаем включить; `None` (строки `Powered` не было) — кнопки нет вовсе:
/// включать «на всякий случай» значит действовать по неизвестному состоянию.
fn power_row(lang: Language, adapter: &BluetoothAdapter) -> Option<Node> {
    let title = match adapter.powered {
        Some(true) => {
            if ru(lang) {
                "Выключить Bluetooth"
            } else {
                "Turn Bluetooth off"
            }
        }
        Some(false) => {
            if ru(lang) {
                "Включить Bluetooth"
            } else {
                "Turn Bluetooth on"
            }
        }
        None => return None,
    };
    let action = match adapter.powered {
        Some(true) => power_action(false),
        _ => power_action(true),
    };
    Some(Node::action(settings_icons::BLUETOOTH, title, action).with_id("bluetooth/power"))
}

/// Подменю одного устройства: первая строка — главное действие (подключить/
/// отключить/сопрячь), дальше доверие и удаление, потом статус. Личность
/// подменю — MAC, а не имя: два одинаковых AirPods остаются разными.
fn device_row(lang: Language, device: &BluetoothDevice, details: Option<&DeviceDetails>) -> Node {
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
fn device_children(
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
fn info_line(device_id: &str, key: &str, text: String) -> Node {
    Node::info(settings_icons::DOT, &text).with_id(&format!("{device_id}/info/{key}"))
}

/// Строка о непрочитанном списке: пустой подменю «Нет устройств» молчит о
/// причине, а `Failed` — это не «пусто», а «не знаю». Пусто/успех — строки
/// нет, чтобы не шуметь.
fn block_failure_row<T>(
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
fn device_list(
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

#[cfg(test)]
mod tests {
    use super::super::action::Action;
    use super::super::system::{CommandError, CommandOutput, ScriptedRunner};
    use super::*;

    const SHOW: &str = "Controller AA:BB:CC:DD:EE:FF (public)\n\tManufacturer: 0x001d (29)\n\tName: hci0\n\tAlias: hci0\n\tClass: 0x006c010c (7078156)\n\tPowered: yes\n\tPowerState: on\n\tDiscoverable: no\n\tDiscovering: no\n";
    const SHOW_OFF: &str = "Controller AA:BB:CC:DD:EE:FF (public)\n\tName: hci0\n\tPowered: no\n";
    const SHOW_NO_POWER: &str =
        "Controller AA:BB:CC:DD:EE:FF (public)\n\tName: hci0\n\tDiscoverable: no\n";
    const DEVICES: &str = "Device AA:BB:CC:DD:EE:01 Sony WH-1000XM5\nDevice AA:BB:CC:DD:EE:02 MX Master 3S\nDevice AA:BB:CC:DD:EE:03 Домашняя клавиатура\nDevice AA:BB:CC:DD:EE:04 -dash-device\nDevice AA:BB:CC:DD:EE:05 Мышь: Pro \\ \"v2\" (2024)\n";
    const PAIRED: &str =
        "Device AA:BB:CC:DD:EE:01 Sony WH-1000XM5\nDevice AA:BB:CC:DD:EE:06 Old Device\n";
    const CONNECTED: &str = "Device AA:BB:CC:DD:EE:01 Sony WH-1000XM5\n";

    fn ok(stdout: &str) -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            status: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    fn missing() -> Result<CommandOutput, CommandError> {
        Err(CommandError::Spawn("нет bluetoothctl".to_string()))
    }

    /// Порядок вызовов провайдера: show, devices Connected, devices,
    /// devices Paired — и больше ничего. Обычный refresh не зависит от числа
    /// устройств: `info <MAC>` на устройство здесь нет, иначе сценарий бы не
    /// совпал с четырьмя ответами.
    fn script() -> Vec<Result<CommandOutput, CommandError>> {
        vec![ok(SHOW), ok(CONNECTED), ok(DEVICES), ok(PAIRED)]
    }

    const INFO_CONNECTED: &str = "Device AA:BB:CC:DD:EE:01 (public)\n\tName: Sony WH-1000XM5\n\tTrusted: yes\n\tConnected: yes\n\tRSSI: -62\n\tBattery: 84%\n\tUUID: SDP  (00000001-0000-1000-8000-00805f9b34fb)\n\tUUID: Audio Sink  (0000110b-0000-1000-8000-00805f9b34fb)\n";
    const INFO_PAIRED_ONLY: &str = "Device AA:BB:CC:DD:EE:06 (public)\n\tName: Old Device\n\tTrusted: no\n\tConnected: no\n\tUUID: Serial Port  (00001101-0000-1000-8000-00805f9b34fb)\n";

    #[test]
    fn parses_controller_power_and_discovering() {
        let adapter = parse_adapter(SHOW);
        assert_eq!(adapter.address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(adapter.name.as_deref(), Some("hci0"));
        assert_eq!(adapter.powered, Some(true));
        assert_eq!(adapter.discovering, Some(false));
    }

    #[test]
    fn powered_no_and_missing_power_are_different_states() {
        let off = parse_adapter(SHOW_OFF);
        assert_eq!(off.powered, Some(false));
        assert_eq!(off.discovering, None, "строки Discovering не было");
        let unknown = parse_adapter(SHOW_NO_POWER);
        assert_eq!(
            unknown.powered, None,
            "нет строки Powered — это «не знаю», а не «выключен»"
        );
    }

    #[test]
    fn show_without_controller_is_an_adapter_without_address() {
        let adapter = parse_adapter("No default controller available\n");
        assert!(adapter.address.is_empty());
        assert_eq!(adapter.powered, None);
    }

    #[test]
    fn devices_parse_unicode_and_every_awkward_character() {
        let devices = parse_devices(DEVICES);
        assert_eq!(devices.len(), 5);
        assert_eq!(devices[0].address, "AA:BB:CC:DD:EE:01");
        assert_eq!(devices[0].name, "Sony WH-1000XM5");
        assert_eq!(devices[2].name, "Домашняя клавиатура");
        assert_eq!(devices[3].name, "-dash-device");
        assert_eq!(devices[4].name, "Мышь: Pro \\ \"v2\" (2024)");
    }

    #[test]
    fn malformed_lines_are_skipped_without_panic() {
        let text = "\nDevice\nDevice не-MAC\nDevice ZZ:BB:CC:DD:EE:FF Имя\nDevice AA:BB:CC:DD:EE\nDevice  AA:BB:CC:DD:EE:07  Два пробела\nмусор\n";
        let devices = parse_devices(text);
        assert_eq!(devices.len(), 1, "выжил только валидный MAC: {devices:?}");
        assert_eq!(devices[0].address, "AA:BB:CC:DD:EE:07");
        assert_eq!(devices[0].name, "Два пробела");
        assert!(parse_devices("").is_empty());
    }

    #[test]
    fn info_parses_trusted_rssi_battery_and_uuid_count() {
        let rich = parse_device_info(INFO_CONNECTED);
        assert_eq!(rich.trusted, Some(true));
        assert_eq!(rich.rssi, Some(-62));
        assert_eq!(rich.battery, Some(84));
        assert_eq!(rich.uuids, 2);
        let idle = parse_device_info(INFO_PAIRED_ONLY);
        assert_eq!(idle.trusted, Some(false));
        assert_eq!(idle.rssi, None, "вне эфира RSSI нет");
        assert_eq!(idle.battery, None);
        assert_eq!(idle.uuids, 1);
        let empty = parse_device_info("Device AA:BB:CC:DD:EE:06 (public)\n");
        assert_eq!(
            empty,
            DeviceDetails::default(),
            "пустые поля — как «не знаю»"
        );
    }

    #[test]
    fn duplicate_mac_merges_instead_of_repeating() {
        let text = "Device AA:BB:CC:DD:EE:01\nDevice AA:BB:CC:DD:EE:01 Имя\n";
        let devices = parse_devices(text);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "Имя", "известное имя перевешивает пустое");
    }

    #[test]
    fn address_normalization_is_stable() {
        assert_eq!(
            normalize_address("aa:bb:cc:dd:ee:ff").as_deref(),
            Some("AA:BB:CC:DD:EE:FF")
        );
        assert_eq!(normalize_address("AA:BB:CC:DD:EE"), None);
        assert_eq!(normalize_address("не MAC"), None);
        assert_eq!(normalize_address("AA:BB:CC:DD:EE:GG"), None);
        assert_eq!(normalize_address(""), None);
    }

    /// Обычный refresh — ровно четыре базовых вызова, сколько бы устройств
    /// ни было: `info <MAC>` на устройство (N+1) здесь запрещён. Порядок —
    /// как в `fetch`, иначе сценарий не совпадёт.
    #[test]
    fn refresh_is_four_base_calls_and_no_per_device_info() {
        let runner = ScriptedRunner::new(script());
        BluetoothProvider.fetch(&runner).expect("снимок");
        let calls = runner.calls();
        let expected = [
            "bluetoothctl show",
            "bluetoothctl devices Connected",
            "bluetoothctl devices",
            "bluetoothctl devices Paired",
        ];
        assert_eq!(
            calls,
            expected.map(str::to_string),
            "refresh — ровно четыре базовых вызова"
        );
        // Самообновление список не загрязняет: scan/power/connect в fetch нет.
        for call in runner.calls() {
            for forbidden in ["scan", "power on", "power off", "connect", "pair", "info"] {
                assert!(!call.contains(forbidden), "лишнее действие в fetch: {call}");
            }
        }
    }

    #[test]
    fn paired_device_survives_missing_from_devices_and_dedups() {
        let runner = ScriptedRunner::new(script());
        let snapshot = BluetoothProvider.fetch(&runner).expect("снимок");
        let paired = snapshot.paired_sorted();
        let addresses: Vec<&str> = paired.iter().map(|d| d.address.as_str()).collect();
        assert!(
            addresses.contains(&"AA:BB:CC:DD:EE:06"),
            "сопряжённый, которого нет в devices, остаётся: {addresses:?}"
        );
        assert_eq!(
            addresses
                .iter()
                .filter(|a| **a == "AA:BB:CC:DD:EE:01")
                .count(),
            1,
            "сопряжённый не дублируется в найденных"
        );
        let discovered = snapshot.discovered_sorted();
        assert!(
            !discovered
                .iter()
                .any(|device| device.address == "AA:BB:CC:DD:EE:01"),
            "сопряжённый показывается только в своём списке"
        );
    }

    #[test]
    fn connected_state_comes_from_the_connected_filter() {
        let runner = ScriptedRunner::new(script());
        let snapshot = BluetoothProvider.fetch(&runner).expect("снимок");
        let sony = snapshot
            .paired_sorted()
            .into_iter()
            .find(|device| device.address == "AA:BB:CC:DD:EE:01")
            .expect("Sony есть");
        assert_eq!(sony.connected, Some(true), "подключённое помечено");
        assert_eq!(sony.paired, Some(true));
        let old = snapshot
            .paired_sorted()
            .into_iter()
            .find(|device| device.address == "AA:BB:CC:DD:EE:06")
            .expect("старое устройство есть");
        assert_eq!(
            old.connected,
            Some(false),
            "сопряжённое, но не подключённое"
        );
    }

    #[test]
    fn sorting_puts_connected_first_then_name_then_mac() {
        let mut list = vec![
            BluetoothDevice {
                address: "AA:BB:CC:DD:EE:09".to_string(),
                name: "airpods".to_string(),
                paired: Some(true),
                connected: Some(false),
            },
            BluetoothDevice {
                address: "AA:BB:CC:DD:EE:08".to_string(),
                name: "AirPods".to_string(),
                paired: Some(true),
                connected: Some(true),
            },
            BluetoothDevice {
                address: "AA:BB:CC:DD:EE:07".to_string(),
                name: "Клавиатура".to_string(),
                paired: Some(true),
                connected: Some(false),
            },
        ];
        sort_devices(&mut list);
        let order: Vec<&str> = list.iter().map(|d| d.address.as_str()).collect();
        // Имя сравнивается без локали и регистра: «airpods» раньше
        // «клавиатура», потому что сравниваются первые буквы.
        assert_eq!(
            order,
            [
                "AA:BB:CC:DD:EE:08",
                "AA:BB:CC:DD:EE:09",
                "AA:BB:CC:DD:EE:07"
            ]
        );
    }

    #[test]
    fn same_name_devices_stay_separate_by_mac() {
        let runner = ScriptedRunner::new(script());
        let snapshot = BluetoothProvider.fetch(&runner).expect("снимок");
        let known = snapshot.known.ready().expect("список есть");
        let ids: Vec<String> = known.iter().map(|device| device.row_id()).collect();
        assert!(ids.contains(&"bluetooth/device/AA:BB:CC:DD:EE:01".to_string()));
        assert!(ids.contains(&"bluetooth/device/AA:BB:CC:DD:EE:02".to_string()));
        assert!(ids.contains(&"bluetooth/device/AA:BB:CC:DD:EE:05".to_string()));
    }

    #[test]
    fn nodes_show_status_lists_and_refresh_without_actions() {
        let runner = ScriptedRunner::new(script());
        let snapshot = BluetoothProvider.fetch(&runner).expect("снимок");
        let nodes = build_nodes(&snapshot, Language::Ru);
        let titles: Vec<&str> = nodes.iter().map(|node| node.title.as_str()).collect();
        assert_eq!(titles[0], "Bluetooth: вкл · hci0");
        assert!(titles.contains(&"Сопряжённые"));
        assert!(titles.contains(&"Найденные"));
        assert!(titles.contains(&"Обновить"));
        for node in &nodes {
            for child in node.children().unwrap_or(&[]) {
                match &node.title[..] {
                    "Сопряжённые" | "Найденные" => {
                        assert!(
                            matches!(child.kind, super::super::tree::NodeKind::Submenu(_)),
                            "устройство — подменю с действиями, не Info: {:?}",
                            child.title
                        );
                    }
                    _ => {}
                }
            }
        }
        let refresh = nodes
            .iter()
            .find(|node| node.identity() == "bluetooth/refresh")
            .expect("пункт обновления");
        assert!(
            matches!(
                refresh.kind,
                super::super::tree::NodeKind::Action(Action::RefreshDynamic(
                    ProviderKey::BLUETOOTH
                ))
            ),
            "обновление read-only: без scan"
        );
    }

    #[test]
    fn powered_off_and_unknown_have_their_own_status() {
        let off = BluetoothSnapshot {
            adapter: Block::Ready(parse_adapter(SHOW_OFF)),
            ..BluetoothSnapshot::default()
        };
        assert_eq!(
            build_nodes(&off, Language::Ru)[0].title,
            "Bluetooth: выкл · hci0"
        );
        let unknown = BluetoothSnapshot {
            adapter: Block::Ready(parse_adapter(SHOW_NO_POWER)),
            ..BluetoothSnapshot::default()
        };
        assert_eq!(
            build_nodes(&unknown, Language::Ru)[0].title,
            "Bluetooth: состояние неизвестно · hci0"
        );
    }

    #[test]
    fn no_adapter_is_not_an_error() {
        let snapshot = BluetoothSnapshot {
            adapter: Block::Ready(parse_adapter("No default controller available\n")),
            ..BluetoothSnapshot::default()
        };
        assert!(!snapshot.is_empty());
        let rows = build_nodes(&snapshot, Language::Ru);
        assert_eq!(rows[0].title, "Нет Bluetooth адаптера");
        assert!(rows.iter().any(|row| row.title == "Сопряжённые"));
    }

    #[test]
    fn empty_lists_say_so() {
        let snapshot = BluetoothSnapshot {
            adapter: Block::Ready(parse_adapter(SHOW)),
            known: Block::Ready(Vec::new()),
            paired: Block::Ready(Vec::new()),
            connected: Block::Ready(Vec::new()),
            infos: HashMap::new(),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        for title in ["Сопряжённые", "Найденные"] {
            let submenu = nodes
                .iter()
                .find(|node| node.title == title)
                .expect("список есть");
            let children = submenu.children().expect("дети есть");
            assert_eq!(children.len(), 1);
            assert_eq!(children[0].title, "Нет устройств");
        }
    }

    #[test]
    fn missing_bluetoothctl_is_unavailable() {
        let runner = ScriptedRunner::new(vec![missing(); 4]);
        let error = BluetoothProvider
            .fetch(&runner)
            .expect_err("раздел недоступен");
        assert!(matches!(error, ProviderError::Unavailable(_)));
        assert_eq!(error.message(), "нет bluetoothctl");
    }

    #[test]
    fn show_failed_keeps_the_device_lists() {
        let mut answers = script();
        answers[0] = Ok(CommandOutput {
            status: Some(1),
            stdout: String::new(),
            stderr: "No default controller available\n".to_string(),
        });
        let runner = ScriptedRunner::new(answers);
        let snapshot = BluetoothProvider.fetch(&runner).expect("снимок");
        assert!(snapshot.adapter.failure().is_some());
        assert_eq!(snapshot.paired.ready().map(Vec::len), Some(2));
        assert_eq!(snapshot.known.ready().map(Vec::len), Some(5));
        let nodes = build_nodes(&snapshot, Language::Ru);
        assert!(
            nodes
                .iter()
                .any(|node| node.title == "Состояние адаптера недоступно")
        );
    }

    #[test]
    fn paired_failed_keeps_adapter_and_devices() {
        let mut answers = script();
        answers[3] = Ok(CommandOutput {
            status: Some(1),
            stdout: String::new(),
            stderr: "org.bluez.Error.NotReady\n".to_string(),
        });
        let runner = ScriptedRunner::new(answers);
        let snapshot = BluetoothProvider.fetch(&runner).expect("снимок");
        assert!(snapshot.paired.failure().is_some());
        assert_eq!(
            snapshot.adapter.ready().map(|a| a.powered),
            Some(Some(true)),
            "адаптер уцелел"
        );
        assert_eq!(snapshot.known.ready().map(Vec::len), Some(5));
    }

    /// Сбой `devices Paired`: сопряжённость неизвестна у всех, а не `false`.
    /// Адаптер и списки при этом на месте — деградация частичная.
    #[test]
    fn failed_paired_list_means_unknown_not_unpaired() {
        let mut answers = script();
        answers[3] = Ok(CommandOutput {
            status: Some(1),
            stdout: String::new(),
            stderr: "org.bluez.Error.NotReady\n".to_string(),
        });
        let runner = ScriptedRunner::new(answers);
        let snapshot = BluetoothProvider.fetch(&runner).expect("снимок");
        assert!(snapshot.paired.failure().is_some());
        let known = snapshot.known.ready().expect("список известен");
        assert_eq!(known.len(), 5, "устройства прочитались");
        assert!(
            known.iter().all(|device| device.paired.is_none()),
            "сопряжённость неизвестна, а не false: {known:?}"
        );
        // Подключённость при этом известна: её список прочитался.
        let sony = known
            .iter()
            .find(|device| device.address == "AA:BB:CC:DD:EE:01")
            .expect("Sony есть");
        assert_eq!(sony.connected, Some(true));
    }

    /// Сбой `devices Connected`: подключённость неизвестна у всех.
    #[test]
    fn failed_connected_list_means_unknown_not_disconnected() {
        let mut answers = script();
        answers[1] = Ok(CommandOutput {
            status: Some(1),
            stdout: String::new(),
            stderr: "org.bluez.Error.NotReady\n".to_string(),
        });
        let runner = ScriptedRunner::new(answers);
        let snapshot = BluetoothProvider.fetch(&runner).expect("снимок");
        assert!(snapshot.connected.failure().is_some());
        let known = snapshot.known.ready().expect("список известен");
        assert!(
            known.iter().all(|device| device.connected.is_none()),
            "подключённость неизвестна, а не false: {known:?}"
        );
        let sony = known
            .iter()
            .find(|device| device.address == "AA:BB:CC:DD:EE:01")
            .expect("Sony есть");
        assert_eq!(sony.paired, Some(true), "сопряжённость известна");
    }

    /// Оба списка не прочитались: всё неизвестно, но адаптер и устройства
    /// на месте, раздел жив.
    #[test]
    fn both_lists_failed_keeps_adapter_and_devices_visible() {
        let mut answers = script();
        for index in [1, 3] {
            answers[index] = Ok(CommandOutput {
                status: Some(1),
                stdout: String::new(),
                stderr: "org.bluez.Error.NotReady\n".to_string(),
            });
        }
        let runner = ScriptedRunner::new(answers);
        let snapshot = BluetoothProvider.fetch(&runner).expect("снимок");
        assert!(snapshot.paired.failure().is_some());
        assert!(snapshot.connected.failure().is_some());
        assert_eq!(
            snapshot.adapter.ready().map(|a| a.powered),
            Some(Some(true)),
            "адаптер уцелел"
        );
        let known = snapshot.known.ready().expect("устройства видны");
        assert_eq!(known.len(), 5);
        assert!(
            known
                .iter()
                .all(|device| device.paired.is_none() && device.connected.is_none()),
            "оба состояния неизвестны: {known:?}"
        );
        // Подменю честно говорит о причине, а не показывает пустой список.
        let nodes = build_nodes(&snapshot, Language::Ru);
        let titles: Vec<&str> = nodes.iter().map(|node| node.title.as_str()).collect();
        assert!(
            titles.contains(&"Сопряжённые: список не прочитался"),
            "{titles:?}"
        );
    }

    /// Неизвестное состояние не даёт опасных действий: ни Pair, ни Connect,
    /// ни Remove, ни Disconnect, ни Trust. Только строка о неизвестности.
    #[test]
    fn unknown_state_offers_no_actions() {
        let device = BluetoothDevice {
            address: "AA:BB:CC:DD:EE:01".to_string(),
            name: "Sony".to_string(),
            paired: None,
            connected: None,
        };
        let titles: Vec<String> = device_children(Language::Ru, &device, None)
            .iter()
            .map(|node| node.title.clone())
            .collect();
        for forbidden in [
            "Сопрячь",
            "Подключить",
            "Отключить",
            "Удалить",
            "Доверять",
            "Не доверять",
        ] {
            assert!(
                !titles.contains(&forbidden.to_string()),
                "недостоверное действие в меню: {titles:?}"
            );
        }
        assert!(
            titles.iter().any(|title| title.contains("неизвестно")),
            "должна быть строка о неизвестности: {titles:?}"
        );
        // Та же строгость, когда сопряжение известно, а подключение — нет:
        // Connect по гаданию запрещён.
        let half_known = BluetoothDevice {
            paired: Some(true),
            ..device
        };
        let titles: Vec<String> = device_children(Language::Ru, &half_known, None)
            .iter()
            .map(|node| node.title.clone())
            .collect();
        assert!(
            !titles.contains(&"Подключить".to_string()),
            "Connect без известного connected: {titles:?}"
        );
    }

    /// Кнопка питания: вкл — выключить, выкл — включить, неизвестно — кнопки
    /// нет. Статусная строка при этом уже сказала «состояние неизвестно».
    #[test]
    fn power_row_follows_powered_and_hides_on_unknown() {
        let on = parse_adapter(SHOW);
        let row = power_row(Language::Ru, &on).expect("кнопка есть");
        assert_eq!(row.title, "Выключить Bluetooth");
        match &row.kind {
            super::super::tree::NodeKind::Action(Action::RefreshAndRun { command, .. }) => {
                assert_eq!(command.argv().1, ["power", "off"]);
            }
            other => panic!("ожидалась команда, {other:?}"),
        }
        let off = parse_adapter(SHOW_OFF);
        let row = power_row(Language::Ru, &off).expect("кнопка есть");
        assert_eq!(row.title, "Включить Bluetooth");
        let unknown = parse_adapter(SHOW_NO_POWER);
        assert_eq!(unknown.powered, None);
        assert!(
            power_row(Language::Ru, &unknown).is_none(),
            "по неизвестному состоянию кнопку не предлагаем"
        );
        // И в собранном разделе кнопки нет, а статус честный.
        let snapshot = BluetoothSnapshot {
            adapter: Block::Ready(unknown),
            known: Block::Ready(Vec::new()),
            paired: Block::Ready(Vec::new()),
            connected: Block::Ready(Vec::new()),
            infos: HashMap::new(),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let titles: Vec<&str> = nodes.iter().map(|node| node.title.as_str()).collect();
        assert!(titles.contains(&"Bluetooth: состояние неизвестно · hci0"));
        assert!(
            !titles
                .iter()
                .any(|title| title.contains("ключить Bluetooth")),
            "кнопки питания нет: {titles:?}"
        );
    }

    #[test]
    fn devices_failed_keeps_paired_list() {
        let mut answers = script();
        answers[2] = missing();
        let runner = ScriptedRunner::new(answers);
        let snapshot = BluetoothProvider.fetch(&runner).expect("снимок");
        assert!(snapshot.known.is_empty());
        assert_eq!(snapshot.paired.ready().map(Vec::len), Some(2));
        let nodes = build_nodes(&snapshot, Language::Ru);
        assert!(nodes.iter().any(|node| node.title == "Найденные"));
    }

    #[test]
    fn nothing_available_at_all_says_so_without_panicking() {
        let mut answers = script();
        for answer in answers.iter_mut().skip(1) {
            *answer = missing();
        }
        let runner = ScriptedRunner::new(answers);
        let snapshot = BluetoothProvider.fetch(&runner).expect("снимок");
        assert!(snapshot.is_empty() || snapshot.known.is_empty());
        let nodes = build_nodes(&snapshot, Language::Ru);
        assert!(!nodes.is_empty(), "раздел что-то показывает");
    }

    /// Больше двенадцати устройств в один подменю не влезает: лишние уходят в
    /// «Все», а не теряются. Те же строки, те же личности.
    #[test]
    fn long_lists_are_capped_and_keep_identity() {
        let many: Vec<BluetoothDevice> = (0..20)
            .map(|index| BluetoothDevice {
                address: format!("AA:BB:CC:DD:EE:{index:02X}"),
                name: format!("Устройство {index}"),
                paired: Some(true),
                connected: Some(false),
            })
            .collect();
        let snapshot = BluetoothSnapshot {
            adapter: Block::Ready(parse_adapter(SHOW)),
            known: Block::Ready(Vec::new()),
            paired: Block::Ready(many.clone()),
            connected: Block::Ready(Vec::new()),
            infos: HashMap::new(),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let paired = nodes
            .iter()
            .find(|node| node.title == "Сопряжённые")
            .expect("список есть");
        let children = paired.children().expect("дети есть");
        assert_eq!(
            children.len(),
            MAX_VISIBLE_DEVICES + 1,
            "12 устройств и строка «Все»"
        );
        let sorted = snapshot.paired_sorted();
        assert_eq!(sorted.len(), 20, "полный список в снимке не обрезан");
        let expected: Vec<String> = sorted.iter().map(|device| device.row_id()).collect();
        let shown: Vec<String> = children
            .iter()
            .take(MAX_VISIBLE_DEVICES)
            .map(|row| row.identity().to_string())
            .collect();
        assert_eq!(shown, expected[..MAX_VISIBLE_DEVICES].to_vec());
        let all = children[MAX_VISIBLE_DEVICES]
            .children()
            .expect("хвост доступен");
        let rest: Vec<String> = all.iter().map(|row| row.identity().to_string()).collect();
        assert_eq!(
            rest,
            expected[MAX_VISIBLE_DEVICES..].to_vec(),
            "хвост не потерян"
        );
    }

    /// Подменю устройства даёт правильные действия по его состоянию:
    /// подключён — отключить, сопряжён — подключить, только известен — со
    /// пряч. Trust/remove — там же, но без них подменю не строится у тех,
    /// кого ещё не сопрягали.
    #[test]
    fn device_submenu_offers_the_action_for_its_state() {
        let device = |paired: Option<bool>, connected: Option<bool>| BluetoothDevice {
            address: "AA:BB:CC:DD:EE:01".to_string(),
            name: "Sony".to_string(),
            paired,
            connected,
        };
        let titles = |device: BluetoothDevice| {
            device_children(Language::Ru, &device, None)
                .iter()
                .map(|node| node.title.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            titles(device(Some(true), Some(true)))[0],
            "Отключить",
            "подключённое — отключить"
        );
        assert_eq!(
            titles(device(Some(true), Some(false)))[0],
            "Подключить",
            "сопряжённое вне эфира — подключить"
        );
        assert_eq!(
            titles(device(Some(false), Some(false)))[0],
            "Сопрячь",
            "известное, но не сопряжённое — сопрячь"
        );
        let connected = titles(device(Some(true), Some(true)));
        assert!(connected.contains(&"Удалить".to_string()));
        assert!(connected.contains(&"Доверять".to_string()));
        let found = titles(device(Some(false), Some(false)));
        assert!(!found.contains(&"Удалить".to_string()));
        assert!(!found.contains(&"Доверять".to_string()));
    }

    /// Команды действий: программа `bluetoothctl`, программе не подсовывают
    /// shell, MAC уходит отдельным argv-элементом.
    #[test]
    fn device_actions_build_expected_argv() {
        let argv_of = |action: Action| match action {
            Action::RefreshAndRun { command, .. } => command.argv().1,
            _ => panic!("ожидалась команда"),
        };
        assert_eq!(
            argv_of(connect_action("AA:BB:CC:DD:EE:01")),
            ["connect", "AA:BB:CC:DD:EE:01"]
        );
        assert_eq!(
            argv_of(disconnect_action("AA:BB:CC:DD:EE:01")),
            ["disconnect", "AA:BB:CC:DD:EE:01"]
        );
        assert_eq!(
            argv_of(trust_action("AA:BB:CC:DD:EE:01", true)),
            ["trust", "AA:BB:CC:DD:EE:01"]
        );
        assert_eq!(
            argv_of(trust_action("AA:BB:CC:DD:EE:01", false)),
            ["untrust", "AA:BB:CC:DD:EE:01"]
        );
        assert_eq!(
            argv_of(remove_action("AA:BB:CC:DD:EE:01")),
            ["remove", "AA:BB:CC:DD:EE:01"]
        );
        assert_eq!(argv_of(power_action(false)), ["power", "off"]);
        assert_eq!(argv_of(scan_action(true)), ["scan", "on"]);
        let pair = argv_of(pair_action("AA:BB:CC:DD:EE:01"));
        assert!(pair.contains(&"pair".to_string()), "{pair:?}");
        assert!(pair.contains(&"NoInputNoOutput".to_string()), "{pair:?}");
    }

    /// Живой опрос хоста: только read-only команды. Запускается вручную
    /// (`cargo test --bin hud-menu-rs -- --ignored live_provider`).
    #[test]
    #[ignore = "обращается к bluetoothctl хоста"]
    fn live_provider_reads_the_host() {
        let snapshot = BluetoothProvider
            .fetch(&SystemCommandRunner::default())
            .expect("на хосте есть bluetoothctl");
        println!("адаптер: {:?}", snapshot.adapter);
        println!("сопряжённые: {:?}", snapshot.paired_sorted());
        println!("найденные: {:?}", snapshot.discovered_sorted());
        assert!(!snapshot.is_empty(), "хотя бы что-то про Bluetooth есть");
    }

    #[test]
    fn identity_survives_a_name_change_and_a_new_order() {
        let first = BluetoothDevice {
            address: "AA:BB:CC:DD:EE:01".to_string(),
            name: "Sony".to_string(),
            paired: Some(true),
            connected: Some(false),
        };
        let renamed = BluetoothDevice {
            address: "AA:BB:CC:DD:EE:01".to_string(),
            name: "Sony WH-1000XM5".to_string(),
            paired: Some(true),
            connected: Some(false),
        };
        assert_eq!(first.row_id(), renamed.row_id(), "имя не часть личности");
        assert_eq!(first.row_id(), "bluetooth/device/AA:BB:CC:DD:EE:01");
    }

    #[test]
    fn title_falls_back_to_mac_and_trims() {
        let nameless = BluetoothDevice {
            address: "AA:BB:CC:DD:EE:01".to_string(),
            ..BluetoothDevice::default()
        };
        assert_eq!(nameless.title(), "AA:BB:CC:DD:EE:01");
        let blank = BluetoothDevice {
            address: "AA:BB:CC:DD:EE:01".to_string(),
            name: "   ".to_string(),
            ..BluetoothDevice::default()
        };
        assert_eq!(blank.title(), "AA:BB:CC:DD:EE:01");
    }

    #[test]
    fn device_title_marks_connected_only() {
        let device = BluetoothDevice {
            address: "AA:BB:CC:DD:EE:01".to_string(),
            name: "Sony".to_string(),
            paired: Some(true),
            connected: Some(true),
        };
        assert_eq!(device_title(Language::Ru, &device), "Sony · подключено");
        let idle = BluetoothDevice {
            connected: Some(false),
            ..device.clone()
        };
        assert_eq!(device_title(Language::Ru, &idle), "Sony");
        // Неизвестное подключение метки не ставит: «подключено?» — это уже
        // вопрос, а не подпись.
        let unknown = BluetoothDevice {
            connected: None,
            ..device.clone()
        };
        assert_eq!(device_title(Language::Ru, &unknown), "Sony");
    }
}

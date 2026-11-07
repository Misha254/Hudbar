use super::*;

/// Сколько устройств показывать в каждом списке: больше не влезет в карточку,
/// а найти можно будет поиском и разделом «Все».
pub const MAX_VISIBLE_DEVICES: usize = 12;

/// Операция над устройством: `connect`/`disconnect`/`power`/`scan`/`trust`/
/// `remove` могут ждать ответа гаджета, односекундный таймаут `oneshot` тут
/// не годится.
pub(crate) const DEVICE_TIMEOUT: Duration = Duration::from_secs(8);
/// Pairing ждёт регистрации агента и, возможно, PIN: даём больше, а не
/// держим меню.
pub(crate) const PAIR_TIMEOUT: Duration = Duration::from_secs(20);
/// Поиск устройств — не мгновенный вызов: `bluetoothctl` держит сканирование
/// указанное число секунд, и только потом возвращает управление.
pub(crate) const SCAN_SECONDS: u64 = 20;
pub(crate) const SCAN_TIMEOUT: Duration = Duration::from_secs(SCAN_SECONDS + 5);

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

pub(crate) fn sort_devices(list: &mut [BluetoothDevice]) {
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
        .map_err(|error| error.message_in(lang))?;
    Ok(provider.nodes(&snapshot, lang))
}

// ── Действия над адаптером и устройствами ────────────────────────────────
//
// Все идут через `CommandSpec` (argv, без shell) и `RefreshAndRun`: после
// успеха слот BLUETOOTH обновляется, строка занята до ответа worker-а.
// Никаких `sh -c`, никаких статических команд в обход runner-а.

/// Включить или выключить радио адаптера: `bluetoothctl power on|off`.
/// Блок команды: успех — данные, отсутствие инструмента — `Missing`, сбой —
/// `Failed` с безопасным сообщением вместо сырого stderr.
pub(crate) fn text_block<T, F: FnOnce(&str) -> Option<T>>(
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
pub(crate) fn failure(stderr: &str) -> String {
    let line = stderr.lines().map(str::trim).find(|line| !line.is_empty());
    match line {
        Some(text) => text.to_string(),
        None => "bluetoothctl: команда не сработала".to_string(),
    }
}

/// Адаптер из `bluetoothctl show`.
pub(crate) fn adapter(runner: &dyn CommandRunner) -> Block<BluetoothAdapter> {
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
pub(crate) fn parse_yes_no(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

/// Список MAC из `bluetoothctl devices <filter>`.
pub(crate) fn addresses(runner: &dyn CommandRunner, filter: &str) -> Block<Vec<String>> {
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
pub(crate) fn devices(runner: &dyn CommandRunner, filter: &str) -> Block<Vec<BluetoothDevice>> {
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
pub(crate) fn split_address(text: &str) -> (String, String) {
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

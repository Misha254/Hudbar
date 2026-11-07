use super::*;

/// Подключение к точке доступа занимает секунды: обычного двухсекундного
/// таймаута чтений мало, но и бесконечно ждать нельзя — иначе меню не
/// отпустит пользователя и worker останется висеть.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(25);

/// Тип защиты сети: открытая или с паролем.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    /// Открытая: подключается без пароля.
    Open,
    /// Нужен ключ.
    Secured,
}

/// Сеть из эфира плюс её связь с сохранённым профилем.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiNet {
    /// Имя сети как в эфире. Может быть любым текстом, включая пустой
    /// (скрытая сеть — такие отбрасываются при разборе).
    pub ssid: String,
    /// Уровень сигнала 0..=100.
    pub signal: u8,
    /// Тип защиты.
    pub security: Security,
    /// Сеть, к которой сейчас подключён интерфейс.
    pub in_use: bool,
    /// UUID сохранённого профиля этой сети, если он есть.
    pub saved_uuid: Option<String>,
}

impl WifiNet {
    /// Личность строки: имя сети. Позиция в списке меняется при каждом
    /// скане, а SSID остаётся — по нему курсор и возвращается на место.
    pub fn row_id(&self) -> String {
        format!("wifi/net/{}", self.ssid)
    }
}

/// Wi-Fi-интерфейс и его состояние.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WifiDevice {
    /// Имя интерфейса, например `wlp0s20f0u2`.
    pub name: String,
    /// Состояние из `nmcli device`: connected, disconnected, unavailable.
    pub state: String,
    /// Имя активного подключения на этом интерфейсе, если есть.
    pub connection: Option<String>,
}

impl WifiDevice {
    /// Подключён ли интерфейс сейчас.
    pub fn is_connected(&self) -> bool {
        self.state.starts_with("connected")
    }
}

/// Активное подключение: интерфейс, имя профиля и его UUID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveLink {
    /// Интерфейс, на котором работает подключение.
    pub device: String,
    /// Имя профиля, как его зовёт NetworkManager.
    pub name: String,
    /// UUID профиля: им выключается соединение. Пусто, если профиля нет.
    pub uuid: Option<String>,
}

/// Состояние одного блока раздела: тот же смысл, что в `audio::Block`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block<T> {
    /// Данные есть.
    Ready(T),
    /// Инструмента нет: блок пуст, раздел в целом жив.
    Missing,
    /// Инструмент ответил, но данных из него не получилось.
    Failed(String),
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
}

/// Типизированный снимок раздела «Wi-Fi».
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiSnapshot {
    /// Радиомодуль включён или выключен.
    pub radio: Block<bool>,
    /// Первый Wi-Fi-интерфейс: у машины их может быть несколько.
    pub device: Block<WifiDevice>,
    /// Активное подключение, если интерфейс подключён.
    pub active: Option<ActiveLink>,
    /// Видимые сети, отсортированные как в меню.
    pub networks: Block<Vec<WifiNet>>,
}

impl WifiSnapshot {
    /// Нет ни одного Wi-Fi-интерфейса. Это не ошибка: на ноутбуке без
    /// радиоадаптера раздел должен сказать об этом и не пугать.
    pub fn has_no_device(&self) -> bool {
        match &self.device {
            Block::Ready(device) => device.name.is_empty(),
            _ => false,
        }
    }
}

/// Провайдер раздела «Wi-Fi».
pub struct WifiProvider;

impl SystemProvider for WifiProvider {
    type Snapshot = WifiSnapshot;

    fn key(&self) -> ProviderKey {
        ProviderKey::WIFI
    }

    fn fetch(&self, runner: &dyn CommandRunner) -> Result<Self::Snapshot, ProviderError> {
        // Порядок вызовов подобран так, чтобы `nmcli device wifi list`
        // сходил ровно один раз: список нужен и для показа, и для того, чтобы
        // понять, каким профилям надо уточнить SSID.
        let radio = radio(runner);
        let devices = all_devices(runner);
        let listed = network_list(runner);
        let known: Vec<&str> = listed
            .ready()
            .map(|nets| nets.iter().map(|net| net.ssid.as_str()).collect())
            .unwrap_or_default();
        let profiles = wireless_profiles(runner, &known);

        if radio.is_empty() && devices.is_empty() {
            return Err(ProviderError::Unavailable("нет nmcli".to_string()));
        }
        let device = device_block(&devices);
        let active = active_link(device.ready().cloned(), &profiles);
        let networks = bind_profiles(listed, &profiles);
        Ok(WifiSnapshot {
            radio,
            device,
            active,
            networks,
        })
    }

    fn nodes(&self, snapshot: &Self::Snapshot, lang: Language) -> Vec<Node> {
        build_nodes(snapshot, lang)
    }
}

/// Fetch + nodes одним вызовом: так провайдера зовёт цикл окна.
pub fn fetch_nodes(lang: Language) -> Result<Vec<Node>, String> {
    let provider = WifiProvider;
    let snapshot = provider
        .fetch(&SystemCommandRunner::default())
        .map_err(|error| error.message_in(lang))?;
    Ok(provider.nodes(&snapshot, lang))
}

pub(crate) fn run(
    runner: &dyn CommandRunner,
    spec: &CommandSpec,
) -> Result<Block<String>, ProviderError> {
    match runner.run(spec) {
        Err(CommandError::Spawn(_)) => Ok(Block::Missing),
        Err(error) => Ok(Block::Failed(error.to_string())),
        Ok(output) if output.success() => Ok(Block::Ready(output.stdout)),
        Ok(output) => Err(ProviderError::Command {
            program: spec.program().to_string(),
            message: output
                .stderr
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string(),
        }),
    }
}

/// Блок данных с разбором из текста: сбой разбора не молчит, а попадает в
/// `Failed` с именем команды.
pub(crate) fn data_block<T, F: FnOnce(&str) -> Option<T>>(
    runner: &dyn CommandRunner,
    spec: &CommandSpec,
    parse: F,
) -> Block<T> {
    match run(runner, spec) {
        Ok(Block::Ready(text)) => match parse(&text) {
            Some(value) => Block::Ready(value),
            None => Block::Failed(format!("{}: не разобрано", spec.program())),
        },
        Ok(Block::Missing) => Block::Missing,
        Ok(Block::Failed(reason)) => Block::Failed(reason),
        Err(error) => Block::Failed(error.message()),
    }
}

/// Радиомодуль: `nmcli -t -f WIFI radio` -> enabled/disabled.
pub(crate) fn radio(runner: &dyn CommandRunner) -> Block<bool> {
    let spec = CommandSpec::new("nmcli")
        .arg("-t")
        .arg("-f")
        .arg("WIFI")
        .arg("radio");
    data_block(runner, &spec, |text| {
        let state = text.trim();
        if state.eq_ignore_ascii_case("enabled") {
            Some(true)
        } else if state.eq_ignore_ascii_case("disabled") {
            Some(false)
        } else {
            None
        }
    })
}

/// Все устройства NetworkManager: `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device`.
pub(crate) fn all_devices(runner: &dyn CommandRunner) -> Block<Vec<WifiDevice>> {
    let spec = CommandSpec::new("nmcli")
        .arg("-t")
        .arg("-f")
        .arg("DEVICE,TYPE,STATE,CONNECTION")
        .arg("device");
    data_block(runner, &spec, |text| Some(parse_devices(text)))
}

/// Wi-Fi-устройство для раздела: подключённое важнее молчаливого, порядок
/// вывода nmcli не гарантирован.
pub(crate) fn device_block(devices: &Block<Vec<WifiDevice>>) -> Block<WifiDevice> {
    match devices {
        Block::Ready(devices) => {
            let wifi: Vec<&WifiDevice> = devices.iter().collect();
            let picked = wifi
                .iter()
                .find(|device| device.is_connected())
                .or_else(|| wifi.first())
                .map(|device| (*device).clone());
            Block::Ready(picked.unwrap_or_default())
        }
        Block::Missing => Block::Missing,
        Block::Failed(reason) => Block::Failed(reason.clone()),
    }
}

/// Сохранённые беспроводные профили: имя, UUID и, если имя сети не совпало
/// ни с одной видимой сетью, SSID из профиля.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// Имя профиля в NetworkManager.
    pub name: String,
    /// UUID профиля.
    pub uuid: String,
    /// SSID из настроек профиля. Пусто, если профиля не касались.
    pub ssid: Option<String>,
}

/// Профили типа 802-11-wireless: `nmcli -t -f NAME,UUID,TYPE connection show`.
/// `known` — имена видимых сетей: для них имя профиля обычно и есть SSID, и
/// отдельный дозапрос не нужен.
pub(crate) fn wireless_profiles(runner: &dyn CommandRunner, known: &[&str]) -> Vec<Profile> {
    let spec = CommandSpec::new("nmcli")
        .arg("-t")
        .arg("-f")
        .arg("NAME,UUID,TYPE")
        .arg("connection")
        .arg("show");
    let Ok(Block::Ready(text)) = run(runner, &spec) else {
        return Vec::new();
    };
    let profiles: Vec<Profile> = text
        .lines()
        .filter_map(|line| {
            let fields = oneshot::split_nmcli_terse(line);
            if fields.len() < 3 || fields[2] != "802-11-wireless" {
                return None;
            }
            Some(Profile {
                name: fields[0].clone(),
                uuid: fields[1].clone(),
                ssid: None,
            })
        })
        .collect();
    // Имя профиля NetworkManager обычно совпадает с SSID, но не обязано.
    // Уточнение нужно только тем профилям, чьё имя не совпало ни с одной
    // видимой сетью: один вызов на «непохожий» профиль, а не на каждый.
    profiles
        .into_iter()
        .map(|mut profile| {
            if !known.contains(&profile.name.as_str()) {
                profile.ssid = profile_ssid(runner, &profile.uuid);
            }
            profile
        })
        .collect()
}

/// UUID сохранённых беспроводных профилей по SSID. Панель бара подключает
/// такую сеть командой `connection up uuid`, не спрашивая пароль: он уже
/// лежит в профиле NetworkManager. Сопоставление — то же, что у строк меню:
/// сначала SSID из профиля, потом имя профиля.
///
/// `known` — SSID видимых сетей: для них SSID не дозапрашивается.
pub fn saved_profile_uuids(runner: &dyn CommandRunner, known: &[&str]) -> Vec<(String, String)> {
    wireless_profiles(runner, known)
        .into_iter()
        .map(|profile| (profile.ssid.unwrap_or(profile.name), profile.uuid))
        .collect()
}

/// SSID профиля: `nmcli -t -f 802-11-wireless.ssid connection show uuid <UUID>`.
///
/// Ответ разбирается тем же `split_nmcli_terse`, что и основной список сетей:
/// двоеточие и обратный слэш внутри SSID приходят экранированными
/// (`My\:Network`, `Дом \\ принтер`), и голый `split_once(':')` оставил бы
/// escape-последовательности как есть — профиль бы не сматчился с видимой
/// сетью.
pub(crate) fn profile_ssid(runner: &dyn CommandRunner, uuid: &str) -> Option<String> {
    let spec = CommandSpec::new("nmcli")
        .arg("-t")
        .arg("-f")
        .arg("802-11-wireless.ssid")
        .arg("connection")
        .arg("show")
        .arg("uuid")
        .arg(uuid);
    match run(runner, &spec) {
        Ok(Block::Ready(text)) => text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .and_then(|line| {
                let fields = oneshot::split_nmcli_terse(line);
                fields
                    .get(1)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            }),
        _ => None,
    }
}

/// Сети эфира без привязки к профилям: по лучшей точке каждой сети.
pub(crate) fn network_list(runner: &dyn CommandRunner) -> Block<Vec<WifiNet>> {
    let spec = CommandSpec::new("nmcli")
        .arg("-t")
        .arg("-f")
        .arg("IN-USE,SSID,SIGNAL,SECURITY,BSSID")
        .arg("device")
        .arg("wifi")
        .arg("list")
        .arg("--rescan")
        .arg("no");
    data_block(runner, &spec, |text| Some(parse_networks(text)))
}

/// Проставляет каждой сети UUID её профиля: сортировка и склейка уже позади,
/// сопоставление идёт по SSID.
pub(crate) fn bind_profiles(
    block: Block<Vec<WifiNet>>,
    profiles: &[Profile],
) -> Block<Vec<WifiNet>> {
    match block {
        Block::Ready(mut list) => {
            for net in &mut list {
                net.saved_uuid = saved_uuid(profiles, &net.ssid);
            }
            Block::Ready(list)
        }
        Block::Missing => Block::Missing,
        Block::Failed(reason) => Block::Failed(reason),
    }
}

pub(crate) fn saved_uuid(profiles: &[Profile], ssid: &str) -> Option<String> {
    profiles
        .iter()
        .find(|profile| profile.ssid.as_deref() == Some(ssid))
        .or_else(|| profiles.iter().find(|profile| profile.name == ssid))
        .map(|profile| profile.uuid.clone())
}

/// Активное подключение: интерфейс плюс UUID его профиля.
pub(crate) fn active_link(device: Option<WifiDevice>, profiles: &[Profile]) -> Option<ActiveLink> {
    let device = device?;
    let name = device.connection.clone().filter(|name| !name.is_empty())?;
    let uuid = profiles
        .iter()
        .find(|profile| profile.name == name)
        .map(|profile| profile.uuid.clone());
    Some(ActiveLink {
        device: device.name,
        name,
        uuid,
    })
}

/// Разбор `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device`.
pub fn parse_devices(text: &str) -> Vec<WifiDevice> {
    text.lines()
        .filter_map(|line| {
            let fields = oneshot::split_nmcli_terse(line);
            if fields.len() < 4 || fields[1] != "wifi" {
                return None;
            }
            let connection = fields[3].trim();
            Some(WifiDevice {
                name: fields[0].trim().to_string(),
                state: fields[2].trim().to_string(),
                connection: (!connection.is_empty()).then(|| connection.to_string()),
            })
        })
        .collect()
}

/// Разбор `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY,BSSID device wifi list`.
/// Скрытые сети (пустой SSID) пропускаются, дубликаты по SSID схлопываются
/// к сильнейшей точке, но флаг IN-USE сохраняется: активная сеть может
/// отдаваться и с более слабого BSSID.
pub fn parse_networks(text: &str) -> Vec<WifiNet> {
    let mut list: Vec<WifiNet> = text
        .lines()
        .filter_map(|line| {
            let fields = oneshot::split_nmcli_terse(line);
            if fields.len() < 4 || fields[1].is_empty() {
                return None;
            }
            let security = fields[3].trim();
            let secured = !security.is_empty() && security != "--";
            Some(WifiNet {
                ssid: fields[1].clone(),
                signal: fields[2].trim().parse::<u16>().unwrap_or(0).min(100) as u8,
                security: if secured {
                    Security::Secured
                } else {
                    Security::Open
                },
                in_use: fields[0].trim() == "*",
                saved_uuid: None,
            })
        })
        .collect();

    let mut best: Vec<WifiNet> = Vec::new();
    for net in list.drain(..) {
        match best.iter_mut().find(|kept| kept.ssid == net.ssid) {
            Some(kept) => {
                if net.signal > kept.signal {
                    kept.signal = net.signal;
                }
                kept.in_use |= net.in_use;
            }
            None => best.push(net),
        }
    }
    best.sort_by(|a, b| {
        b.in_use
            .cmp(&a.in_use)
            .then(b.signal.cmp(&a.signal))
            .then(a.ssid.cmp(&b.ssid))
    });
    best
}

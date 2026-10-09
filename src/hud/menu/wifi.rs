//! Динамический раздел «Wi-Fi»: провайдер поверх `nmcli`.
//!
//! Чтение — тот же конвейер W5.0, что у звука: worker -> [`CommandRunner`] ->
//! типизированный снимок -> строки меню. Запись — [`Action::RefreshAndRun`]:
//! команда уходит в worker цикла окна, после успеха слот WIFI мягко
//! перезапрашивается, а строка, из которой нажали, до этого момента
//! помечена «выполняется».
//!
//! Пароли вводятся через [`Action::SecretInput`](super::action::Action):
//! защищённая сеть без профиля открывает поле ввода, а после `Enter` команда
//! `nmcli device wifi connect <SSID> password <PASSWORD>` собирается с
//! паролем как `CmdArg::Secret` — в журналах и `Debug` его нет, но в `argv`
//! процесса он попадает (см. `secret`). Открытые сети и сети с сохранённым
//! профилем подключаются без ввода.
//!
//! Безопасность argv. SSID приходит из эфира и может содержать пробелы,
//! кавычки, `:`, `\\` и начинаться с `-`. Поэтому:
//!   * сети опознаются по имени из профиля и по SSID, а не по позиции;
//!   * сохранённая сеть включается и выключается по UUID профиля, а не по
//!     имени: идентификатор не из эфира и не может быть опцией;
//!   * единственное место, где SSID уходит в argv как есть, — `device wifi
//!     connect` для открытой сети без профиля. Если SSID начинается с `-`,
//!     перед ним ставится `--`, иначе он выглядел бы опцией nmcli;
//!   * shell не используется нигде: `argv` уходит в `Command` напрямую.

use super::super::data::oneshot;
use super::action::Action;
use super::secret::SecretTarget;
use super::settings::Language;
use super::settings_icons;
use super::system::{
    CommandError, CommandRunner, CommandSpec, ProviderError, ProviderKey, SystemCommandRunner,
    SystemProvider,
};
use super::tree::Node;
use std::time::Duration;

/// Подключение к точке доступа занимает секунды: обычного двухсекундного
/// таймаута чтений мало, но и бесконечно ждать нельзя — иначе меню не
/// отпустит пользователя и worker останется висеть.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(25);

/// Сколько сетей показывать прямо в разделе. Остальные — в подменю «Все
/// сети»: поиск меню по динамическим строкам не ходит, а молча терять
/// сети нельзя.
pub const MAX_INLINE_NETWORKS: usize = 12;

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
        let profiles = wireless_profiles(
            runner,
            listed.ready().map(Vec::as_slice).unwrap_or_default(),
        );

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
        .map_err(|error| error.message())?;
    Ok(provider.nodes(&snapshot, lang))
}

fn run(runner: &dyn CommandRunner, spec: &CommandSpec) -> Result<Block<String>, ProviderError> {
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
fn data_block<T, F: FnOnce(&str) -> Option<T>>(
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
fn radio(runner: &dyn CommandRunner) -> Block<bool> {
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
fn all_devices(runner: &dyn CommandRunner) -> Block<Vec<WifiDevice>> {
    let spec = CommandSpec::new("nmcli")
        .arg("-t")
        .arg("-f")
        .arg("DEVICE,TYPE,STATE,CONNECTION")
        .arg("device");
    data_block(runner, &spec, |text| Some(parse_devices(text)))
}

/// Wi-Fi-устройство для раздела: подключённое важнее молчаливого, порядок
/// вывода nmcli не гарантирован.
fn device_block(devices: &Block<Vec<WifiDevice>>) -> Block<WifiDevice> {
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
fn wireless_profiles(runner: &dyn CommandRunner, visible: &[WifiNet]) -> Vec<Profile> {
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
    let visible: Vec<&str> = visible.iter().map(|net| net.ssid.as_str()).collect();
    profiles
        .into_iter()
        .map(|mut profile| {
            if !visible.contains(&profile.name.as_str()) {
                profile.ssid = profile_ssid(runner, &profile.uuid);
            }
            profile
        })
        .collect()
}

/// SSID профиля: `nmcli -t -f 802-11-wireless.ssid connection show uuid <UUID>`.
///
/// Ответ разбирается тем же `split_nmcli_terse`, что и основной список сетей:
/// двоеточие и обратный слэш внутри SSID приходят экранированными
/// (`My\:Network`, `Дом \\ принтер`), и голый `split_once(':')` оставил бы
/// escape-последовательности как есть — профиль бы не сматчился с видимой
/// сетью.
fn profile_ssid(runner: &dyn CommandRunner, uuid: &str) -> Option<String> {
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
fn network_list(runner: &dyn CommandRunner) -> Block<Vec<WifiNet>> {
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
fn bind_profiles(block: Block<Vec<WifiNet>>, profiles: &[Profile]) -> Block<Vec<WifiNet>> {
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

fn saved_uuid(profiles: &[Profile], ssid: &str) -> Option<String> {
    profiles
        .iter()
        .find(|profile| profile.ssid.as_deref() == Some(ssid))
        .or_else(|| profiles.iter().find(|profile| profile.name == ssid))
        .map(|profile| profile.uuid.clone())
}

/// Активное подключение: интерфейс плюс UUID его профиля.
fn active_link(device: Option<WifiDevice>, profiles: &[Profile]) -> Option<ActiveLink> {
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
pub fn connect_saved_action(uuid: &str) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("nmcli")
            .arg("connection")
            .arg("up")
            .arg("uuid")
            .arg(uuid)
            .with_timeout(CONNECT_TIMEOUT),
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
/// `nmcli device wifi connect <SSID> password <PASSWORD>`.
///
/// SSID — отдельный `CmdArg::Plain`, пароль — `CmdArg::Secret`. Shell не
/// используется. ВНИМАНИЕ: `CmdArg::Secret` прячет значение от журналов,
/// статусов и `Debug`, но не от `ps` или `/proc/<pid>/cmdline`: секрет
/// физически передаётся в `argv`. От того, чтобы его там не было, спасает
/// только stdin, и это отдельная задача.
///
/// Правило для SSID с ведущим дефисом — то же, что у открытой сети в W5.2a.
pub fn connect_with_password(ssid: &str, password: &str) -> CommandSpec {
    let mut command = CommandSpec::new("nmcli")
        .arg("device")
        .arg("wifi")
        .arg("connect");
    if ssid.starts_with('-') {
        command = command.arg("--");
    }
    command
        .arg(ssid)
        .arg("password")
        .secret_arg(password)
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

fn ru(lang: Language) -> bool {
    lang == Language::Ru
}

/// Отметка типа защиты: замок для закрытой сети, точка для открытой.
fn security_mark(security: Security) -> &'static str {
    match security {
        Security::Open => "открытая",
        Security::Secured => "защищённая",
    }
}

fn net_row_id(net: &WifiNet) -> String {
    net.row_id()
}

/// Строка сети: подпись, личность и действие по её состоянию.
fn net_row(lang: Language, net: &WifiNet, active_uuid: Option<&str>) -> Node {
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
fn status_row(lang: Language, snapshot: &WifiSnapshot) -> Node {
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

/// Подменю со всеми сетями: тот же список, что и в разделе, но без обрезки.
fn all_networks_submenu(
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
        format!("Все сети ({})", networks.len())
    } else {
        format!("All networks ({})", networks.len())
    };
    Some(Node::submenu(settings_icons::NETWORK, &title, children).with_id("wifi/all"))
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
        let inline = networks
            .iter()
            .take(MAX_INLINE_NETWORKS)
            .map(|net| net_row(lang, net, active_uuid))
            .collect::<Vec<Node>>();
        let hidden = networks.len().saturating_sub(MAX_INLINE_NETWORKS);
        rows.extend(inline);
        if hidden > 0
            && let Some(submenu) = all_networks_submenu(lang, snapshot, active_uuid)
        {
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
fn error_row(_lang: Language, snapshot: &WifiSnapshot) -> Option<Node> {
    let reason = match (&snapshot.radio, &snapshot.device, &snapshot.networks) {
        (Block::Failed(reason), _, _) => ("nmcli radio", reason),
        (_, Block::Failed(reason), _) => ("nmcli device", reason),
        (_, _, Block::Failed(reason)) => ("nmcli list", reason),
        _ => return None,
    };
    Some(
        Node::info(
            settings_icons::DOT,
            &format!("{}: {}", reason.0, first_line(reason.1)),
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

fn first_line(reason: &str) -> String {
    reason.lines().next().unwrap_or(reason).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::super::action::{Action, RecordingRunner};
    use super::super::system::{CommandError, CommandOutput, ScriptedRunner};
    use super::*;

    const RADIO_ON: &str = "enabled\n";
    const DEVICES: &str = "wlp0s20f0u2:wifi:connected:HONOR 200\nlo:loopback:connected (externally):lo\np2p-dev-wlp4s0:wifi-p2p:disconnected:\nwlp4s0:wifi:disconnected:\n";
    const LISTS: &str = r"*:My\:Net:78:WPA2:AA\:BB\:CC\:DD\:EE\:FF
 :My\:Net:92:WPA2:11\:22\:33\:44\:55\:66
 :Дом \\ принтер:54:WPA2:77\:88\:99\:AA\:BB\:CC
 :open-air:40::FF\:EE\:DD\:CC\:BB\:AA
 ::60:WPA2:01\:02\:03\:04\:05\:06
 :Дом \\ принтер:61:WPA2:88\:99\:AA\:BB\:CC\:DD
 :-dash-net:33:WPA2:DE\:AD\:BE\:EF\:00\:11
";

    fn ok(stdout: &str) -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            status: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    fn missing() -> Result<CommandOutput, CommandError> {
        Err(CommandError::Spawn("нет nmcli".to_string()))
    }

    /// Полный сценарий в порядке вызовов провайдера: radio, device, один
    /// список сетей, профили и дозапрос SSID для профиля с именем не из эфира.
    fn script() -> Vec<Result<CommandOutput, CommandError>> {
        vec![
            ok(RADIO_ON),
            ok(DEVICES),
            ok(LISTS),
            ok("HONOR 200:uuid-honor:802-11-wireless\nMeta:uuid-meta:tun\n"),
            ok("802-11-wireless.ssid:Cudy-5g\n"),
        ]
    }

    #[test]
    fn parses_networks_unescaping_ssid_and_drops_hidden() {
        let nets = parse_networks(LISTS);
        let names: Vec<&str> = nets.iter().map(|n| n.ssid.as_str()).collect();
        assert!(names.contains(&"My:Net"), "двоеточие из SSID: {names:?}");
        assert!(names.contains(&"Дом \\ принтер"));
        assert!(names.contains(&"-dash-net"));
        assert_eq!(
            names.len(),
            4,
            "сеть без SSID (скрытая) пропущена: {names:?}"
        );
        assert!(
            !names.iter().any(|name| name.is_empty()),
            "пустых имён в списке быть не должно"
        );
    }

    #[test]
    fn duplicates_collapse_to_the_strongest_but_keep_in_use() {
        let nets = parse_networks(LISTS);
        let net = nets.iter().find(|n| n.ssid == "My:Net").expect("сеть есть");
        assert_eq!(net.signal, 92, "сильнейшая точка");
        assert!(net.in_use, "флаг IN-USE сохранён");
        let printer = nets.iter().find(|n| n.ssid == "Дом \\ принтер").unwrap();
        assert_eq!(printer.signal, 61);
        assert_eq!(
            nets.iter().filter(|n| n.ssid == "Дом \\ принтер").count(),
            1,
            "дубль схлопнут"
        );
    }

    #[test]
    fn current_network_sorts_first_and_rest_by_signal() {
        let nets = parse_networks(LISTS);
        assert!(nets[0].in_use, "первой идёт текущая");
        let rest: Vec<u8> = nets[1..].iter().map(|n| n.signal).collect();
        let mut sorted = rest.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(rest, sorted, "остальные по убыванию сигнала");
    }

    #[test]
    fn security_field_marks_open_and_secured() {
        let nets = parse_networks(LISTS);
        let open = nets.iter().find(|n| n.ssid == "open-air").unwrap();
        assert_eq!(open.security, Security::Open);
        let closed = nets.iter().find(|n| n.ssid == "My:Net").unwrap();
        assert_eq!(closed.security, Security::Secured);
    }

    #[test]
    fn devices_pick_connected_wifi_over_idle_ones() {
        let devices = parse_devices(DEVICES);
        assert_eq!(devices.len(), 2, "wifi-p2p и loopback не устройства Wi-Fi");
        let block = device_block(&Block::Ready(devices));
        let device = block.ready().expect("устройство есть");
        assert_eq!(device.name, "wlp0s20f0u2");
        assert!(device.is_connected());
        assert_eq!(device.connection.as_deref(), Some("HONOR 200"));
    }

    #[test]
    fn no_wifi_device_is_not_an_error() {
        let text = "lo:loopback:connected (externally):lo\nenp3s0:ethernet:unavailable:\n";
        let block = device_block(&Block::Ready(parse_devices(text)));
        assert!(block.ready().expect("блок есть").name.is_empty());
        let snapshot = WifiSnapshot {
            radio: Block::Ready(true),
            device: block,
            active: None,
            networks: Block::Ready(Vec::new()),
        };
        assert!(snapshot.has_no_device());
        let titles: Vec<String> = build_nodes(&snapshot, Language::Ru)
            .into_iter()
            .map(|node| node.title)
            .collect();
        assert_eq!(titles, ["Нет Wi-Fi адаптера"]);
    }

    #[test]
    fn fetch_matches_networks_with_profiles() {
        let runner = ScriptedRunner::new(script());
        let snapshot = WifiProvider.fetch(&runner).expect("снимок");
        let networks = snapshot.networks.ready().expect("сети есть");
        assert_eq!(snapshot.radio, Block::Ready(true));
        assert_eq!(
            snapshot.active.as_ref().map(|a| a.uuid.clone()),
            Some(Some("uuid-honor".to_string())),
            "активное подключение опознано по профилю"
        );
        assert_eq!(networks.len(), 4, "скрытая отброшена, дубли схлопнуты");
        let calls = runner.calls();
        assert!(calls.iter().any(|c| c == "nmcli -t -f WIFI radio"));
        assert!(
            calls
                .iter()
                .any(|c| c == "nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device")
        );
        assert!(
            calls
                .iter()
                .any(|c| c == "nmcli -t -f NAME,UUID,TYPE connection show")
        );
        assert!(
            calls.iter().any(|c| c
                == "nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY,BSSID device wifi list --rescan no")
        );
        // Один дозапрос SSID — только для профиля, чьё имя не совпало ни с
        // одной видимой сетью.
        assert_eq!(
            calls
                .iter()
                .filter(|c| c.starts_with("nmcli -t -f 802-11-wireless.ssid connection show uuid"))
                .count(),
            1
        );
        // Список сетей запрашивается ровно один раз: и для показа, и для
        // решения, каким профилям уточнять SSID.
        assert_eq!(
            calls
                .iter()
                .filter(|c| c.contains("device wifi list"))
                .count(),
            1
        );
    }

    #[test]
    fn profile_name_may_differ_from_ssid_and_still_matches() {
        let runner = ScriptedRunner::new(vec![
            ok(RADIO_ON),
            ok("wlp0s20f0u2:wifi:connected:\n"),
            ok(":Cudy-5g:70:WPA2:AA:BB\n"),
            ok("Cudy-5g:uuid-cudy:802-11-wireless\n"),
            ok("802-11-wireless.ssid:Cudy-5g\n"),
        ]);
        let snapshot = WifiProvider.fetch(&runner).expect("снимок");
        let networks = snapshot.networks.ready().expect("сети есть");
        let cudy = networks.iter().find(|n| n.ssid == "Cudy-5g").unwrap();
        assert_eq!(cudy.saved_uuid.as_deref(), Some("uuid-cudy"));
    }

    /// SSID профиля приходит экранированным, как в списке сетей: `My\:Network`
    /// — это `My:Network`, а не строка с обратным слэшем. Голый
    /// `split_once(':')` оставлял escape как есть, и профиль не матчился.
    #[test]
    fn escaped_profile_ssid_unescapes_like_the_network_list() {
        let runner = ScriptedRunner::new(vec![ok("802-11-wireless.ssid:My\\:Network\n")]);
        assert_eq!(
            profile_ssid(&runner, "uuid-odd"),
            Some("My:Network".to_string())
        );
        let runner = ScriptedRunner::new(vec![ok("802-11-wireless.ssid:Дом \\\\ принтер\n")]);
        assert_eq!(
            profile_ssid(&runner, "uuid-odd"),
            Some("Дом \\ принтер".to_string())
        );
    }

    /// Профиль с экранированным SSID матчится с видимой сетью: UUID
    /// проставляется, сеть подключается по профилю, а не как новая.
    #[test]
    fn escaped_profile_ssid_matches_the_visible_network() {
        let runner = ScriptedRunner::new(vec![
            ok(RADIO_ON),
            ok("wlp0s20f0u2:wifi:connected:\n"),
            ok(":My\\:Net:70:WPA2:AA:BB\n"),
            ok("OddName:uuid-odd:802-11-wireless\n"),
            ok("802-11-wireless.ssid:My\\:Net\n"),
        ]);
        let snapshot = WifiProvider.fetch(&runner).expect("снимок");
        let networks = snapshot.networks.ready().expect("сети есть");
        let net = networks.iter().find(|n| n.ssid == "My:Net").unwrap();
        assert_eq!(net.saved_uuid.as_deref(), Some("uuid-odd"));
    }

    /// Сбой чтения радио — это «неизвестно», а не «выкл». Переключателя при
    /// этом нет (состояние недостоверно), а прочитанный список сетей
    /// показывается как есть.
    #[test]
    fn radio_failure_is_unknown_not_off() {
        let snapshot = WifiSnapshot {
            radio: Block::Failed("не ответил".to_string()),
            device: device_block(&Block::Ready(parse_devices(DEVICES))),
            active: None,
            networks: Block::Ready(parse_networks(LISTS)),
        };
        let rows = build_nodes(&snapshot, Language::Ru);
        let titles: Vec<&str> = rows.iter().map(|n| n.title.as_str()).collect();
        assert!(
            titles[0].starts_with("Wi-Fi: неизвестно"),
            "ошибка чтения не превращается в «выкл»: {titles:?}"
        );
        assert!(
            !titles.iter().any(|t| t.contains("ключить Wi-Fi")),
            "переключателя по недостоверному состоянию нет: {titles:?}"
        );
        assert!(
            titles.iter().any(|t| t.contains("My:Net")),
            "прочитанные сети показываются: {titles:?}"
        );
    }

    #[test]
    fn radio_off_hides_the_network_list() {
        let snapshot = WifiSnapshot {
            radio: Block::Ready(false),
            device: device_block(&Block::Ready(parse_devices(DEVICES))),
            active: None,
            networks: Block::Ready(parse_networks(LISTS)),
        };
        let rows = build_nodes(&snapshot, Language::Ru);
        let titles: Vec<&str> = rows.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles[0], "Wi-Fi: выкл · не подключено");
        // Отдельной строки «Включить Wi-Fi» больше нет: заголовок сам
        // переключает радио.
        assert!(matches!(
            rows[0].kind,
            super::super::tree::NodeKind::Action(_)
        ));
        assert!(
            !titles.iter().any(|t| t.contains("My")),
            "сети при выключенном радио не показываются: {titles:?}"
        );
    }

    #[test]
    fn radio_off_still_fetches_but_marks_the_list_missing() {
        let mut answers = script();
        answers[0] = ok("disabled\n");
        let runner = ScriptedRunner::new(answers);
        let snapshot = WifiProvider.fetch(&runner).expect("снимок");
        assert_eq!(snapshot.radio, Block::Ready(false));
    }

    #[test]
    fn no_nmcli_is_unavailable() {
        let runner = ScriptedRunner::new(vec![missing(); 5]);
        let error = WifiProvider.fetch(&runner).expect_err("раздел недоступен");
        assert!(matches!(error, ProviderError::Unavailable(_)), "{error:?}");
        assert_eq!(error.message(), "нет nmcli");
    }

    #[test]
    fn partial_failure_keeps_the_other_blocks() {
        let mut answers = script();
        answers[3] = missing(); // connection show не ответил
        let runner = ScriptedRunner::new(answers);
        let snapshot = WifiProvider.fetch(&runner).expect("снимок");
        assert_eq!(snapshot.radio, Block::Ready(true));
        let networks = snapshot.networks.ready().expect("сети есть");
        assert!(networks.iter().all(|n| n.saved_uuid.is_none()));
        assert_eq!(
            snapshot.active.as_ref().map(|a| a.uuid.clone()),
            Some(None),
            "активное подключение есть, профиля не видно"
        );
    }

    #[test]
    fn radio_action_toggles_with_exact_argv() {
        let off = radio_action(false);
        let runner = RecordingRunner::new();
        off.perform_with(&runner).expect("выполнено");
        assert_eq!(runner.calls(), ["run nmcli radio wifi off"]);
        let on = radio_action(true);
        let runner = RecordingRunner::new();
        on.perform_with(&runner).expect("выполнено");
        assert_eq!(runner.calls(), ["run nmcli radio wifi on"]);
    }

    #[test]
    fn saved_network_is_switched_by_uuid() {
        let up = connect_saved_action("uuid-cudy");
        let runner = RecordingRunner::new();
        up.perform_with(&runner).expect("выполнено");
        assert_eq!(runner.calls(), ["run nmcli connection up uuid uuid-cudy"]);
        let down = disconnect_action("uuid-honor");
        let runner = RecordingRunner::new();
        down.perform_with(&runner).expect("выполнено");
        assert_eq!(
            runner.calls(),
            ["run nmcli connection down uuid uuid-honor"]
        );
    }

    #[test]
    fn open_network_connects_by_name_and_dash_ssid_is_guarded() {
        let plain = connect_open_action("open-air");
        let runner = RecordingRunner::new();
        plain.perform_with(&runner).expect("выполнено");
        assert_eq!(runner.calls(), ["run nmcli device wifi connect open-air"]);

        let dash = connect_open_action("-dash-net");
        let runner = RecordingRunner::new();
        dash.perform_with(&runner).expect("выполнено");
        assert_eq!(
            runner.calls(),
            ["run nmcli device wifi connect -- -dash-net"],
            "SSID с ведущим дефисом не должен выглядеть опцией"
        );

        let weird = connect_open_action("Мой Дом: принтер");
        let runner = RecordingRunner::new();
        weird.perform_with(&runner).expect("выполнено");
        assert_eq!(
            runner.calls(),
            ["run nmcli device wifi connect \"Мой Дом: принтер\""],
            "пробел в SSID — отдельный argv-элемент, в журнале в кавычках"
        );
    }

    /// Живой опрос хоста: только команды чтения. Запускается вручную
    /// (`cargo test --bin hud-menu-rs -- --ignored live_provider`).
    #[test]
    #[ignore = "обращается к nmcli хоста"]
    fn live_provider_reads_the_host() {
        let snapshot = WifiProvider
            .fetch(&SystemCommandRunner::default())
            .expect("на хосте есть nmcli");
        println!("радио: {:?}", snapshot.radio);
        println!("устройство: {:?}", snapshot.device);
        println!("активно: {:?}", snapshot.active);
        println!(
            "сети: {:?}",
            snapshot.networks.ready().map(|list| list
                .iter()
                .map(|net| (
                    net.ssid.clone(),
                    net.signal,
                    net.in_use,
                    net.saved_uuid.clone()
                ))
                .collect::<Vec<_>>())
        );
        assert!(matches!(snapshot.radio, Block::Ready(_)));
        assert!(
            !snapshot
                .device
                .ready()
                .expect("устройство есть")
                .name
                .is_empty()
        );
        for net in snapshot.networks.ready().expect("сети есть") {
            assert!(
                !net.ssid.is_empty(),
                "скрытых сетей в снимке быть не должно"
            );
        }
    }

    #[test]
    fn rate_limit_is_recognized_as_a_warning_not_a_failure() {
        assert!(is_rate_limited(
            "Error: Connection attempt failed: too frequent"
        ));
        assert!(is_rate_limited("Ошибка: сканирование слишком часто"));
        assert!(!is_rate_limited("Error: No network devices available"));
    }

    #[test]
    fn every_action_refreshes_wifi() {
        for action in [
            radio_action(true),
            connect_saved_action("uuid"),
            disconnect_action("uuid"),
            connect_open_action("net"),
            rescan_action(),
        ] {
            match &action {
                Action::RefreshAndRun { refresh, .. } => assert_eq!(*refresh, ProviderKey::WIFI),
                other => panic!("ожидался RefreshAndRun: {other:?}"),
            }
        }
    }

    /// Подключение и пересканирование идут дольше обычного чтения, а
    /// переключение радио и отключение остаются быстрыми: иначе меню будет
    /// ждать секунды там, где хватило бы десяти миллисекунд.
    #[test]
    fn slow_actions_carry_the_connect_timeout() {
        for action in [
            connect_saved_action("uuid"),
            connect_open_action("net"),
            rescan_action(),
        ] {
            match &action {
                Action::RefreshAndRun { command, .. } => {
                    assert_eq!(command.timeout(), CONNECT_TIMEOUT, "{}", command.log_line());
                }
                other => panic!("ожидался RefreshAndRun: {other:?}"),
            }
        }
        for action in [radio_action(false), disconnect_action("uuid")] {
            match &action {
                Action::RefreshAndRun { command, .. } => {
                    assert!(
                        command.timeout() < CONNECT_TIMEOUT,
                        "{}",
                        command.log_line()
                    );
                }
                other => panic!("ожидался RefreshAndRun: {other:?}"),
            }
        }
    }

    #[test]
    fn rescan_is_a_separate_argv_and_not_a_shell_line() {
        let rescan = rescan_action();
        let runner = RecordingRunner::new();
        rescan.perform_with(&runner).expect("выполнено");
        assert_eq!(runner.calls(), ["run nmcli device wifi rescan"]);
        if let Action::RefreshAndRun { command, .. } = &rescan {
            let (program, args) = command.argv();
            assert_eq!(program, "nmcli");
            assert!(!args.iter().any(|a| a.contains("-c")), "shell запрещён");
            assert!(!args.iter().any(|a| a.contains(';')), "shell запрещён");
        }
    }

    /// Защищённая сеть без профиля открывает ввод пароля и НЕ запускает
    /// nmcli: до `Enter` в поле пароля выполнять нечего, а SSID в этот момент
    /// не должен никуда уходить.
    #[test]
    fn protected_network_without_profile_only_opens_the_password_input() {
        let snapshot = WifiSnapshot {
            radio: Block::Ready(true),
            device: device_block(&Block::Ready(parse_devices(DEVICES))),
            active: None,
            networks: Block::Ready(parse_networks(LISTS)),
        };
        let rows = build_nodes(&snapshot, Language::Ru);
        let protected = rows
            .iter()
            .find(|row| row.title.starts_with("Дом"))
            .expect("защищённая сеть без профиля в списке");
        match &protected.kind {
            super::super::tree::NodeKind::Action(Action::SecretInput(target)) => {
                assert_eq!(target.subject(), "Дом \\ принтер");
                assert_eq!(target.row_id(), protected.identity());
                assert_eq!(target.provider_key(), ProviderKey::WIFI);
            }
            other => panic!("ожидался ввод пароля, {other:?}"),
        }
        // Подпись больше не обещает «скоро»: пароль вводится здесь и сейчас.
        assert!(
            !protected.title.contains("нужен пароль"),
            "{}",
            protected.title
        );
    }

    /// Подключение с паролем: SSID — обычный аргумент, пароль — секретный.
    /// Никакого `sh -c`, пароля в строке журнала нет.
    #[test]
    fn connect_with_password_keeps_the_password_out_of_every_text() {
        let spec = connect_with_password("HONOR 200", "тихийпароль");
        let (program, args) = spec.argv();
        assert_eq!(program, "nmcli");
        assert_eq!(
            args,
            [
                "device",
                "wifi",
                "connect",
                "HONOR 200",
                "password",
                "тихийпароль"
            ]
        );
        assert!(
            !args.iter().any(|arg| arg == "-c" || arg.contains("sh -c")),
            "shell не используется"
        );
        let line = spec.log_line();
        assert!(line.contains("nmcli device wifi connect"), "{line}");
        assert!(!line.contains("тихийпароль"), "пароль в log_line: {line}");
        assert!(line.contains("<redacted>"), "{line}");
        let debug = format!("{spec:?}");
        assert!(!debug.contains("тихийпароль"), "пароль в Debug: {debug}");
        assert!(spec.has_secrets());
    }

    /// SSID с двоеточием, пробелом, юникодом и ведущим дефисом остаётся
    /// отдельным argv-элементом; ведущий дефис по-прежнему закрывается `--`,
    /// как в W5.2a для открытых сетей.
    #[test]
    fn password_connect_guards_odd_ssdids() {
        for (ssid, expected) in [
            (
                "My:Network",
                vec!["device", "wifi", "connect", "My:Network", "password"],
            ),
            (
                "Дом \\ принтер",
                vec!["device", "wifi", "connect", "Дом \\ принтер", "password"],
            ),
            (
                "HONOR 200",
                vec!["device", "wifi", "connect", "HONOR 200", "password"],
            ),
            (
                "-dash-net",
                vec!["device", "wifi", "connect", "--", "-dash-net", "password"],
            ),
            (
                "Сеть Wi-Fi",
                vec!["device", "wifi", "connect", "Сеть Wi-Fi", "password"],
            ),
        ] {
            let spec = connect_with_password(ssid, "pw");
            let (_, args) = spec.argv();
            assert_eq!(
                &args[..expected.len()],
                expected.as_slice(),
                "SSID {ssid:?}"
            );
            assert_eq!(args.len(), expected.len() + 1, "после SSID идёт пароль");
            assert!(
                !spec.log_line().contains("pw") || ssid.contains("pw"),
                "{ssid:?}"
            );
        }
    }

    /// Пустой пароль не запрещаем: NetworkManager сам решит, допустим ли он.
    /// Важно, что он всё равно уходит секретным аргументом, а не пустым
    /// `Plain`.
    #[test]
    fn empty_password_is_still_a_secret_argument() {
        let spec = connect_with_password("open-ish", "");
        let (_, args) = spec.argv();
        assert_eq!(
            args,
            ["device", "wifi", "connect", "open-ish", "password", ""]
        );
        assert!(spec.has_secrets());
        assert!(!spec.log_line().contains("Plain"), "{}", spec.log_line());
    }

    #[test]
    fn rows_keep_their_ids_across_a_new_scan() {
        let first = build_nodes(
            &WifiSnapshot {
                radio: Block::Ready(true),
                device: device_block(&Block::Ready(parse_devices(DEVICES))),
                active: None,
                networks: Block::Ready(parse_networks(LISTS)),
            },
            Language::Ru,
        );
        let second = build_nodes(
            &WifiSnapshot {
                radio: Block::Ready(true),
                device: device_block(&Block::Ready(parse_devices(DEVICES))),
                active: None,
                networks: Block::Ready(parse_networks(LISTS)),
            },
            Language::Ru,
        );
        let ids = |rows: Vec<Node>| {
            rows.into_iter()
                .map(|n| n.identity().to_string())
                .collect::<Vec<_>>()
        };
        let (first, second) = (ids(first), ids(second));
        assert_eq!(first, second);
        assert!(second.contains(&"wifi/net/My:Net".to_string()));
    }

    #[test]
    fn inline_list_is_capped_and_the_rest_lives_in_a_submenu() {
        let many: String = (0..20)
            .map(|index| format!(" :net{index}:{}:WPA2:AA:BB\n", 20 + index))
            .collect();
        let snapshot = WifiSnapshot {
            radio: Block::Ready(true),
            device: device_block(&Block::Ready(parse_devices(DEVICES))),
            active: None,
            networks: Block::Ready(parse_networks(&many)),
        };
        let rows = build_nodes(&snapshot, Language::Ru);
        let inline = rows
            .iter()
            .filter(|r| r.identity().starts_with("wifi/net/"))
            .count();
        assert_eq!(inline, MAX_INLINE_NETWORKS);
        assert!(rows.iter().any(|r| r.identity() == "wifi/all"));
    }

    #[test]
    fn status_row_reads_radio_and_link() {
        let snapshot = WifiSnapshot {
            radio: Block::Ready(true),
            device: device_block(&Block::Ready(parse_devices(DEVICES))),
            active: Some(ActiveLink {
                device: "wlp0s20f0u2".to_string(),
                name: "HONOR 200".to_string(),
                uuid: Some("uuid-honor".to_string()),
            }),
            networks: Block::Ready(Vec::new()),
        };
        let rows = build_nodes(&snapshot, Language::Ru);
        assert_eq!(rows[0].title, "Wi-Fi: вкл · HONOR 200");
    }
}

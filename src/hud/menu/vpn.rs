//! VPN-раздел: три режима поверх селекторов Mihomo.
//!
//! Протокол здесь не реализуется: окно управляет уже запущенным Mihomo через
//! локальный external controller. Поэтому у режима ровно два селектора:
//!
//! | режим     | `MODE-RU` | `MODE-REST` | смысл                            |
//! |-----------|-----------|-------------|----------------------------------|
//! | `all`     | `PROXY`   | `PROXY`     | всё через прокси                 |
//! | `noru`    | `DIRECT`  | `PROXY`     | RU напрямую, остальное через прокси |
//! | `blocked` | `DIRECT`  | `DIRECT`    | только заблокированное через прокси |
//!
//! Имена селекторов и значения берутся из ответа контроллера, а не выдумываются
//! здесь: провайдер может переименовать группу, и тогда режим должен честно
//! показать «неизвестно», а не тихо ничего не применить.
//!
//! Модуль чистый: ни сети, ни Wayland. Всё, что связано с HTTP, лежит в
//! [`super::vpn_api`].

use super::settings::Language;
use super::settings_icons;
use super::tree::Node;
use super::vpn_api::{self, Controller};

/// Селектор, которым управляет режим `MODE-RU`. Имя из конфига Mihomo.
pub const SELECTOR_RU: &str = "MODE-RU";
/// Селектор, которым управляет режим `MODE-REST`.
pub const SELECTOR_REST: &str = "MODE-REST";
/// Значение «напрямую».
pub const DIRECT: &str = "DIRECT";
/// Значение «через прокси».
pub const PROXY: &str = "PROXY";
/// Селектор, в котором выбирается конкретный узел.
pub const PROXY_GROUP: &str = "PROXY";
/// Страны в списке серверов: флаг из названия узла и его название.
///
/// Фильтр по флагу, а не по слову в названии: у Mihomo в одном списке живут
/// «🇳🇴 Норвегия», «🇳🇴 LTE Авто - Норвегия» и «🇳🇴 0.1X - LTE №61 -
/// Норвегия», и слово «Норвегия» есть не у всех узлов этой страны, а флаг —
/// у всех. Скандинавия и Финляндия вместе: различать их в списке не зачем.
pub const REGIONS: [(&str, &str); 7] = [
    ("🇳🇴", "Норвегия"),
    ("🇸🇪", "Швеция"),
    ("🇩🇰", "Дания"),
    ("🇫🇮", "Финляндия"),
    ("🇮🇸", "Исландия"),
    ("🇺🇸", "США"),
    ("🇯🇵", "Япония"),
];

/// Идентификатор строки узла. Само название и есть личность строки: после
/// перечитывания списка выбранный узел должен остаться выбранным, а не съехать
/// на другую строку.
pub fn node_id(node: &ProxyNode) -> String {
    format!("vpn/node/{}", node.name)
}

/// Страна узла по флагу в названии.
pub fn region_of(name: &str) -> Option<&'static str> {
    REGIONS
        .iter()
        .find(|(flag, _)| name.contains(flag))
        .map(|(_, country)| *country)
}

/// Узел из списка `PROXY`, прошедший фильтр по странам.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyNode {
    /// Название узла так, как его знает Mihomo: его и надо слать в `PUT`.
    pub name: String,
    /// Страна из [`REGIONS`].
    pub country: &'static str,
}

/// Режим VPN-окна.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpnMode {
    /// Всё через прокси.
    All,
    /// Россия напрямую, остальное через прокси.
    NoRu,
    /// Только заблокированное через прокси.
    Blocked,
}

impl VpnMode {
    /// Все режимы в порядке показа в меню.
    pub const ALL: [VpnMode; 3] = [VpnMode::All, VpnMode::NoRu, VpnMode::Blocked];

    /// Подпись режима на языке окна.
    pub fn label(self, lang: Language) -> &'static str {
        match (lang, self) {
            (Language::Ru, Self::All) => "Всё через прокси",
            (Language::Ru, Self::NoRu) => "RU напрямую",
            (Language::Ru, Self::Blocked) => "Только заблокированное",
            (Language::En, Self::All) => "All via proxy",
            (Language::En, Self::NoRu) => "RU direct",
            (Language::En, Self::Blocked) => "Blocked only",
        }
    }

    /// Идентификатор строки режима. Собирается здесь, а не в дереве: id строки
    /// переживает смену языка и смену снимка, поэтому он должен быть стабильным.
    pub fn id(mode: VpnMode) -> &'static str {
        match mode {
            Self::All => "vpn/all",
            Self::NoRu => "vpn/noru",
            Self::Blocked => "vpn/blocked",
        }
    }

    /// Короткое имя для поиска и для идентификатора строки.
    pub fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::NoRu => "noru",
            Self::Blocked => "blocked",
        }
    }

    /// Режим по короткому имени. Неизвестное имя — `None`, а не значение по
    /// умолчанию: подпись в файле может прийти из правки руками.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "all" => Some(Self::All),
            "noru" => Some(Self::NoRu),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }

    /// Что должно стоять в каждом селекторе для этого режима.
    ///
    /// Порядок важен: сначала `MODE-RU`, потом `MODE-REST`. Между двумя
    /// записями трафик живёт в промежуточном состоянии — при `all` это лишняя
    /// секунды прямого соединения для RU-сайтов, при `blocked` — наоборот,
    /// лишняя секунды прокси для всего остального.
    pub fn selectors(self) -> [(&'static str, &'static str); 2] {
        match self {
            Self::All => [(SELECTOR_RU, PROXY), (SELECTOR_REST, PROXY)],
            Self::NoRu => [(SELECTOR_RU, DIRECT), (SELECTOR_REST, PROXY)],
            Self::Blocked => [(SELECTOR_RU, DIRECT), (SELECTOR_REST, DIRECT)],
        }
    }

    /// Режим по текущим значениям селекторов.
    ///
    /// `None` означает «состояние не соответствует ни одному режиму»: это
    /// честнее, чем подгонять чужое сочетание под один из трёх режимов, потому
    /// что тогда интерфейс соврал бы о том, что включено.
    pub fn detect(ru: &str, rest: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| {
            let selectors = mode.selectors();
            selectors[0].1 == ru && selectors[1].1 == rest
        })
    }
}

/// Значение одного селектора в ответе контроллера.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    /// Имя селектора: `MODE-RU` или `MODE-REST`.
    pub name: &'static str,
    /// Что выбрано сейчас.
    pub now: String,
}

/// Снимок состояния VPN для окна.
///
/// Ошибка хранится вместе с данными, а не вместо них: когда демон отвечает, но
/// один селектор прочитать не удалось, полезно показать и второе, и неудачу.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct VpnSnapshot {
    /// Ответил ли контроллер.
    pub reachable: bool,
    /// Версия Mihomo, как она в `/version`.
    pub version: String,
    /// Значения двух селекторов: пусто, если дочитать не удалось.
    pub selectors: Vec<Selector>,
    /// Что не получилось: пусто, когда снимок полный.
    pub error: String,
    /// Выбранный сейчас узел из группы `PROXY`.
    pub selected: String,
    /// Узлы из [`REGIONS`], в порядке Mihomo.
    pub servers: Vec<ProxyNode>,
}

impl VpnSnapshot {
    /// Значение селектора по имени.
    pub fn selector(&self, name: &str) -> Option<&str> {
        self.selectors
            .iter()
            .find(|selector| selector.name == name)
            .map(|selector| selector.now.as_str())
    }

    /// Текущий режим, если состояние соответствует одному из трёх.
    pub fn mode(&self) -> Option<VpnMode> {
        VpnMode::detect(
            self.selector(SELECTOR_RU).unwrap_or_default(),
            self.selector(SELECTOR_REST).unwrap_or_default(),
        )
    }

    /// Снимок с ошибкой вместо данных.
    pub fn failed(error: impl Into<String>) -> Self {
        VpnSnapshot {
            reachable: false,
            error: error.into(),
            ..VpnSnapshot::default()
        }
    }

    /// Число узлов в списке: показывается в подписи подменю.
    pub fn servers_count(&self) -> usize {
        self.servers.len()
    }
}

/// Разбор `GET /proxies/PROXY`: выбранный узел и отфильтрованный список.
///
/// Возвращается только то, что попало в [`REGIONS`]: 542 узла в селекторе
/// этому окну не нужны, а имена содержат флаг страны, по которому и идёт
/// отбор. Порядок сохраняется: он от Mihomo, и после перезагрузки подписок
/// список не прыгает сам по себе.
pub fn parse_members(text: &str) -> Result<(String, Vec<ProxyNode>), String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("список серверов: {error}"))?;
    let selected = value
        .get("now")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "в ответе нет поля now".to_string())?
        .to_string();
    let all = value
        .get("all")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "в ответе нет поля all".to_string())?;
    let mut servers = Vec::new();
    for entry in all {
        let Some(name) = entry.as_str() else { continue };
        // Узлы-группы в списке не нужны: и `AUTO`, и прочие группы выбираются
        // отдельно, а в списке стран они были лишней строкой без флага.
        if let Some(country) = region_of(name) {
            servers.push(ProxyNode {
                name: name.to_string(),
                country,
            });
        }
    }
    Ok((selected, servers))
}

/// Ответ `GET /proxies/<имя>`: у селектора нужны `now` и `all`.
pub fn parse_selector(name: &'static str, text: &str) -> Result<Selector, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("{name}: {error}"))?;
    let now = value
        .get("now")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("{name}: в ответе нет поля now"))?;
    Ok(Selector {
        name,
        now: now.to_string(),
    })
}

/// Ответ `GET /version`: версия Mihomo для строки статуса.
pub fn parse_version(text: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("version: {error}"))?;
    Ok(value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?")
        .to_string())
}

/// Собирает снимок из уже прочитанных ответов контроллера.
///
/// Ошибка чтения одного селектора не отменяет второй: окно покажет то, что
/// успело прочитать, и подсказку в подвале. `selector_errors` — подписи
/// неудачных чтений в порядке селекторов.
pub fn snapshot(
    version: Result<String, String>,
    selectors: [Result<Selector, String>; 2],
    selector_errors: [Option<&'static str>; 2],
    members: Result<(String, Vec<ProxyNode>), String>,
) -> VpnSnapshot {
    let mut problems = Vec::new();
    let version = match version {
        Ok(version) => version,
        Err(error) => {
            problems.push(format!("version: {error}"));
            String::new()
        }
    };
    // Список серверов — самая крупная выдача. Его ошибка не отменяет режимы:
    // три строки режимов полезнее, чем пустой список из-за одного запроса.
    let (selected, servers) = match members {
        Ok(members) => members,
        Err(error) => {
            problems.push(format!("серверы: {error}"));
            (String::new(), Vec::new())
        }
    };
    let mut state = VpnSnapshot {
        reachable: true,
        version,
        selectors: Vec::with_capacity(2),
        error: String::new(),
        selected,
        servers,
    };
    for (index, result) in selectors.into_iter().enumerate() {
        match result {
            Ok(selector) => state.selectors.push(selector),
            Err(error) => {
                let name = selector_errors[index].unwrap_or("?");
                problems.push(format!("{name}: {error}"));
            }
        }
    }
    state.error = problems.join("; ");
    state
}

/// Контроллер из `settings.json`: адрес и таймаут берутся из настроек, секрет —
/// из `HUD_VPN_SECRET` или файла `~/.config/mihomo/hud-secret`.
pub fn controller() -> Controller {
    let settings = super::settings::load();
    let secret = vpn_api::secret_from_env_or_file(&secret_path());
    Controller::with_addr(&settings.vpn_controller, settings.vpn_timeout()).with_secret(secret)
}

/// Путь к файлу с секретом контроллера.
pub fn secret_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::PathBuf::from(home).join(".config/mihomo/hud-secret")
}

/// Читает снимок состояния через контроллер.
///
/// Три запроса подряд, но ошибка одного не отменяет остальные: окно покажет
/// то, что прочиталось, и подсказку в подвале. `version` читается первым,
/// потому что он самый дешёвый и сразу говорит, что демон жив.
pub fn fetch(client: &Controller) -> VpnSnapshot {
    let version = match client.get("/version") {
        Ok(text) => parse_version(&text),
        // Без ответа контроллера селекторы всё равно не прочитать. Возвращаем
        // недоступный snapshot, а не «доступный» с пустыми значениями.
        Err(error) => return VpnSnapshot::failed(error),
    };
    let selectors = [
        client
            .get(&vpn_api::selector_path(SELECTOR_RU))
            .and_then(|text| parse_selector(SELECTOR_RU, &text)),
        client
            .get(&vpn_api::selector_path(SELECTOR_REST))
            .and_then(|text| parse_selector(SELECTOR_REST, &text)),
    ];
    // Список серверов читается последним: он самый крупный, и без него три
    // режима всё равно показываются.
    let members = client
        .get(&vpn_api::selector_path(PROXY_GROUP))
        .and_then(|text| parse_members(&text));
    snapshot(
        version,
        selectors,
        [Some(SELECTOR_RU), Some(SELECTOR_REST)],
        members,
    )
}

/// Снимок и строки раздела для окна и провайдера меню.
///
/// Ошибка чтения не поднимается: недоступный контроллер — это строка «Mihomo не
/// отвечает» с текстом причины, а не пустой раздел. Иначе окно показывало бы
/// «ошибка» без того, что именно сломалось.
pub fn fetch_nodes(lang: Language) -> Result<Vec<Node>, String> {
    let snapshot = fetch(&controller());
    Ok(nodes(&snapshot, lang))
}

/// Строки раздела VPN по снимку.
///
/// Корень короткий: статус, две папки и обновление. Семьдесят серверов и три
/// режима живут в своих подменю — иначе список не влезал в карточку и выбор
/// терялся среди десятков строк. Текущий режим виден уже в заголовке папки,
/// а текущий сервер отмечен галочкой внутри папки серверов.
pub fn nodes(snapshot: &VpnSnapshot, lang: Language) -> Vec<Node> {
    if !snapshot.reachable {
        let title = match lang {
            Language::Ru => format!("Mihomo не отвечает: {}", snapshot.error),
            Language::En => format!("Mihomo unreachable: {}", snapshot.error),
        };
        return vec![Node::info(settings_icons::NETWORK, &title).search_as(&[
            "mihomo",
            "vpn",
            "прокси",
        ])];
    }
    let mut rows = vec![status_row(snapshot, lang)];
    if !snapshot.error.is_empty() {
        let title = match lang {
            Language::Ru => format!("Ошибка: {}", snapshot.error),
            Language::En => format!("Error: {}", snapshot.error),
        };
        rows.push(
            Node::info(settings_icons::DOT, &title)
                .with_id("vpn/error")
                .search_as(&["ошибка", "error"]),
        );
    }
    let current = snapshot.mode();
    rows.push(modes_section(current, lang));
    rows.push(servers_section(snapshot, lang));
    let refresh_title = match lang {
        Language::Ru => "Обновить",
        Language::En => "Refresh",
    };
    rows.push(
        Node::action(
            settings_icons::UP,
            refresh_title,
            super::action::Action::RefreshDynamic(super::system::ProviderKey::VPN),
        )
        .with_id("vpn/refresh")
        .search_as(&["refresh", "обновить"]),
    );
    rows
}

/// Папка режимов: три строки, текущий отмечен. Текущий режим виден и снаружи —
/// в заголовке папки, чтобы ради ответа «что включено» не заходить внутрь.
fn modes_section(current: Option<VpnMode>, lang: Language) -> Node {
    let title = match (lang, current) {
        (Language::Ru, Some(mode)) => format!("Режим: {}", mode.label(lang)),
        (Language::Ru, None) => "Режим: неизвестен".to_string(),
        (Language::En, Some(mode)) => format!("Mode: {}", mode.label(lang)),
        (Language::En, None) => "Mode: unknown".to_string(),
    };
    let children: Vec<Node> = VpnMode::ALL
        .into_iter()
        .map(|mode| {
            let keywords: &'static [&'static str] = match mode {
                VpnMode::All => &["vpn", "all", "всё"],
                VpnMode::NoRu => &["vpn", "noru", "ru"],
                VpnMode::Blocked => &["vpn", "blocked", "заблокированное"],
            };
            Node::action(
                mode_icon(current == Some(mode)),
                mode.label(lang),
                super::action::Action::VpnMode(mode),
            )
            .with_id(VpnMode::id(mode))
            .search_as(keywords)
        })
        .collect();
    Node::submenu(settings_icons::CONTROLS, &title, children)
        .with_id("vpn/modes")
        .search_as(&["режим", "режимы", "mode", "modes"])
}

/// Папка серверов: только страны из [`REGIONS`], текущий узел отмечен
/// галочкой. Отдельный уровень нужен из-за объёма: 70 узлов в карточку
/// не помещаются, но поиск по ним работает и здесь.
fn servers_section(snapshot: &VpnSnapshot, lang: Language) -> Node {
    let title = match (lang, snapshot.servers_count()) {
        (Language::Ru, 0) => "Серверы: список пуст".to_string(),
        (Language::Ru, count) => format!("Серверы ({count})"),
        (Language::En, 0) => "Servers: empty list".to_string(),
        (Language::En, count) => format!("Servers ({count})"),
    };
    let children: Vec<Node> = snapshot
        .servers
        .iter()
        .map(|node| {
            let label = format!("{} {}", node.country, node.name);
            let icon = if node.name == snapshot.selected {
                settings_icons::CHECK
            } else {
                settings_icons::DOT
            };
            Node::action(
                icon,
                &label,
                super::action::Action::VpnNode(node.name.clone()),
            )
            .with_id(&node_id(node))
        })
        .collect();
    Node::submenu(settings_icons::STYLE, &title, children)
        .with_id("vpn/servers")
        .search_as(&["серверы", "servers", "узлы", "nodes"])
}

/// Строка состояния: режим, версия и оба селектора одной строкой.
///
/// Динамический текст кладётся в заголовок, а не в значение справа: значение
/// у узла — это `fn()`, без захвата, а снимок живёт дольше дерева. Так же
/// поступает раздел хранилищ.
fn status_row(snapshot: &VpnSnapshot, lang: Language) -> Node {
    let mode = match (snapshot.mode(), lang) {
        (Some(VpnMode::All), Language::Ru) => "всё через прокси",
        (Some(VpnMode::NoRu), Language::Ru) => "RU напрямую",
        (Some(VpnMode::Blocked), Language::Ru) => "только заблокированное",
        (Some(VpnMode::All), Language::En) => "all via proxy",
        (Some(VpnMode::NoRu), Language::En) => "RU direct",
        (Some(VpnMode::Blocked), Language::En) => "blocked only",
        (None, Language::Ru) => "состояние не соответствует режиму",
        (None, Language::En) => "state matches no mode",
    };
    let version = match (snapshot.version.is_empty(), lang) {
        (false, _) => snapshot.version.clone(),
        (true, Language::Ru) => "версия неизвестна".to_string(),
        (true, Language::En) => "version unknown".to_string(),
    };
    let title = format!(
        "{mode} · {version} · {} · {}",
        snapshot.selector(SELECTOR_RU).unwrap_or("—"),
        snapshot.selector(SELECTOR_REST).unwrap_or("—")
    );
    Node::info(settings_icons::SOUND, &title)
        .with_id("vpn/status")
        .search_as(&["статус", "status", "version"])
}

fn mode_icon(selected: bool) -> &'static str {
    if selected {
        settings_icons::CHECK
    } else {
        settings_icons::DOT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_maps_both_selectors() {
        assert_eq!(
            VpnMode::All.selectors(),
            [(SELECTOR_RU, PROXY), (SELECTOR_REST, PROXY)]
        );
        assert_eq!(
            VpnMode::NoRu.selectors(),
            [(SELECTOR_RU, DIRECT), (SELECTOR_REST, PROXY)]
        );
        assert_eq!(
            VpnMode::Blocked.selectors(),
            [(SELECTOR_RU, DIRECT), (SELECTOR_REST, DIRECT)]
        );
    }

    #[test]
    fn modes_are_distinguishable_by_their_pair() {
        let pairs: Vec<(String, String)> = VpnMode::ALL
            .into_iter()
            .map(|mode| {
                let s = mode.selectors();
                (s[0].1.to_string(), s[1].1.to_string())
            })
            .collect();
        assert_eq!(pairs.len(), 3);
        for (index, first) in pairs.iter().enumerate() {
            for second in pairs.iter().skip(index + 1) {
                assert_ne!(first, second, "два режима с одинаковой парой");
            }
        }
    }

    #[test]
    fn detect_reads_back_the_mihomo_state_observed_live() {
        // Это реальное состояние машины: MODE-RU=DIRECT, MODE-REST=PROXY.
        assert_eq!(VpnMode::detect(DIRECT, PROXY), Some(VpnMode::NoRu));
        assert_eq!(VpnMode::detect(PROXY, PROXY), Some(VpnMode::All));
        assert_eq!(VpnMode::detect(DIRECT, DIRECT), Some(VpnMode::Blocked));
    }

    #[test]
    fn detect_refuses_a_state_that_is_not_a_mode() {
        // Провайдер переименовал значение: показывать «режим» здесь нельзя.
        assert_eq!(VpnMode::detect("AUTO", PROXY), None);
        assert_eq!(VpnMode::detect("", ""), None);
    }

    #[test]
    fn keys_round_trip() {
        for mode in VpnMode::ALL {
            assert_eq!(VpnMode::from_key(mode.key()), Some(mode));
        }
        assert_eq!(VpnMode::from_key("nope"), None);
    }

    #[test]
    fn labels_follow_the_window_language() {
        assert_eq!(VpnMode::All.label(Language::Ru), "Всё через прокси");
        assert_eq!(VpnMode::NoRu.label(Language::En), "RU direct");
        assert_eq!(VpnMode::Blocked.label(Language::En), "Blocked only");
    }

    #[test]
    fn parses_a_selector_answer() {
        let selector = parse_selector(SELECTOR_RU, r#"{"now":"DIRECT","all":["DIRECT","PROXY"]}"#)
            .expect("селектор");

        assert_eq!(selector.name, SELECTOR_RU);
        assert_eq!(selector.now, "DIRECT");
    }

    #[test]
    fn selector_answer_without_now_is_an_error() {
        let error = parse_selector(SELECTOR_RU, r#"{"all":["DIRECT"]}"#).unwrap_err();

        assert!(error.contains("now"), "{error}");
    }

    #[test]
    fn broken_selector_json_is_an_error_not_a_panic() {
        assert!(parse_selector(SELECTOR_REST, "not json").is_err());
    }

    #[test]
    fn parses_the_version_answer() {
        assert_eq!(
            parse_version(r#"{"meta":true,"version":"v1.19.31"}"#).unwrap(),
            "v1.19.31"
        );
    }

    #[test]
    fn version_without_the_field_is_unknown_not_empty() {
        assert_eq!(parse_version("{}").unwrap(), "?");
    }

    /// Готовая пара селекторов для снимков.
    fn selectors() -> [Result<Selector, String>; 2] {
        [
            Ok(Selector {
                name: SELECTOR_RU,
                now: DIRECT.into(),
            }),
            Ok(Selector {
                name: SELECTOR_REST,
                now: PROXY.into(),
            }),
        ]
    }

    /// Список серверов из настоящего ответа Mihomo: три страны плюс посторонние
    /// узлы, которые фильтр обязан выбросить.
    fn members() -> Result<(String, Vec<ProxyNode>), String> {
        parse_members(
            r#"{"now":"[Q] 🇳🇴 Норвегия","all":["AUTO",
                "[Q] 🇳🇴 Норвегия",
                "[Q] 🇳🇴 ⚡️ Норвегия #2",
                "[Q] 🇺🇸 США, Атланта",
                "[Q] 🇯🇵 Япония, Токио",
                "[Q] 🇩🇪 Германия",
                "[S] 🇫🇮 Финляндия"]}"#,
        )
    }

    #[test]
    fn keeps_only_the_regions_from_the_list() {
        let (_, servers) = members().expect("список");

        let countries: Vec<&str> = servers.iter().map(|node| node.country).collect();
        assert_eq!(
            countries,
            ["Норвегия", "Норвегия", "США", "Япония", "Финляндия"]
        );
        assert!(
            !servers.iter().any(|node| node.name.contains("Германия")),
            "Германия вне списка"
        );
    }

    #[test]
    fn a_group_member_is_not_a_server() {
        let (_, servers) = members().expect("список");

        assert!(
            !servers.iter().any(|node| node.name == "AUTO"),
            "группа не сервер"
        );
    }

    #[test]
    fn the_selected_node_comes_through_unchanged() {
        let (selected, servers) = members().expect("список");

        assert_eq!(selected, "[Q] 🇳🇴 Норвегия");
        assert_eq!(servers[0].name, selected);
    }

    #[test]
    fn every_region_is_reachable_by_its_flag() {
        for (flag, country) in REGIONS {
            assert_eq!(
                region_of(&format!("[Q] {flag} Узел")),
                Some(country),
                "{flag}"
            );
        }
    }

    #[test]
    fn a_node_without_a_flag_has_no_region() {
        assert_eq!(region_of("[Q] 🇩🇪 Германия"), None);
        assert_eq!(region_of("AUTO"), None);
        assert_eq!(region_of(""), None);
    }

    #[test]
    fn a_members_answer_without_all_is_an_error() {
        let error = parse_members(r#"{"now":"[Q] 🇳🇴 Норвегия"}"#).unwrap_err();

        assert!(error.contains("all"), "{error}");
    }

    #[test]
    fn a_members_answer_without_now_is_an_error() {
        let error = parse_members(r#"{"all":["🇳🇴 Норвегия"]}"#).unwrap_err();

        assert!(error.contains("now"), "{error}");
    }

    #[test]
    fn broken_members_json_is_an_error() {
        assert!(parse_members("не json").is_err());
    }

    #[test]
    fn node_id_carries_the_name_so_the_selection_survives_a_refresh() {
        let node = ProxyNode {
            name: "[Q] 🇺🇸 США, Атланта".to_string(),
            country: "США",
        };

        let id = node_id(&node);

        assert!(id.starts_with("vpn/node/"), "{id}");
        assert!(id.ends_with("США, Атланта"), "{id}");
    }

    #[test]
    fn snapshot_of_a_healthy_controller() {
        let snapshot = snapshot(
            Ok("v1.19.31".to_string()),
            selectors(),
            [None, None],
            members(),
        );

        assert!(snapshot.reachable);
        assert_eq!(snapshot.version, "v1.19.31");
        assert_eq!(snapshot.mode(), Some(VpnMode::NoRu));
        assert_eq!(snapshot.error, "");
    }

    #[test]
    fn one_failed_selector_keeps_the_other_readable() {
        let snapshot = snapshot(
            Ok("v1.19.31".to_string()),
            [
                Err("соединение сброшено".to_string()),
                Ok(Selector {
                    name: SELECTOR_REST,
                    now: PROXY.into(),
                }),
            ],
            [Some(SELECTOR_RU), None],
            members(),
        );

        assert_eq!(snapshot.selector(SELECTOR_RU), None);
        assert_eq!(snapshot.selector(SELECTOR_REST), Some(PROXY));
        assert!(snapshot.error.contains(SELECTOR_RU), "{}", snapshot.error);
        assert!(snapshot.reachable, "контроллер-то ответил");
    }

    #[test]
    fn a_failed_server_list_keeps_the_modes_visible() {
        let snapshot = snapshot(
            Ok("v1.19.31".to_string()),
            selectors(),
            [None, None],
            Err("таймаут".to_string()),
        );

        assert_eq!(snapshot.mode(), Some(VpnMode::NoRu));
        assert_eq!(snapshot.servers_count(), 0);
        assert!(snapshot.error.contains("серверы"), "{}", snapshot.error);
    }

    #[test]
    fn an_unreachable_controller_produces_no_modes_and_no_servers() {
        let snapshot = VpnSnapshot::failed("таймаут");

        assert_eq!(snapshot.mode(), None);
        assert_eq!(snapshot.servers_count(), 0);
        assert_eq!(snapshot.selected, "");
    }

    #[test]
    fn rows_list_the_modes_the_servers_and_the_refresh() {
        let (_, servers) = members().expect("список");
        let snapshot = snapshot(
            Ok("v1.19.31".to_string()),
            selectors(),
            [None, None],
            Ok(("[Q] 🇳🇴 Норвегия".to_string(), servers)),
        );

        let rows = nodes(&snapshot, Language::Ru);

        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        assert!(titles[0].contains("RU напрямую"), "{titles:?}");
        assert_eq!(titles[1], "Режим: RU напрямую", "{titles:?}");
        assert_eq!(titles[2], "Серверы (5)", "{titles:?}");
        assert_eq!(*titles.last().expect("обновление"), "Обновить");
    }

    #[test]
    fn an_unreachable_controller_shows_one_row_and_no_modes() {
        let rows = nodes(&VpnSnapshot::failed("таймаут"), Language::Ru);

        assert_eq!(
            rows.len(),
            1,
            "{:?}",
            rows.iter().map(|r| &r.title).collect::<Vec<_>>()
        );
        assert_eq!(rows[0].title, "Mihomo не отвечает: таймаут");
    }

    #[test]
    fn a_partial_snapshot_shows_its_read_error() {
        let snapshot = snapshot(
            Ok("v1.19.31".to_string()),
            [Err("connection reset".to_string()), selectors()[1].clone()],
            [Some(SELECTOR_RU), None],
            members(),
        );

        let rows = nodes(&snapshot, Language::En);

        assert!(rows[1].title.contains("Error:"), "{}", rows[1].title);
        assert!(
            rows[1].title.contains(SELECTOR_RU),
            "ошибка селектора теряется: {}",
            rows[1].title
        );
    }

    #[test]
    fn the_modes_folder_shows_the_current_mode_and_marks_it_inside() {
        let (_, servers) = members().expect("список");
        let snapshot = snapshot(
            Ok("v1.19.31".to_string()),
            selectors(),
            [None, None],
            Ok(("[Q] 🇳🇴 Норвегия".to_string(), servers)),
        );

        let rows = nodes(&snapshot, Language::Ru);
        let modes = rows
            .iter()
            .find(|row| row.id == "vpn/modes")
            .expect("папка режимов");
        let children = modes.children().expect("режимы внутри");

        assert_eq!(modes.title, "Режим: RU напрямую");
        assert_eq!(children.len(), 3);
        assert_eq!(children[1].icon, settings_icons::CHECK);
        assert_eq!(children[0].icon, settings_icons::DOT);
        assert_eq!(
            children
                .iter()
                .map(|child| child.id.as_str())
                .collect::<Vec<_>>(),
            ["vpn/all", "vpn/noru", "vpn/blocked"]
        );
        assert!(
            children
                .iter()
                .all(|child| matches!(child.kind, super::super::tree::NodeKind::Action(_))),
            "в папке режимов только действия"
        );
    }

    #[test]
    fn an_unknown_controller_state_says_so_in_the_modes_folder_title() {
        let snapshot = snapshot(
            Ok("v1.19.31".to_string()),
            selectors(),
            [None, None],
            members(),
        );
        let mut unknown = snapshot;
        unknown.selectors.clear();

        let rows = nodes(&unknown, Language::En);
        let modes = rows
            .iter()
            .find(|row| row.id == "vpn/modes")
            .expect("папка режимов");

        assert_eq!(modes.title, "Mode: unknown");
    }

    #[test]
    fn the_servers_folder_holds_only_filtered_nodes_and_marks_the_selection() {
        let (_, servers) = members().expect("список");
        let snapshot = snapshot(
            Ok("v1.19.31".to_string()),
            selectors(),
            [None, None],
            Ok(("[Q] 🇳🇴 Норвегия".to_string(), servers)),
        );

        let rows = nodes(&snapshot, Language::En);
        let folder = rows
            .iter()
            .find(|row| row.id == "vpn/servers")
            .expect("папка серверов");
        let servers = folder.children().expect("серверы внутри");

        assert_eq!(servers.len(), 5);
        assert!(
            servers[0].title.starts_with("Норвегия"),
            "{}",
            servers[0].title
        );
        assert_eq!(
            servers[0].icon,
            settings_icons::CHECK,
            "выбранный сервер отмечен"
        );
        assert_eq!(
            servers[1].icon,
            settings_icons::DOT,
            "остальные серверы не отмечены"
        );
        assert!(
            servers
                .iter()
                .all(|child| !child.title.contains("Германия")),
            "посторонний узел попал в список"
        );
    }

    #[test]
    fn a_failed_snapshot_has_no_mode() {
        let snapshot = VpnSnapshot::failed("таймаут");

        assert!(!snapshot.reachable);
        assert_eq!(snapshot.mode(), None);
        assert_eq!(snapshot.error, "таймаут");
    }
}

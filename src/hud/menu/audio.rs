//! Динамический раздел «Звук»: провайдер поверх `wpctl` и `pactl`.
//!
//! Чтение идёт конвейером W5.0: worker -> [`CommandRunner`] -> типизированный
//! снимок -> строки меню. Запись (громкость, mute, выбор устройства) — это
//! [`Action::RefreshAndRun`]: команда уходит в worker цикла окна, после
//! успеха слот AUDIO мягко перезапрашивается, и открытый уровень
//! перерисовывается без «Загрузка…».
//!
//! Почему wpctl для громкости, а pactl для списка: `wpctl get-volume` отдаёт
//! громкость и mute устройства по умолчанию, а `pactl --format=json list`
//! — полный список sinks/sources вместе с человеческими описаниями. Имя
//! устройства здесь и есть его личность: числовой index у pactl меняется при
//! переподключении, имя остаётся.
//!
//! Деградация намеренно частичная: упавший `wpctl` гасит только блоки
//! громкости, упавший `pactl` — только списки устройств. Весь раздел
//! недоступен лишь когда нет обоих инструментов.

use super::super::data::oneshot;
use super::action::Action;
use super::settings::Language;
use super::settings_icons;
use super::system::{
    CommandError, CommandRunner, CommandSpec, ProviderError, ProviderKey, SystemCommandRunner,
    SystemProvider,
};
use super::tree::Node;

/// Шаг громкости в процентах: относительный, а не прибавление к устаревшему
/// снимку. `wpctl` считает шаг от текущего значения сам, поэтому две быстрые
/// нажатия не теряются, а снимок между ними может устареть.
pub const VOLUME_STEP: u8 = 5;

/// Личность строки громкости: раздел и команда, а не «40% вкл». По ней
/// курсор остаётся на своей строке, когда подпись сменилась.
fn volume_id(kind: &str, step: &str) -> String {
    format!("audio/volume/{kind}/{step}")
}

/// Личность строки устройства: имя уникально в пределах списка.
fn device_id(kind: &str, name: &str) -> String {
    format!("audio/device/{kind}/{name}")
}

/// Одно устройство ввода или вывода.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    /// Стабильное имя: ключ `pactl set-default-sink`.
    pub name: String,
    /// Человеческое описание из `pactl --format=json`.
    pub description: String,
    /// Состояние: RUNNING, SUSPENDED, IDLE.
    pub state: String,
}

impl AudioDevice {
    /// Мониторный источник: технический дублёр sink-а, пользователю не
    /// нужен. Определяется и по `monitor_of_sink`/`monitor_source` pactl, и
    /// по имени — на PipeWire поле бывает пустым даже у мониторов.
    pub fn is_monitor(&self) -> bool {
        self.name.ends_with(".monitor")
            || self.name.contains("_monitor")
            || self.description.starts_with("Monitor of ")
    }
}

/// Громкость и mute устройства по умолчанию.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// Громкость 0..=100.
    pub volume: u8,
    /// Звук выключен.
    pub muted: bool,
}

/// Состояние одного блока раздела. Отдельный тип вместо `Option`: раздел
/// должен различать «инструмента нет» и «инструмент ответил мусором» — это
/// разные подписи и разные причины.
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

/// Типизированный снимок раздела «Звук».
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioSnapshot {
    /// Громкость и mute устройства вывода по умолчанию.
    pub output: Block<Endpoint>,
    /// Громкость и mute устройства ввода по умолчанию.
    pub input: Block<Endpoint>,
    /// Все output-устройства, включая SUSPENDED HDMI.
    pub sinks: Block<Vec<AudioDevice>>,
    /// Все input-устройства без мониторов.
    pub sources: Block<Vec<AudioDevice>>,
    /// Имя текущего output по умолчанию.
    pub sink_default: Option<String>,
    /// Имя текущего input по умолчанию.
    pub source_default: Option<String>,
}

/// Провайдер раздела «Звук».
pub struct AudioProvider;

impl SystemProvider for AudioProvider {
    type Snapshot = AudioSnapshot;

    fn key(&self) -> ProviderKey {
        ProviderKey::AUDIO
    }

    fn fetch(&self, runner: &dyn CommandRunner) -> Result<Self::Snapshot, ProviderError> {
        let output = endpoint(runner, "@DEFAULT_AUDIO_SINK@");
        let input = endpoint(runner, "@DEFAULT_AUDIO_SOURCE@");
        let sinks = devices(runner, "sinks");
        let sources = devices(runner, "sources");
        let sink_default = default_name(runner, "get-default-sink");
        let source_default = default_name(runner, "get-default-source");

        let wpctl_alive = !matches!(output, Block::Missing) || !matches!(input, Block::Missing);
        let pactl_alive = !matches!(sinks, Block::Missing) || !matches!(sources, Block::Missing);
        if !wpctl_alive && !pactl_alive {
            return Err(ProviderError::Unavailable(
                "нет ни wpctl, ни pactl".to_string(),
            ));
        }
        Ok(AudioSnapshot {
            output,
            input,
            sinks,
            sources,
            sink_default,
            source_default,
        })
    }

    fn nodes(&self, snapshot: &Self::Snapshot, lang: Language) -> Vec<Node> {
        build_nodes(snapshot, lang)
    }
}

/// Fetch + nodes одним вызовом: так провайдера зовёт цикл окна.
pub fn fetch_nodes(lang: Language) -> Result<Vec<Node>, String> {
    let provider = AudioProvider;
    let snapshot = provider
        .fetch(&SystemCommandRunner::default())
        .map_err(|error| error.message())?;
    Ok(provider.nodes(&snapshot, lang))
}

const SINK_TARGET: &str = "@DEFAULT_AUDIO_SINK@";
const SOURCE_TARGET: &str = "@DEFAULT_AUDIO_SOURCE@";

/// Блок громкости одного устройства по умолчанию.
fn endpoint(runner: &dyn CommandRunner, target: &str) -> Block<Endpoint> {
    let spec = CommandSpec::new("wpctl").arg("get-volume").arg(target);
    match runner.run(&spec) {
        Err(error) => match error {
            CommandError::Spawn(_) => Block::Missing,
            other => Block::Failed(other.to_string()),
        },
        Ok(output) if !output.success() => Block::Failed(failure(&spec, &output.stderr)),
        Ok(output) => match oneshot::parse_wpctl_volume(&output.stdout) {
            Some((volume, muted)) => Block::Ready(Endpoint { volume, muted }),
            None => Block::Failed(format!("{}: непонятный ответ", spec.program())),
        },
    }
}

/// Блок списка устройств одного направления.
fn devices(runner: &dyn CommandRunner, kind: &str) -> Block<Vec<AudioDevice>> {
    let spec = CommandSpec::new("pactl")
        .arg("--format=json")
        .arg("list")
        .arg(kind);
    match runner.run(&spec) {
        Err(error) => match error {
            CommandError::Spawn(_) => Block::Missing,
            other => Block::Failed(other.to_string()),
        },
        Ok(output) if !output.success() => Block::Failed(failure(&spec, &output.stderr)),
        Ok(output) => match parse_devices(&output.stdout, kind) {
            Ok(parsed) => Block::Ready(parsed),
            Err(reason) => Block::Failed(format!("{}: {reason}", spec.program())),
        },
    }
}

/// Имя устройства по умолчанию: первая строка ответа `pactl get-default-*`.
fn default_name(runner: &dyn CommandRunner, subcommand: &str) -> Option<String> {
    let spec = CommandSpec::new("pactl").arg(subcommand);
    match runner.run(&spec) {
        Ok(output) if output.success() => output
            .stdout
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

/// Причина неудачи: stderr команды, а если он пуст — код возврата.
fn failure(spec: &CommandSpec, stderr: &str) -> String {
    let text = stderr.lines().find(|line| !line.trim().is_empty());
    match text {
        Some(line) => format!("{}: {}", spec.program(), line.trim()),
        None => format!("{}: команда не сработала", spec.program()),
    }
}

/// Разбор `pactl --format=json list sinks| sources`. Виден тестам: чистая
/// функция без процесса.
pub fn parse_devices(text: &str, kind: &str) -> Result<Vec<AudioDevice>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let entries = value
        .as_array()
        .ok_or_else(|| "ожидался список устройств".to_string())?;
    let mut devices = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(device) = parse_device(entry, kind) else {
            continue;
        };
        if kind == "sources" && device.is_monitor() {
            continue;
        }
        devices.push(device);
    }
    Ok(devices)
}

fn parse_device(entry: &serde_json::Value, _kind: &str) -> Option<AudioDevice> {
    let name = entry.get("name")?.as_str()?.to_string();
    let description = entry
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let state = entry
        .get("state")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    Some(AudioDevice {
        name: name.clone(),
        description: if description.is_empty() {
            prettify_device_name(&name)
        } else {
            description
        },
        state,
    })
}

/// Мониторный ли источник по полям pactl. Основной признак —
/// непустой `monitor_of_sink` (PulseAudio); на PipeWire это поле пустое, и
/// тогда срабатывает `monitor_source` — имя sink-а, чьим дублём является
/// источник.
pub fn is_monitor_source(entry: &serde_json::Value) -> bool {
    let filled = |key: &str| match entry.get(key) {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::String(value)) => !value.trim().is_empty(),
        Some(serde_json::Value::Number(value)) => value.as_u64().is_some_and(|index| index > 0),
        Some(_) => false,
    };
    filled("monitor_of_sink") || filled("monitor_source")
}

/// Запасное имя, если pactl не отдал описание: осмысленная часть стабильного
/// имени PipeWire. Основной путь разбора — `description`, это только fallback.
pub fn prettify_device_name(name: &str) -> String {
    let last = name
        .rsplit("__")
        .find(|part| {
            let part = *part;
            !part.is_empty() && !matches!(part, "sink" | "source" | "input" | "output")
        })
        .unwrap_or(name);
    last.replace('_', " ")
}

/// Относительный шаг громкости: `wpctl set-volume -l 1.0 target 5%-`.
fn volume_action(target: &str, step: &str) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("wpctl")
            .arg("set-volume")
            .arg("-l")
            .arg("1.0")
            .arg(target)
            .arg(&format!("{VOLUME_STEP}%{step}")),
        refresh: ProviderKey::AUDIO,
    }
}

/// Mute-переключатель устройства по умолчанию.
fn mute_action(target: &str) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("wpctl")
            .arg("set-mute")
            .arg(target)
            .arg("toggle"),
        refresh: ProviderKey::AUDIO,
    }
}

/// Выбор устройства по умолчанию.
fn device_action(kind: &str, name: &str) -> Action {
    let subcommand = if kind == "sinks" {
        "set-default-sink"
    } else {
        "set-default-source"
    };
    Action::RefreshAndRun {
        command: CommandSpec::new("pactl").arg(subcommand).arg(name),
        refresh: ProviderKey::AUDIO,
    }
}

/// Строки громкости и mute для одного направления.
fn endpoint_rows(
    lang: Language,
    kind: &str,
    target: &str,
    label: &str,
    icon: &'static str,
    block: &Block<Endpoint>,
) -> Vec<Node> {
    let block = match block {
        Block::Ready(endpoint) => endpoint,
        Block::Missing => {
            return vec![Node::info(icon, &format!("{label}: {}", missing(lang)))];
        }
        Block::Failed(reason) => {
            return vec![Node::info(icon, &format!("{label}: {}", short(reason)))];
        }
    };
    let state = match (block.muted, lang) {
        (true, Language::Ru) => "выкл",
        (true, Language::En) => "off",
        (false, Language::Ru) => "вкл",
        (false, Language::En) => "on",
    };
    vec![
        Node::info(icon, &format!("{label}: {}% {state}", block.volume)),
        Node::action(
            settings_icons::MINUS,
            if lang == Language::Ru {
                "Тише на 5%"
            } else {
                "Quieter by 5%"
            },
            volume_action(target, "-"),
        )
        .with_id(&volume_id(kind, "-")),
        Node::action(
            settings_icons::PLUS,
            if lang == Language::Ru {
                "Громче на 5%"
            } else {
                "Louder by 5%"
            },
            volume_action(target, "+"),
        )
        .with_id(&volume_id(kind, "+")),
        Node::action(settings_icons::SOUND, "Mute", mute_action(target))
            .with_id(&format!("audio/mute/{kind}")),
    ]
}

/// Подменю со списком устройств. Отметка текущего идёт по имени, а не по
/// описанию: описание у разных карт бывает одинаковым.
fn device_rows(
    lang: Language,
    kind: &str,
    devices: &[AudioDevice],
    current: Option<&str>,
) -> Vec<Node> {
    if devices.is_empty() {
        return vec![Node::info(
            settings_icons::DOT,
            if lang == Language::Ru {
                "Нет устройств"
            } else {
                "No devices"
            },
        )];
    }
    devices
        .iter()
        .map(|device| {
            let mut title = device.description.clone();
            if current == Some(device.name.as_str()) {
                title.push_str(if lang == Language::Ru {
                    " · текущий"
                } else {
                    " · current"
                });
            }
            if !device.state.is_empty() && !device.state.eq_ignore_ascii_case("RUNNING") {
                title.push_str(&format!(" · {}", device.state.to_lowercase()));
            }
            let action = device_action(kind, &device.name);
            Node::action(settings_icons::DOT, &title, action)
                .with_id(&device_id(kind, &device.name))
        })
        .collect()
}

/// Подпись отсутствующего инструмента.
fn missing(lang: Language) -> &'static str {
    match lang {
        Language::Ru => "инструмента нет",
        Language::En => "tool is missing",
    }
}

/// Причина слишком длинная для строки меню: обрезаем по первой строке.
fn short(reason: &str) -> String {
    let line = reason.lines().next().unwrap_or(reason).trim();
    if line.chars().count() <= 48 {
        return line.to_string();
    }
    let head: String = line.chars().take(45).collect();
    format!("{head}…")
}

/// Блок списка устройств: подменю, если есть что выбирать, и строка с
/// причиной, если нет. Пустое подменю с одним «Нет элементов» внутри хуже
/// прямой строки: лишний вход в раздел, где ничего нельзя нажать.
fn device_block(
    lang: Language,
    kind: &str,
    icon: &'static str,
    title: &str,
    block: &Block<Vec<AudioDevice>>,
    current: Option<&str>,
) -> Vec<Node> {
    let ru = lang == Language::Ru;
    let children = match block {
        Block::Ready(devices) if !devices.is_empty() => device_rows(lang, kind, devices, current),
        Block::Ready(_) => return Vec::new(),
        Block::Missing => {
            let reason = if ru {
                "pactl нет"
            } else {
                "pactl is missing"
            };
            return vec![Node::info(icon, reason).with_id(&format!("audio/devices/{kind}"))];
        }
        Block::Failed(reason) => {
            return vec![
                Node::info(icon, &format!("pactl: {}", short(reason)))
                    .with_id(&format!("audio/devices/{kind}")),
            ];
        }
    };
    vec![Node::submenu(icon, title, children).with_id(&format!("audio/devices/{kind}"))]
}

/// Строки раздела «Звук» из готового снимка.
pub fn build_nodes(snapshot: &AudioSnapshot, lang: Language) -> Vec<Node> {
    let ru = lang == Language::Ru;
    let mut rows = Vec::new();
    rows.extend(endpoint_rows(
        lang,
        "output",
        SINK_TARGET,
        if ru { "Звук" } else { "Output" },
        settings_icons::SOUND,
        &snapshot.output,
    ));
    rows.extend(endpoint_rows(
        lang,
        "input",
        SOURCE_TARGET,
        if ru { "Микрофон" } else { "Input" },
        settings_icons::DOT,
        &snapshot.input,
    ));
    rows.extend(device_block(
        lang,
        "sinks",
        settings_icons::SOUND,
        if ru {
            "Устройства вывода"
        } else {
            "Output devices"
        },
        &snapshot.sinks,
        snapshot.sink_default.as_deref(),
    ));
    rows.extend(device_block(
        lang,
        "sources",
        settings_icons::DOT,
        if ru {
            "Устройства ввода"
        } else {
            "Input devices"
        },
        &snapshot.sources,
        snapshot.source_default.as_deref(),
    ));
    rows.push(
        Node::action(
            settings_icons::UP,
            if ru { "Обновить" } else { "Refresh" },
            Action::RefreshDynamic(ProviderKey::AUDIO),
        )
        .with_id("audio/refresh"),
    );
    rows
}

#[cfg(test)]
mod tests {
    use super::super::action::{Action, RecordingRunner};
    use super::super::system::{CommandOutput, CommandSpec, ScriptedRunner, SlotState};
    use super::*;

    const SINKS_JSON: &str = r#"[{"index":88,"state":"SUSPENDED","name":"easyeffects_sink","description":"Easy Effects Sink"},
{"index":189045,"state":"SUSPENDED","name":"alsa_output.pci-0000_00_1f.3.HiFi__HDMI3__sink","description":"HD Audio HDMI / DisplayPort 3 Output"},
{"index":189048,"state":"SUSPENDED","name":"alsa_output.pci-0000_00_1f.3.HiFi__Speaker__sink","description":"HD Audio Speaker"}]"#;

    const SOURCES_JSON: &str = r#"[{"index":88,"state":"SUSPENDED","name":"easyeffects_sink.monitor","description":"Monitor of Easy Effects Sink","monitor_of_sink":88},
{"index":89,"state":"RUNNING","name":"easyeffects_source","description":"Easy Effects Source","monitor_of_sink":null},
{"index":189048,"state":"SUSPENDED","name":"alsa_output.pci-0000_00_1f.3.HiFi__Speaker__sink.monitor","description":"Monitor of HD Audio Speaker","monitor_source":"alsa_output.pci-0000_00_1f.3.HiFi__Speaker__sink"},
{"index":189050,"state":"RUNNING","name":"alsa_input.pci-0000_00_1f.3.HiFi__Mic1__source","description":"HD Audio Digital Microphone","monitor_source":""}]"#;

    fn ok(stdout: &str) -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            status: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    fn missing_tool() -> Result<CommandOutput, CommandError> {
        Err(CommandError::Spawn("нет такой программы".to_string()))
    }

    fn command_error() -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            status: Some(1),
            stdout: String::new(),
            stderr: "Server returned error\n".to_string(),
        })
    }

    /// Полный сценарий в порядке вызовов провайдера.
    fn full_script() -> Vec<Result<CommandOutput, CommandError>> {
        vec![
            ok("Volume: 0.40"),
            ok("Volume: 1.00"),
            ok(SINKS_JSON),
            ok(SOURCES_JSON),
            ok("alsa_output.pci-0000_00_1f.3.HiFi__Speaker__sink\n"),
            ok("alsa_input.pci-0000_00_1f.3.HiFi__Mic1__source\n"),
        ]
    }

    #[test]
    fn parses_sinks_json_with_descriptions_and_states() {
        let sinks = parse_devices(SINKS_JSON, "sinks").expect("список sinks");
        assert_eq!(sinks.len(), 3);
        assert_eq!(sinks[0].name, "easyeffects_sink");
        assert_eq!(sinks[0].description, "Easy Effects Sink");
        assert_eq!(sinks[1].description, "HD Audio HDMI / DisplayPort 3 Output");
        assert_eq!(sinks[2].state, "SUSPENDED");
    }

    #[test]
    fn drops_monitor_sources_by_both_pactl_markers() {
        let sources = parse_devices(SOURCES_JSON, "sources").expect("список sources");
        let names: Vec<&str> = sources.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names.len(), 2, "мониторы должны отсеяться: {names:?}");
        assert!(names.contains(&"alsa_input.pci-0000_00_1f.3.HiFi__Mic1__source"));
        assert!(names.contains(&"easyeffects_source"));
    }

    #[test]
    fn monitor_of_sink_field_is_recognized() {
        let entry = serde_json::json!({"name": "x.monitor", "monitor_of_sink": 88});
        assert!(is_monitor_source(&entry));
        let entry = serde_json::json!({"name": "mic", "monitor_of_sink": null});
        assert!(!is_monitor_source(&entry));
        let entry = serde_json::json!({"name": "mon", "monitor_source": "sink0"});
        assert!(is_monitor_source(&entry));
    }

    #[test]
    fn broken_json_is_an_error_not_an_empty_list() {
        let error = parse_devices("{не json", "sinks").expect_err("мусор");
        assert!(!error.is_empty());
    }

    #[test]
    fn empty_description_falls_back_to_prettified_name() {
        let json = r#"[{"index":7,"state":"IDLE","name":"alsa_output.pci-0000_00_1f.3.HiFi__Speaker__sink","description":""}]"#;
        let sinks = parse_devices(json, "sinks").expect("список");
        assert_eq!(sinks[0].description, "Speaker");
    }

    #[test]
    fn fetch_reads_everything_and_marks_current_device() {
        let runner = ScriptedRunner::new(full_script());
        let provider = AudioProvider;
        let snapshot = provider.fetch(&runner).expect("снимок");
        assert_eq!(
            snapshot.output,
            Block::Ready(Endpoint {
                volume: 40,
                muted: false
            })
        );
        assert_eq!(
            snapshot.input,
            Block::Ready(Endpoint {
                volume: 100,
                muted: false
            })
        );
        assert_eq!(snapshot.sinks.ready().map(Vec::len), Some(3));
        assert_eq!(snapshot.sources.ready().map(Vec::len), Some(2));
        assert_eq!(
            snapshot.sink_default.as_deref(),
            Some("alsa_output.pci-0000_00_1f.3.HiFi__Speaker__sink")
        );
        let calls = runner.calls();
        assert!(
            calls
                .iter()
                .any(|call| call == "wpctl get-volume @DEFAULT_AUDIO_SINK@")
        );
        assert!(
            calls
                .iter()
                .any(|call| call == "pactl --format=json list sinks")
        );
    }

    #[test]
    fn nodes_show_volume_controls_and_device_lists() {
        let snapshot = AudioProvider
            .fetch(&ScriptedRunner::new(full_script()))
            .expect("снимок");
        let nodes = build_nodes(&snapshot, Language::Ru);
        let titles: Vec<&str> = nodes.iter().map(|node| node.title.as_str()).collect();
        assert!(
            titles.iter().any(|title| title.contains("Звук: 40% вкл")),
            "{titles:?}"
        );
        assert!(titles.contains(&"Тише на 5%"));
        assert!(titles.contains(&"Громче на 5%"));
        assert!(titles.contains(&"Устройства вывода"));
        assert!(titles.contains(&"Устройства ввода"));

        let outputs = nodes
            .iter()
            .find(|node| node.title == "Устройства вывода")
            .and_then(|node| node.children())
            .expect("подменю вывода");
        let current = outputs
            .iter()
            .find(|node| node.title.contains("текущий"))
            .expect("текущее устройство отмечено");
        assert!(
            current.title.starts_with("HD Audio Speaker"),
            "{}",
            current.title
        );
    }

    /// Команды всех строк раздела и их личности: проверяется разом, что
    /// argv собран верно, а строки узнаваемы между снимками.
    #[test]
    fn every_action_carries_exact_argv_audio_refresh_and_stable_id() {
        let runner = ScriptedRunner::new(full_script());
        let snapshot = AudioProvider.fetch(&runner).expect("снимок");
        let nodes = build_nodes(&snapshot, Language::Ru);
        let expected = [
            (
                "audio/volume/output/-",
                "run wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%-",
            ),
            (
                "audio/volume/output/+",
                "run wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+",
            ),
            (
                "audio/mute/output",
                "run wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle",
            ),
            (
                "audio/volume/input/-",
                "run wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SOURCE@ 5%-",
            ),
            (
                "audio/volume/input/+",
                "run wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SOURCE@ 5%+",
            ),
            (
                "audio/mute/input",
                "run wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle",
            ),
        ];
        for (id, argv) in expected {
            let node = nodes
                .iter()
                .find(|node| node.identity() == id)
                .unwrap_or_else(|| panic!("нет строки {id}"));
            let action = match &node.kind {
                super::super::tree::NodeKind::Action(action) => action,
                other => panic!("{id} не действие: {other:?}"),
            };
            match action {
                Action::RefreshAndRun { refresh, .. } => assert_eq!(*refresh, ProviderKey::AUDIO),
                other => panic!("{id}: ожидался RefreshAndRun, {other:?}"),
            }
            let recorder = RecordingRunner::new();
            action.perform_with(&recorder).expect("выполнено");
            assert_eq!(recorder.calls(), vec![argv.to_string()], "строка {id}");
        }

        let devices = nodes
            .iter()
            .find(|node| node.identity() == "audio/devices/sinks")
            .and_then(|node| node.children())
            .expect("подменю вывода");
        for (id, argv) in [
            (
                "audio/device/sinks/easyeffects_sink",
                "run pactl set-default-sink easyeffects_sink",
            ),
            (
                "audio/device/sinks/alsa_output.pci-0000_00_1f.3.HiFi__Speaker__sink",
                "run pactl set-default-sink alsa_output.pci-0000_00_1f.3.HiFi__Speaker__sink",
            ),
        ] {
            let node = devices
                .iter()
                .find(|node| node.identity() == id)
                .unwrap_or_else(|| panic!("нет строки {id}"));
            let action = match &node.kind {
                super::super::tree::NodeKind::Action(action) => action,
                other => panic!("{id} не действие: {other:?}"),
            };
            let recorder = RecordingRunner::new();
            action.perform_with(&recorder).expect("выполнено");
            assert_eq!(recorder.calls(), vec![argv.to_string()], "строка {id}");
            match action {
                Action::RefreshAndRun { refresh, .. } => assert_eq!(*refresh, ProviderKey::AUDIO),
                other => panic!("{id}: ожидался RefreshAndRun, {other:?}"),
            }
        }

        let inputs = nodes
            .iter()
            .find(|node| node.identity() == "audio/devices/sources")
            .and_then(|node| node.children())
            .expect("подменю ввода");
        let node = inputs
            .iter()
            .find(|node| {
                node.identity()
                    == "audio/device/sources/alsa_input.pci-0000_00_1f.3.HiFi__Mic1__source"
            })
            .expect("микрофон есть");
        let action = match &node.kind {
            super::super::tree::NodeKind::Action(action) => action,
            other => panic!("не действие: {other:?}"),
        };
        let recorder = RecordingRunner::new();
        action.perform_with(&recorder).expect("выполнено");
        assert_eq!(
            recorder.calls(),
            vec![
                "run pactl set-default-source alsa_input.pci-0000_00_1f.3.HiFi__Mic1__source"
                    .to_string()
            ]
        );
    }

    #[test]
    fn device_rows_carry_stable_ids_across_snapshots() {
        let first = AudioProvider
            .fetch(&ScriptedRunner::new(full_script()))
            .expect("снимок");
        let second = AudioProvider
            .fetch(&ScriptedRunner::new(full_script()))
            .expect("снимок");
        let ids = |snapshot: &AudioSnapshot| {
            build_nodes(snapshot, Language::Ru)
                .into_iter()
                .find(|node| node.title == "Устройства вывода")
                .and_then(|node| node.children().map(<[Node]>::to_vec))
                .unwrap_or_default()
                .into_iter()
                .map(|node| node.identity().to_string())
                .collect::<Vec<String>>()
        };
        assert_eq!(ids(&first), ids(&second));
        assert!(
            ids(&first)
                .iter()
                .any(|id| id == "audio/device/sinks/easyeffects_sink")
        );
    }

    #[test]
    fn no_wpctl_degrades_only_volume_blocks() {
        let mut script = full_script();
        script[0] = missing_tool();
        script[1] = missing_tool();
        let runner = ScriptedRunner::new(script);
        let snapshot = AudioProvider.fetch(&runner).expect("снимок");
        assert_eq!(snapshot.output, Block::Missing);
        assert_eq!(snapshot.input, Block::Missing);
        assert_eq!(snapshot.sinks.ready().map(Vec::len), Some(3));
        let titles: Vec<String> = build_nodes(&snapshot, Language::Ru)
            .into_iter()
            .map(|node| node.title)
            .collect();
        assert!(
            titles.iter().any(|title| title.contains("инструмента нет")),
            "{titles:?}"
        );
        assert!(titles.iter().any(|title| title == "Устройства вывода"));
    }

    #[test]
    fn no_pactl_degrades_only_device_lists() {
        let mut script = full_script();
        for answer in script.iter_mut().skip(2) {
            *answer = missing_tool();
        }
        let runner = ScriptedRunner::new(script);
        let snapshot = AudioProvider.fetch(&runner).expect("снимок");
        assert_eq!(snapshot.sinks, Block::Missing);
        assert_eq!(snapshot.sources, Block::Missing);
        assert_eq!(
            snapshot.output,
            Block::Ready(Endpoint {
                volume: 40,
                muted: false
            })
        );
        let titles: Vec<String> = build_nodes(&snapshot, Language::Ru)
            .into_iter()
            .map(|node| node.title)
            .collect();
        assert!(
            titles.iter().any(|title| title.contains("Звук: 40%")),
            "{titles:?}"
        );
        assert!(titles.iter().any(|title| title == "pactl нет"));
    }

    #[test]
    fn one_failed_subcommand_keeps_the_other_blocks() {
        let mut script = full_script();
        script[2] = command_error();
        let runner = ScriptedRunner::new(script);
        let snapshot = AudioProvider.fetch(&runner).expect("снимок");
        assert_eq!(
            snapshot.sinks,
            Block::Failed("pactl: Server returned error".to_string())
        );
        assert_eq!(snapshot.sources.ready().map(Vec::len), Some(2));
        assert_eq!(
            snapshot.output,
            Block::Ready(Endpoint {
                volume: 40,
                muted: false
            })
        );
    }

    #[test]
    fn no_tools_at_all_is_unavailable() {
        let runner = ScriptedRunner::new(vec![missing_tool(); 6]);
        let error = AudioProvider.fetch(&runner).expect_err("раздел недоступен");
        assert!(matches!(error, ProviderError::Unavailable(_)), "{error:?}");
        assert_eq!(error.message(), "нет ни wpctl, ни pactl");
    }

    #[test]
    fn muted_default_is_marked_in_rows() {
        let mut script = full_script();
        script[0] = ok("Volume: 0.40 [MUTED]");
        let runner = ScriptedRunner::new(script);
        let snapshot = AudioProvider.fetch(&runner).expect("снимок");
        let titles: Vec<String> = build_nodes(&snapshot, Language::Ru)
            .into_iter()
            .map(|node| node.title)
            .collect();
        assert!(
            titles.iter().any(|title| title == "Звук: 40% выкл"),
            "{titles:?}"
        );
    }

    #[test]
    fn commands_never_use_a_shell() {
        let spec = CommandSpec::new("wpctl")
            .arg("set-volume")
            .arg("-l")
            .arg("1.0")
            .arg("@DEFAULT_AUDIO_SINK@")
            .arg("5%+");
        let (program, args) = spec.argv();
        assert_eq!(program, "wpctl");
        assert!(
            !args
                .iter()
                .any(|arg| arg.contains("-c") || arg.contains("sh"))
        );
        assert_eq!(
            spec.log_line(),
            "wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+"
        );
    }

    /// Живой опрос хоста: только команды чтения, поэтому запускается вручную
    /// (`cargo test --bin hud-menu-rs -- --ignored live_provider`). Обычный
    /// `cargo test` систему не трогает.
    #[test]
    #[ignore = "обращается к wpctl/pactl хоста"]
    fn live_provider_reads_the_host() {
        let snapshot = AudioProvider
            .fetch(&SystemCommandRunner::default())
            .expect("на хосте есть wpctl или pactl");
        println!("вывод: {:?}", snapshot.output);
        println!("ввод: {:?}", snapshot.input);
        println!("sinks: {:?}", snapshot.sinks);
        println!("sources: {:?}", snapshot.sources);
        assert!(
            matches!(snapshot.sinks, Block::Ready(_)),
            "хотя бы один sink"
        );
        assert!(!snapshot.sources.is_empty(), "хотя бы один источник");
        assert!(
            snapshot
                .sources
                .ready()
                .is_some_and(|devices| devices.iter().all(|d| !d.is_monitor())),
            "мониторы отсеяны"
        );
    }

    /// Пока идёт refresh после действия, уровень продолжает показывать
    /// прежние строки: «Загрузка…» мигала бы вместо списка, который
    /// пользователь только что читал.
    #[test]
    fn slot_keeps_previous_rows_while_soft_refresh_runs() {
        use super::super::system::ProviderSlot;
        let mut slot: ProviderSlot<Vec<Node>> = ProviderSlot::new(ProviderKey::AUDIO);
        let generation = slot.request().expect("первый запрос");
        let rows = build_nodes(
            &AudioProvider
                .fetch(&ScriptedRunner::new(full_script()))
                .expect("снимок"),
            Language::Ru,
        );
        let ready = rows.len();
        assert!(slot.apply(generation, Ok(rows)), "снимок принят");

        let soft = slot.refresh_soft().expect("мягкое обновление");
        assert!(slot.is_refreshing());
        assert!(
            matches!(slot.state(), SlotState::Ready(nodes) if nodes.len() == ready),
            "старый снимок обязан остаться на экране"
        );
        assert!(
            !slot.apply(generation, Ok(Vec::new())),
            "устаревший ответ отброшен"
        );

        let fresh = build_nodes(
            &AudioProvider
                .fetch(&ScriptedRunner::new(full_script()))
                .expect("снимок"),
            Language::Ru,
        );
        assert!(slot.apply(soft, Ok(fresh)), "свежий ответ принят");
        assert!(!slot.is_refreshing());
        assert!(matches!(slot.state(), SlotState::Ready(nodes) if nodes.len() == ready));
    }
}

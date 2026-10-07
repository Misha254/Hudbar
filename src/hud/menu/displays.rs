//! Динамический раздел «Дисплеи»: провайдер поверх `niri msg --json outputs`.
//!
//! W5.4a — строго read-only: раздел показывает выводы, их режимы, масштаб,
//! поворот, позицию и VRR. Ни одной команды записи (`niri msg output …`)
//! здесь нет: смена режима — это W5.4b.
//!
//! Один вызов на обновление. niri отдаёт все выводы разом объектом, ключ
//! которого — имя вывода (`eDP-1`, `HDMI-A-1`), поэтому `info` на каждый
//! вывод по отдельности здесь просто не нужен: это был бы N+1 к одному
//! вызову.
//!
//! Числа приходят в машинном виде: `refresh_rate` — в миллигерцах (144000 =
//! 144.000 Гц), `scale` — дробный множитель логического размера. Человеческие
//! подписи собираются из них в том же виде, что и сам niri: `1920x1080 @ 144.000`.
//!
//! Чего в выводе нет, того в разделе нет: `mirroring`, `dpms` и
//! `variable_refresh_ranges` niri в `--json outputs` не отдаёт. Отсутствие
//! поля — это `None`, а не выдуманное значение.

use super::action::Action;
use super::bluetooth::Block;
use super::settings::Language;
use super::settings_icons;
use super::system::{
    CommandError, CommandRunner, CommandSpec, ProviderError, ProviderKey, SystemCommandRunner,
    SystemProvider,
};
use super::tree::Node;
use std::time::Duration;

/// Время, которое ждём ответа композитора на смену вывода. Это локальный
/// IPC-вызов, но он меняет картинку на экране, поэтому секунды односекундного
/// `oneshot::TIMEOUT` на всякий случай не берём.
const OUTPUT_TIMEOUT: Duration = Duration::from_secs(3);

/// Масштабы, которые предлагает меню. Список закрытый и разумный: niri
/// принимает любое положительное число, но перебор в меню превращается в
/// список из двадцати пунктов с дробными хвостами.
pub const SCALE_CHOICES: [f64; 7] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

/// Личность строки вывода: имя вывода, а не позиция в списке и не режим.
pub fn display_row_id(name: &str) -> String {
    format!("displays/output/{name}")
}

/// Поворот вывода. Значения — ровно те, что перечисляет сам niri в своём
/// сообщении об ошибке: иначе он бы просто отверг команду.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    /// Обычный.
    Normal,
    /// 90° по часовой.
    Rotate90,
    /// 180°.
    Rotate180,
    /// 270°.
    Rotate270,
    /// Отразить по горизонтали.
    Flipped,
    /// Отразить и повернуть на 90°.
    Flipped90,
    /// Отразить и повернуть на 180°.
    Flipped180,
    /// Отразить и повернуть на 270°.
    Flipped270,
}

impl Transform {
    /// Все повороты в порядке показа меню.
    pub const ALL: [Transform; 8] = [
        Transform::Normal,
        Transform::Rotate90,
        Transform::Rotate180,
        Transform::Rotate270,
        Transform::Flipped,
        Transform::Flipped90,
        Transform::Flipped180,
        Transform::Flipped270,
    ];

    /// Слово, которое понимает niri.
    pub fn as_str(self) -> &'static str {
        match self {
            Transform::Normal => "normal",
            Transform::Rotate90 => "90",
            Transform::Rotate180 => "180",
            Transform::Rotate270 => "270",
            Transform::Flipped => "flipped",
            Transform::Flipped90 => "flipped-90",
            Transform::Flipped180 => "flipped-180",
            Transform::Flipped270 => "flipped-270",
        }
    }

    /// Разбор строки, которую отдаёт niri в `logical.transform`.
    pub fn parse(text: &str) -> Option<Transform> {
        Transform::ALL
            .into_iter()
            .find(|item| item.as_str().eq_ignore_ascii_case(text.trim()))
    }

    /// Подпись в меню: человеческая, а не `flipped-180`.
    pub fn title(self, lang: Language) -> &'static str {
        match (self, lang) {
            (Transform::Normal, _) => {
                if lang == Language::Ru {
                    "Без поворота"
                } else {
                    "No rotation"
                }
            }
            // Градусы переводить нечего: знак один на оба языка.
            (Transform::Rotate90, _) => "90°",
            // Градусы переводить нечего: знак один на оба языка.
            (Transform::Rotate180, _) => "180°",
            // Градусы переводить нечего: знак один на оба языка.
            (Transform::Rotate270, _) => "270°",
            (Transform::Flipped, _) => {
                if lang == Language::Ru {
                    "Отразить"
                } else {
                    "Flipped"
                }
            }
            (Transform::Flipped90, _) => {
                if lang == Language::Ru {
                    "Отразить + 90°"
                } else {
                    "Flipped + 90°"
                }
            }
            (Transform::Flipped180, _) => {
                if lang == Language::Ru {
                    "Отразить + 180°"
                } else {
                    "Flipped + 180°"
                }
            }
            (Transform::Flipped270, _) => {
                if lang == Language::Ru {
                    "Отразить + 270°"
                } else {
                    "Flipped + 270°"
                }
            }
        }
    }
}

/// Направление сдвига вывода по экрану.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nudge {
    /// Левее.
    Left,
    /// Правее.
    Right,
    /// Выше.
    Up,
    /// Ниже.
    Down,
}

/// Безопасное имя вывода для `argv`. Имена приходят от niri и в реальности
/// всегда вида `eDP-1`, но имя с ведущим дефисом clap разобрал бы как опцию,
/// а `--` перед именем niri не принимает. Такое имя в меню просто не
/// показывается.
pub fn name_is_safe(name: &str) -> bool {
    !name.trim().is_empty() && !name.trim_start().starts_with('-')
}

/// Частота режима словами niri: `144` вместо `144.000`, но `60.002` — как есть.
pub fn format_refresh(mhz: u32) -> String {
    let text = format!("{:.3}", mhz as f64 / 1000.0);
    let trimmed = text.trim_end_matches('0');
    trimmed.trim_end_matches('.').to_string()
}

/// Масштаб словами niri: `1`, `1.25`, `2` — без хвостовых нулей.
pub fn format_scale(scale: f64) -> String {
    let text = format!("{scale:.3}");
    let trimmed = text.trim_end_matches('0');
    trimmed.trim_end_matches('.').to_string()
}

/// Подпись режима словами niri для команды: `1920x1080@144`.
pub fn mode_argument(mode: &DisplayMode) -> String {
    format!(
        "{}x{}@{}",
        mode.width,
        mode.height,
        format_refresh(mode.refresh_mhz)
    )
}

/// Собрать команду вывода: `niri msg output <OUTPUT> <…>`.
///
/// Имя вывода проверяется: без этого `-`-строка стала бы набором флагов, а
/// `niri msg output -- …` clap не разбирает вовсе.
fn output_spec(name: &str, args: &[&str]) -> CommandSpec {
    let mut spec = CommandSpec::new("niri").arg("msg").arg("output");
    if name_is_safe(name) {
        spec = spec.arg(name);
    }
    for arg in args {
        spec = spec.arg(arg);
    }
    spec.with_timeout(OUTPUT_TIMEOUT)
}

/// Включить или выключить вывод: `niri msg output <OUTPUT> on|off`.
pub fn power_action(name: &str, on: bool) -> Action {
    Action::RefreshAndRun {
        command: output_spec(name, &[if on { "on" } else { "off" }]),
        refresh: ProviderKey::OUTPUTS,
    }
}

/// Сменить режим с частотой: `niri msg output <OUTPUT> mode <W>x<H>@<Hz>`.
pub fn mode_action(name: &str, mode: &DisplayMode) -> Action {
    let argument = mode_argument(mode);
    Action::RefreshAndRun {
        command: output_spec(name, &["mode", &argument]),
        refresh: ProviderKey::OUTPUTS,
    }
}

/// Сменить масштаб: `niri msg output <OUTPUT> scale <SCALE>`.
pub fn scale_action(name: &str, scale: f64) -> Action {
    let argument = format_scale(scale);
    Action::RefreshAndRun {
        command: output_spec(name, &["scale", &argument]),
        refresh: ProviderKey::OUTPUTS,
    }
}

/// Сменить поворот: `niri msg output <OUTPUT> transform <TRANSFORM>`.
pub fn transform_action(name: &str, transform: Transform) -> Action {
    Action::RefreshAndRun {
        command: output_spec(name, &["transform", transform.as_str()]),
        refresh: ProviderKey::OUTPUTS,
    }
}

/// Включить или выключить VRR: `niri msg output <OUTPUT> vrr on|off`.
pub fn vrr_action(name: &str, on: bool) -> Action {
    Action::RefreshAndRun {
        command: output_spec(name, &["vrr", if on { "on" } else { "off" }]),
        refresh: ProviderKey::OUTPUTS,
    }
}

/// Вернуть автоматическую раскладку: `niri msg output <OUTPUT> position auto`.
pub fn position_auto_action(name: &str) -> Action {
    Action::RefreshAndRun {
        command: output_spec(name, &["position", "auto"]),
        refresh: ProviderKey::OUTPUTS,
    }
}

/// Поставить вывод в точку: `niri msg output <OUTPUT> position set <X> <Y>`.
pub fn position_action(name: &str, x: i64, y: i64) -> Action {
    let x = x.to_string();
    let y = y.to_string();
    Action::RefreshAndRun {
        command: output_spec(name, &["position", "set", &x, &y]),
        refresh: ProviderKey::OUTPUTS,
    }
}

/// Один режим вывода из списка `modes`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayMode {
    /// Ширина в пикселях.
    pub width: u32,
    /// Высота в пикселях.
    pub height: u32,
    /// Частота в миллигерцах: 144000 — это 144.000 Гц.
    pub refresh_mhz: u32,
    /// Режим, который niri считает предпочтительным для этого вывода.
    pub is_preferred: bool,
}

impl DisplayMode {
    /// Подпись режима в том же виде, что у niri: `1920x1080 @ 144.000`.
    pub fn label(&self) -> String {
        let hz = self.refresh_mhz as f64 / 1000.0;
        format!("{}x{} @ {hz:.3}", self.width, self.height)
    }
}

/// Логическое состояние вывода: во что он нарисован на экране.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayLogical {
    /// Позиция по X в логических пикселях.
    pub x: i64,
    /// Позиция по Y в логических пикселях.
    pub y: i64,
    /// Логическая ширина.
    pub width: u32,
    /// Логическая высота.
    pub height: u32,
    /// Масштаб: 1.25 — четверть пикселя на логический.
    pub scale: f64,
    /// Поворот как его отдаёт niri: `Normal`, `90`, `Flipped`, …
    pub transform: String,
}

/// Один вывод как его видит niri.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Display {
    /// Имя вывода: ключ объекта в ответе и поле `name`.
    pub name: String,
    /// Производитель, как его знает niri.
    pub make: String,
    /// Модель.
    pub model: String,
    /// Серийный номер, если niri его знает.
    pub serial: Option<String>,
    /// Физический размер в миллиметрах: `[ширина, высота]`.
    pub physical_size_mm: Option<(u32, u32)>,
    /// Все режимы, которые niri отдаёт для вывода.
    pub modes: Vec<DisplayMode>,
    /// Индекс текущего режима в `modes`.
    pub current_mode: Option<usize>,
    /// Текущий режим задан вручную и не совпадает со списком.
    pub is_custom_mode: bool,
    /// Вывод умеет VRR.
    pub vrr_supported: bool,
    /// VRR включён.
    pub vrr_enabled: bool,
    /// Логическое состояние; `None`, если вывод выключен и niri его не описал.
    pub logical: Option<DisplayLogical>,
}

impl Display {
    /// Текущий режим из списка: при `is_custom_mode` или неверном индексе —
    /// `None`, потому что режима из списка тогда просто нет.
    pub fn current(&self) -> Option<&DisplayMode> {
        if self.is_custom_mode {
            return None;
        }
        self.current_mode.and_then(|index| self.modes.get(index))
    }

    /// Личность строки: имя вывода. Переключение режима и перестановка
    /// списка её не меняют.
    pub fn row_id(&self) -> String {
        display_row_id(&self.name)
    }

    /// Включён ли вывод: включённый niri описывает логикой, выключенный — нет.
    pub fn is_on(&self) -> bool {
        self.logical.is_some()
    }

    /// Новая точка вывода при сдвиге на размер его же логической области.
    ///
    /// Шаг — собственная логическая ширина или высота: рядом с другим
    /// выводом этого достаточно, чтобы убрать зазор, а дробные координаты
    /// niri всё равно округляет. Без логики (вывод выключен) сдвигать нечего,
    /// поэтому возвращается `None`.
    pub fn nudged(&self, direction: Nudge) -> Option<(i64, i64)> {
        let logical = self.logical.as_ref()?;
        let width = logical.width.max(1) as i64;
        let height = logical.height.max(1) as i64;
        let (dx, dy) = match direction {
            Nudge::Left => (-width, 0),
            Nudge::Right => (width, 0),
            Nudge::Up => (0, -height),
            Nudge::Down => (0, height),
        };
        Some((logical.x + dx, logical.y + dy))
    }
}

/// Типизированный снимок раздела «Дисплеи».
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplaysSnapshot {
    /// Все выводы: `Missing` — нет `niri`, `Failed` — команда ответила мусором.
    pub outputs: Block<Vec<Display>>,
}

/// Провайдер раздела «Дисплеи».
pub struct DisplaysProvider;

impl SystemProvider for DisplaysProvider {
    type Snapshot = DisplaysSnapshot;

    fn key(&self) -> ProviderKey {
        ProviderKey::OUTPUTS
    }

    fn fetch(&self, runner: &dyn CommandRunner) -> Result<Self::Snapshot, ProviderError> {
        // Один вызов на весь раздел и заодно проверка инструмента: без
        // `niri` раздел скрывается, а не рисует пустоту.
        let outputs = outputs(runner);
        if matches!(outputs, Block::Missing) {
            return Err(ProviderError::Unavailable("нет niri".to_string()));
        }
        Ok(DisplaysSnapshot { outputs })
    }

    fn nodes(&self, snapshot: &Self::Snapshot, lang: Language) -> Vec<Node> {
        build_nodes(snapshot, lang)
    }
}

/// Fetch + nodes одним вызовом: так провайдера зовёт цикл окна.
pub fn fetch_nodes(lang: Language) -> Result<Vec<Node>, String> {
    let provider = DisplaysProvider;
    let snapshot = provider
        .fetch(&SystemCommandRunner::default())
        .map_err(|error| error.message())?;
    Ok(provider.nodes(&snapshot, lang))
}

/// Блок выводов одним вызовом `niri msg --json outputs`.
fn outputs(runner: &dyn CommandRunner) -> Block<Vec<Display>> {
    let spec = CommandSpec::new("niri")
        .arg("msg")
        .arg("--json")
        .arg("outputs");
    match runner.run(&spec) {
        Err(CommandError::Spawn(_)) => Block::Missing,
        Err(error) => Block::Failed(error.to_string()),
        Ok(output) if output.success() => match parse_outputs(&output.stdout) {
            Ok(list) => Block::Ready(list),
            Err(reason) => Block::Failed(reason),
        },
        Ok(output) => Block::Failed(failure(&output.stderr)),
    }
}

/// Причина сбоя: первая строка stderr, а если её нет — код возврата. Сырой
/// вывод в меню не идёт.
fn failure(stderr: &str) -> String {
    let line = stderr.lines().map(str::trim).find(|line| !line.is_empty());
    match line {
        Some(text) => text.to_string(),
        None => "niri msg: команда не сработала".to_string(),
    }
}

/// Разбор `niri msg --json outputs`. Корень — объект, ключ которого и есть
/// имя вывода; само имя дублируется в поле `name`, но ключ надёжнее: он есть
/// всегда, даже если `name` не отдан.
pub fn parse_outputs(text: &str) -> Result<Vec<Display>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let object = value
        .as_object()
        .ok_or_else(|| "ожидался объект выводов".to_string())?;
    Ok(object
        .iter()
        .map(|(key, entry)| parse_display(key, entry))
        .collect())
}

/// Разбор одного вывода. Отсутствующие поля не выдумываются: `Option` там,
/// где niri может промолчать, и `Vec`, где список может быть пустым.
pub fn parse_display(key: &str, entry: &serde_json::Value) -> Display {
    let text = |path: &str| {
        entry
            .get(path)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    Display {
        name: match entry.get("name").and_then(serde_json::Value::as_str) {
            Some(name) if !name.trim().is_empty() => name.trim().to_string(),
            _ => key.to_string(),
        },
        make: text("make"),
        model: text("model"),
        serial: entry
            .get("serial")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|serial| !serial.is_empty())
            .map(str::to_string),
        physical_size_mm: entry.get("physical_size").and_then(|size| {
            let pair = size.as_array()?;
            let width = pair.first()?.as_u64()? as u32;
            let height = pair.get(1)?.as_u64()? as u32;
            Some((width, height))
        }),
        modes: entry
            .get("modes")
            .and_then(serde_json::Value::as_array)
            .map(|modes| modes.iter().map(parse_mode).collect())
            .unwrap_or_default(),
        current_mode: entry
            .get("current_mode")
            .and_then(serde_json::Value::as_u64)
            .map(|index| index as usize),
        is_custom_mode: entry
            .get("is_custom_mode")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        vrr_supported: entry
            .get("vrr_supported")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        vrr_enabled: entry
            .get("vrr_enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        logical: entry.get("logical").and_then(parse_logical),
    }
}

/// Разбор режима из списка `modes`.
pub fn parse_mode(mode: &serde_json::Value) -> DisplayMode {
    let number = |path: &str| {
        mode.get(path)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default() as u32
    };
    DisplayMode {
        width: number("width"),
        height: number("height"),
        refresh_mhz: number("refresh_rate"),
        is_preferred: mode
            .get("is_preferred")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    }
}

/// Разбор логического состояния: у выключенного вывода объекта нет вовсе.
fn parse_logical(value: &serde_json::Value) -> Option<DisplayLogical> {
    let object = value.as_object()?;
    let number = |path: &str| object.get(path).and_then(serde_json::Value::as_i64);
    Some(DisplayLogical {
        x: number("x")?,
        y: number("y")?,
        width: number("width")? as u32,
        height: number("height")? as u32,
        scale: object
            .get("scale")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0),
        transform: object
            .get("transform")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Normal")
            .to_string(),
    })
}

fn ru(lang: Language) -> bool {
    lang == Language::Ru
}

/// Статусная строка раздела: сколько выводов и с чем разбирались.
fn status_row(lang: Language, snapshot: &DisplaysSnapshot) -> Node {
    let id = "displays/status";
    let text = match &snapshot.outputs {
        Block::Missing => {
            return Node::info(
                settings_icons::DOT,
                if ru(lang) {
                    "niri нет"
                } else {
                    "niri is missing"
                },
            )
            .with_id(id);
        }
        Block::Failed(reason) => {
            return Node::info(
                settings_icons::DOT,
                &if ru(lang) {
                    format!("Дисплеи: {reason}")
                } else {
                    format!("Displays: {reason}")
                },
            )
            .with_id(id);
        }
        Block::Ready(list) if list.is_empty() => {
            return Node::info(
                settings_icons::DOT,
                if ru(lang) {
                    "Нет дисплеев"
                } else {
                    "No displays"
                },
            )
            .with_id(id);
        }
        Block::Ready(list) => {
            let count = if ru(lang) {
                "дисплеев"
            } else {
                "displays"
            };
            format!("{count}: {}", list.len())
        }
    };
    Node::info(settings_icons::OVERVIEW, &text).with_id(id)
}

/// Пресет раскладки: одна кнопка на несколько команд `niri msg output`.
///
/// Сценарии те же, что человек делает руками: отодвинуть монитор, когда он
/// подключился, и убрать его, когда закрыл сумку на столе. Названия взяты из
/// того, что человек говорит о раскладке, а не из документации niri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// Только экран ноутбука: внешние выводы выключаются.
    LaptopOnly,
    /// Ноутбук и внешний вывод рядом, внешний справа.
    LaptopPlusRight,
    /// Ноутбук и внешний вывод, внешний слева.
    LaptopPlusLeft,
    /// Только внешний вывод, экран ноутбука выключается.
    ExternalOnly,
}

impl Preset {
    /// Все пресеты в порядке показа: от простого к выдуманному.
    pub const ALL: [Preset; 4] = [
        Preset::LaptopOnly,
        Preset::LaptopPlusRight,
        Preset::LaptopPlusLeft,
        Preset::ExternalOnly,
    ];

    /// Подпись в меню.
    pub fn title(self, lang: Language) -> &'static str {
        match (self, lang) {
            (Preset::LaptopOnly, _) => label(lang, "Только ноутбук", "Laptop only"),
            (Preset::LaptopPlusRight, _) => label(
                lang,
                "Ноутбук + внешний справа",
                "Laptop + display on the right",
            ),
            (Preset::LaptopPlusLeft, _) => label(
                lang,
                "Ноутбук + внешний слева",
                "Laptop + display on the left",
            ),
            (Preset::ExternalOnly, _) => label(lang, "Только внешний", "External only"),
        }
    }

    /// Собирает команды пресета по текущему списку выводов.
    ///
    /// Логика та же, что у [`Display::is_on`]: включённый вывод описан
    /// логикой, выключенный — нет. Внешним считается всё, что не `eDP-*`:
    /// так niri сам называет встроенную панель, и это единственное, на что
    /// здесь можно опереться без догадок.
    pub fn commands(self, outputs: &[Display]) -> Vec<CommandSpec> {
        let internal: Vec<&Display> = outputs
            .iter()
            .filter(|output| is_internal(output) && name_is_safe(&output.name))
            .collect();
        let external: Vec<&Display> = outputs
            .iter()
            .filter(|output| !is_internal(output) && name_is_safe(&output.name))
            .collect();
        let mut commands: Vec<CommandSpec> = Vec::new();

        match self {
            Preset::LaptopOnly => {
                // Внутреннюю панель не просто оставляем как есть, а включаем:
                // после пресета «только внешний» она выключена, и «только
                // ноутбук» обязан вернуть её, а не сделать вид, что всё в
                // порядке.
                for output in &internal {
                    commands.push(output_spec(&output.name, &["on"]));
                }
                for output in &external {
                    commands.push(output_spec(&output.name, &["off"]));
                }
            }
            Preset::LaptopPlusRight | Preset::LaptopPlusLeft => {
                let right = self == Preset::LaptopPlusRight;
                for output in &internal {
                    commands.push(output_spec(&output.name, &["on"]));
                    commands.push(output_spec(&output.name, &["position", "set", "0", "0"]));
                }
                for output in &external {
                    commands.push(output_spec(&output.name, &["on"]));
                    commands.extend(external_offset(output, right, &internal));
                }
            }
            Preset::ExternalOnly => {
                for output in &internal {
                    commands.push(output_spec(&output.name, &["off"]));
                }
                for output in &external {
                    commands.push(output_spec(&output.name, &["on"]));
                    commands.push(output_spec(&output.name, &["position", "set", "0", "0"]));
                }
            }
        }
        commands
    }

    /// Действие пресета для меню: команды и общий ключ раздела. Пресета без
    /// единой команды не бывает — такой пункт был бы мёртвой строкой.
    pub fn action(self, outputs: &[Display]) -> Option<Action> {
        let commands = self.commands(outputs);
        (!commands.is_empty()).then_some(Action::RefreshAndRunAll {
            commands,
            refresh: ProviderKey::OUTPUTS,
        })
    }
}

/// Сдвиг внешнего вывода относительно экрана ноутбука: вплотную, без зазора.
///
/// Ширина берётся из логики внутренней панели. Если её нет — вывод выключен и
/// гадать не о чем, — отдаём `position auto`: композитор раскладывает сам, и
/// это честнее, чем выдуманная координата.
fn external_offset(output: &Display, right: bool, internal: &[&Display]) -> Vec<CommandSpec> {
    let width = internal
        .iter()
        .filter_map(|output| output.logical.as_ref())
        .map(|logical| logical.width as i64)
        .max();
    match width {
        Some(width) => {
            let x = if right { width } else { -width };
            vec![output_spec(
                &output.name,
                &["position", "set", &x.to_string(), "0"],
            )]
        }
        None => vec![output_spec(&output.name, &["position", "auto"])],
    }
}

/// Встроенная панель ноутбука. niri так называет её `eDP-*`, и это единственное
/// имя, по которому можно отличить ноутбук от внешнего монитора без
/// догадок о расположении.
fn is_internal(output: &Display) -> bool {
    output.name.starts_with("eDP")
}

/// Подменю пресетов. Показывается не всегда: пресет про внешний вывод без
/// единого внешнего вывода — мёртвая строка, поэтому при одном `eDP-*` в
/// списке остаются только те пресеты, которые что-то меняют и на ноутбуке.
fn preset_rows(outputs: &[Display], lang: Language) -> Option<Node> {
    let has_external = outputs.iter().any(|output| !is_internal(output));
    let presets: Vec<Node> = Preset::ALL
        .into_iter()
        .filter_map(|preset| {
            // Прессет про внешний вывод на ноутбуке без внешнего вывода
            // не делает ничего: прячем его, а не показываем в заглушку.
            if !has_external && preset != Preset::LaptopOnly {
                return None;
            }
            let action = preset.action(outputs)?;
            let title = preset.title(lang);
            Some(
                Node::action(settings_icons::CHECK, title, action)
                    .with_id(&format!("displays/preset/{preset:?}")),
            )
        })
        .collect();
    if presets.is_empty() {
        return None;
    }
    Some(
        Node::submenu(
            settings_icons::CHECK,
            label(lang, "Раскладка", "Layout"),
            presets,
        )
        .with_id("displays/presets"),
    )
}

/// Подпись вывода в списке: имя плюс текущий режим, когда он есть.
fn display_title(lang: Language, display: &Display) -> String {
    let mut title = display.name.clone();
    match display.current() {
        Some(mode) => {
            title.push_str(" · ");
            title.push_str(&mode.label());
        }
        None if display.is_custom_mode => {
            title.push_str(if ru(lang) {
                " · настраиваемый"
            } else {
                " · custom"
            });
        }
        None => {}
    }
    title
}

/// Подменю одного вывода: модель, режим, логика, VRR, физический размер.
fn display_submenu(lang: Language, display: &Display) -> Node {
    let id = display.row_id();
    let mut children = Vec::new();

    let model = [display.make.as_str(), display.model.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !model.is_empty() {
        children.push(
            Node::info(
                settings_icons::DOT,
                &if ru(lang) {
                    format!("Модель: {model}")
                } else {
                    format!("Model: {model}")
                },
            )
            .with_id(&format!("{id}/info/model")),
        );
    }

    let mode = if ru(lang) { "Режим" } else { "Mode" };
    let current = match display.current() {
        Some(mode) => mode.label(),
        None if display.is_custom_mode => {
            if ru(lang) {
                "настраиваемый".to_string()
            } else {
                "custom".to_string()
            }
        }
        None => "—".to_string(),
    };
    children.push(
        Node::info(settings_icons::DOT, &format!("{mode}: {current}"))
            .with_id(&format!("{id}/info/mode")),
    );

    if let Some(logical) = &display.logical {
        let transform = match logical.transform.as_str() {
            "Normal" if ru(lang) => "без поворота",
            "Normal" => "no rotation",
            other => other,
        };
        let text = if ru(lang) {
            format!(
                "Логический: {}x{} · масштаб {} · {} · позиция {}, {}",
                logical.width, logical.height, logical.scale, transform, logical.x, logical.y
            )
        } else {
            format!(
                "Logical: {}x{} · scale {} · {} · position {}, {}",
                logical.width, logical.height, logical.scale, transform, logical.x, logical.y
            )
        };
        children
            .push(Node::info(settings_icons::DOT, &text).with_id(&format!("{id}/info/logical")));
    }

    let vrr = if !display.vrr_supported {
        if ru(lang) {
            "VRR: не поддерживается"
        } else {
            "VRR: not supported"
        }
    } else if display.vrr_enabled {
        if ru(lang) {
            "VRR: включён"
        } else {
            "VRR: on"
        }
    } else if ru(lang) {
        "VRR: выключен"
    } else {
        "VRR: off"
    };
    children.push(Node::info(settings_icons::DOT, vrr).with_id(&format!("{id}/info/vrr")));

    if let Some((width, height)) = display.physical_size_mm {
        let text = if ru(lang) {
            format!("Физический размер: {width}x{height} мм")
        } else {
            format!("Physical size: {width}x{height} mm")
        };
        children
            .push(Node::info(settings_icons::DOT, &text).with_id(&format!("{id}/info/physical")));
    }

    children.extend(control_rows(lang, display));

    Node::submenu(
        settings_icons::OVERVIEW,
        &display_title(lang, display),
        children,
    )
    .with_id(&id)
}

/// Действия над выводом: питание, режим, масштаб, поворот, VRR, позиция.
///
/// Всё уходит через `niri msg output`, а niri сам пишет в `--help`, что эти
/// изменения временные и в конфиг не попадают. Записи в
/// `~/.config/niri/outputs.kdl` здесь нет намеренно: это отдельное решение
/// с бэкапом файла, а не побочный эффект нажатия на режим.
fn control_rows(lang: Language, display: &Display) -> Vec<Node> {
    if !name_is_safe(&display.name) {
        return Vec::new();
    }
    let id = display.row_id();
    let name = display.name.as_str();
    let on = display.is_on();
    let mut rows = vec![
        Node::action(
            settings_icons::BATTERY,
            if on {
                label(lang, "Выключить вывод", "Turn off")
            } else {
                label(lang, "Включить вывод", "Turn on")
            },
            power_action(name, !on),
        )
        .with_id(&format!("{id}/power")),
    ];

    if !display.modes.is_empty() {
        let current = display.current().map(mode_argument);
        let modes: Vec<Node> = display
            .modes
            .iter()
            .map(|mode| {
                let argument = mode_argument(mode);
                let is_current = current.as_deref() == Some(argument.as_str());
                let mut title = mode.label();
                if is_current {
                    title.push_str(label(lang, " · текущий", " · current"));
                }
                if mode.is_preferred {
                    title.push_str(label(lang, " · предпочтительный", " · preferred"));
                }
                Node::action(settings_icons::OVERVIEW, &title, mode_action(name, mode))
                    .with_id(&format!("{id}/mode/{argument}"))
            })
            .collect();
        rows.push(
            Node::submenu(
                settings_icons::OVERVIEW,
                label(lang, "Режим", "Mode"),
                modes,
            )
            .with_id(&format!("{id}/modes")),
        );
    }

    let current_scale = display.logical.as_ref().map(|logical| logical.scale);
    let scales: Vec<Node> = SCALE_CHOICES
        .into_iter()
        .map(|scale| {
            let is_current = current_scale.is_some_and(|current| current == scale);
            let mut title = format_scale(scale);
            if is_current {
                title.push_str(label(lang, " · текущий", " · current"));
            }
            Node::action(settings_icons::DOT, &title, scale_action(name, scale))
                .with_id(&format!("{id}/scale/{title}"))
        })
        .collect();
    rows.push(
        Node::submenu(settings_icons::DOT, label(lang, "Масштаб", "Scale"), scales)
            .with_id(&format!("{id}/scales")),
    );

    let current_transform = display
        .logical
        .as_ref()
        .and_then(|logical| Transform::parse(&logical.transform));
    let transforms: Vec<Node> = Transform::ALL
        .into_iter()
        .map(|transform| {
            let is_current = current_transform == Some(transform);
            let mut title = transform.title(lang).to_string();
            if is_current {
                title.push_str(label(lang, " · текущий", " · current"));
            }
            Node::action(
                settings_icons::DOT,
                &title,
                transform_action(name, transform),
            )
            .with_id(&format!("{id}/transform/{transform:?}"))
        })
        .collect();
    rows.push(
        Node::submenu(
            settings_icons::DOT,
            label(lang, "Поворот", "Rotation"),
            transforms,
        )
        .with_id(&format!("{id}/transforms")),
    );

    // VRR предлагается только там, где вывод его вообще умеет: у ноутбучной
    // панели `vrr_supported: false`, и кнопка была бы заведомым отказом.
    if display.vrr_supported {
        rows.push(
            Node::action(
                settings_icons::DOT,
                if display.vrr_enabled {
                    label(lang, "VRR: выключить", "VRR: off")
                } else {
                    label(lang, "VRR: включить", "VRR: on")
                },
                vrr_action(name, !display.vrr_enabled),
            )
            .with_id(&format!("{id}/vrr")),
        );
    }

    let mut position = vec![
        Node::action(
            settings_icons::DOT,
            label(lang, "Автоматически", "Automatic"),
            position_auto_action(name),
        )
        .with_id(&format!("{id}/position/auto")),
    ];
    for direction in [
        (Nudge::Left, label(lang, "Левее", "Left")),
        (Nudge::Right, label(lang, "Правее", "Right")),
        (Nudge::Up, label(lang, "Выше", "Up")),
        (Nudge::Down, label(lang, "Ниже", "Down")),
    ] {
        let Some((x, y)) = display.nudged(direction.0) else {
            continue;
        };
        position.push(
            Node::action(
                settings_icons::DOT,
                direction.1,
                position_action(name, x, y),
            )
            .with_id(&format!("{id}/position/{direction:?}")),
        );
    }
    rows.push(
        Node::submenu(
            settings_icons::DOT,
            label(lang, "Позиция", "Position"),
            position,
        )
        .with_id(&format!("{id}/positions")),
    );

    rows
}

/// Подпись сразу на двух языках: строки действий короткие, и переводить их
/// дважды в коде — значит разойтись.
fn label<'a>(lang: Language, ru: &'a str, en: &'a str) -> &'a str {
    if lang == Language::Ru { ru } else { en }
}

/// Строки раздела «Дисплеи» из готового снимка.
pub fn build_nodes(snapshot: &DisplaysSnapshot, lang: Language) -> Vec<Node> {
    let mut rows = vec![status_row(lang, snapshot)];
    if let Block::Ready(list) = &snapshot.outputs {
        if let Some(presets) = preset_rows(list, lang) {
            rows.insert(1, presets);
        }
        rows.extend(list.iter().map(|display| display_submenu(lang, display)));
    }
    rows.push(
        Node::action(
            settings_icons::UP,
            if ru(lang) {
                "Обновить"
            } else {
                "Refresh"
            },
            Action::RefreshDynamic(ProviderKey::OUTPUTS),
        )
        .with_id("displays/refresh"),
    );
    rows
}

#[cfg(test)]
mod tests {
    use super::super::system::{CommandError, CommandOutput, ScriptedRunner};
    use super::*;

    const OUTPUTS: &str = r#"{"eDP-1":{"name":"eDP-1","make":"Najing CEC Panda FPD Technology CO. ltd","model":"0x004D","serial":null,"physical_size":[340,190],"modes":[{"width":1920,"height":1080,"refresh_rate":144000,"is_preferred":true},{"width":1920,"height":1080,"refresh_rate":60002,"is_preferred":false}],"current_mode":0,"is_custom_mode":false,"vrr_supported":false,"vrr_enabled":true,"logical":{"x":0,"y":0,"width":1536,"height":864,"scale":1.25,"transform":"Normal"}}}"#;

    const TWO_OUTPUTS: &str = r#"{"eDP-1":{"name":"eDP-1","make":"Panel","model":"0x004D","modes":[{"width":1920,"height":1080,"refresh_rate":144000,"is_preferred":true}],"current_mode":0,"is_custom_mode":false,"vrr_supported":false,"vrr_enabled":false,"logical":{"x":0,"y":0,"width":1536,"height":864,"scale":1.25,"transform":"Normal"}},"HDMI-A-1":{"name":"HDMI-A-1","make":"Dell","model":"U2720Q","modes":[{"width":3840,"height":2160,"refresh_rate":60000,"is_preferred":true}],"current_mode":null,"is_custom_mode":true,"vrr_supported":true,"vrr_enabled":true}}"#;

    fn ok(stdout: &str) -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            status: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    fn missing() -> Result<CommandOutput, CommandError> {
        Err(CommandError::Spawn("нет niri".to_string()))
    }

    fn failed() -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            status: Some(1),
            stdout: String::new(),
            stderr: "Failed to connect to niri\n".to_string(),
        })
    }

    #[test]
    fn parses_a_single_output_with_its_modes() {
        let list = parse_outputs(OUTPUTS).expect("вывод разобран");
        assert_eq!(list.len(), 1);
        let display = &list[0];
        assert_eq!(display.name, "eDP-1");
        assert_eq!(display.make, "Najing CEC Panda FPD Technology CO. ltd");
        assert_eq!(display.model, "0x004D");
        assert_eq!(
            display.serial, None,
            "null — это «не знаю», а не пустая строка"
        );
        assert_eq!(display.physical_size_mm, Some((340, 190)));
        assert_eq!(display.modes.len(), 2);
        assert!(display.modes[0].is_preferred);
        assert_eq!(display.current_mode, Some(0));
        assert!(!display.vrr_supported, "VRR на eDP-1 не поддерживается");
        assert!(display.vrr_enabled, "но niri отдаёт enabled=true");
        let logical = display.logical.as_ref().expect("логика есть");
        assert_eq!(logical.scale, 1.25);
        assert_eq!(logical.width, 1536);
        assert_eq!(logical.transform, "Normal");
    }

    #[test]
    fn mode_labels_look_like_niri_prints_them() {
        let list = parse_outputs(OUTPUTS).expect("вывод разобран");
        let modes = &list[0].modes;
        assert_eq!(modes[0].label(), "1920x1080 @ 144.000");
        assert_eq!(modes[1].label(), "1920x1080 @ 60.002");
    }

    #[test]
    fn current_mode_points_at_the_matching_entry() {
        let list = parse_outputs(OUTPUTS).expect("вывод разобран");
        let current = list[0].current().expect("текущий режим есть");
        assert!(current.is_preferred);
        assert_eq!(current.refresh_mhz, 144000);
    }

    #[test]
    fn a_custom_mode_has_no_entry_in_the_modes_list() {
        let list = parse_outputs(TWO_OUTPUTS).expect("вывод разобран");
        let hdmi = list
            .iter()
            .find(|display| display.name == "HDMI-A-1")
            .expect("HDMI есть");
        assert!(hdmi.is_custom_mode);
        assert_eq!(hdmi.current_mode, None, "niri отдал null");
        assert!(hdmi.current().is_none(), "взять из списка нечего");
        assert!(hdmi.logical.is_none(), "выключенный вывод без логики");
        assert!(hdmi.vrr_supported && hdmi.vrr_enabled);
    }

    #[test]
    fn parse_rejects_json_that_is_not_an_object() {
        let error = parse_outputs("[]").expect_err("массив — не объект выводов");
        assert!(error.contains("объект"), "{error}");
        assert!(parse_outputs("мусор").is_err());
    }

    #[test]
    fn fetch_asks_niri_exactly_once() {
        let runner = ScriptedRunner::new(vec![ok(OUTPUTS)]);
        let snapshot = DisplaysProvider.fetch(&runner).expect("снимок");
        assert_eq!(runner.calls(), ["niri msg --json outputs"]);
        assert_eq!(snapshot.outputs.ready().map(Vec::len), Some(1));
    }

    #[test]
    fn missing_niri_makes_the_section_unavailable() {
        let runner = ScriptedRunner::new(vec![missing()]);
        let error = DisplaysProvider
            .fetch(&runner)
            .expect_err("без niri раздел недоступен");
        assert!(matches!(error, ProviderError::Unavailable(_)), "{error:?}");
        assert_eq!(error.message(), "нет niri");
    }

    #[test]
    fn a_failed_command_is_a_failed_block_not_an_error() {
        let runner = ScriptedRunner::new(vec![failed()]);
        let snapshot = DisplaysProvider.fetch(&runner).expect("снимок");
        assert!(snapshot.outputs.is_empty());
        assert!(
            snapshot
                .outputs
                .failure()
                .unwrap()
                .contains("Failed to connect"),
            "{:?}",
            snapshot.outputs
        );
    }

    #[test]
    fn broken_json_is_a_failed_block_too() {
        let runner = ScriptedRunner::new(vec![ok("{\"eDP-1\": ")]);
        let snapshot = DisplaysProvider.fetch(&runner).expect("снимок");
        assert!(snapshot.outputs.failure().is_some());
    }

    #[test]
    fn nodes_list_outputs_and_a_refresh_row() {
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(parse_outputs(TWO_OUTPUTS).expect("вывод разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        assert_eq!(nodes[0].title, "дисплеев: 2");
        let by_id = |id: &str| {
            nodes
                .iter()
                .find(|node| node.identity() == id)
                .unwrap_or_else(|| panic!("нет строки {id}"))
        };
        assert_eq!(
            by_id("displays/output/eDP-1").title,
            "eDP-1 · 1920x1080 @ 144.000"
        );
        assert_eq!(
            by_id("displays/output/HDMI-A-1").title,
            "HDMI-A-1 · настраиваемый"
        );
        let refresh = nodes.last().expect("строка обновления");
        assert_eq!(refresh.identity(), "displays/refresh");
        assert!(
            matches!(
                refresh.kind,
                super::super::tree::NodeKind::Action(Action::RefreshDynamic(ProviderKey::OUTPUTS))
            ),
            "обновление read-only: без команд записи"
        );
    }

    #[test]
    fn english_keeps_the_same_identity() {
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(parse_outputs(OUTPUTS).expect("вывод разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::En);
        assert_eq!(nodes[0].title, "displays: 1");
        let output = nodes
            .iter()
            .find(|node| node.identity() == "displays/output/eDP-1")
            .expect("вывод есть");
        assert_eq!(output.title, "eDP-1 · 1920x1080 @ 144.000");
        let children = output.children().expect("дети есть");
        assert!(
            children
                .iter()
                .any(|row| row.title == "Model: Najing CEC Panda FPD Technology CO. ltd 0x004D")
        );
        assert!(children.iter().any(|row| row.title == "VRR: not supported"));
        assert!(
            children
                .iter()
                .any(|row| row.title
                    == "Logical: 1536x864 · scale 1.25 · no rotation · position 0, 0")
        );
        assert!(
            children
                .iter()
                .any(|row| row.title == "Physical size: 340x190 mm")
        );
    }

    #[test]
    fn empty_outputs_say_so_and_keep_the_refresh_row() {
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(Vec::new()),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        assert_eq!(nodes[0].title, "Нет дисплеев");
        assert_eq!(nodes.len(), 2, "статус и обновление");
    }

    /// Пресет собирает команды в том же порядке, в каком человек делает их
    /// руками: сперва питание, потом место. «Только ноутбук» гасит внешний
    /// вывод и ничего не трогает во внутреннем.
    #[test]
    fn presets_build_the_commands_a_person_would_type() {
        let outputs = parse_outputs(TWO_OUTPUTS).expect("вывод разобран");
        fn argv(command: &CommandSpec) -> String {
            command.argv().1.join(" ")
        }
        let laptop_only = Preset::LaptopOnly.commands(&outputs);
        assert_eq!(
            laptop_only.iter().map(argv).collect::<Vec<_>>(),
            ["msg output eDP-1 on", "msg output HDMI-A-1 off",]
        );

        let right = Preset::LaptopPlusRight.commands(&outputs);
        assert_eq!(
            right.iter().map(argv).collect::<Vec<_>>(),
            [
                "msg output eDP-1 on",
                "msg output eDP-1 position set 0 0",
                "msg output HDMI-A-1 on",
                // Ширина экрана ноутбука 1536 логических пикселя.
                "msg output HDMI-A-1 position set 1536 0",
            ]
        );

        let left = Preset::LaptopPlusLeft.commands(&outputs);
        assert_eq!(
            left.last().map(argv),
            Some("msg output HDMI-A-1 position set -1536 0".to_string())
        );

        let external_only = Preset::ExternalOnly.commands(&outputs);
        assert_eq!(
            external_only.iter().map(argv).collect::<Vec<_>>(),
            [
                "msg output eDP-1 off",
                "msg output HDMI-A-1 on",
                "msg output HDMI-A-1 position set 0 0",
            ]
        );
    }

    /// Ноутбук без внешнего вывода: пресет «внешний справа» нечего делать,
    /// поэтому в меню его нет, а «только ноутбук» остаётся — он включает
    /// выключенный экран.
    #[test]
    fn presets_without_an_external_output_are_hidden() {
        let outputs = parse_outputs(OUTPUTS).expect("вывод разобран");
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(outputs),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let presets = nodes
            .iter()
            .find(|node| node.identity() == "displays/presets")
            .and_then(|node| node.children())
            .expect("подменю раскладки есть");
        let ids: Vec<&str> = presets.iter().map(|row| row.identity()).collect();
        assert_eq!(ids, ["displays/preset/LaptopOnly"], "{ids:?}");

        let off = Display {
            logical: None,
            ..parse_outputs(OUTPUTS).expect("вывод разобран").remove(0)
        };
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(vec![off]),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let preset = nodes
            .iter()
            .find(|node| node.identity() == "displays/presets")
            .and_then(|node| node.children())
            .and_then(|rows| rows.first())
            .expect("пресет есть");
        match &preset.kind {
            super::super::tree::NodeKind::Action(Action::RefreshAndRunAll { commands, .. }) => {
                assert_eq!(
                    commands.len(),
                    1,
                    "«только ноутбук» включает выключенный экран"
                );
                assert_eq!(commands[0].argv().1.join(" "), "msg output eDP-1 on");
            }
            other => panic!("ожидалось действие с командами: {other:?}"),
        }
    }

    /// Выключенный ноутбук не даёт ширины для сдвига, поэтому вместо
    /// выдуманной координаты пресет отдаёт `position auto`.
    #[test]
    fn a_laptop_without_logical_size_leaves_the_layout_to_niri() {
        let mut outputs = parse_outputs(TWO_OUTPUTS).expect("вывод разобран");
        outputs[0].logical = None;
        let commands = Preset::LaptopPlusRight.commands(&outputs);
        let last = commands.last().expect("есть команда");
        assert_eq!(last.argv().1.join(" "), "msg output HDMI-A-1 position auto");
    }

    /// Пресет без единой команды не рисуется: `None`, а не мёртвая строка.
    #[test]
    fn a_preset_without_commands_has_no_action() {
        assert!(Preset::LaptopOnly.action(&[]).is_none());
        assert!(Preset::LaptopPlusRight.action(&[]).is_none());
        let only_internal = parse_outputs(OUTPUTS).expect("вывод разобран");
        assert!(Preset::LaptopOnly.action(&only_internal).is_some());
    }

    /// Личности строк не повторяются: курсор и его восстановление держатся на
    /// id, и две строки с одним id означают, что после refresh выделенная
    /// строка скачет. Строка статуса VRR и кнопка VRR когда-то делили один.
    #[test]
    fn every_row_in_a_display_submenu_has_its_own_identity() {
        for lang in [Language::Ru, Language::En] {
            let snapshot = DisplaysSnapshot {
                outputs: Block::Ready(parse_outputs(TWO_OUTPUTS).expect("вывод разобран")),
            };
            let nodes = build_nodes(&snapshot, lang);
            let mut seen = Vec::new();
            for node in &nodes {
                seen.push(node.identity().to_string());
                if let Some(children) = node.children() {
                    seen.extend(children.iter().map(|child| child.identity().to_string()));
                }
            }
            let before = seen.len();
            seen.sort();
            seen.dedup();
            assert_eq!(before, seen.len(), "повторяющиеся личности: {seen:?}");
        }
    }

    /// Команды действий собираются ровно так, как их ждёт niri:
    /// `niri msg output <OUTPUT> <ACTION> <…>`.
    #[test]
    fn action_commands_match_the_grammar_niri_prints_in_its_error() {
        let argv = |action: Action| match action {
            Action::RefreshAndRun { command, .. } => command.argv().1,
            _ => panic!("ожидалась команда"),
        };
        let mode = DisplayMode {
            width: 1920,
            height: 1080,
            refresh_mhz: 60002,
            is_preferred: false,
        };
        assert_eq!(
            argv(power_action("eDP-1", true)),
            ["msg", "output", "eDP-1", "on"]
        );
        assert_eq!(
            argv(power_action("eDP-1", false)),
            ["msg", "output", "eDP-1", "off"]
        );
        assert_eq!(
            argv(mode_action("eDP-1", &mode)),
            ["msg", "output", "eDP-1", "mode", "1920x1080@60.002"]
        );
        assert_eq!(
            argv(scale_action("eDP-1", 1.25)),
            ["msg", "output", "eDP-1", "scale", "1.25"]
        );
        assert_eq!(
            argv(transform_action("eDP-1", Transform::Flipped90)),
            ["msg", "output", "eDP-1", "transform", "flipped-90"]
        );
        assert_eq!(
            argv(vrr_action("eDP-1", true)),
            ["msg", "output", "eDP-1", "vrr", "on"]
        );
        assert_eq!(
            argv(position_auto_action("eDP-1")),
            ["msg", "output", "eDP-1", "position", "auto"]
        );
        assert_eq!(
            argv(position_action("eDP-1", 1920, 0)),
            ["msg", "output", "eDP-1", "position", "set", "1920", "0"]
        );
        assert_eq!(
            argv(position_action("HDMI-A-1", -1920, -1080)),
            [
                "msg", "output", "HDMI-A-1", "position", "set", "-1920", "-1080"
            ],
            "отрицательные координаты не экранируются и не теряются"
        );
    }

    /// Имя вывода с ведущим дефисом clap принял бы за флаги, а `--` перед ним
    /// niri не разбирает, поэтому такое имя в раздел не попадает вовсе.
    #[test]
    fn an_output_name_with_a_leading_dash_is_never_shown() {
        assert!(name_is_safe("eDP-1"));
        assert!(name_is_safe("HDMI-A-1"));
        assert!(!name_is_safe("-bad"));
        assert!(!name_is_safe(""));
        let display = Display {
            name: "-bad".to_string(),
            modes: vec![DisplayMode {
                width: 1920,
                height: 1080,
                refresh_mhz: 60000,
                is_preferred: true,
            }],
            logical: Some(DisplayLogical::default()),
            ..Display::default()
        };
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(vec![display]),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let output = nodes
            .iter()
            .find(|node| node.identity() == "displays/output/-bad")
            .expect("вывод есть");
        let children = output.children().expect("дети есть");
        assert!(
            children
                .iter()
                .all(|row| !matches!(row.kind, super::super::tree::NodeKind::Action(_))),
            "у небезопасного имени нет ни одного действия: {:?}",
            children.iter().map(|row| &row.title).collect::<Vec<_>>()
        );
    }

    /// Частота и масштаб словами niri: без хвостовых нулей, но с точностью
    /// до трёх знаков, как их печатает сам композитор.
    #[test]
    fn numbers_are_formatted_without_noise() {
        assert_eq!(format_refresh(144000), "144");
        assert_eq!(format_refresh(60002), "60.002");
        assert_eq!(format_refresh(60000), "60");
        assert_eq!(format_scale(1.0), "1");
        assert_eq!(format_scale(1.25), "1.25");
        assert_eq!(format_scale(2.5), "2.5");
    }

    /// Повороты в меню — ровно те восемь слов, что перечисляет niri в своём
    /// сообщении об ошибке, иначе команда была бы отвергнута.
    #[test]
    fn transform_words_are_exactly_the_ones_niri_accepts() {
        let words: Vec<&str> = Transform::ALL
            .into_iter()
            .map(|item| item.as_str())
            .collect();
        assert_eq!(
            words,
            [
                "normal",
                "90",
                "180",
                "270",
                "flipped",
                "flipped-90",
                "flipped-180",
                "flipped-270"
            ]
        );
        assert_eq!(Transform::parse("flipped-180"), Some(Transform::Flipped180));
        assert_eq!(Transform::parse("Normal"), Some(Transform::Normal));
        assert_eq!(Transform::parse("восемьдесят"), None);
    }

    /// Сдвиг идёт на размер собственной логической области, а выключенный
    /// вывод не сдвигается вовсе: точки у него нет.
    #[test]
    fn nudging_steps_by_the_output_own_size() {
        let mut display = parse_outputs(OUTPUTS).expect("вывод разобран").remove(0);
        assert_eq!(display.nudged(Nudge::Right), Some((1536, 0)));
        assert_eq!(display.nudged(Nudge::Down), Some((0, 864)));
        assert_eq!(display.nudged(Nudge::Left), Some((-1536, 0)));
        assert_eq!(display.nudged(Nudge::Up), Some((0, -864)));
        display.logical = None;
        assert_eq!(
            display.nudged(Nudge::Right),
            None,
            "выключенный вывод не двигается"
        );
    }

    /// В подменю вывода есть действия для питания, режима, масштаба, поворота
    /// и позиции, а VRR — только там, где вывод его умеет.
    #[test]
    fn control_rows_offer_every_change_and_hide_impossible_vrr() {
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(parse_outputs(OUTPUTS).expect("вывод разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let output = nodes
            .iter()
            .find(|node| node.identity() == "displays/output/eDP-1")
            .expect("вывод есть");
        let children = output.children().expect("дети есть");
        let ids: Vec<&str> = children.iter().map(|row| row.identity()).collect();
        for expected in [
            "displays/output/eDP-1/power",
            "displays/output/eDP-1/modes",
            "displays/output/eDP-1/scales",
            "displays/output/eDP-1/transforms",
            "displays/output/eDP-1/positions",
        ] {
            assert!(ids.contains(&expected), "нет строки {expected}: {ids:?}");
        }
        assert!(
            !ids.contains(&"displays/output/eDP-1/vrr"),
            "VRR на eDP-1 не поддерживается, кнопки быть не должно"
        );
        let power = children
            .iter()
            .find(|row| row.identity() == "displays/output/eDP-1/power")
            .expect("питание есть");
        assert_eq!(
            power.title, "Выключить вывод",
            "вывод включён — предлагаем выключить"
        );

        let modes = children
            .iter()
            .find(|row| row.identity() == "displays/output/eDP-1/modes")
            .and_then(|row| row.children())
            .expect("режимы есть");
        assert_eq!(modes.len(), 2);
        assert!(modes[0].title.contains("текущий"), "{}", modes[0].title);
        assert!(
            modes[0].title.contains("предпочтительный"),
            "{}",
            modes[0].title
        );
    }

    /// Выключенный вывод получает «Включить», а умеющий VRR — кнопку VRR.
    #[test]
    fn an_off_output_offers_power_on_and_keeps_its_vrr_button() {
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(parse_outputs(TWO_OUTPUTS).expect("вывод разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::En);
        let hdmi = nodes
            .iter()
            .find(|node| node.identity() == "displays/output/HDMI-A-1")
            .expect("HDMI есть");
        let children = hdmi.children().expect("дети есть");
        let power = children
            .iter()
            .find(|row| row.identity() == "displays/output/HDMI-A-1/power")
            .expect("питание есть");
        assert_eq!(power.title, "Turn on", "выключен — предлагаем включить");
        assert!(
            children
                .iter()
                .any(|row| row.identity() == "displays/output/HDMI-A-1/vrr"),
            "вывод умеет VRR — кнопка обязана быть"
        );
        assert!(
            !children
                .iter()
                .any(|row| row.identity().ends_with("/position/Right")
                    || row.identity().ends_with("/position/Down")),
            "у выключенного вывода нет точки, значит нет и сдвигов: {:?}",
            children
                .iter()
                .map(|row| row.identity())
                .collect::<Vec<_>>()
        );
    }

    /// Всё, что меняет вывод, идёт через `RefreshAndRun` с ключом OUTPUTS:
    /// после успеха слот перечитывается, иначе мену показывало бы старый
    /// режим до следующего обновления вручную.
    #[test]
    fn every_control_action_refreshes_the_section() {
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(parse_outputs(OUTPUTS).expect("вывод разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let output = nodes
            .iter()
            .find(|node| node.identity() == "displays/output/eDP-1")
            .expect("вывод есть");
        let children = output.children().expect("дети есть");
        let mut actions = 0;
        for row in children {
            let grand = row.children().map(<[Node]>::to_vec).unwrap_or_default();
            for node in std::iter::once(row).chain(grand.iter()) {
                if let super::super::tree::NodeKind::Action(Action::RefreshAndRun {
                    refresh, ..
                }) = &node.kind
                {
                    assert_eq!(*refresh, ProviderKey::OUTPUTS);
                    actions += 1;
                }
            }
        }
        // Питание, два режима, семь масштабов, восемь поворотов и пять
        // позиций: без VRR, который этому выводу не положен.
        assert_eq!(actions, 1 + 2 + 7 + 8 + 5);
    }

    /// Живой опрос хоста: только read-only команда `niri msg --json outputs`.
    /// Запускается вручную (`cargo test --bin hud-menu-rs -- --ignored live`).
    #[test]
    #[ignore = "обращается к niri хоста"]
    fn live_reads_the_host() {
        let snapshot = DisplaysProvider
            .fetch(&SystemCommandRunner::default())
            .expect("на хосте есть niri");
        let outputs = snapshot.outputs.ready().expect("выводы есть");
        println!("выводов: {}", outputs.len());
        for display in outputs {
            println!(
                "{display:?}",
                display = Display {
                    make: display.make.clone(),
                    ..display.clone()
                }
            );
        }
        assert!(!outputs.is_empty(), "у ноутбука есть хоть один вывод");
    }

    #[test]
    fn identity_is_the_output_name_not_the_mode() {
        let list = parse_outputs(OUTPUTS).expect("вывод разобран");
        let first = &list[0];
        let renamed = Display {
            current_mode: Some(1),
            ..first.clone()
        };
        assert_eq!(
            first.row_id(),
            renamed.row_id(),
            "смена режима не часть личности"
        );
        assert_eq!(first.row_id(), "displays/output/eDP-1");
    }
}

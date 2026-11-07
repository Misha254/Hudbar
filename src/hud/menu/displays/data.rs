use super::*;

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

pub(crate) fn format_scale_percent(scale: f64) -> String {
    format!("{} %", format_scale(scale * 100.0))
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
pub(crate) fn output_spec(name: &str, args: &[&str]) -> CommandSpec {
    let mut spec = CommandSpec::new("niri").arg("msg").arg("output");
    if name_is_safe(name) {
        spec = spec.arg(name);
    }
    for arg in args {
        spec = spec.arg(arg);
    }
    spec.with_timeout(OUTPUT_TIMEOUT)
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

    /// Человеческая подпись режима для меню: `1920×1080 · 144 Гц`.
    /// Частоту собирает `format_refresh`, поэтому `60.002` не теряет знаки.
    pub fn human_label(&self, lang: Language) -> String {
        format!(
            "{}×{} · {} {}",
            self.width,
            self.height,
            format_refresh(self.refresh_mhz),
            if ru(lang) { "Гц" } else { "Hz" }
        )
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
        .map_err(|error| error.message_in(lang))?;
    Ok(provider.nodes(&snapshot, lang))
}

/// Блок выводов одним вызовом `niri msg --json outputs`.
pub(crate) fn outputs(runner: &dyn CommandRunner) -> Block<Vec<Display>> {
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
pub(crate) fn failure(stderr: &str) -> String {
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
pub(crate) fn parse_logical(value: &serde_json::Value) -> Option<DisplayLogical> {
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

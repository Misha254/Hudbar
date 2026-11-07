use super::*;

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
pub(crate) fn external_offset(
    output: &Display,
    right: bool,
    internal: &[&Display],
) -> Vec<CommandSpec> {
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
pub(crate) fn is_internal(output: &Display) -> bool {
    output.name.starts_with("eDP")
        || output.name.starts_with("LVDS")
        || output.name.starts_with("DSI")
}

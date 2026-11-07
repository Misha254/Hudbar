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
    SystemProvider, provider_text,
};
use super::tree::Node;
use std::time::Duration;

/// Время, которое ждём ответа композитора на смену вывода. Это локальный
/// IPC-вызов, но он меняет картинку на экране, поэтому секунды односекундного
/// `oneshot::TIMEOUT` на всякий случай не берём.
const OUTPUT_TIMEOUT: Duration = Duration::from_secs(3);

#[path = "displays/actions.rs"]
mod actions;
#[path = "displays/data.rs"]
mod data;
#[path = "displays/view.rs"]
mod view;
pub(crate) use actions::*;
pub(crate) use data::*;
pub(crate) use view::*;

#[cfg(test)]
mod tests {
    use super::super::system::{CommandError, CommandOutput, ScriptedRunner};
    use super::*;

    const OUTPUTS: &str = r#"{"eDP-1":{"name":"eDP-1","make":"Najing CEC Panda FPD Technology CO. ltd","model":"0x004D","serial":null,"physical_size":[340,190],"modes":[{"width":1920,"height":1080,"refresh_rate":144000,"is_preferred":true},{"width":1920,"height":1080,"refresh_rate":60002,"is_preferred":false}],"current_mode":0,"is_custom_mode":false,"vrr_supported":false,"vrr_enabled":true,"logical":{"x":0,"y":0,"width":1536,"height":864,"scale":1.25,"transform":"Normal"}}}"#;

    const TWO_OUTPUTS: &str = r#"{"eDP-1":{"name":"eDP-1","make":"Panel","model":"0x004D","modes":[{"width":1920,"height":1080,"refresh_rate":144000,"is_preferred":true}],"current_mode":0,"is_custom_mode":false,"vrr_supported":false,"vrr_enabled":false,"logical":{"x":0,"y":0,"width":1536,"height":864,"scale":1.25,"transform":"Normal"}},"HDMI-A-1":{"name":"HDMI-A-1","make":"Dell","model":"U2720Q","modes":[{"width":3840,"height":2160,"refresh_rate":60000,"is_preferred":true}],"current_mode":null,"is_custom_mode":true,"vrr_supported":true,"vrr_enabled":true,"logical":{"x":1536,"y":0,"width":3840,"height":2160,"scale":1.0,"transform":"Normal"}}}"#;

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
    fn human_mode_labels_are_short() {
        let list = parse_outputs(OUTPUTS).expect("вывод разобран");
        let modes = &list[0].modes;
        assert_eq!(modes[0].human_label(Language::Ru), "1920×1080 · 144 Гц");
        assert_eq!(modes[0].human_label(Language::En), "1920×1080 · 144 Hz");
        assert_eq!(modes[1].human_label(Language::Ru), "1920×1080 · 60.002 Гц");
        let zero = DisplayMode {
            width: 800,
            height: 600,
            refresh_mhz: 0,
            is_preferred: false,
        };
        assert_eq!(zero.human_label(Language::Ru), "800×600 · 0 Гц");
    }

    #[test]
    fn human_labels_never_contain_the_machine_refresh_format() {
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(parse_outputs(OUTPUTS).expect("вывод разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let title = &nodes
            .iter()
            .find(|node| node.identity() == "displays/output/eDP-1")
            .expect("вывод есть")
            .title;
        assert!(!title.contains("144.000"), "{title}");
        let children = nodes
            .iter()
            .find(|node| node.identity() == "displays/output/eDP-1")
            .and_then(Node::children)
            .expect("дети есть");
        for row in children {
            assert!(
                !row.title.contains("144.000"),
                "машинный формат в человеческой подписи: {}",
                row.title
            );
        }
    }

    #[test]
    fn logical_line_is_short_and_mentions_rotation_only_when_needed() {
        let normal = DisplayLogical {
            width: 1536,
            height: 864,
            scale: 1.25,
            transform: "Normal".to_string(),
            ..DisplayLogical::default()
        };
        assert_eq!(
            logical_text(Language::Ru, &normal),
            "Логический: 1536×864 · ×1.25"
        );
        assert_eq!(
            logical_text(Language::En, &normal),
            "Logical: 1536×864 · ×1.25"
        );
        let rotated = DisplayLogical {
            transform: "90".to_string(),
            ..normal
        };
        assert_eq!(
            logical_text(Language::Ru, &rotated),
            "Логический: 1536×864 · ×1.25 · поворот 90°"
        );
        assert_eq!(
            logical_text(Language::En, &rotated),
            "Logical: 1536×864 · ×1.25 · rotated 90°"
        );
    }

    #[test]
    fn corporate_tails_are_removed_from_make_only() {
        assert_eq!(
            clean_make("Najing CEC Panda FPD Technology CO. ltd"),
            "Najing CEC Panda FPD Technology"
        );
        assert_eq!(clean_make("Dell Inc."), "Dell");
        assert_eq!(clean_make("Panel"), "Panel");
        assert_eq!(clean_make(""), "");
    }

    #[test]
    fn raw_model_code_is_hidden_and_technology_is_kept() {
        let list = parse_outputs(OUTPUTS).expect("вывод разобран");
        let node = display_submenu(Language::Ru, &list[0], 1);
        let children = node.children().expect("дети есть");
        let model = children
            .iter()
            .find(|row| row.identity().ends_with("/info/model"))
            .expect("строка модели");
        assert_eq!(model.title, "Модель");
        assert_eq!(model.value.as_ref().map(|_| "value"), Some("value"));
        assert!(!model.title.contains("0x004D"));

        let hidden = Display {
            make: "CO. ltd".to_string(),
            model: "0x004D".to_string(),
            ..Display::default()
        };
        let empty = display_submenu(Language::Ru, &hidden, 0);
        assert!(
            !empty
                .children()
                .expect("дети есть")
                .iter()
                .any(|row| row.identity().ends_with("/info/model")),
            "пустая модель не показывает строку"
        );
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
        assert!(hdmi.logical.is_some(), "второй вывод включён в fixture");
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
        assert_eq!(nodes[0].title, "Дисплеи");
        assert!(
            matches!(nodes[0].value, Some(super::super::tree::ValueSource::Text(ref s)) if s == "2 подключено")
        );
        let by_id = |id: &str| {
            nodes
                .iter()
                .find(|node| node.identity() == id)
                .unwrap_or_else(|| panic!("нет строки {id}"))
        };
        assert_eq!(by_id("displays/output/eDP-1").title, "eDP-1 — встроенный");
        assert_eq!(by_id("displays/output/HDMI-A-1").title, "HDMI-A-1");
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
    fn displays_list_header_counts_outputs_with_plural() {
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(parse_outputs(TWO_OUTPUTS).unwrap()),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        assert_eq!(nodes[0].title, "Дисплеи");
        assert!(
            matches!(nodes[0].value, Some(super::super::tree::ValueSource::Text(ref value)) if value == "2 подключено")
        );
        let one = DisplaysSnapshot {
            outputs: Block::Ready(parse_outputs(OUTPUTS).unwrap()),
        };
        let nodes = build_nodes(&one, Language::Ru);
        assert!(
            matches!(nodes[0].value, Some(super::super::tree::ValueSource::Text(ref value)) if value == "1 подключён")
        );
        let nodes = build_nodes(&one, Language::En);
        assert!(
            matches!(nodes[0].value, Some(super::super::tree::ValueSource::Text(ref value)) if value == "1 connected")
        );
    }

    #[test]
    fn displays_output_level_has_groups_in_order() {
        let display = parse_outputs(OUTPUTS).unwrap().remove(0);
        let node = display_submenu(Language::Ru, &display, 1);
        let titles: Vec<_> = node
            .children()
            .unwrap()
            .iter()
            .filter(|row| matches!(row.kind, super::super::tree::NodeKind::Header))
            .map(|row| row.title.as_str())
            .collect();
        assert_eq!(titles, ["Информация", "Настройки"]);
        assert!(!titles.contains(&"Действия"));
        assert!(!titles.contains(&"Опасные действия"));
    }

    /// Уровень вывода целиком помещается в карточку: справка, настройки и
    /// выключение не должны прятаться за прокруткой.
    #[test]
    fn output_level_fits_the_card_without_scrolling() {
        for (outputs, on_count) in [(OUTPUTS, 1), (TWO_OUTPUTS, 2)] {
            for display in parse_outputs(outputs).unwrap() {
                let node = display_submenu(Language::Ru, &display, on_count);
                let rows = node.children().unwrap().len();
                assert!(
                    rows <= super::super::view::MAX_ROWS,
                    "{}: {rows} строк не влезают в карточку",
                    display.name
                );
            }
        }
    }

    #[test]
    fn displays_output_level_opens_with_none() {
        let display = parse_outputs(OUTPUTS).unwrap().remove(0);
        let node = display_submenu(Language::Ru, &display, 1);
        let value = match node.value {
            Some(super::super::tree::ValueSource::Text(value)) => value,
            _ => panic!("value"),
        };
        assert_eq!(value, "1920×1080 · 144 Гц");
        assert_eq!(node.crumb.as_deref(), Some("eDP-1"));
    }

    #[test]
    fn off_output_shows_enable_without_danger_group() {
        let mut display = parse_outputs(OUTPUTS).unwrap().remove(0);
        display.logical = None;
        let node = display_submenu(Language::Ru, &display, 1);
        let children = node.children().unwrap();
        assert!(!children.iter().any(|row| row.title == "Опасные действия"));
        assert!(children.iter().any(|row| row.title == "Включить вывод"));
        assert!(!children.iter().any(|row| row.title == "Настройки"));
    }

    #[test]
    fn english_keeps_the_same_identity() {
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(parse_outputs(OUTPUTS).expect("вывод разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::En);
        assert_eq!(nodes[0].title, "Displays");
        assert!(
            matches!(nodes[0].value, Some(super::super::tree::ValueSource::Text(ref s)) if s == "1 connected")
        );
        let output = nodes
            .iter()
            .find(|node| node.identity() == "displays/output/eDP-1")
            .expect("вывод есть");
        assert_eq!(output.title, "eDP-1 — built-in");
        let children = output.children().expect("дети есть");
        assert!(
            children
                .iter()
                .any(|row| row.title == "Model" && row.value.is_some())
        );
        assert!(children.iter().any(|row| row.title == "VRR" && row.muted));
        assert!(
            children
                .iter()
                .any(|row| row.title == "Scale" && row.value.is_some())
        );
        assert!(
            children
                .iter()
                .any(|row| row.title == "Size" && row.value.is_some())
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

    /// Узел вывода носит короткую крошку «eDP-1», не теряя полный заголовок
    /// строки с режимом.
    #[test]
    fn output_level_has_a_short_crumb() {
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(parse_outputs(OUTPUTS).expect("вывод разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let output = nodes
            .iter()
            .find(|node| node.identity() == "displays/output/eDP-1")
            .expect("вывод есть");
        assert_eq!(output.crumb.as_deref(), Some("eDP-1"));
        assert_eq!(output.title, "eDP-1 — встроенный");
    }

    /// Иконки строк соответствуют смыслу, а не одной точке: вывод и статус —
    /// монитор, обновление — круговая стрелка, режим/масштаб/поворот/позиция
    /// различаются.
    #[test]
    fn display_rows_use_meaningful_icons() {
        let snapshot = DisplaysSnapshot {
            outputs: Block::Ready(parse_outputs(OUTPUTS).expect("вывод разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let output = nodes
            .iter()
            .find(|node| node.identity() == "displays/output/eDP-1")
            .expect("вывод есть");
        assert_eq!(output.icon, settings_icons::PANEL_MONITOR);
        let refresh = nodes.last().expect("обновление");
        assert_eq!(refresh.icon, settings_icons::REFRESH);
        let children = output.children().expect("дети есть");
        let icon = |id: &str| {
            children
                .iter()
                .find(|row| row.identity() == id)
                .map(|row| row.icon)
                .unwrap_or_else(|| panic!("нет строки {id}"))
        };
        assert_eq!(icon("displays/output/eDP-1/power"), settings_icons::POWER);
        assert!(
            children
                .iter()
                .any(|row| row.identity() == "displays/output/eDP-1/group/info"
                    && matches!(row.kind, super::super::tree::NodeKind::Header)
                    && row.icon.is_empty()),
            "информационный заголовок не несёт иконку"
        );
        assert_eq!(
            icon("displays/output/eDP-1/modes"),
            settings_icons::TACHOMETER
        );
        assert_eq!(
            icon("displays/output/eDP-1/scales"),
            settings_icons::ZOOM_IN
        );
        assert_eq!(
            icon("displays/output/eDP-1/transforms"),
            settings_icons::ROTATE
        );
        assert_eq!(
            icon("displays/output/eDP-1/positions"),
            settings_icons::CROSSHAIR
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
        for row in children {
            if let super::super::tree::NodeKind::Action(action) = &row.kind {
                match action {
                    Action::RefreshAndRun { command, .. } => {
                        assert!(
                            !command.argv().1.iter().any(|arg| arg == "-bad"),
                            "небезопасное имя не попадает в argv: {:?}",
                            command.argv()
                        );
                    }
                    Action::RefreshAndRunAll { commands, .. } => {
                        assert!(
                            commands.iter().all(|command| !command
                                .argv()
                                .1
                                .iter()
                                .any(|arg| arg == "-bad")),
                            "небезопасное имя не попадает в команды пресета"
                        );
                    }
                    other => panic!("неожиданное действие с небезопасным именем: {other:?}"),
                }
            }
        }
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
        assert_eq!(format_scale_percent(1.25), "125 %");
        assert_eq!(format_scale_percent(1.2345), "123.45 %");
        assert_eq!(
            format!("{} (логический 1536×864)", format_scale_percent(1.25)),
            "125 % (логический 1536×864)"
        );
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
        assert!(matches!(
            power.kind,
            super::super::tree::NodeKind::Action(_)
        ));
        assert!(
            power.disabled,
            "единственный включённый вывод нельзя отключить"
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

    /// Второй включённый вывод получает подтверждение выключения.
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
        assert_eq!(
            power.title, "Turn off HDMI-A-1?",
            "второй включённый вывод требует подтверждения"
        );
        let super::super::tree::NodeKind::Submenu(confirm_children) = &power.kind else {
            panic!("питание должно быть подтверждением");
        };
        assert_eq!(confirm_children.len(), 2);
        assert!(confirm_children.iter().all(|row| !row.search));
        assert!(confirm_children.iter().any(|row| row.danger));
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

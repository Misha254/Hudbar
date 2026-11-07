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
//! списка. Сканирование не запускается автоматически: «Начать поиск» гоняет
//! `bluetoothctl --timeout 20 scan on` и обновляет список после него.
//! «Найденные» — это то, что BlueZ уже знает.
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

#[path = "bluetooth/actions.rs"]
mod actions;
#[path = "bluetooth/data.rs"]
mod data;
#[path = "bluetooth/view.rs"]
mod view;
pub(crate) use actions::*;
pub(crate) use data::*;
pub(crate) use view::*;

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

    /// Пустой список устройств — уровень из одного Info: открывается без
    /// курсора.
    #[test]
    fn bluetooth_empty_list_has_no_selection() {
        let snapshot = BluetoothSnapshot {
            adapter: Block::Ready(parse_adapter(SHOW)),
            known: Block::Ready(Vec::new()),
            paired: Block::Ready(Vec::new()),
            connected: Block::Ready(Vec::new()),
            infos: HashMap::new(),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let mut menu = super::super::state::Menu::new(nodes);
        let index = menu
            .current()
            .list
            .items()
            .iter()
            .position(|item| item.title == "Сопряжённые")
            .expect("список сопряжённых");
        assert!(menu.current_mut().list.select(index));
        assert_eq!(menu.enter(), super::super::state::Outcome::Pushed);
        assert_eq!(menu.current().list.items()[0].title, "Нет устройств");
        assert_eq!(menu.current().list.selected(), None);
    }

    /// Если статус адаптера — Info, корень раздела открывается без курсора,
    /// хотя ниже есть выбираемые списки.
    #[test]
    fn bluetooth_root_with_info_status_has_no_selection() {
        let snapshot = BluetoothSnapshot {
            adapter: Block::Failed("не отвечает".to_string()),
            ..BluetoothSnapshot::default()
        };
        let menu = super::super::state::Menu::new(build_nodes(&snapshot, Language::Ru));
        assert!(matches!(
            menu.current().list.items()[0].kind,
            super::super::item::ItemKind::Info
        ));
        assert_eq!(menu.current().list.selected(), None);
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

    /// Поиск — одна строка: выключен — запускает сканирование на 20 секунд,
    /// идёт или неизвестно — справка без команды, чужой процесс не трогаем.
    #[test]
    fn scan_row_starts_a_timed_scan_and_stays_plain_when_unknown() {
        let snapshot = |adapter| BluetoothSnapshot {
            adapter: Block::Ready(adapter),
            known: Block::Ready(Vec::new()),
            paired: Block::Ready(Vec::new()),
            connected: Block::Ready(Vec::new()),
            infos: HashMap::new(),
        };
        let scan = |node: &super::super::tree::Node| match &node.kind {
            super::super::tree::NodeKind::Action(Action::RefreshAndRun { command, .. }) => {
                command.argv().1
            }
            other => panic!("ожидалась команда поиска, {other:?}"),
        };
        let finding = SHOW.replace("Discovering: no", "Discovering: yes");
        let row = scan_row(Language::Ru, &snapshot(parse_adapter(&finding)));
        assert_eq!(row.title, "Поиск: идёт");
        assert!(
            matches!(row.kind, super::super::tree::NodeKind::Info),
            "идёт чужое сканирование — команду не предлагаем"
        );

        let off = scan_row(Language::Ru, &snapshot(parse_adapter(SHOW)));
        assert_eq!(off.title, "Начать поиск");
        assert_eq!(scan(&off), ["--timeout", "20", "scan", "on"]);

        let unknown = parse_adapter(SHOW_OFF);
        let row = scan_row(Language::Ru, &snapshot(unknown));
        assert_eq!(row.title, "Поиск: состояние неизвестно");
        assert!(
            matches!(row.kind, super::super::tree::NodeKind::Info),
            "без `Discovering` команду не предлагаем"
        );

        // И в разделе ровно одна строка про поиск, а не две.
        let nodes = build_nodes(&snapshot(parse_adapter(SHOW)), Language::Ru);
        let titles: Vec<&str> = nodes.iter().map(|node| node.title.as_str()).collect();
        let scans = titles.iter().filter(|title| title.contains("оиск")).count();
        assert_eq!(scans, 1, "строк про поиск ровно одна: {titles:?}");
    }

    /// Питание живёт на статусной строке: вкл — гасит, выкл — включает, а
    /// неизвестное состояние оставляет справкой без команды. Действовать по
    /// неизвестному нельзя, а «включить на всякий случай» — это оно и есть.
    #[test]
    fn status_row_switches_power_and_stays_plain_when_unknown() {
        let snapshot = |adapter| BluetoothSnapshot {
            adapter: Block::Ready(adapter),
            known: Block::Ready(Vec::new()),
            paired: Block::Ready(Vec::new()),
            connected: Block::Ready(Vec::new()),
            infos: HashMap::new(),
        };
        let power = |node: &super::super::tree::Node| match &node.kind {
            super::super::tree::NodeKind::Action(Action::RefreshAndRun { command, .. }) => {
                command.argv().1
            }
            other => panic!("ожидалась команда питания, {other:?}"),
        };

        let on = status_row(Language::Ru, &snapshot(parse_adapter(SHOW)));
        assert_eq!(on.title, "Bluetooth: вкл · hci0");
        assert_eq!(power(&on), ["power", "off"]);

        let off = status_row(Language::Ru, &snapshot(parse_adapter(SHOW_OFF)));
        assert_eq!(off.title, "Bluetooth: выкл · hci0");
        assert_eq!(power(&off), ["power", "on"]);

        let unknown = parse_adapter(SHOW_NO_POWER);
        assert_eq!(unknown.powered, None);
        let row = status_row(Language::Ru, &snapshot(unknown.clone()));
        assert_eq!(row.title, "Bluetooth: состояние неизвестно · hci0");
        assert!(
            matches!(row.kind, super::super::tree::NodeKind::Info),
            "по неизвестному состоянию команда не предлагается"
        );
        // И в собранном разделе отдельной кнопки питания больше нет.
        let nodes = build_nodes(&snapshot(unknown), Language::Ru);
        let titles: Vec<&str> = nodes.iter().map(|node| node.title.as_str()).collect();
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
        assert_eq!(argv_of(scan_action()), ["--timeout", "20", "scan", "on"]);
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

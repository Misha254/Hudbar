//! Динамический раздел «Wi-Fi»: провайдер поверх `nmcli`.
//!
//! Чтение — тот же конвейер W5.0, что у звука: worker -> [`CommandRunner`] ->
//! типизированный снимок -> строки меню. Запись — [`Action::RefreshAndRun`]:
//! команда уходит в worker цикла окна, после успеха слот WIFI мягко
//! перезапрашивается, а строка, из которой нажали, до этого момента
//! помечена «выполняется».
//!
//! Пароли вводятся через [`Action::SecretInput`](super::action::Action):
//! защищённая сеть без профиля открывает поле ввода, а после `Enter`
//! собирается `nmcli --ask device wifi connect <SSID>`, где пароль едет по
//! stdin (`SecretData`) — ни в `argv`, ни в журналах, ни в `Debug` его нет
//! (см. `secret`). Открытые сети и сети с сохранённым профилем подключаются
//! без ввода.
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
    CommandError, CommandRunner, CommandSpec, ProviderError, ProviderKey, SecretData,
    SystemCommandRunner, SystemProvider, provider_text,
};
use super::tree::Node;
use std::time::Duration;

#[path = "wifi/actions.rs"]
mod actions;
#[path = "wifi/data.rs"]
mod data;
#[path = "wifi/view.rs"]
mod view;
pub(crate) use actions::*;
pub(crate) use data::*;
pub(crate) use view::*;

#[cfg(test)]
mod tests {
    use super::super::action::{Action, RecordingRunner};
    use super::super::system::{CommandError, CommandOutput, ScriptedRunner};
    use super::*;

    const RADIO_ON: &str = "enabled\n";
    const DEVICES: &str = "wlp0s20f0u2:wifi:connected:Home 5G\nlo:loopback:connected (externally):lo\np2p-dev-wlp4s0:wifi-p2p:disconnected:\nwlp4s0:wifi:disconnected:\n";
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
            ok("Home 5G:uuid-honor:802-11-wireless\nMeta:uuid-meta:tun\n"),
            ok("802-11-wireless.ssid:Office-5g\n"),
        ]
    }

    /// Панель бара берёт отсюда UUID профилей: сеть с профилем подключается
    /// без пароля, даже если имя профиля не совпало с SSID.
    #[test]
    fn saved_profiles_are_keyed_by_ssid_and_fall_back_to_the_profile_name() {
        let runner = ScriptedRunner::new(vec![
            ok(
                "Home 5G:uuid-honor:802-11-wireless\nMeta:uuid-meta:tun\nOffice-5g:uuid-office:802-11-wireless\n",
            ),
            ok("802-11-wireless.ssid:Office-5g\n"),
        ]);

        let saved = saved_profile_uuids(&runner, &["Home 5G"]);

        assert_eq!(
            saved,
            vec![
                ("Home 5G".to_string(), "uuid-honor".to_string()),
                ("Office-5g".to_string(), "uuid-office".to_string()),
            ],
            "tun отброшен, SSID уточнён только у профиля без видимого имени"
        );
        assert_eq!(
            runner
                .calls()
                .iter()
                .filter(|call| call.starts_with("nmcli -t -f 802-11-wireless.ssid"))
                .count(),
            1,
            "для видимой сети лишнего вызова нет"
        );
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
        assert_eq!(device.connection.as_deref(), Some("Home 5G"));
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
            ok(":Office-5g:70:WPA2:AA:BB\n"),
            ok("Office-5g:uuid-office:802-11-wireless\n"),
            ok("802-11-wireless.ssid:Office-5g\n"),
        ]);
        let snapshot = WifiProvider.fetch(&runner).expect("снимок");
        let networks = snapshot.networks.ready().expect("сети есть");
        let office = networks.iter().find(|n| n.ssid == "Office-5g").unwrap();
        assert_eq!(office.saved_uuid.as_deref(), Some("uuid-office"));
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
            net_rows(&rows)
                .iter()
                .any(|row| row.title.contains("My:Net")),
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
        let up = connect_saved_action("uuid-office");
        let runner = RecordingRunner::new();
        up.perform_with(&runner).expect("выполнено");
        assert_eq!(runner.calls(), ["run nmcli connection up uuid uuid-office"]);
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
        let protected = net_rows(&rows)
            .into_iter()
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

    /// Подключение с паролем: пароля нет ни в `argv` (его не увидит `ps`),
    /// ни в журнале — он уезжает по stdin вместе с `--ask`.
    #[test]
    fn connect_with_password_keeps_the_password_out_of_every_text() {
        let spec = connect_with_password("Home 5G", "тихийпароль");
        let (program, args) = spec.argv();
        assert_eq!(program, "nmcli");
        assert_eq!(args, ["--ask", "device", "wifi", "connect", "Home 5G"]);
        assert!(
            !args.iter().any(|arg| arg == "-c" || arg.contains("sh -c")),
            "shell не используется"
        );
        assert_eq!(spec.stdin_bytes(), Some("тихийпароль".as_bytes()));
        let line = spec.log_line();
        assert!(line.contains("nmcli --ask device wifi connect"), "{line}");
        assert!(!line.contains("тихийпароль"), "пароль в log_line: {line}");
        assert!(line.contains("<stdin redacted>"), "{line}");
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
                vec!["--ask", "device", "wifi", "connect", "My:Network"],
            ),
            (
                "Дом \\ принтер",
                vec!["--ask", "device", "wifi", "connect", "Дом \\ принтер"],
            ),
            (
                "Home 5G",
                vec!["--ask", "device", "wifi", "connect", "Home 5G"],
            ),
            (
                "-dash-net",
                vec!["--ask", "device", "wifi", "connect", "--", "-dash-net"],
            ),
            (
                "Сеть Wi-Fi",
                vec!["--ask", "device", "wifi", "connect", "Сеть Wi-Fi"],
            ),
        ] {
            let spec = connect_with_password(ssid, "pw");
            let (_, args) = spec.argv();
            assert_eq!(args, expected, "SSID {ssid:?}");
            assert_eq!(spec.stdin_bytes(), Some("pw".as_bytes()), "SSID {ssid:?}");
            assert!(
                !spec.log_line().contains("pw") || ssid.contains("pw"),
                "{ssid:?}"
            );
        }
    }

    /// Пустой пароль не запрещаем: NetworkManager сам решит, допустим ли он.
    /// Важно, что он всё равно уезжает по stdin, а не пустым аргументом.
    #[test]
    fn empty_password_still_travels_by_stdin() {
        let spec = connect_with_password("open-ish", "");
        let (_, args) = spec.argv();
        assert_eq!(args, ["--ask", "device", "wifi", "connect", "open-ish"]);
        assert_eq!(spec.stdin_bytes(), Some(&[][..]));
        assert!(spec.has_secrets());
        assert!(
            spec.log_line().contains("<stdin redacted>"),
            "{}",
            spec.log_line()
        );
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
            net_rows(&rows)
                .into_iter()
                .map(|row| row.identity().to_string())
                .collect::<Vec<_>>()
        };
        let (first, second) = (ids(first), ids(second));
        assert_eq!(first, second);
        assert!(second.contains(&"wifi/net/My:Net".to_string()));
    }

    /// Сети раздела: подменю целиком, без обрезки.
    fn net_rows(rows: &[Node]) -> Vec<&Node> {
        let menu = rows
            .iter()
            .find(|row| row.identity() == "wifi/networks")
            .expect("подменю со списком сетей");
        match &menu.kind {
            super::super::tree::NodeKind::Submenu(children) => children.iter().collect(),
            other => panic!("ожидалось подменю, {other:?}"),
        }
    }

    /// Сети не мешаются в корне раздела: там только статус, подменю, ошибка
    /// и обновление. Раньше первая точка стояла вплотную к строке статуса, и
    /// отличать их приходилось по иконке.
    #[test]
    fn networks_live_in_a_submenu_and_not_in_the_root() {
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
        assert!(
            !rows
                .iter()
                .any(|row| row.identity().starts_with("wifi/net/")),
            "в корне не должно быть сетей: {:?}",
            rows.iter().map(|row| row.identity()).collect::<Vec<_>>()
        );
        let menu = rows
            .iter()
            .find(|row| row.identity() == "wifi/networks")
            .expect("подменю со списком сетей");
        let title = &menu.title;
        assert_eq!(title, "Доступные сети (20)");
        let super::super::tree::NodeKind::Submenu(children) = &menu.kind else {
            panic!("ожидалось подменю, {:?} ({title})", menu.kind);
        };
        assert_eq!(children.len(), 20, "в подменю все сети, без обрезки");
    }

    #[test]
    fn status_row_reads_radio_and_link() {
        let snapshot = WifiSnapshot {
            radio: Block::Ready(true),
            device: device_block(&Block::Ready(parse_devices(DEVICES))),
            active: Some(ActiveLink {
                device: "wlp0s20f0u2".to_string(),
                name: "Home 5G".to_string(),
                uuid: Some("uuid-honor".to_string()),
            }),
            networks: Block::Ready(Vec::new()),
        };
        let rows = build_nodes(&snapshot, Language::Ru);
        assert_eq!(rows[0].title, "Wi-Fi: вкл · Home 5G");
    }

    /// Раздел без адаптера — одна Info: курсора при открытии нет.
    #[test]
    fn wifi_info_status_level_has_no_selection() {
        let snapshot = WifiSnapshot {
            radio: Block::Ready(false),
            device: Block::Ready(WifiDevice::default()),
            active: None,
            networks: Block::Ready(Vec::new()),
        };
        let rows = build_nodes(&snapshot, Language::Ru);
        assert_eq!(rows.len(), 1);
        let menu = super::super::state::Menu::new(rows);
        assert_eq!(menu.current().list.items()[0].title, "Нет Wi-Fi адаптера");
        assert_eq!(menu.current().list.selected(), None);
    }
}

//! Действия пунктов меню: что именно произойдёт по Enter.
//!
//! В меню смешаны три разные сущности, и их важно не сливать в одну строку
//! команды. `Patch` меняет `settings.json` через `config_io` — там уже есть
//! точечная запись, которая не трогает чужие ключи. `Live` меняет состояние
//! системы: громкость, mute, DND, Wi-Fi. `Run` и `Spawn` запускают внешние
//! программы, `Niri` — действия композитора.
//!
//! Разница между `Run` и `Spawn` — в ожидании. Скрипт (`note.sh`, `calc.sh`)
//! запускается и ждёт: его ошибку есть смысл показать в подвале. Приложение
//! (`kitty`, `steam`) запускается и сразу отпускается: ждать запуска браузера
//! бессмысленно, а меню после него всё равно закрывается.
//!
//! Всё исполнение идёт через [`Runner`]: в живом окне это настоящие вызовы, в
//! тестах — записывающая заглушка. Поэтому тест может проверить, что пункт
//! мапится на нужную команду, не запуская ни одного процесса.

use super::super::config_io::Patch;
use super::super::log;
use super::super::settings::Module;
use super::system::{CommandOutput, CommandRunner, CommandSpec, ProviderKey};

/// Операция над состоянием системы.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    /// Переключить «не беспокоить».
    DndToggle,
    /// Переключить mute звука.
    VolMuteToggle,
    /// Переключить mute микрофона.
    MicMuteToggle,
    /// Включить или выключить Wi-Fi.
    WifiRadio(bool),
}

impl Live {
    /// Короткое имя операции: попадает в статус и в подсказку снапшота.
    pub fn label(&self) -> &'static str {
        match self {
            Live::DndToggle => "Не беспокоить",
            Live::VolMuteToggle => "Звук",
            Live::MicMuteToggle => "Микрофон",
            Live::WifiRadio(true) => "Wi-Fi вкл",
            Live::WifiRadio(false) => "Wi-Fi выкл",
        }
    }

    /// Читатель состояния для этого действия: возвращает `true`, если
    /// операция «включена». Нужен пункту меню, чтобы показать значение.
    pub fn read(&self) -> bool {
        match self {
            Live::DndToggle => super::super::data::oneshot::dnd_paused(),
            Live::VolMuteToggle => {
                super::super::data::oneshot::wpctl_volume("@DEFAULT_AUDIO_SINK@")
                    .is_some_and(|(_, muted)| muted)
            }
            Live::MicMuteToggle => {
                super::super::data::oneshot::wpctl_volume("@DEFAULT_AUDIO_SOURCE@")
                    .is_some_and(|(_, muted)| muted)
            }
            Live::WifiRadio(_) => super::super::data::oneshot::network_name() != "Нет сети",
        }
    }

    /// Аргументы команды, которая выполняет операцию. Разделено на чтение и
    /// запись: mute надо сначала прочитать, чтобы записать противоположное.
    fn command(&self) -> (&'static str, Vec<String>) {
        match self {
            Live::DndToggle => ("dunstctl", vec!["set-paused".into(), "toggle".into()]),
            Live::VolMuteToggle => (
                "wpctl",
                vec![
                    "set-mute".into(),
                    "@DEFAULT_AUDIO_SINK@".into(),
                    "toggle".into(),
                ],
            ),
            Live::MicMuteToggle => (
                "wpctl",
                vec![
                    "set-mute".into(),
                    "@DEFAULT_AUDIO_SOURCE@".into(),
                    "toggle".into(),
                ],
            ),
            Live::WifiRadio(on) => (
                "nmcli",
                vec![
                    "radio".into(),
                    "wifi".into(),
                    if *on { "on".into() } else { "off".into() },
                ],
            ),
        }
    }
}

/// Что делает пункт.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Точечная запись в `settings.json`.
    Patch(Patch),
    /// Операция над состоянием системы.
    Live(Live),
    /// Скрипт: запустить и дождаться результата.
    Run(&'static str, &'static [&'static str]),
    /// Приложение: запустить и отпустить.
    Spawn(&'static str, &'static [&'static str]),
    /// Действие niri: только аргументы, имя команды добавит исполнитель.
    Niri(&'static [&'static str]),
    /// Вернуться на уровень выше: строка «Отмена» в вопросе подтверждения.
    /// Команды нет и исполнителю она не достаётся: `Menu::enter` обрабатывает
    /// её сам, до разбора `ItemKind::Action`, поэтому прямой `perform` —
    /// no-op, как у ввода пароля.
    Back,
    /// Перезапуск панели.
    RestartHudbar,
    /// Сдвиг модуля в порядке панели: применяется через `ModuleOrder`.
    MoveModule { module: Module, delta: i32 },
    /// Системная команда с обновлением динамического раздела: выполнить,
    /// меню не закрывать, после успеха обновить слот `refresh`. Живые вызовы
    /// идут из worker-потока цикла окна (см. `system::spawn_fetch`): модель
    /// здесь только описывает команду, а не запускает потоки.
    RefreshAndRun {
        /// Что выполнить: программа, owned-аргументы, секретный stdin.
        command: CommandSpec,
        /// Чей слот обновить после успеха.
        refresh: ProviderKey,
    },
    /// Несколько системных команд подряд одним нажатием: так устроены
    /// пресеты раскладки дисплеев, где «только ноутбук» — это `off` для
    /// каждого внешнего вывода. Команды уходят в ту же очередь, что и
    /// `RefreshAndRun`, выполняются по порядку, срыв на первой ошибке.
    RefreshAndRunAll {
        /// Что выполнить, по порядку.
        commands: Vec<CommandSpec>,
        /// Чей слот обновить после успеха.
        refresh: ProviderKey,
    },
    /// Явное обновление динамического раздела из строки «Обновить».
    /// Выполняется только через `Menu::enter`: прямой `perform` — no-op,
    /// потому что слот живёт в меню, а не в исполнителе.
    RefreshDynamic(ProviderKey),
    /// Открыть ввод секрета для несекретной цели. Команда при этом не
    /// составляется и не запускается: пока пароль не введён, выполнять
    /// нечего. Дальше `Menu::submit_secret_input` собирает `CommandSpec` с
    /// `CmdArg::Secret` и кладёт её в ту же очередь, что и `RefreshAndRun`.
    SecretInput(super::secret::SecretTarget),
    /// Выбрать узел в группе `PROXY`. Отдельное действие, а не режим: режимы
    /// двигают два селектора, а узел — один, и имя узла приходит из снимка, а не
    /// из дерева.
    VpnNode(String),
    /// Переключить режим VPN: выбор двух селекторов у Mihomo. Это не команда
    /// и не патч файла, поэтому у исполнителя появился отдельный вызов
    /// [`Runner::set_selector`], а не ещё один `CommandSpec`.
    VpnMode(super::vpn::VpnMode),
}

impl Action {
    /// Короткое описание для подвала и статуса: не программистское, а то, что
    /// видит человек.
    pub fn describe(&self) -> String {
        match self {
            Action::Patch(patch) => patch.label(),
            Action::Live(live) => live.label().to_string(),
            Action::Run(program, _) | Action::Spawn(program, _) => (*program).to_string(),
            Action::Niri(args) => format!("niri {}", args.join(" ")),
            Action::Back => "Отмена".to_string(),
            Action::RestartHudbar => "Перезапуск панели".to_string(),
            Action::MoveModule { .. } => "порядок модулей".to_string(),
            Action::RefreshAndRun { command, .. } => command.program().to_string(),
            Action::RefreshAndRunAll { commands, .. } => commands
                .first()
                .map(CommandSpec::program)
                .unwrap_or_default()
                .to_string(),
            Action::RefreshDynamic(_) => "Обновить".to_string(),
            Action::SecretInput(target) => target.subject().to_string(),
            Action::VpnMode(mode) => mode.label(super::super::settings::Language::Ru).to_string(),
            Action::VpnNode(name) => name.clone(),
        }
    }

    /// Режим VPN по действию: нужно тесту, чтобы проверить, что в дереве стоит
    /// именно выбранный режим.
    #[allow(dead_code)]
    pub fn vpn_mode(&self) -> Option<super::vpn::VpnMode> {
        match self {
            Action::VpnMode(mode) => Some(*mode),
            _ => None,
        }
    }

    /// Имя узла по действию: нужно тесту, чтобы проверить, что в строке списка
    /// стоит именно то название, которое пришло из снимка.
    #[allow(dead_code)]
    pub fn vpn_node(&self) -> Option<&str> {
        match self {
            Action::VpnNode(node) => Some(node),
            _ => None,
        }
    }

    /// Имя внешней команды для теста: у `Run` и `Spawn` это имя программы, у
    /// `Niri` — первое слово аргументов. Пусто у `Patch` и `Live`, потому что
    /// там команда не нужна.
    pub fn command(&self) -> Option<&'static str> {
        match self {
            Action::Patch(_) | Action::Live(_) | Action::RestartHudbar => None,
            Action::Run(program, _) | Action::Spawn(program, _) => Some(program),
            Action::Niri(args) => args.first().copied(),
            // Возврат уровня решает меню, а не исполнитель: программы нет.
            Action::Back => None,
            Action::MoveModule { .. } => None,
            // Команда динамическая (owned-строки, не `&'static str`), а
            // обновление — через меню: статическое имя здесь нечего отдать.
            Action::RefreshAndRun { .. }
            | Action::RefreshAndRunAll { .. }
            | Action::RefreshDynamic(_)
            | Action::SecretInput(_)
            | Action::VpnMode(_)
            | Action::VpnNode(_) => None,
        }
    }

    /// Закрывать ли меню после действия. Приложения, снимки и обои закрывают:
    /// за ними всё равно надо переключиться, а `wall.sh` ещё и долго
    /// применяется. Скрипты и настройки — нет: человек продолжает выбирать.
    pub fn closes_menu(&self) -> bool {
        match self {
            Action::Patch(_) | Action::Live(_) => false,
            Action::Run(_, _) => false,
            Action::Spawn(_, _) | Action::Niri(_) | Action::RestartHudbar => true,
            // Возврат уровня меню не закрывает: это «Отмена», а не выход.
            Action::Back => false,
            Action::MoveModule { .. } => false,
            // Динамические операции меню не закрывают: человек остаётся в
            // разделе и видит обновлённый список.
            Action::RefreshAndRun { .. }
            | Action::RefreshAndRunAll { .. }
            | Action::RefreshDynamic(_) => false,
            // Ввод пароля оставляет меню открытым на том же разделе.
            Action::SecretInput(_) => false,
            // Смена режима и выбор узла — как и другие системные операции:
            // человек остаётся в разделе и видит, что получилось.
            Action::VpnMode(_) | Action::VpnNode(_) => false,
        }
    }

    /// Исполняет действие. В живой программе — настоящие вызовы.
    pub fn perform(&self) -> Result<(), String> {
        self.perform_with(&SystemRunner)
    }

    /// Исполняет действие заданным исполнителем. Тесты подставляют
    /// записывающий, поэтому ни одна команда не запускается.
    pub fn perform_with(&self, runner: &dyn Runner) -> Result<(), String> {
        match self {
            Action::Patch(patch) => runner.save(patch),
            Action::Live(live) => {
                let (program, args) = live.command();
                let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
                runner.status(program, &borrowed)
            }
            Action::Run(program, args) => runner.status(program, args),
            Action::Spawn(program, args) => runner.spawn(program, args),
            Action::Niri(args) => runner.status("niri", args),
            // Уровень убирает меню: у исполнителя нет стека уровней, и
            // запускать здесь нечего.
            Action::Back => Ok(()),
            Action::RestartHudbar => runner.restart_hudbar(),
            Action::MoveModule { module, delta } => {
                let order = super::values::moved_order(*module, *delta);
                runner.save(&Patch::ModuleOrder(order))
            }
            Action::RefreshAndRun { command, .. } => runner.run_command(command).map(|_| ()),
            // Прямой вызов мимо worker-потока: команды идут по порядку, и
            // первая ошибка останавливает цепочку — иначе следующий вызов
            // опирался бы на несделанное.
            Action::RefreshAndRunAll { commands, .. } => {
                for command in commands {
                    runner.run_command(command)?;
                }
                Ok(())
            }
            // Обновление выполняет `Menu::enter`: у исполнителя нет доступа
            // к слотам, поэтому прямой вызов — честный no-op.
            Action::RefreshDynamic(_) => Ok(()),
            // Секрет не умеет исполнитель: пароля у него нет, а собирает
            // команду модель из буфера ввода.
            Action::SecretInput(_) => Ok(()),
            // Сначала `MODE-RU`, потом `MODE-REST`: между двумя записями трафик
            // живёт в промежуточном состоянии, и порядок задаёт модель, а не
            // исполнитель. Срыв на первой ошибке — вторая запись опиралась бы
            // на несделанную первую.
            Action::VpnMode(mode) => {
                for (name, value) in mode.selectors() {
                    runner.set_selector(name, value)?;
                }
                Ok(())
            }
            Action::VpnNode(node) => runner.set_selector(super::vpn::PROXY_GROUP, node),
        }
    }
}

/// Исполнитель действия. Живой — системный, тестовый — записывающий.
pub trait Runner {
    /// Точечная запись в `settings.json`.
    fn save(&self, patch: &Patch) -> Result<(), String>;
    /// Команда, результат которой ждём.
    fn status(&self, program: &str, args: &[&str]) -> Result<(), String>;
    /// Приложение: запустить и отпустить.
    fn spawn(&self, program: &str, args: &[&str]) -> Result<(), String>;
    /// Перезапуск панели: так окно настроек это делает, теми же средствами.
    fn restart_hudbar(&self) -> Result<(), String>;
    /// Системная команда из `CommandSpec`: аргументы без shell, секретный
    /// stdin, timeout внутри. По умолчанию не поддерживается — так старые
    /// записывающие исполнители не ломаются, пока им это не понадобится.
    fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput, String> {
        let _ = spec;
        Err("исполнитель не умеет запускать команды".to_string())
    }

    /// Сменить выбор селектора у VPN-контроллера. Отдельный вызов, потому
    /// что это HTTP, а не процесс: собирать его в `CommandSpec` пришлось бы
    /// оборачивать в `curl` и класть секрет в `argv`.
    fn set_selector(&self, name: &str, value: &str) -> Result<(), String> {
        let _ = (name, value);
        Err("исполнитель не умеет менять селекторы".to_string())
    }
}

/// Настоящий исполнитель. Всё идёт через `log`, поэтому команды пишутся в
/// журнал, а секреты в аргументах редактируются перед записью.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn save(&self, patch: &Patch) -> Result<(), String> {
        // Тема пишется вместе с файлом шрифта: иначе JSON сказал бы «pixel»,
        // а painter остался бы на обычном шрифте.
        if let Patch::Theme(theme) = patch {
            return super::super::config_io::apply_theme(*theme)
                .map(|_| ())
                .map_err(|error| error.to_string());
        }
        super::super::config_io::apply_patch(patch)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn status(&self, program: &str, args: &[&str]) -> Result<(), String> {
        if log::status(program, args) {
            Ok(())
        } else {
            Err(format!("{program} вернул ошибку"))
        }
    }

    fn spawn(&self, program: &str, args: &[&str]) -> Result<(), String> {
        super::exec::spawn_detached(program, args)
    }

    fn restart_hudbar(&self) -> Result<(), String> {
        super::exec::restart_hudbar()
    }

    fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput, String> {
        super::system::SystemCommandRunner::with_timeout(spec.timeout())
            .run(spec)
            .map_err(|error| error.to_string())
    }

    fn set_selector(&self, name: &str, value: &str) -> Result<(), String> {
        let client = super::vpn::controller();
        client.put(
            &super::vpn_api::selector_path(name),
            &super::vpn_api::select_body(value),
        )
    }
}

/// Точечная запись в `settings.json` из окна: тот же путь, что у панели.
pub fn patch_module(module: Module, on: bool) -> Action {
    Action::Patch(Patch::ModuleVisible { module, on })
}

/// Перезапуск панели одной кнопкой.
pub fn restart_hudbar() -> Action {
    Action::RestartHudbar
}

/// Заглушка для тестов: ничего не выполняет, всё записывает. Записи
/// копятся внутри, потому что `Runner` работает по `&self`.
#[derive(Debug, Default)]
pub struct RecordingRunner {
    calls: std::cell::RefCell<Vec<String>>,
}

impl RecordingRunner {
    /// Пустая записная книжка: вызовы накапливаются в [`RecordingRunner::calls`].
    pub fn new() -> Self {
        RecordingRunner::default()
    }

    /// Что было вызвано: `save высота: 32 px`, `status wall.sh --set`.
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Последний вызов — то, что обычно и проверяется.
    pub fn last(&self) -> Option<String> {
        self.calls.borrow().last().cloned()
    }

    /// Разделяет команду и аргументы пробелом — читаемо в ассертах.
    fn line(kind: &str, program: &str, args: &[&str]) -> String {
        if args.is_empty() {
            format!("{kind} {program}")
        } else {
            format!("{kind} {program} {}", args.join(" "))
        }
    }

    fn push(&self, line: String) {
        self.calls.borrow_mut().push(line);
    }
}

impl Runner for RecordingRunner {
    fn save(&self, patch: &Patch) -> Result<(), String> {
        // Файл не трогаем: тесты проверяют только маппинг.
        self.push(format!("save {}", patch.label()));
        Ok(())
    }

    fn status(&self, program: &str, args: &[&str]) -> Result<(), String> {
        self.push(RecordingRunner::line("status", program, args));
        Ok(())
    }

    fn spawn(&self, program: &str, args: &[&str]) -> Result<(), String> {
        self.push(RecordingRunner::line("spawn", program, args));
        Ok(())
    }

    fn restart_hudbar(&self) -> Result<(), String> {
        self.push("restart hudbar".to_string());
        Ok(())
    }

    fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput, String> {
        // Пишется отредактированная строка: секрет из argv сюда не попадает.
        self.push(format!("run {}", spec.log_line()));
        Ok(CommandOutput::empty_success())
    }

    fn set_selector(&self, name: &str, value: &str) -> Result<(), String> {
        self.push(format!("selector {name}={value}"));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::config::Theme;
    use super::*;

    /// Каждое действие должно мапиться ровно на свою команду. Проверяется
    /// записывающим исполнителем: ни один процесс не запускается.
    #[test]
    fn actions_map_to_their_commands() {
        let cases: Vec<(Action, &str)> = vec![
            (Action::Patch(Patch::Height(32)), "save высота: 32 px"),
            (patch_module(Module::Dnd, false), "save dnd: выкл"),
            (
                Action::Patch(Patch::Notifications {
                    font_size: Some(15),
                    line_height: None,
                    position: None,
                }),
                "save уведомления",
            ),
            (
                Action::Live(Live::DndToggle),
                "status dunstctl set-paused toggle",
            ),
            (
                Action::Live(Live::VolMuteToggle),
                "status wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle",
            ),
            (
                Action::Live(Live::MicMuteToggle),
                "status wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle",
            ),
            (
                Action::Live(Live::WifiRadio(true)),
                "status nmcli radio wifi on",
            ),
            (
                Action::Live(Live::WifiRadio(false)),
                "status nmcli radio wifi off",
            ),
            (
                Action::Run("wall.sh", &["--random"]),
                "status wall.sh --random",
            ),
            (
                Action::Spawn("kitty", &["-e", "yazi"]),
                "spawn kitty -e yazi",
            ),
            (
                Action::Niri(&["msg", "action", "screenshot", "interactive"]),
                "status niri msg action screenshot interactive",
            ),
            (restart_hudbar(), "restart hudbar"),
            (
                Action::MoveModule {
                    module: Module::Audio,
                    delta: 1,
                },
                "save порядок модулей",
            ),
        ];
        for (action, expected) in cases {
            let runner = RecordingRunner::default();
            action.perform_with(&runner).expect("выполнено");
            assert_eq!(
                runner.last().as_deref(),
                Some(expected),
                "{action:?} вызвала не то"
            );
            assert_eq!(runner.calls().len(), 1, "{action:?} зовёт один раз");
        }
    }

    #[test]
    fn patch_is_written_through_config_io() {
        let runner = RecordingRunner::default();
        Action::Patch(Patch::Theme(Theme::Pixel))
            .perform_with(&runner)
            .expect("тема записана");
        assert_eq!(runner.calls(), vec!["save тема: pixel".to_string()]);
    }

    #[test]
    fn module_visibility_patches_are_available_for_the_tree() {
        let runner = RecordingRunner::default();
        patch_module(Module::Weather, true)
            .perform_with(&runner)
            .expect("модуль переключён");
        assert_eq!(runner.calls(), vec!["save weather: вкл".to_string()]);
    }

    /// Скрипт ждёт результата, приложение — нет. От этого зависит, остаётся ли
    /// меню открытым и показывается ли ошибка.
    #[test]
    fn scripts_wait_and_apps_close_the_menu() {
        assert!(!Action::Run("note.sh", &[]).closes_menu());
        assert!(!Action::Run("wall.sh", &["--random"]).closes_menu());
        assert!(Action::Spawn("kitty", &[]).closes_menu());
        assert!(Action::Spawn("flatpak", &["run", "x"]).closes_menu());
        assert!(Action::Niri(&["msg", "action", "quit"]).closes_menu());
        assert!(Action::RestartHudbar.closes_menu());
        assert!(
            !Action::MoveModule {
                module: Module::Audio,
                delta: 1
            }
            .closes_menu()
        );
        assert!(!Action::Patch(Patch::Height(32)).closes_menu());
        assert!(!Action::Live(Live::DndToggle).closes_menu());
    }

    #[test]
    fn descriptions_are_human_readable() {
        assert_eq!(Action::Live(Live::DndToggle).describe(), "Не беспокоить");
        assert_eq!(Action::Run("wall.sh", &["--random"]).describe(), "wall.sh");
        assert_eq!(
            Action::Niri(&["msg", "action", "screenshot", "screen"]).describe(),
            "niri msg action screenshot screen"
        );
        assert_eq!(restart_hudbar().describe(), "Перезапуск панели");
    }

    #[test]
    fn command_names_the_binary_for_external_actions() {
        assert_eq!(Action::Run("note.sh", &[]).command(), Some("note.sh"));
        assert_eq!(Action::Spawn("steam", &[]).command(), Some("steam"));
        assert_eq!(Action::Niri(&["msg", "action"]).command(), Some("msg"));
        assert_eq!(Action::Live(Live::DndToggle).command(), None);
        assert_eq!(Action::Patch(Patch::Height(1)).command(), None);
        assert_eq!(
            Action::MoveModule {
                module: Module::Audio,
                delta: 1
            }
            .command(),
            None
        );
        assert_eq!(restart_hudbar().command(), None);
    }

    fn refresh_action() -> Action {
        Action::RefreshAndRun {
            command: super::super::system::CommandSpec::new("wpctl")
                .arg("set-volume")
                .arg("@DEFAULT_AUDIO_SINK@")
                .arg("0.42"),
            refresh: super::super::system::ProviderKey::AUDIO,
        }
    }

    /// Динамические операции меню не закрывают и статического имени команды
    /// не имеют: аргументы owned, а не `&'static str`.
    #[test]
    fn dynamic_actions_keep_the_menu_open() {
        assert!(!refresh_action().closes_menu());
        assert!(!Action::RefreshDynamic(super::super::system::ProviderKey::AUDIO).closes_menu());
        assert_eq!(refresh_action().command(), None);
        assert_eq!(
            Action::RefreshDynamic(super::super::system::ProviderKey::AUDIO).command(),
            None
        );
        assert_eq!(refresh_action().describe(), "wpctl");
        assert_eq!(
            Action::RefreshDynamic(super::super::system::ProviderKey::AUDIO).describe(),
            "Обновить"
        );
    }

    #[test]
    fn vpn_actions_write_the_expected_selectors_in_order() {
        use super::super::vpn::{PROXY, PROXY_GROUP, SELECTOR_REST, SELECTOR_RU, VpnMode};

        let runner = RecordingRunner::default();
        Action::VpnMode(VpnMode::All)
            .perform_with(&runner)
            .expect("режим применён");
        assert_eq!(
            runner.calls(),
            [
                format!("selector {SELECTOR_RU}={PROXY}"),
                format!("selector {SELECTOR_REST}={PROXY}")
            ]
        );

        let runner = RecordingRunner::default();
        let node = "[Q] 🇺🇸 США, Атланта";
        Action::VpnNode(node.to_string())
            .perform_with(&runner)
            .expect("узел выбран");
        assert_eq!(runner.calls(), [format!("selector {PROXY_GROUP}={node}")]);
    }

    #[test]
    fn vpn_actions_keep_the_window_open() {
        assert!(!Action::VpnMode(super::super::vpn::VpnMode::NoRu).closes_menu());
        assert!(!Action::VpnNode("узел".to_string()).closes_menu());
    }

    /// Команда уходит исполнителю целиком, в записи — без секретов.
    #[test]
    fn refresh_and_run_records_the_redacted_command() {
        let runner = RecordingRunner::default();
        Action::RefreshAndRun {
            command: super::super::system::CommandSpec::new("nmcli")
                .arg("connect")
                .secret_arg("s3cr3t"),
            refresh: super::super::system::ProviderKey::WIFI,
        }
        .perform_with(&runner)
        .expect("команда записана");
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].starts_with("run nmcli"), "не та запись: {calls:?}");
        assert!(!calls[0].contains("s3cr3t"), "секрет в записи: {calls:?}");
    }

    /// Пресет — это несколько команд подряд, и порядок сохраняется: команды
    /// выполняются все, пока не встретится отказ.
    #[test]
    fn several_commands_run_in_order_and_stop_at_the_first_refusal() {
        // Команды собираем как настоящие: имя вывода и действие — отдельные
        // аргументы, иначе `log_line` возьмёт их в кавычки как одно слово.
        let spec = |name: &str, action: &str| {
            super::super::system::CommandSpec::new("niri")
                .arg("msg")
                .arg("output")
                .arg(name)
                .arg(action)
        };
        let action = || Action::RefreshAndRunAll {
            commands: vec![spec("eDP-1", "on"), spec("HDMI-A-1", "off")],
            refresh: super::super::system::ProviderKey::OUTPUTS,
        };
        let runner = RecordingRunner::default();
        action()
            .perform_with(&runner)
            .expect("все команды выполнены");
        assert_eq!(
            runner.calls(),
            [
                "run niri msg output eDP-1 on",
                "run niri msg output HDMI-A-1 off"
            ]
        );

        assert!(!action().closes_menu(), "меню остаётся открытым");
        assert_eq!(action().describe(), "niri", "описание — по первой команде");
        assert_eq!(action().command(), None, "имя команды статическое — нет");

        // Команда, которая не выполнилась, останавливает цепочку: следующая
        // опиралась бы на несделанное.
        let runner = RefusingRunner::new();
        let refused = action().perform_with(&runner);
        assert_eq!(refused, Err("niri не ответил".to_string()));
        assert_eq!(
            runner.calls(),
            1,
            "вторая команда не должна была даже запускаться"
        );
    }

    /// Исполнитель, который отказывает на первой же команде и считает
    /// попытки: им проверяется, что цепочка пресета рвётся, а не идёт
    /// дальше по инерции.
    struct RefusingRunner {
        calls: std::cell::Cell<u32>,
    }

    impl RefusingRunner {
        fn new() -> Self {
            RefusingRunner {
                calls: std::cell::Cell::new(0),
            }
        }

        fn calls(&self) -> u32 {
            self.calls.get()
        }
    }

    impl Runner for RefusingRunner {
        fn save(&self, _patch: &Patch) -> Result<(), String> {
            Ok(())
        }

        fn status(&self, _program: &str, _args: &[&str]) -> Result<(), String> {
            Ok(())
        }

        fn spawn(&self, _program: &str, _args: &[&str]) -> Result<(), String> {
            Ok(())
        }

        fn restart_hudbar(&self) -> Result<(), String> {
            Ok(())
        }

        fn run_command(&self, _spec: &CommandSpec) -> Result<CommandOutput, String> {
            self.calls.set(self.calls.get() + 1);
            Err("niri не ответил".to_string())
        }
    }

    /// Обновление через меню — no-op для прямого вызова: слоты недоступны
    /// исполнителю, выполняет только `Menu::enter`.
    #[test]
    fn refresh_dynamic_direct_call_is_a_noop() {
        let runner = RecordingRunner::default();
        Action::RefreshDynamic(super::super::system::ProviderKey::AUDIO)
            .perform_with(&runner)
            .expect("no-op успешен");
        assert!(runner.calls().is_empty(), "исполнитель ничего не писал");
    }
}

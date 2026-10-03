//! Действия пунктов меню: что именно произойдёт по Enter.
//!
//! В меню смешаны три разные сущности, и их важно не сливать в одну строку
//! команды. `Patch` меняет `settings.json` через `config_io` — там уже есть
//! точечная запись, которая не трогает чужие ключи. `Live` меняет состояние
//! панели (том, сеть, DND) — там уже есть функции в `actions`. `Run` и `Niri`
//! запускают внешние программы.
//!
//! Поэтому здесь только типы и описание действия для подвала. Реальные вызовы
//! появятся в M4, когда появится окно: `perform` намеренно ничего не делает и
//! честно сообщает об этом, чтобы в M2 статус «ПРИМЕНЕНО» был нарисован
//! тестовым деревом, а не выдуманным успехом.

use super::config_io::Patch;

/// Действие над живым состоянием панели. Значения панели живут в
/// `data::Shared`, а меняются функциями из `hud::actions`, поэтому пункт
/// называет операцию, а не команду оболочки.
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

    /// Имя функции `hud::actions`, которая выполнит операцию в M4. Нужно для
    /// теста: он проверяет, что каждый вариант куда-то ведёт, иначе пункт
    /// останется пустым навсегда.
    pub fn action_fn(&self) -> &'static str {
        match self {
            Live::DndToggle => "actions::dnd_toggle",
            Live::VolMuteToggle => "actions::vol_mute_toggle",
            Live::MicMuteToggle => "actions::mic_mute_toggle",
            Live::WifiRadio(_) => "data::wifi_radio",
        }
    }
}

/// Что делает пункт.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Точечная запись в `settings.json`.
    Patch(Patch),
    /// Операция над состоянием панели.
    Live(Live),
    /// Запуск программы: имя и аргументы.
    Run(&'static str, &'static [&'static str]),
    /// Действие niri: только аргументы, имя команды добавит исполнитель.
    Niri(&'static [&'static str]),
}

impl Action {
    /// Короткое описание для подвала и статуса: не программистское, а то, что
    /// видит человек.
    pub fn describe(&self) -> String {
        match self {
            Action::Patch(patch) => patch.label(),
            Action::Live(live) => live.label().to_string(),
            Action::Run(program, _) => (*program).to_string(),
            Action::Niri(args) => format!("niri {}", args.join(" ")),
        }
    }

    /// Имя внешней команды для теста: у `Run` это имя программы, у `Niri` —
    /// первое слово аргументов. Пусто у `Patch` и `Live`, потому что там
    /// команда не нужна.
    pub fn command(&self) -> Option<&'static str> {
        match self {
            Action::Patch(_) | Action::Live(_) => None,
            Action::Run(program, _) => Some(program),
            Action::Niri(args) => args.first().copied(),
        }
    }

    /// Заглушка исполнения. В M1 она всегда возвращает ошибку с названием
    /// Milestone: окна ещё нет, запускать нечего. Когда появится M4, здесь
    /// будет вызов `config_io::apply_patch`, `actions::*` и `log::status`.
    pub fn perform(&self) -> Result<(), String> {
        Err(format!(
            "«{}» пока не выполняется: живое окно появится в M4",
            self.describe()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::settings::Language;
    use super::super::settings_ui::Module;
    use super::*;

    #[test]
    fn patch_describes_the_changed_field() {
        let action = Action::Patch(Patch::Language(Language::Ru));
        assert_eq!(action.describe(), "язык: ru");
        assert_eq!(action.command(), None);
    }

    #[test]
    fn live_operations_name_a_real_action() {
        for live in [
            Live::DndToggle,
            Live::VolMuteToggle,
            Live::MicMuteToggle,
            Live::WifiRadio(true),
        ] {
            assert!(
                live.action_fn().starts_with("actions::") || live.action_fn().starts_with("data::"),
                "{live:?} не ссылается на живую функцию"
            );
        }
        assert_eq!(Live::DndToggle.label(), "Не беспокоить");
    }

    #[test]
    fn run_and_niri_expose_their_command() {
        let run = Action::Run("wall.sh", &["--set"]);
        assert_eq!(run.command(), Some("wall.sh"));
        assert_eq!(run.describe(), "wall.sh");

        let niri = Action::Niri(&["msg", "action", "focus-workspace", "1"]);
        assert_eq!(niri.command(), Some("msg"));
        assert_eq!(niri.describe(), "niri msg action focus-workspace 1");
    }

    #[test]
    fn module_visibility_patch_is_available_for_the_model() {
        let patch = Patch::ModuleVisible {
            module: Module::Dnd,
            on: true,
        };
        let action = Action::Patch(patch);
        assert!(action.describe().contains("вкл"));
    }

    /// Заглушка обязана сообщать, что ничего не сделано: иначе M2 нарисует
    /// «ПРИМЕНЕНО» по-настоящему и тест будет врать.
    #[test]
    fn perform_is_an_explicit_stub() {
        let err = Action::Live(Live::DndToggle).perform().unwrap_err();
        assert!(
            err.contains("M4"),
            "заглушка должна называть Milestone: {err}"
        );
    }
}

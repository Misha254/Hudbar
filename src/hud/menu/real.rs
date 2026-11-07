//! Настоящее дерево меню: разделы, значения и действия из системы.
//!
//! В отличие от `demo_tree`, здесь нет фиктивных значений: каждый тумблер
//! читает `settings.json` или разовый опрос, каждое действие мапится на
//! `Patch`, системную команду, скрипт или приложение. Пункты, чей скрипт не
//! установлен, скрываются условием `visible`, а не висят мёртвым грузом.
//!
//! Почти все скрипты приложений открывают rofi, а меню держит клавиатуру в
//! `Exclusive`: ждать их (`Run`) значило бы повесить и меню, и rofi. Поэтому
//! скрипты с интерфейсом запускаются откреплённо (`Spawn`) с закрытием меню,
//! а `Run` остаётся быстрым неинтерактивным командам (`wall.sh --random`,
//! переключатели без внешней команды): меню ждёт завершения, но остаётся
//! открытым и показывает статус.

use super::super::data::oneshot;
use super::super::schemes;
use super::action::Action;
use super::settings::{
    ANIMATION_SPEED_MAX, ANIMATION_SPEED_MIN, IDLE_SCREEN_OFF_MAX_SECONDS,
    IDLE_SCREEN_OFF_MIN_SECONDS, IDLE_SUSPEND_MAX_SECONDS, IDLE_SUSPEND_MIN_SECONDS, Language,
    Module, OSD_DURATION_MAX_MS, OSD_DURATION_MIN_MS,
};
use super::settings_icons;
use super::state::Menu;
use super::system::ProviderKey;
use super::tree::Node;
use super::values;

/// Версия пакета для строки «Версия» раздела «Hudbar».
const VERSION: &str = env!("CARGO_PKG_VERSION");

const LANG_OPTS: [&str; 2] = ["Русский", "English"];
const POS_RU: [&str; 4] = [
    "Слева сверху",
    "Справа сверху",
    "Слева снизу",
    "Справа снизу",
];
const POS_EN: [&str; 4] = ["Top left", "Top right", "Bottom left", "Bottom right"];

#[path = "real/actions.rs"]
mod actions;
#[path = "real/data.rs"]
mod data;
#[path = "real/sections.rs"]
mod sections;
pub(crate) use actions::*;
pub(crate) use data::*;
pub(crate) use sections::*;

/// Настоящее дерево на указанном языке. Порядок видимого корня повторяет
/// `SECTIONS`: сначала настройки и система, потом инструменты и справка.
pub fn tree_in(lang: Language) -> Vec<Node> {
    vec![
        apps_section(lang),
        panel_section(lang),
        style_section(lang),
        notifications_section(lang),
        language_row(lang),
        system_section(lang),
        power_section(lang).when(power_hidden),
        capture_section(lang),
        keybinds_section(lang),
        about_section(lang),
    ]
}

/// Настоящее дерево на текущем языке из `settings.json`.
pub fn tree() -> Vec<Node> {
    tree_in(super::super::settings::load().language)
}

/// Готовое меню на настоящем дереве и текущем языке.
pub fn menu() -> Menu {
    let lang = super::super::settings::load().language;
    let mut menu = Menu::new(tree_in(lang));
    menu.set_lang(lang);
    menu
}

#[cfg(test)]
mod tests {
    use super::super::state::{self, Outcome};
    use super::*;

    /// «Конфиги» — подменю с целями, а не запуск второго лаунчера.
    /// Цели здесь свои: настоящий список зависит от того, какие конфиги лежат
    /// на машине, и на чистой машине такой тест не проверял бы ничего.
    #[test]
    fn control_panel_keeps_only_targets_that_exist_on_disk() {
        let existing = std::env::current_exe().expect("путь тестового бинаря");
        let missing = std::env::temp_dir().join(format!(
            "hudbar-control-target-missing-{}",
            std::process::id()
        ));
        let targets = vec![
            (
                settings_icons::SYSTEM,
                "niri",
                existing.to_string_lossy().into_owned(),
            ),
            (
                settings_icons::PANEL,
                "waybar",
                missing.to_string_lossy().into_owned(),
            ),
        ];
        let control = control_node_with(Language::Ru, targets).expect("цель есть");
        assert_eq!(control.title, "Конфиги");
        let super::super::tree::NodeKind::Submenu(children) = &control.kind else {
            panic!("ожидалось подменю, {:?}", control.kind);
        };
        assert_eq!(children.len(), 1, "строка без пути на диске не строится");
        let child = &children[0];
        let super::super::tree::NodeKind::Action(Action::Spawn(program, args)) = &child.kind else {
            panic!("строка должна запускать редактор: {:?}", child.kind);
        };
        assert_eq!(*program, "kitty");
        assert_eq!(args[1], "nvim", "открываем nvim");
        assert_eq!(std::path::Path::new(args[2]), existing);
        assert_eq!(
            child.identity(),
            format!(
                "control/{}",
                existing.file_name().unwrap_or_default().to_string_lossy()
            )
        );
    }

    /// Цели не должны повторяться: два пункта на один и тот же путь — это
    /// шум, а не удобство.
    #[test]
    fn control_targets_are_unique_and_exist_on_this_machine() {
        let mut paths: Vec<String> = targets().into_iter().map(|(_, _, path)| path).collect();
        let count = paths.len();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), count, "пути повторяются: {paths:?}");
        assert!(
            count >= 8,
            "ожидался рабочий набор целей, а не {count}: {paths:?}"
        );
        assert!(
            !paths.iter().any(|path| path.contains("noctalia")),
            "noctalia из списка ушёл: {paths:?}"
        );
    }

    #[test]
    fn real_tree_is_valid() {
        for lang in [Language::Ru, Language::En] {
            assert!(state::validate(&tree_in(lang)).is_ok());
        }
    }

    /// Поиск строки по идентификатору: у прямых детей или внутри групп
    /// глубиной один — Wi-Fi, Bluetooth и прочие сейчас живут в подменю
    /// «Связь» и «Устройства».
    fn find_by_id<'a>(children: &'a [Node], id: &str) -> &'a Node {
        for node in children {
            if node.identity() == id {
                return node;
            }
            if let Some(kids) = node.children()
                && let Some(found) = kids.iter().find(|inner| inner.identity() == id)
            {
                return found;
            }
        }
        panic!("нет строки {id}");
    }

    /// Раздел «Звук» — динамический вход с ключом AUDIO и устойчивой
    /// личностью: по ней путь и курсор восстанавливаются после пересборки
    /// дерева. Проверка на дереве, а не на видимости: наличие wpctl/pactl
    /// зависит от машины.
    #[test]
    fn system_section_has_the_audio_entry_by_key() {
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            let audio = children
                .iter()
                .find(|node| node.identity() == "system/audio")
                .expect("вход в «Звук»");
            assert!(
                matches!(audio.kind, super::super::tree::NodeKind::Dynamic { key } if key == ProviderKey::AUDIO),
                "вход динамический и с ключом AUDIO"
            );
            let (audio_title, system_title) = match lang {
                Language::Ru => ("Звук", "Система"),
                Language::En => ("Audio", "System"),
            };
            let expected = audio_title;
            assert_eq!(audio.title, expected);
            assert!(
                audio.visible.is_some(),
                "видимость входа задана условием has_audio"
            );

            let mut menu = Menu::new(tree_in(lang));
            let index = menu
                .frame()
                .items
                .iter()
                .position(|item| item.title == system_title)
                .expect("раздел «Система» на корне");
            menu.current_mut().list.select(index);
            assert_eq!(menu.enter(), Outcome::Pushed);
            let frame = menu.frame();
            let visible = frame.items.iter().any(|item| item.title == expected);
            assert_eq!(
                visible,
                has_audio(),
                "видимость входа обязана совпадать с наличием wpctl/pactl"
            );
        }
    }

    /// Три настройки энергии собраны в одно подменю «Питание»: простой
    /// экрана, сон и профиль. В самом разделе «Система» их строк больше нет —
    /// иначе группа дублировалась бы тремя пунктами подряд.
    #[test]
    fn power_group_keeps_idle_and_profile_rows_together() {
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            for id in ["system/idle-screen-off", "system/idle-suspend"] {
                assert!(
                    !children.iter().any(|node| node.identity() == id),
                    "строка {id} должна жить в «Питании», а не в «Системе»"
                );
            }

            let group = find_by_id(children, "system/power");
            assert!(
                matches!(group.kind, super::super::tree::NodeKind::Submenu(_)),
                "питание — подменю"
            );
            assert!(
                group.value.is_none(),
                "строка подменю без значения: профиль виден внутри, а не справа"
            );
            assert_eq!(
                group.title,
                if lang == Language::Ru {
                    "Питание"
                } else {
                    "Power"
                }
            );

            let kids = group.children().expect("у подменю есть пункты");
            let ids: Vec<&str> = kids.iter().map(Node::identity).collect();
            assert_eq!(
                ids,
                [
                    "system/idle-screen-off",
                    "system/idle-suspend",
                    "system/power-profile"
                ]
            );

            let screen = find_by_id(kids, "system/idle-screen-off");
            assert_eq!(
                screen.title,
                if lang == Language::Ru {
                    "Экран гаснет"
                } else {
                    "Screen off"
                },
                "единица измерения остаётся в значении справа, а не в заголовке"
            );
            let suspend = find_by_id(kids, "system/idle-suspend");
            assert_eq!(
                suspend.title,
                if lang == Language::Ru {
                    "Сон ноутбука"
                } else {
                    "Suspend after"
                }
            );
        }
    }

    /// Раздел «Wi-Fi» — динамический вход с ключом WIFI, личностью
    /// `system/wifi` и условием `has_nmcli`. Старый статичный тумблер Wi-Fi
    /// убран: он дёргал `nmcli radio` из UI-потока, а радио теперь
    /// переключаетсяworker-ом внутри раздела.
    #[test]
    fn system_section_has_the_wifi_entry_by_key() {
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            let wifi = find_by_id(children, "system/wifi");
            assert!(
                matches!(wifi.kind, super::super::tree::NodeKind::Dynamic { key } if key == ProviderKey::WIFI),
                "вход динамический и с ключом WIFI"
            );
            assert!(
                wifi.visible.is_some(),
                "видимость задана условием has_nmcli"
            );
            assert_eq!(wifi.title, "Wi-Fi");
            // Статичного тумблера больше нет: две строки с названием «Wi-Fi»
            // в одном разделе пользователю только мешали бы.
            assert!(
                !children.iter().any(|node| {
                    node.title == "Wi-Fi"
                        && matches!(node.kind, super::super::tree::NodeKind::Toggle { .. })
                }),
                "старый тумблер Wi-Fi удалён из раздела"
            );
        }
    }

    /// Раздел «Bluetooth» — динамический вход с ключом BLUETOOTH и
    /// условием `has_bluetoothctl`. Действий у него нет: W5.3a read-only.
    #[test]
    fn system_section_has_the_bluetooth_entry_by_key() {
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            let bluetooth = find_by_id(children, "system/bluetooth");
            assert!(
                matches!(bluetooth.kind, super::super::tree::NodeKind::Dynamic { key } if key == ProviderKey::BLUETOOTH),
                "вход динамический и с ключом BLUETOOTH"
            );
            assert!(
                bluetooth.visible.is_some(),
                "видимость задана условием has_bluetoothctl"
            );
            assert_eq!(bluetooth.title, "Bluetooth");
        }
    }

    /// Раздел «Дисплеи» — динамический вход с ключом OUTPUTS и условием
    /// `has_niri`. Действий над выводами у него нет: W5.4a read-only.
    #[test]
    fn system_section_has_the_displays_entry_by_key() {
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            let displays = find_by_id(children, "system/displays");
            assert!(
                matches!(displays.kind, super::super::tree::NodeKind::Dynamic { key } if key == ProviderKey::OUTPUTS),
                "вход динамический и с ключом OUTPUTS"
            );
            assert!(
                displays.visible.is_some(),
                "видимость задана условием has_niri"
            );
            assert_eq!(
                displays.title,
                if lang == Language::Ru {
                    "Дисплеи"
                } else {
                    "Displays"
                }
            );
        }
    }

    /// Раздел «Устройства» — динамический вход с ключом STORAGE и условием
    /// `has_udisksctl`: без `lsblk` и `udisksctl` накопители нечем ни
    /// показать, ни извлечь.
    #[test]
    fn system_section_has_the_storage_entry_by_key() {
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            let storage = find_by_id(children, "system/storage");
            assert!(
                matches!(storage.kind, super::super::tree::NodeKind::Dynamic { key } if key == ProviderKey::STORAGE),
                "вход динамический и с ключом STORAGE"
            );
            assert!(
                storage.visible.is_some(),
                "видимость задана условием has_udisksctl"
            );
            assert_eq!(
                storage.title,
                if lang == Language::Ru {
                    "Накопители"
                } else {
                    "Drives"
                }
            );
        }
    }

    #[test]
    fn updates_group_has_bilingual_rows_by_id() {
        let expected = [
            ("system/updates/all", "Обновить всё", "Update everything"),
            ("system/updates/pacman", "Только pacman", "Pacman only"),
            ("system/updates/aur", "Только AUR", "AUR only"),
            ("system/updates/flatpak", "Только Flatpak", "Flatpak only"),
            (
                "system/updates/check",
                "Проверить обновления",
                "Check for updates",
            ),
        ];
        let mut titles_by_lang: Vec<Vec<String>> = Vec::new();
        for lang in [Language::Ru, Language::En] {
            let system = tree_in(lang)
                .into_iter()
                .find(|node| node.identity() == "Система" || node.identity() == "System")
                .expect("раздел «Система»");
            let children = system.children().expect("у раздела есть дети");
            let updates = find_by_id(children, "system/updates");
            assert!(
                matches!(updates.kind, super::super::tree::NodeKind::Submenu(_)),
                "обновление системы — подменю"
            );
            assert!(
                updates.visible.is_some(),
                "подменю скрыто без kitty условием has_kitty"
            );
            assert_eq!(
                updates.title,
                if lang == Language::Ru {
                    "Обновление системы"
                } else {
                    "System update"
                }
            );
            let kids = updates.children().expect("у подменю есть пункты");
            assert_eq!(kids.len(), expected.len());
            let mut titles = Vec::new();
            for (id, ru, en) in expected {
                let node = kids
                    .iter()
                    .find(|node| node.identity() == id)
                    .unwrap_or_else(|| panic!("нет пункта {id}"));
                let want = if lang == Language::Ru { ru } else { en };
                assert_eq!(node.title, want, "подпись {id} на языке {lang:?}");
                titles.push(node.title.clone());
            }
            for id in ["system/updates/aur", "system/updates/flatpak"] {
                let node = kids
                    .iter()
                    .find(|node| node.identity() == id)
                    .unwrap_or_else(|| panic!("нет пункта {id}"));
                assert!(node.visible.is_some(), "у {id} задано условие показа");
            }
            titles_by_lang.push(titles);
        }
        assert_ne!(
            titles_by_lang[0], titles_by_lang[1],
            "русские и английские подписи должны различаться"
        );
    }

    /// Каждый пункт запускает ровно `kitty` с нужным режимом hud-update:
    /// ни одного `Run`, который заблокировал бы меню на время обновления.
    #[test]
    fn updates_rows_spawn_kitty_with_exact_argv() {
        let expected: [(&str, &[&str]); 5] = [
            (
                "system/updates/all",
                &["--class", "hud-update", "-e", "hud-update", "all"],
            ),
            (
                "system/updates/pacman",
                &["--class", "hud-update", "-e", "hud-update", "pacman"],
            ),
            (
                "system/updates/aur",
                &["--class", "hud-update", "-e", "hud-update", "aur"],
            ),
            (
                "system/updates/flatpak",
                &["--class", "hud-update", "-e", "hud-update", "flatpak"],
            ),
            (
                "system/updates/check",
                &["--class", "hud-update", "-e", "hud-update", "check"],
            ),
        ];
        let system = tree_in(Language::Ru)
            .into_iter()
            .find(|node| node.identity() == "Система")
            .expect("раздел «Система»");
        let children = system.children().expect("у раздела есть дети");
        let updates = find_by_id(children, "system/updates");
        let kids = updates.children().expect("у подменю есть пункты");
        assert_eq!(kids.len(), expected.len());
        let mut spawns = 0;
        for (id, args) in expected {
            let node = kids
                .iter()
                .find(|node| node.identity() == id)
                .unwrap_or_else(|| panic!("нет пункта {id}"));
            match &node.kind {
                super::super::tree::NodeKind::Action(Action::Spawn(program, actual)) => {
                    assert_eq!(*program, "kitty", "программа {id}");
                    assert_eq!(*actual, args, "argv {id}");
                    spawns += 1;
                }
                other => panic!("{id} должен быть Action::Spawn, а не {other:?}"),
            }
        }
        assert_eq!(spawns, expected.len(), "ровно один Spawn на пункт");
    }

    #[test]
    fn root_has_nine_sections_without_power() {
        for lang in [Language::Ru, Language::En] {
            let menu = Menu::new(tree_in(lang));
            assert_eq!(menu.depth(), 0);
            assert_eq!(menu.frame().items.len(), 9);
            assert_eq!(
                menu.frame().items.len(),
                super::super::SECTIONS.len(),
                "порядок прямого входа повторяет видимый корень"
            );
        }
        let ru = Menu::new(tree_in(Language::Ru));
        let frame = ru.frame();
        let titles: Vec<&str> = frame.items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Приложения",
                "Панель",
                "Стиль",
                "Уведомления",
                "Язык",
                "Система",
                "Захват",
                "Бинды",
                "Hudbar"
            ]
        );
        let panel = frame
            .items
            .iter()
            .find(|item| item.title == "Панель")
            .expect("строка «Панель»");
        assert!(
            panel.value.is_empty(),
            "у раздела нет значения справа: «27 px» в корне только мешало"
        );
    }

    /// Control Center удалён целиком: в «Панель › Вид панели» осталась только
    /// высота, а строки с этим названием нет ни в одном разделе.
    #[test]
    fn control_center_is_gone_from_the_tree() {
        use super::super::tree::NodeKind;

        fn titles(node: &Node, out: &mut Vec<String>) {
            out.push(node.title.clone());
            if let NodeKind::Submenu(children) = &node.kind {
                for child in children {
                    titles(child, out);
                }
            }
        }

        for lang in [Language::Ru, Language::En] {
            let look = look_group(lang);
            let NodeKind::Submenu(children) = &look.kind else {
                panic!("«Вид панели» — подменю, а не {:?}", look.kind);
            };
            assert_eq!(
                children.len(),
                1,
                "в «Вид панели» осталась только высота, а не переключатель Control Center"
            );
            assert_eq!(
                children[0].title,
                match lang {
                    Language::Ru => "Высота",
                    Language::En => "Height",
                }
            );

            let mut found = Vec::new();
            for node in tree_in(lang) {
                titles(&node, &mut found);
            }
            assert!(
                !found.iter().any(|title| title == "Control Center"),
                "строка Control Center не должна оставаться в дереве: {found:?}"
            );
        }
    }

    /// «Питание» убрано из корня, но бинд `super+P` зовёт `hud-menu power`:
    /// скрытый раздел обязан открываться прямым входом.
    #[test]
    fn hidden_power_section_opens_by_id() {
        for lang in [Language::Ru, Language::En] {
            let mut menu = Menu::new(tree_in(lang));
            menu.set_lang(lang);
            assert!(
                !menu.frame().items.iter().any(|item| item.title
                    == if lang == Language::Ru {
                        "Питание"
                    } else {
                        "Power"
                    }),
                "раздел питания не должен попадать в корень"
            );
            assert!(
                super::super::open_section(&mut menu, "power"),
                "прямой вход по идентификатору работает"
            );
            assert_eq!(menu.depth(), 1);
            assert_eq!(
                menu.current().title,
                if lang == Language::Ru {
                    "Питание"
                } else {
                    "Power"
                }
            );
        }
    }

    #[test]
    fn searching_dnd_finds_the_notifications_row() {
        let mut menu = Menu::new(tree_in(Language::Ru));
        for ch in "dnd".chars() {
            menu.type_char(ch);
        }
        let frame = menu.frame();
        let notifications: Vec<&str> = frame
            .items
            .iter()
            .filter(|item| item.caption.as_deref() == Some("Уведомления ›"))
            .map(|item| item.title.as_str())
            .collect();
        assert_eq!(
            notifications,
            ["Не беспокоить"],
            "в «Уведомлениях» ровно одна строка DND"
        );
        for item in &frame.items {
            assert!(
                !item.title.contains("(DND)"),
                "аббревиатуры в названии нет: {}",
                item.title
            );
        }
    }

    #[test]
    fn english_tree_has_english_titles() {
        let menu = Menu::new(tree_in(Language::En));
        let frame = menu.frame();
        let titles: Vec<&str> = frame.items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles[0], "Applications");
        assert_eq!(titles[2], "Style");
    }

    /// Раздел «Питание»: обратимые действия в начале, необратимые — в конце
    /// и обязательно за вопросом. Проверка по порядку, а не по наличию:
    /// список из трёх строк, где «Выключение» стоит первым, опасен.
    /// Гибернация показывается только там, где ядро её умеет: решение
    /// принимает `/sys/power/state`, а не наличие `systemctl`.
    #[test]
    fn hibernate_follows_the_kernel_states() {
        let states = |text: &str| text.split_whitespace().any(|state| state == "disk");
        assert!(states("freeze mem disk"), "диск есть — гибернация есть");
        assert!(!states("freeze mem"), "без диска обещать нечего");
    }

    /// Строка гибернации в «Питание» — с вопросом и `systemctl hibernate`,
    /// и на этой машине скрыта: `/sys/power/state` без `disk`.
    #[test]
    fn hibernate_row_is_a_confirmed_systemctl_call() {
        use super::super::tree::NodeKind;
        let power = tree_in(Language::Ru)
            .into_iter()
            .find(|node| node.identity() == "Питание")
            .expect("раздел «Питание»");
        let rows = power.children().expect("у раздела есть дети");
        let hibernate = rows.iter().find(|node| node.title == "Гибернация?");
        match hibernate {
            Some(row) => {
                let NodeKind::Submenu(answers) = &row.kind else {
                    panic!("ожидался вопрос, {:?}", row.kind);
                };
                let yes = answers
                    .iter()
                    .find(|answer| answer.title.starts_with("Да"))
                    .expect("есть подтверждение");
                let NodeKind::Action(Action::Spawn(program, args)) = &yes.kind else {
                    panic!("подтверждение должно звать systemctl, {:?}", yes.kind);
                };
                assert_eq!(*program, "systemctl");
                assert_eq!(*args, ["hibernate"]);
            }
            // На машине без гибернации строки нет — и это правильно.
            None => assert!(
                !has_hibernate(),
                "строка спрятана, хотя ядро гибернацию умеет: {:?}",
                rows.iter()
                    .map(|row| row.title.as_str())
                    .collect::<Vec<_>>()
            ),
        }
    }

    #[test]
    fn power_section_puts_destructive_rows_last_and_behind_a_question() {
        use super::super::tree::NodeKind;
        let power = tree_in(Language::Ru)
            .into_iter()
            .find(|node| node.identity() == "Питание")
            .expect("раздел «Питание»");
        let titles: Vec<&str> = power
            .children()
            .expect("у раздела есть строки")
            .iter()
            .map(|node| node.title.as_str())
            .collect();
        assert_eq!(
            titles.last().copied(),
            Some("Выключить?"),
            "выключение обязано быть последним: {titles:?}"
        );
        assert!(
            titles.contains(&"Перезагрузить?"),
            "перезагрузка за вопросом: {titles:?}"
        );
        for question in ["Перезагрузить?", "Выключить?"] {
            let node = power
                .children()
                .unwrap()
                .iter()
                .find(|node| node.title == question)
                .expect("вопрос");
            let children = node.children().expect("у вопроса есть ответы");
            assert_eq!(children.len(), 2, "у вопроса ровно два ответа: {question}");
            assert_eq!(children[1].title, "Отмена");
            assert!(
                matches!(&children[1].kind, NodeKind::Action(Action::Back)),
                "отмена возвращает уровень, а не запускает команду"
            );
        }
    }

    /// Подтверждение: «Да, выключить» действительно выключает, а «Отмена» и
    /// `Esc` возвращают в «Питание», не выполнив ничего.
    #[test]
    fn power_confirm_runs_the_command_or_returns_without_it() {
        let mut menu = Menu::new(tree_in(Language::Ru));
        assert!(
            super::super::open_section(&mut menu, "power"),
            "скрытый раздел открывается прямым входом"
        );

        let index = menu
            .frame()
            .items
            .iter()
            .position(|item| item.title == "Выключить?")
            .expect("строка выключения");
        menu.current_mut().list.select(index);
        assert_eq!(menu.enter(), Outcome::Pushed, "открылся вопрос");
        assert_eq!(menu.opened_submenu(), Some("Выключить?"));

        // Отмена первой строкой снизу: сначала выбираем «Отмена».
        let cancel = menu
            .frame()
            .items
            .iter()
            .position(|item| item.title == "Отмена")
            .expect("строка отмены");
        menu.current_mut().list.select(cancel);
        assert_eq!(menu.enter(), Outcome::Popped, "отмена вернула в раздел");
        assert_eq!(menu.opened_submenu(), Some("Питание"));
    }

    /// Внутренние строки вопроса не должны попадать в глобальный поиск: там лежат
    /// два одинаковых «Отмена», а «Да, выключить» по `Enter` из поиска выключила бы
    /// машину одним нажатием мимо вопроса. Выход только через сам раздел.
    #[test]
    fn confirm_answers_stay_out_of_the_global_search() {
        let mut menu = Menu::new(tree_in(Language::Ru));
        for ch in "выключ".chars() {
            menu.type_char(ch);
        }
        let frame = menu.frame();
        let titles: Vec<&str> = frame.items.iter().map(|item| item.title.as_str()).collect();
        assert!(
            titles.is_empty(),
            "из поиска выключение недостижимо: {titles:?}"
        );
    }

    /// Проверка машины, а не кода: на чистом раннере ни `niri`, ни `wpctl`,
    /// ни `nmcli` не стоят. Запускается руками: `cargo test -- --ignored`.
    #[ignore = "нужны установленные программы машины"]
    #[test]
    fn every_external_command_is_installed() {
        use super::super::tree::NodeKind;
        fn walk(nodes: &[Node], missing: &mut Vec<String>) {
            for node in nodes {
                if let NodeKind::Action(action) = &node.kind {
                    // Действие niri выполняется композитором: проверяется сам
                    // niri, а не первое слово его аргументов.
                    let program = match action {
                        Action::Niri(_) => Some("niri"),
                        _ => action.command(),
                    };
                    if let Some(program) = program
                        && !oneshot::program_exists(program)
                    {
                        missing.push(format!("{}: {program}", node.title));
                    }
                }
                if let Some(children) = node.children() {
                    walk(children, missing);
                }
            }
        }
        let mut missing = Vec::new();
        walk(&tree(), &mut missing);
        assert!(
            missing.is_empty(),
            "пункты без установленных программ: {missing:?}"
        );
    }

    #[test]
    fn every_real_node_has_an_icon() {
        use super::super::tree::NodeKind;

        fn walk(nodes: &[Node]) {
            for node in nodes {
                let icon_is_optional = matches!(&node.kind, NodeKind::Info | NodeKind::Header);
                assert!(
                    (icon_is_optional && node.icon.is_empty()) || node.icon.chars().count() == 1,
                    "у «{}» иконка не один глиф: {:?}",
                    node.title,
                    node.icon
                );
                if let Some(children) = node.children() {
                    walk(children);
                }
            }
        }
        walk(&tree_in(Language::Ru));
        walk(&tree_in(Language::En));
    }

    #[test]
    fn order_section_lists_every_module_once() {
        let section = order_section(Language::Ru);
        let children = section.children().expect("подменю порядка");
        // Подсказка плюс десять модулей в текущем порядке.
        assert_eq!(children.len(), 11);
    }

    /// Пункт выбора обоев: окно бинарником, если оно установлено, иначе
    /// прежний `wall.sh`. Оба варианта — `Spawn`, то есть с закрытием меню, и
    /// оба проверяются без установленных на машине программ.
    #[test]
    fn wallpaper_item_prefers_the_picker_and_falls_back_to_wall_script() {
        for (picker, expected) in [(true, "hud-wallpaper-rs"), (false, "wall.sh")] {
            let node = wallpaper_row(Language::Ru, picker);
            let super::super::tree::NodeKind::Action(action) = &node.kind else {
                panic!("пункт не действие: {:?}", node.kind);
            };
            assert!(action.closes_menu(), "окно обоев закрывает меню");
            assert_eq!(action.describe(Language::Ru), expected, "окно={picker}");
        }
    }

    /// Раздел стиля: строки обоев зависят от того, что стоит на машине, и
    /// приходят флагами. Проверяются обе ветки — с программами и без.
    #[test]
    fn style_section_moves_values_with_choice() {
        let tools = WallpaperTools {
            picker: true,
            script: true,
        };
        let mut menu = Menu::new(vec![style_section_with(Language::Ru, tools)]);
        assert_eq!(menu.enter(), Outcome::Pushed);
        let frame = menu.frame();
        let titles: Vec<&str> = frame.items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["Выбрать обои…", "Случайные обои", "Схема matugen"]);

        let no_tools = WallpaperTools {
            picker: false,
            script: false,
        };
        let mut menu = Menu::new(vec![style_section_with(Language::Ru, no_tools)]);
        assert_eq!(menu.enter(), Outcome::Pushed);
        let frame = menu.frame();
        let titles: Vec<&str> = frame.items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["Схема matugen"]);
    }

    /// В английском дереве не должно оставаться кириллицы: подписи приходят
    /// из словаря, а не из русского текста по умолчанию. Исключение — название
    /// языка в списке выбора: «Русский» пишется на самом языке.
    #[test]
    fn english_tree_has_no_russian_labels() {
        use super::super::tree::{Node, NodeKind, ValueSource};

        fn is_cyrillic(text: &str) -> bool {
            text.chars().any(|c| matches!(c, 'А'..='я' | 'Ё' | 'ё'))
        }

        fn walk(nodes: &[Node], path: &str, bad: &mut Vec<String>) {
            for node in nodes {
                let here = format!("{path} › {}", node.title);
                if is_cyrillic(&node.title) {
                    bad.push(format!("title {here}"));
                }
                if let Some(crumb) = &node.crumb
                    && is_cyrillic(crumb)
                {
                    bad.push(format!("crumb {here}: {crumb}"));
                }
                match &node.kind {
                    NodeKind::Submenu(children) => walk(children, &here, bad),
                    NodeKind::Choice { options, .. } => {
                        for option in *options {
                            if *option != "Русский" && is_cyrillic(option) {
                                bad.push(format!("option {here}: {option}"));
                            }
                        }
                    }
                    NodeKind::Number { unit, .. } if is_cyrillic(unit) => {
                        bad.push(format!("unit {here}: {unit}"));
                    }
                    _ => {}
                }
                if let Some(ValueSource::Text(text)) = &node.value
                    && is_cyrillic(text)
                {
                    bad.push(format!("value {here}: {text}"));
                }
            }
        }

        let mut bad = Vec::new();
        walk(&tree_in(Language::En), "HUD", &mut bad);
        assert!(bad.is_empty(), "русские подписи в EN-дереве: {bad:#?}");
    }
}

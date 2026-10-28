use super::super::action::{Action, Live};
use super::super::item::Origin;
use super::super::snapshot::render_menu_card;
use super::super::system::SlotState;
use super::super::tree::Node;
use super::*;

fn tree() -> Vec<Node> {
    vec![
        Node::submenu(
            "apps",
            "Приложения",
            vec![Node::action("term", "Терминал", Action::Run("kitty", &[]))],
        ),
        Node::submenu(
            "style",
            "Стиль",
            vec![Node::number(
                "height",
                "Высота",
                20.0,
                48.0,
                1.0,
                "px",
                || 27.0,
                |_| {},
            )],
        ),
        Node::toggle("dnd", "Не беспокоить", || true, |_| {}),
    ]
}

fn menu() -> Menu {
    Menu::new(tree())
}

fn titles(menu: &Menu) -> Vec<String> {
    menu.current()
        .list
        .items()
        .iter()
        .map(|item| item.title.clone())
        .collect()
}

#[test]
fn new_menu_opens_the_root() {
    let menu = menu();
    assert_eq!(menu.depth(), 0);
    assert_eq!(menu.trail(), vec!["HUD"]);
    assert_eq!(menu.opened_submenu(), None);
    assert_eq!(titles(&menu), ["Приложения", "Стиль", "Не беспокоить"]);
}

#[test]
fn enter_pushes_a_level_and_back_pops_it() {
    let mut menu = menu();
    menu.move_sel(1);
    assert_eq!(menu.enter(), Outcome::Pushed);
    assert_eq!(menu.depth(), 1);
    assert_eq!(menu.opened_submenu(), Some("Стиль"));
    assert_eq!(menu.trail(), vec!["HUD", "Стиль"]);
    assert_eq!(titles(&menu), ["Высота"]);

    assert_eq!(menu.back(), Outcome::Popped);
    assert_eq!(menu.depth(), 0);
    assert_eq!(menu.opened_submenu(), None);
    assert_eq!(titles(&menu), ["Приложения", "Стиль", "Не беспокоить"]);
}

#[test]
fn crumbed_submenu_names_its_level_but_keeps_the_row_and_search_path() {
    let tree = || {
        vec![
            Node::submenu(
                "i",
                "eDP-1 · 1920×1080 · 144 Гц",
                vec![Node::info("d", "Модель")],
            )
            .with_id("displays/output/eDP-1")
            .with_crumb("eDP-1"),
        ]
    };
    let mut menu = Menu::new(tree());
    assert_eq!(titles(&menu), ["eDP-1 · 1920×1080 · 144 Гц"]);

    // Info не становится отдельным результатом поиска: запрос по его тексту
    // ничего не находит. Раздел из одних Info остаётся находимым по
    // заголовку родителя.
    menu.type_char('М');
    assert!(
        menu.frame().items.is_empty(),
        "информационная строка не должна быть результатом"
    );
    menu.clear_query();
    for ch in "eDP".chars() {
        menu.type_char(ch);
    }
    let found = menu.frame();
    assert_eq!(found.items[0].title, "eDP-1 · 1920×1080 · 144 Гц");
    assert!(found.items[0].kind.is_submenu());
    assert_eq!(found.items[0].caption, None, "крошка не путь поиска");
    menu.clear_query();

    menu.enter();
    assert_eq!(menu.opened_submenu(), Some("eDP-1"));
    assert_eq!(menu.trail(), vec!["HUD", "eDP-1"]);
    assert_eq!(titles(&menu), ["Модель"]);

    // Путь переживает пересборку дерева: личность уровня — id, не заголовок.
    let position = menu.position();
    menu.retree(tree());
    menu.restore(&position);
    assert_eq!(menu.trail(), vec!["HUD", "eDP-1"]);
    assert_eq!(titles(&menu), ["Модель"]);

    assert_eq!(menu.back(), Outcome::Popped);
    assert_eq!(menu.trail(), vec!["HUD"]);
}

#[test]
fn escape_on_the_root_closes_the_menu() {
    let mut menu = menu();
    assert_eq!(menu.escape(), Outcome::Closed);
}

#[test]
fn escape_inside_a_submenu_returns_one_level() {
    let mut menu = menu();
    menu.move_sel(1);
    menu.enter();
    assert_eq!(menu.escape(), Outcome::Popped);
    assert_eq!(menu.depth(), 0);
    assert_eq!(menu.escape(), Outcome::Closed);
}

#[test]
fn each_level_keeps_its_own_query_and_selection() {
    let mut menu = menu();
    menu.move_sel(1);
    menu.enter();
    menu.type_char('в');
    assert_eq!(menu.query(), "в");
    menu.back();
    assert_eq!(menu.query(), "", "запрос не должен утекать между уровнями");
}

#[test]
fn root_search_spans_all_leaves_and_shows_the_path() {
    // Запрос должен находить листья из разных веток: терминал из «Приложений»
    // и высоту из «Стиля» — путь листа показывается в подписи.
    let mut menu = menu();
    for ch in "тер".chars() {
        menu.type_char(ch);
    }
    let frame = menu.frame();
    assert_eq!(frame.items.len(), 1);
    assert_eq!(frame.items[0].title, "Терминал");
    assert_eq!(frame.items[0].caption.as_deref(), Some("Приложения ›"));
    assert_eq!(frame.total, 1);
}

#[test]
fn root_search_finds_leaves_from_other_branches() {
    let mut menu = menu();
    for ch in "выс".chars() {
        menu.type_char(ch);
    }
    let frame = menu.frame();
    assert_eq!(frame.items.len(), 1);
    assert_eq!(frame.items[0].title, "Высота");
    assert_eq!(frame.items[0].origin, Origin { depth: 0, index: 1 });
}

#[test]
fn search_inside_a_level_stays_inside_that_level() {
    let mut menu = menu();
    menu.move_sel(1);
    menu.enter();
    for ch in "выс".chars() {
        menu.type_char(ch);
    }
    assert_eq!(menu.query(), "выс");
    assert_eq!(titles(&menu), ["Высота"], "поиск уровня не лезет в корень");

    menu.back();
    assert_eq!(titles(&menu), ["Приложения", "Стиль", "Не беспокоить"]);
}

#[test]
fn clearing_the_root_query_restores_the_sections() {
    let mut menu = menu();
    for ch in "выс".chars() {
        menu.type_char(ch);
    }
    assert_eq!(menu.frame().items.len(), 1);
    menu.clear_query();
    assert_eq!(titles(&menu), ["Приложения", "Стиль", "Не беспокоить"]);
}

#[test]
fn backspace_shrinks_the_query_and_the_result() {
    let mut menu = menu();
    for ch in "высота".chars() {
        menu.type_char(ch);
    }
    assert_eq!(menu.frame().items.len(), 1);
    menu.backspace();
    assert_eq!(menu.query(), "высот");
    menu.backspace();
    menu.backspace();
    assert_eq!(menu.query(), "выс");
    assert_eq!(menu.frame().items.len(), 1);
    menu.clear_query();
    menu.type_char('ф');
    assert_eq!(menu.query(), "ф");
    assert_eq!(
        menu.frame().items.len(),
        0,
        "буквы «ф» нет ни в одном названии и ни в одном пути"
    );
    assert_eq!(menu.enter(), Outcome::Idle);
}

#[test]
fn enter_on_a_search_result_opens_its_own_level() {
    let mut menu = menu();
    for ch in "выс".chars() {
        menu.type_char(ch);
    }
    assert_eq!(menu.enter(), Outcome::Pushed);
    assert_eq!(menu.depth(), 1);
    assert_eq!(menu.opened_submenu(), Some("Стиль"));
    assert_eq!(titles(&menu), ["Высота"]);
    assert_eq!(
        menu.current().list.selected(),
        Some(0),
        "найденная строка выбрана"
    );
}

#[test]
fn hidden_nodes_are_absent_from_every_level_and_from_search() {
    let mut menu = Menu::new(vec![
        Node::toggle("dnd", "Не беспокоить", || false, |_| {}).when(|| false),
        Node::action("x", "Терминал", Action::Live(Live::DndToggle)),
    ]);
    assert_eq!(titles(&menu), ["Терминал"]);
    for ch in "беспокоить".chars() {
        menu.type_char(ch);
    }
    assert!(menu.frame().items.is_empty());
}

#[test]
fn selection_wraps_around_the_level_while_scroll_stays_clamped() {
    let mut menu = menu();
    menu.set_page(2);
    menu.move_sel(99);
    assert_eq!(menu.current().list.selected(), Some(0), "99 % 3 — круг");
    menu.move_sel(-1);
    assert_eq!(
        menu.current().list.selected(),
        Some(2),
        "вверх с первой — вниз"
    );

    menu.scroll(99);
    assert_eq!(menu.current().list.top(), 1, "нижняя страница");
    menu.scroll(-99);
    assert_eq!(menu.current().list.top(), 0);
}

#[test]
fn toggle_reports_a_change_and_rebuilds_the_row() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ON: AtomicBool = AtomicBool::new(true);
    fn get() -> bool {
        ON.load(Ordering::Relaxed)
    }
    fn set(on: bool) {
        ON.store(on, Ordering::Relaxed);
    }
    let mut menu = Menu::new(vec![Node::toggle("dnd", "Не беспокоить", get, set)]);
    assert_eq!(menu.frame().items[0].value, "вкл");
    assert_eq!(menu.enter(), Outcome::Changed);
    assert_eq!(menu.frame().items[0].value, "выкл", "строка пересобралась");
    ON.store(true, Ordering::Relaxed);
}

#[test]
fn numbers_never_leave_their_bounds() {
    use std::sync::atomic::{AtomicU32, Ordering};
    static VALUE: AtomicU32 = AtomicU32::new(27);
    fn get() -> f32 {
        VALUE.load(Ordering::Relaxed) as f32
    }
    fn set(value: f32) {
        VALUE.store(value.round() as u32, Ordering::Relaxed);
    }
    let mut menu = Menu::new(vec![Node::submenu(
        "panel",
        "Панель",
        vec![Node::number("h", "Высота", 20.0, 48.0, 1.0, "px", get, set)],
    )]);
    menu.enter();
    assert_eq!(menu.frame().items[0].value, "27px");

    for _ in 0..40 {
        menu.adjust_value(1);
    }
    assert_eq!(menu.frame().items[0].value, "48px", "потолок держит");
    for _ in 0..60 {
        menu.adjust_value(-1);
    }
    assert_eq!(menu.frame().items[0].value, "20px", "пол держит");
}

#[test]
fn arrows_on_a_row_without_a_number_do_nothing() {
    let mut menu = menu();
    assert_eq!(menu.adjust_value(1), Outcome::Idle);
    assert_eq!(menu.adjust_value(-1), Outcome::Idle);
}

#[test]
fn a_stub_action_reports_failure_in_the_footer() {
    fn fail_action(_: &Action) -> Result<(), String> {
        Err("M4 test action failed".to_string())
    }

    let mut menu = Menu::with_action_runner(
        vec![Node::action("x", "Терминал", Action::Run("kitty", &[]))],
        fail_action,
    );
    assert_eq!(menu.enter(), Outcome::Idle);
    let status = menu.status().expect("статус после неудачи");
    assert!(status.is_failed());
    assert!(status.text().contains("M4"));
    menu.clear_status();
    assert!(menu.status().is_none());
}

#[test]
fn vpn_selection_refreshes_its_dynamic_snapshot_after_success() {
    fn succeed(_: &Action) -> Result<(), String> {
        Ok(())
    }

    let mut menu = Menu::with_action_runner(
        vec![Node::dynamic("network", "VPN", ProviderKey::VPN)],
        succeed,
    );
    assert_eq!(menu.enter(), Outcome::Pushed);
    let (key, generation) = menu
        .take_dynamic_requests()
        .pop()
        .expect("первый snapshot запрошен");
    assert_eq!(key, ProviderKey::VPN);
    menu.apply_dynamic(
        key,
        generation,
        Ok(vec![Node::action(
            "server",
            "🇺🇸 США, Атланта",
            Action::VpnNode("🇺🇸 США, Атланта".to_string()),
        )]),
    );

    assert_eq!(menu.enter(), Outcome::Changed);

    let (key, generation) = menu
        .take_dynamic_requests()
        .pop()
        .expect("snapshot обновляется");
    assert!(menu.status().is_some_and(|status| !status.is_failed()));
    menu.apply_dynamic(
        key,
        generation,
        Ok(vec![Node::info("server", "обновлённый выбор")]),
    );
    assert_eq!(menu.frame().items[0].title, "обновлённый выбор");
}

#[test]
fn a_failed_vpn_mode_change_still_refreshes_partial_controller_state() {
    fn fail(_: &Action) -> Result<(), String> {
        Err("второй селектор не ответил".to_string())
    }

    let mut menu = Menu::with_action_runner(
        vec![Node::dynamic("network", "VPN", ProviderKey::VPN)],
        fail,
    );
    assert_eq!(menu.enter(), Outcome::Pushed);
    let (key, generation) = menu
        .take_dynamic_requests()
        .pop()
        .expect("первый snapshot запрошен");
    menu.apply_dynamic(
        key,
        generation,
        Ok(vec![Node::action(
            "mode",
            "Всё через прокси",
            Action::VpnMode(super::super::vpn::VpnMode::All),
        )]),
    );

    assert_eq!(menu.enter(), Outcome::Idle);

    assert_eq!(
        menu.take_dynamic_requests().len(),
        1,
        "перечитать частичное состояние"
    );
    assert!(menu.status().is_some_and(|status| status.is_failed()));
}

/// На строке-ползунке Enter переключает mute, а `Shift+←/→` кладёт в
/// очередь шаг громкости. На любой другой строке и то и другое — no-op:
/// шаг громкости вне раздела звука означал бы, что меняется устройство,
/// которого тут нет.
#[test]
fn volume_row_answers_enter_and_shift_arrows_only_on_itself() {
    let mut menu = Menu::new(vec![volume_node("Звук · Speaker", 40)]);
    assert_eq!(menu.adjust_volume(-1), Outcome::Changed, "шаг вниз");
    assert_eq!(menu.adjust_volume(1), Outcome::Changed, "шаг вверх");
    assert_eq!(menu.enter(), Outcome::Changed, "Enter — mute");
    assert_eq!(menu.pending_commands.len(), 3, "три команды в очереди");
    assert!(
        menu.pending_commands
            .iter()
            .any(|job| job.commands.iter().any(|command| command
                .argv()
                .1
                .iter()
                .any(|arg| arg == "toggle"))),
        "mute ушёл отдельной командой"
    );

    let mut menu = Menu::new(vec![Node::action("", "Обычная", Action::Back)]);
    assert_eq!(menu.adjust_volume(-1), Outcome::Idle, "не ползунок — пусто");
    assert_eq!(menu.adjust_volume(1), Outcome::Idle, "не ползунок — пусто");
    // Enter на обычной строке ведёт себя как всегда: на корне он
    // закрывает меню, и ползунок тут ни при чём.
    assert_eq!(
        menu.enter(),
        Outcome::Closed,
        "Enter на корне закрывает меню"
    );
    assert!(
        menu.pending_commands
            .iter()
            .all(|job| job.commands.is_empty()
                || !job.commands.iter().any(|command| command
                    .argv()
                    .1
                    .iter()
                    .any(|arg| arg == "toggle"))),
        "у обычной строки команд mute нет"
    );
}

/// Строка-ползунок для проверки: подставлены команды, важно только то,
/// что строка ведёт себя как громкость.
fn volume_node(title: &str, level: u8) -> Node {
    Node::volume(
        "",
        title,
        super::super::system::CommandSpec::new("wpctl")
            .arg("set-volume")
            .arg("5%-"),
        super::super::system::CommandSpec::new("wpctl")
            .arg("set-volume")
            .arg("5%+"),
        super::super::system::CommandSpec::new("wpctl")
            .arg("set-mute")
            .arg("toggle"),
    )
    .with_level(level, false)
}

#[test]
fn nudge_moves_only_the_module_order_row() {
    use std::sync::atomic::{AtomicI32, Ordering};
    static LAST_DELTA: AtomicI32 = AtomicI32::new(0);
    fn record_or_idle(action: &Action) -> Result<(), String> {
        if let Action::MoveModule { delta, .. } = action {
            LAST_DELTA.store(*delta, Ordering::SeqCst);
        }
        Ok(())
    }
    use super::super::settings::Module;

    let mut menu = Menu::with_action_runner(
        vec![
            Node::action("a", "Терминал", Action::Run("kitty", &[])),
            Node::action(
                "b",
                "audio",
                Action::MoveModule {
                    module: Module::Audio,
                    delta: 1,
                },
            ),
        ],
        record_or_idle,
    );
    // Строка без move: не двигает порядок, статус Idle.
    menu.move_sel(0);
    assert_eq!(menu.nudge_module(1), Outcome::Idle);
    assert_eq!(LAST_DELTA.load(Ordering::SeqCst), 0);
    // Строка порядка модулей: Enter-equivalent и Shift в обе стороны.
    menu.move_sel(1);
    assert_eq!(menu.nudge_module(1), Outcome::Changed);
    assert_eq!(LAST_DELTA.load(Ordering::SeqCst), 1);
    assert_eq!(menu.nudge_module(-1), Outcome::Changed);
    assert_eq!(LAST_DELTA.load(Ordering::SeqCst), -1);
}

#[test]
fn back_from_the_root_clears_the_status() {
    let mut menu = menu();
    menu.set_status(Status::Applied("высота: 30".to_string()));
    assert!(menu.status().is_some());
    assert_eq!(menu.back(), Outcome::Closed);
    assert!(menu.status().is_none());
}

#[test]
fn empty_submenu_is_not_pushed() {
    let mut menu = Menu::new(vec![Node::submenu("empty", "Пусто", Vec::new())]);
    assert_eq!(
        menu.enter(),
        Outcome::Idle,
        "пустое подменю открывать некуда"
    );
    assert_eq!(menu.depth(), 0);
    assert_eq!(titles(&menu), ["Пусто"]);
}

#[test]
fn validate_catches_broken_trees() {
    assert!(validate(&tree()).is_ok());
    assert!(validate(&[Node::submenu("a", "Пусто", Vec::new())]).is_err());
    assert!(validate(&[Node::action("a", "  ", Action::Live(Live::DndToggle))]).is_err());
    assert!(
        validate(&[Node::choice("a", "Тема", &[], || 0, |_| {})]).is_err(),
        "пустой список вариантов — ошибка"
    );
}

#[test]
fn control_count_sees_leaves_only() {
    assert_eq!(control_count(&tree()), 3);
}

#[test]
fn number_value_of_a_leaf_is_bounded() {
    let nodes = [Node::number(
        "h",
        "Высота",
        20.0,
        48.0,
        1.0,
        "px",
        || 99.0,
        |_| {},
    )];
    assert_eq!(number_value(&nodes[0]), Some(48.0));
}

#[test]
fn frame_reports_the_counter_parts() {
    let mut menu = menu();
    menu.set_page(5);
    let frame = menu.frame();
    assert_eq!(frame.trail, vec!["HUD".to_string()]);
    assert_eq!(frame.total, 3);
    assert_eq!(frame.selected, Some(0));
    assert_eq!(frame.top, 0);
    assert_eq!(frame.query, "");
    assert_eq!(frame.status, None);
}

#[test]
fn deep_nesting_keeps_every_level() {
    let mut menu = Menu::new(vec![Node::submenu(
        "a",
        "Панель",
        vec![Node::submenu(
            "b",
            "Модули",
            vec![Node::submenu(
                "c",
                "Глубоко",
                vec![Node::action("d", "Конец", Action::Niri(&["msg"]))],
            )],
        )],
    )]);
    for expected in ["Панель", "Модули", "Глубоко"] {
        menu.enter();
        assert_eq!(menu.opened_submenu(), Some(expected));
    }
    assert_eq!(menu.depth(), 3);
    assert_eq!(menu.trail(), vec!["HUD", "Панель", "Модули", "Глубоко"]);
    assert_eq!(menu.enter(), Outcome::Idle, "лист не открывается");
    assert_eq!(menu.opened_submenu(), Some("Глубоко"));
}

fn dynamic_tree() -> Vec<Node> {
    vec![
        Node::submenu(
            "panel",
            "Панель",
            vec![Node::toggle("dnd", "Не беспокоить", || true, |_| {})],
        ),
        Node::dynamic("audio", "Звук", ProviderKey::new("test-audio")),
    ]
}

fn dynamic_menu() -> Menu {
    Menu::new(dynamic_tree())
}

fn ready_nodes() -> Vec<Node> {
    vec![
        Node::toggle("v", "Громкость", || true, |_| {}),
        Node::action("m", "Устройство", Action::Run("true", &[])),
    ]
}

/// Раздел, который стал длиннее, не должен прокручиваться: высота
/// страницы считается по новой длине списка. Иначе список листается при
/// свободном месте, а карточка рисует под ним пустую строку.
#[test]
fn a_longer_dynamic_level_fits_without_scrolling() {
    let mut menu = dynamic_menu();
    menu.move_sel(1);
    assert_eq!(menu.enter(), Outcome::Pushed);
    let key = ProviderKey::new("test-audio");
    let (_, generation) = menu.take_dynamic_requests()[0];
    let few: Vec<Node> = (0..3)
        .map(|i| Node::action("s", &format!("мало {i}"), Action::Back))
        .collect();
    assert!(menu.apply_dynamic(key, generation, Ok(few)));
    let short = menu.current().list.len();
    assert_eq!(
        menu.current().list.page(),
        super::super::view::rows_shown(short),
        "страница по трём строкам"
    );

    let fresh = menu.refresh_dynamic(key).expect("запрос на обновление");
    let many: Vec<Node> = (0..7)
        .map(|i| Node::action("s", &format!("строка {i}"), Action::Back))
        .collect();
    assert!(menu.apply_dynamic(key, fresh, Ok(many)));
    let len = menu.current().list.len();
    assert!(len > short, "раздел стал длиннее");
    assert_eq!(
        menu.current().list.page(),
        super::super::view::rows_shown(len),
        "страница пересчитана под новую длину"
    );
    menu.move_sel(99);
    assert_eq!(menu.current().list.top(), 0, "всё влезает — прокрутки нет");
    assert_eq!(
        super::super::view::rows_for(&menu.frame()),
        len,
        "карточка не должна рисовать пустую строку снизу"
    );
}

/// Вход в динамический раздел сразу показывает загрузку и ставит один
/// запрос в очередь worker-потока.
#[test]
fn dynamic_enter_shows_loading_and_queues_one_request() {
    let mut menu = dynamic_menu();
    menu.move_sel(1);
    assert_eq!(menu.enter(), Outcome::Pushed);
    assert_eq!(menu.depth(), 1);
    assert_eq!(menu.opened_submenu(), Some("Звук"));
    assert_eq!(titles(&menu), ["Загрузка…"]);
    let key = ProviderKey::new("test-audio");
    assert!(matches!(menu.dynamic_state(key), Some(SlotState::Loading)));
    let pending = menu.take_dynamic_requests();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, key);
    assert!(menu.take_dynamic_requests().is_empty(), "очередь забрана");
}

/// Динамический вход рисуется той же стрелкой подменю: рендер не менялся.
#[test]
fn dynamic_entry_looks_like_a_submenu() {
    let menu = dynamic_menu();
    let row = menu
        .frame()
        .items
        .into_iter()
        .find(|item| item.title == "Звук")
        .expect("вход в раздел");
    assert!(row.kind.is_submenu());
    assert_eq!(row.value, "", "у входа значения нет");
}

/// Готовый ответ один раз перестраивает уровень; повторный вход берёт
/// кэш и нового запроса не ставит.
#[test]
fn dynamic_ready_rebuilds_once_and_caches() {
    let mut menu = dynamic_menu();
    menu.move_sel(1);
    menu.enter();
    let key = ProviderKey::new("test-audio");
    let (pending_key, generation) = menu.take_dynamic_requests()[0];
    assert_eq!(pending_key, key);
    assert!(menu.apply_dynamic(key, generation, Ok(ready_nodes())));
    assert_eq!(titles(&menu), ["Громкость", "Устройство"]);
    menu.back();
    // Возврат не трогает выбор корня: курсор уже стоит на «Звуке», и
    // двигать его не надо. Со списком-кольцом лишний шаг вниз ушёл бы на
    // первый пункт и открыл бы не тот раздел.
    assert_eq!(menu.enter(), Outcome::Pushed);
    assert_eq!(titles(&menu), ["Громкость", "Устройство"]);
    assert!(
        menu.take_dynamic_requests().is_empty(),
        "кэш: нового запроса нет"
    );
}

/// Устаревший ответ не перетирает уровень и не трогает состояние.
#[test]
fn dynamic_stale_result_is_ignored() {
    let mut menu = dynamic_menu();
    menu.move_sel(1);
    menu.enter();
    let key = ProviderKey::new("test-audio");
    let (_, old) = menu.take_dynamic_requests()[0];
    let fresh = menu.refresh_dynamic(key).expect("обновление");
    assert!(!menu.apply_dynamic(key, old, Ok(ready_nodes())));
    assert_eq!(titles(&menu), ["Загрузка…"]);
    assert!(menu.apply_dynamic(key, fresh, Ok(ready_nodes())));
    assert_eq!(titles(&menu), ["Громкость", "Устройство"]);
}

/// Снимок раздела с личностями строк: заголовок громкости меняется от
/// снимка к снимку, а личность остаётся — на этом и держится курсор.
fn audio_nodes(volume: u8) -> Vec<Node> {
    vec![
        Node::info("v", &format!("Звук: {volume}% вкл")),
        Node::action(
            "−",
            "Тише на 5%",
            Action::RefreshAndRun {
                command: super::super::system::CommandSpec::new("wpctl"),
                refresh: ProviderKey::new("test-audio"),
            },
        )
        .with_id("audio/volume/output/-"),
        Node::submenu("s", "Устройства вывода", vec![Node::info("d", "Speaker")])
            .with_id("audio/devices/sinks"),
    ]
}

/// Путь и курсор переживают `retree`: снимок положения снимается до
/// пересборки дерева и возвращается после. Заголовок строки при этом
/// меняется — если бы курсор искался по нему, он прыгнул бы в начало.
#[test]
fn position_and_restore_keep_the_path_and_the_selected_row() {
    let mut menu = dynamic_menu();
    menu.move_sel(1);
    menu.enter();
    let key = ProviderKey::new("test-audio");
    let (_, generation) = menu.take_dynamic_requests()[0];
    assert!(menu.apply_dynamic(key, generation, Ok(audio_nodes(40))));
    menu.move_sel(1);
    let position = menu.position();

    // Ровно то, что делает окно после действия: дерево пересобрано,
    // снимок провайдера уже с другой громкостью.
    menu.retree(dynamic_tree());
    assert_eq!(menu.depth(), 0, "retree возвращает на корень");
    let key = ProviderKey::new("test-audio");
    let fresh = menu.refresh_dynamic(key).expect("мягкое обновление");
    assert!(
        menu.apply_dynamic(key, fresh, Ok(audio_nodes(45))),
        "свежий снимок принят"
    );
    menu.restore(&position);

    assert_eq!(menu.depth(), 1, "путь восстановлен");
    assert_eq!(menu.opened_submenu(), Some("Звук"));
    let row = menu
        .current()
        .list
        .selected_item()
        .expect("выбранная строка");
    assert_eq!(row.id, "audio/volume/output/-");
    assert_eq!(row.title, "Тише на 5%");
}

/// Исчезнувшая строка не подменяется нулевой: путь и уровень живы, а курсор
/// остаётся пустым до следующего движения.
#[test]
fn restore_clears_selection_when_the_row_disappeared() {
    let tree = |second: bool| {
        let mut nodes = vec![Node::action("a", "Первый", Action::Back).with_id("first")];
        if second {
            nodes.push(Node::action("b", "Второй", Action::Back).with_id("second"));
        }
        nodes
    };
    let mut menu = Menu::new(tree(true));
    menu.move_sel(1);
    assert_eq!(menu.current().list.selected(), Some(1));
    let position = menu.position();
    menu.retree(tree(false));
    menu.restore(&position);
    assert_eq!(menu.current().list.selected(), None);
    assert_eq!(titles(&menu), ["Первый"]);
}

/// Строка с тем же id, но ставшая информационной, больше не выбирается:
/// восстановление не должно оставлять курсор на невыбираемой строке.
#[test]
fn restore_clears_selection_when_the_row_became_info() {
    let mut menu = Menu::new(vec![
        Node::action("a", "Действие", Action::Back).with_id("same"),
    ]);
    assert_eq!(menu.current().list.selected(), Some(0));
    let position = menu.position();
    menu.retree(vec![Node::info("i", "Информация").with_id("same")]);
    menu.restore(&position);
    assert_eq!(menu.current().list.selected(), None);
    assert_eq!(titles(&menu), ["Информация"]);
}

/// Enter без выбора — Idle: на уровне из Header/Info нечего активировать и
/// не должно происходить соседнего действия.
#[test]
fn enter_without_a_selection_is_idle() {
    let mut menu = Menu::new(vec![
        Node::header("Заголовок"),
        Node::info("i", "Информация"),
    ]);
    assert_eq!(menu.current().list.selected(), None);
    assert_eq!(menu.enter(), Outcome::Idle);
    assert_eq!(menu.depth(), 0);
}

/// Refresh после действия не прячет готовый снимок: уровень продолжает
/// показывать прежние строки, пока worker не ответил.
#[test]
fn refresh_after_an_action_keeps_the_previous_rows() {
    let mut menu = dynamic_menu();
    menu.move_sel(1);
    menu.enter();
    let key = ProviderKey::new("test-audio");
    let (_, generation) = menu.take_dynamic_requests()[0];
    assert!(menu.apply_dynamic(key, generation, Ok(audio_nodes(40))));
    let jobs = menu.take_pending_commands();
    assert!(jobs.is_empty(), "команд ещё не было");

    let fresh = menu.refresh_dynamic(key).expect("мягкое обновление");
    assert_eq!(
        titles(&menu),
        ["Звук: 40% вкл", "Тише на 5%", "Устройства вывода"]
    );
    assert!(!menu.apply_dynamic(key, generation, Ok(audio_nodes(99))));
    assert!(menu.apply_dynamic(key, fresh, Ok(audio_nodes(45))));
    assert_eq!(
        titles(&menu),
        ["Звук: 45% вкл", "Тише на 5%", "Устройства вывода"]
    );
}

/// Команда динамического раздела уходит в очередь worker-а, а слот
/// раздела запрашивается отдельно — UI-поток ничего не ждёт.
#[test]
fn run_command_is_queued_with_its_own_generation() {
    let mut menu = dynamic_menu();
    menu.move_sel(1);
    menu.enter();
    let key = ProviderKey::new("test-audio");
    let (_, generation) = menu.take_dynamic_requests()[0];
    assert!(menu.apply_dynamic(key, generation, Ok(audio_nodes(40))));
    menu.move_sel(1);
    assert_eq!(menu.enter(), Outcome::Changed);
    let jobs = menu.take_pending_commands();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].refresh, key);
    assert!(jobs[0].generation > 0);
    assert!(menu.take_pending_commands().is_empty(), "очередь забрана");
}

/// Сети Wi-Fi: личность строки по SSID, поэтому пересканирование и
/// пересборка дерева не уводят курсор на другую сеть.
fn wifi_nodes(current: &str, other: &str) -> Vec<Node> {
    let connect = || Action::RefreshAndRun {
        command: super::super::system::CommandSpec::new("nmcli")
            .arg("connection")
            .arg("up"),
        refresh: ProviderKey::new("test-wifi"),
    };
    vec![
        Node::action("n", &format!("{other} · 78% · защищённая"), connect())
            .with_id(&format!("wifi/net/{other}")),
        Node::action("n", &format!("{current} · 70% · защищённая"), connect())
            .with_id(&format!("wifi/net/{current}")),
        Node::info("p", "Обновить"),
    ]
}

/// Путь и курсор переживают отметку «выполняется» и пересборку дерева:
/// сначала нажали на сеть, потом скан показал другой порядок.
#[test]
fn busy_row_and_cursor_survive_a_rescan() {
    let key = ProviderKey::new("test-wifi");
    let mut menu = Menu::new(vec![Node::dynamic("w", "Wi-Fi", key)]);
    assert_eq!(menu.enter(), Outcome::Pushed);
    let (_, generation) = menu.take_dynamic_requests()[0];
    assert!(menu.apply_dynamic(key, generation, Ok(wifi_nodes("net0", "net1"))));
    menu.move_sel(1);
    let before = menu
        .current()
        .list
        .selected_item()
        .expect("строка выбрана")
        .id
        .clone();
    assert_eq!(before, "wifi/net/net0", "второй строкой идёт текущая сеть");

    // Команда поставлена в очередь: строка помечена, но ещё не ушла.
    assert_eq!(menu.enter(), Outcome::Changed);
    let jobs = menu.take_pending_commands();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].row, before);
    assert_eq!(menu.busy_rows(), vec!["wifi/net/net0".to_string()]);
    let busy_title = menu
        .current()
        .list
        .selected_item()
        .expect("строка на месте")
        .title
        .clone();
    assert!(busy_title.contains(busy_note(Language::Ru)), "{busy_title}");

    // Пересборка дерева и другой порядок сетей не должны сбить курсор.
    let position = menu.position();
    menu.retree(vec![Node::dynamic("w", "Wi-Fi", key)]);
    let fresh = menu.refresh_dynamic(key).expect("мягкое обновление");
    assert!(menu.apply_dynamic(key, fresh, Ok(wifi_nodes("net1", "net0"))));
    menu.restore(&position);
    let row = menu.current().list.selected_item().expect("строка есть");
    assert_eq!(row.id, "wifi/net/net0", "курсор остался на той же сети");
    assert_eq!(
        menu.current().list.selected(),
        Some(0),
        "и переехал на её новое место"
    );
    assert!(
        row.title.contains(busy_note(Language::Ru)),
        "отметка держится: {}",
        row.title
    );

    let id = row.id.clone();
    menu.clear_busy_row(&id);
    assert!(menu.busy_rows().is_empty());
    let row = menu.current().list.selected_item().expect("строка есть");
    assert!(!row.title.contains(busy_note(Language::Ru)));
}

/// Меню на разделе Wi-Fi со строкой защищённой сети без профиля: её
/// действие открывает ввод пароля, а не запускает nmcli.
fn wifi_password_menu() -> Menu {
    let key = ProviderKey::new("test-wifi");
    let tree = vec![Node::dynamic("w", "Wi-Fi", key)];
    let mut menu = Menu::new(tree);
    assert_eq!(menu.enter(), Outcome::Pushed);
    let (_, generation) = menu.take_dynamic_requests()[0];
    let row_id = "wifi/net/Дом \\ принтер".to_string();
    let nodes = vec![
        Node::action(
            "n",
            "Дом \\ принтер · 61% · защищённая",
            Action::SecretInput(super::super::secret::SecretTarget::wifi(
                "Дом \\ принтер",
                &row_id,
            )),
        )
        .with_id(&row_id),
        Node::info("i", "Обновить"),
    ];
    assert!(menu.apply_dynamic(key, generation, Ok(nodes)));
    menu
}

fn type_secret(menu: &mut Menu, text: &str) {
    for ch in text.chars() {
        menu.type_secret_char(ch);
    }
}

/// Защищённая сеть без профиля открывает ввод, и до `Enter` в нём не
/// выполняется ни одной команды.
#[test]
fn protected_network_opens_the_password_input_without_running_anything() {
    let mut menu = wifi_password_menu();
    menu.type_char('Д');
    assert_eq!(menu.query(), "Д", "запрос уровня не должен уехать в пароль");
    assert!(
        menu.take_pending_commands().is_empty(),
        "поиск не запускает команды"
    );
    menu.clear_query();
    assert_eq!(menu.enter(), Outcome::Value);
    assert!(menu.secret_active());
    assert_eq!(menu.query(), "", "ввод не трогает запрос");
    assert!(
        menu.take_pending_commands().is_empty(),
        "пароль ещё не введён"
    );
    let input = menu.secret_input().expect("ввод открыт");
    assert_eq!(input.target.subject(), "Дом \\ принтер");
    assert_eq!(input.target.row_id(), "wifi/net/Дом \\ принтер");
    assert!(input.is_empty());
}

/// Печатные символы, включая юникод и пробелы, идут в буфер; `Backspace`
/// удаляет по символу, а не по байту.
#[test]
fn secret_input_types_unicode_and_erases_by_character() {
    let mut menu = wifi_password_menu();
    menu.enter();
    type_secret(&mut menu, "Пароль 123");
    let input = menu.secret_input().expect("ввод открыт");
    assert_eq!(input.value, "Пароль 123", "пробелы внутри сохраняются");
    assert_eq!(input.len(), 10);
    assert_eq!(input.masked(), "••••••••••");
    menu.secret_backspace();
    assert_eq!(menu.secret_input().expect("ввод").value, "Пароль 12");
    type_secret(&mut menu, "ы");
    assert_eq!(menu.secret_input().expect("ввод").value, "Пароль 12ы");
    menu.secret_backspace();
    assert_eq!(menu.secret_input().expect("ввод").value, "Пароль 12");
    assert_eq!(menu.query(), "", "запрос уровня пуст");
}

/// `Ctrl+U` очищает пароль, но не трогает цель и не закрывает ввод.
#[test]
fn ctrl_u_clears_the_password_but_keeps_the_input_open() {
    let mut menu = wifi_password_menu();
    menu.enter();
    type_secret(&mut menu, "тихийпароль");
    menu.secret_clear();
    let input = menu.secret_input().expect("ввод открыт");
    assert!(input.is_empty());
    assert_eq!(input.target.row_id(), "wifi/net/Дом \\ принтер");
    assert_eq!(input.masked(), "");
}

/// Подтверждение собирает команду с секретом на stdin, кладёт её в
/// очередь worker-а, помечает строку занятой и выбрасывает буфер.
#[test]
fn submit_queues_a_secret_command_and_clears_the_buffer() {
    let mut menu = wifi_password_menu();
    menu.enter();
    type_secret(&mut menu, "тихийпароль");
    assert_eq!(menu.submit_secret_input(), Outcome::Changed);
    assert!(!menu.secret_active(), "ввод закрыт");
    assert!(
        menu.secret_input().is_none(),
        "буфер не должен переживать отправку"
    );
    assert_eq!(menu.query(), "", "запрос не задет");
    let jobs = menu.take_pending_commands();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].row, "wifi/net/Дом \\ принтер");
    // Цель знает свой раздел: это настоящий WIFI, а не ключ тестового
    // дерева, по которому построены строки.
    assert_eq!(jobs[0].refresh, ProviderKey::WIFI);
    let line = jobs[0].commands[0].log_line();
    assert!(line.contains("nmcli --ask device wifi connect"), "{line}");
    assert!(
        !line.contains("тихийпароль"),
        "пароль ушёл в очередь: {line}"
    );
    assert_eq!(
        jobs[0].commands[0].stdin_bytes(),
        Some("тихийпароль".as_bytes()),
        "пароль должен уехать на stdin"
    );
    assert!(jobs[0].commands[0].has_secrets());
    assert_eq!(
        menu.busy_rows(),
        vec!["wifi/net/Дом \\ принтер".to_string()],
        "строка помечена занятой"
    );
    let status = menu.status().expect("статус есть").clone();
    assert!(!status.text().contains("тихийпароль"), "{status:?}");
    assert!(
        menu.current()
            .list
            .selected_item()
            .expect("строка")
            .title
            .contains(busy_note(Language::Ru))
    );
}

/// Отмена не запускает ничего, пароль пропадает, уровень и выбор на месте.
#[test]
fn escape_cancels_the_input_and_keeps_the_row_and_the_query() {
    let mut menu = wifi_password_menu();
    menu.type_char('h');
    menu.enter();
    type_secret(&mut menu, "не_сохраняй");
    let position = menu.position();
    assert_eq!(menu.cancel_secret_input(), Outcome::Value);
    assert!(!menu.secret_active());
    assert!(
        menu.take_pending_commands().is_empty(),
        "отмена без команды"
    );
    assert_eq!(menu.query(), "h", "запрос уровня не тронут");
    assert!(menu.busy_rows().is_empty());
    let current = menu.current().list.selected_item().expect("строка");
    assert_eq!(current.id, "wifi/net/Дом \\ принтер");
    assert_eq!(menu.position(), position, "положение меню не изменилось");
    // Повторная отмена без ввода — no-op, а не паника.
    assert_eq!(menu.cancel_secret_input(), Outcome::Idle);
}

/// Кадр в режиме ввода содержит только маску: пароля в нём нет ни в
/// заголовке, ни в запросе, ни в статусе.
#[test]
fn frame_carries_only_the_mask() {
    let mut menu = wifi_password_menu();
    menu.enter();
    type_secret(&mut menu, "p@ss:word");
    let frame = menu.frame();
    assert_eq!(frame.query, "", "запрос пуст");
    let secret = frame.secret.clone().expect("ввод виден в кадре");
    assert_eq!(secret.subject, "Дом \\ принтер");
    assert_eq!(secret.masked, "•••••••••");
    assert_eq!(secret.len, 9);
    let everything = format!("{frame:?}");
    for text in [
        frame.query.as_str(),
        secret.masked.as_str(),
        secret.subject.as_str(),
    ] {
        assert!(!text.contains("p@ss"), "{text}");
    }
    assert!(!everything.contains("p@ss"), "пароль не попал в кадр");
    menu.submit_secret_input();
    let after = menu.frame();
    assert!(after.secret.is_none(), "после отправки ввода в кадре нет");
    assert!(
        !format!("{after:?}").contains("p@ss"),
        "и в кадре после отправки пароля нет"
    );
}

/// Пустой пароль отправляется как есть: правил паролей тут не изобретаем.
/// Пустой stdin — это ровно то, что ввели.
#[test]
fn empty_password_is_sent_to_nmcli_unchanged() {
    let mut menu = wifi_password_menu();
    menu.enter();
    assert_eq!(menu.submit_secret_input(), Outcome::Changed);
    let jobs = menu.take_pending_commands();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].commands[0].stdin_bytes(), Some(&[][..]));
    assert!(jobs[0].commands[0].has_secrets());
}

/// Строка сети остаётся выбранной и после отправки команды, и после
/// прихода свежего снимка: курсор идёт по личности, а не по позиции.
#[test]
fn cursor_and_path_survive_the_password_flow() {
    let mut menu = wifi_password_menu();
    menu.enter();
    type_secret(&mut menu, "pw");
    let position = menu.position();
    menu.submit_secret_input();
    let key = ProviderKey::new("test-wifi");
    let fresh = menu.refresh_dynamic(key).expect("мягкое обновление");
    // Сеть остаётся той же по id: пересканирование меняет порядок и
    // заголовок, но не личность строки.
    assert!(menu.apply_dynamic(key, fresh, Ok(wifi_nodes("Дом \\ принтер", "other"))));
    menu.restore(&position);
    assert_eq!(menu.depth(), 1, "мы на уровне Wi-Fi");
    assert_eq!(menu.opened_submenu(), Some("Wi-Fi"));
    assert_eq!(
        menu.current()
            .list
            .selected_item()
            .expect("выбранная строка восстановлена")
            .id,
        "wifi/net/Дом \\ принтер"
    );
    assert_eq!(menu.current().list.selected(), Some(1));
}

/// Async-цепочка подключения с паролем: Enter ничего не запускает, в
/// очередь попадает одна команда, устаревший ответ worker-а не
/// перетирает свежий, а отметка «выполняется» снимается по `row`.
#[test]
fn password_connect_runs_through_the_worker_queue() {
    let mut menu = wifi_password_menu();
    menu.enter();
    type_secret(&mut menu, "pw");
    assert_eq!(menu.submit_secret_input(), Outcome::Changed);
    let jobs = menu.take_pending_commands();
    assert_eq!(jobs.len(), 1, "Enter не блокирует и не дублирует команду");
    let row = jobs[0].row.clone();

    let key = ProviderKey::new("test-wifi");
    let first = menu.refresh_dynamic(key).expect("обновление после команды");
    let stale = first - 1;
    assert!(
        !menu.apply_dynamic(key, stale, Ok(Vec::new())),
        "устаревший ответ отброшен"
    );
    let second = menu.refresh_dynamic(key).expect("второе обновление");
    assert!(menu.apply_dynamic(key, second, Ok(wifi_nodes("net0", "net1"))));

    // Ответ пришёл: строка перестаёт быть занятой.
    menu.clear_busy_row(&row);
    assert!(menu.busy_rows().is_empty());
    assert_eq!(menu.depth(), 1, "меню осталось на уровне Wi-Fi");
}

/// Даже если действие кто-то попробует выполнить напрямую, пароля у
/// исполнителя нет: команда составляется только из буфера ввода.
#[test]
fn secret_input_action_is_a_no_op_for_the_runner() {
    use super::super::action::RecordingRunner;
    let target = super::super::secret::SecretTarget::wifi("Дом", "wifi/net/Дом");
    let action = Action::SecretInput(target);
    let runner = RecordingRunner::new();
    action.perform_with(&runner).expect("выполнение не падает");
    assert!(
        runner.calls().is_empty(),
        "исполнитель не запускает nmcli: {:?}",
        runner.calls()
    );
    assert!(!action.closes_menu(), "меню остаётся открытым");
    assert!(action.command().is_none());
    assert_eq!(action.describe(Language::Ru), "Дом");
}

/// Глобальный поиск находит «Wi-Fi» и по `Enter` открывает раздел, а не
/// ввод пароля: пароль начинается только со строки сети.
#[test]
fn search_opens_the_wifi_section_without_entering_a_password() {
    let key = ProviderKey::new("test-wifi");
    let tree = vec![
        Node::submenu("s", "Система", vec![Node::dynamic("w", "Wi-Fi", key)]),
        Node::submenu(
            "z",
            "Захват",
            vec![Node::toggle("c", "Микрофон", || true, |_| {})],
        ),
    ];
    let mut menu = Menu::new(tree);
    for ch in "Wi-Fi".chars() {
        menu.type_char(ch);
    }
    let rows = menu.frame().items;
    let hit = rows
        .iter()
        .find(|item| item.caption.is_some())
        .expect("найден раздел с путём");
    assert!(hit.title.contains("Wi-Fi"));
    assert_eq!(menu.enter(), Outcome::Pushed);
    assert!(!menu.secret_active(), "поиск в пароль не превращается");
    assert_eq!(menu.depth(), 1);
    assert_eq!(menu.opened_submenu(), Some("Система"));
}

/// Список Bluetooth-устройств как строки уровня: identity по MAC. Поведение
/// строки здесь не важно — проверяется только перенос курсора по id.
fn bluetooth_nodes(order: &[&str]) -> Vec<Node> {
    let mut rows = vec![Node::info("b", "Bluetooth: вкл")];
    rows.push(Node::submenu(
        "p",
        "Сопряжённые",
        order
            .iter()
            .map(|mac| {
                Node::action("d", &format!("Устройство {mac}"), Action::Back)
                    .with_id(&format!("bluetooth/device/{mac}"))
            })
            .collect(),
    ));
    rows.push(Node::submenu(
        "f",
        "Найденные",
        vec![Node::info("d", "Нет устройств")],
    ));
    rows.push(Node::action(
        "u",
        "Обновить",
        Action::RefreshDynamic(ProviderKey::BLUETOOTH),
    ));
    rows
}

/// Курсор в разделе Bluetooth переживает refresh: устройство выбрано по
/// MAC, а не по позиции, поэтому перестановка списка его не сдвигает.
#[test]
fn bluetooth_cursor_follows_the_mac_across_a_refresh() {
    let key = ProviderKey::BLUETOOTH;
    let mut menu = Menu::new(vec![Node::dynamic("b", "Bluetooth", key)]);
    assert_eq!(menu.enter(), Outcome::Pushed);
    let (_, generation) = menu.take_dynamic_requests()[0];
    assert!(menu.apply_dynamic(
        key,
        generation,
        Ok(bluetooth_nodes(&[
            "AA:BB:CC:DD:EE:01",
            "AA:BB:CC:DD:EE:02",
            "AA:BB:CC:DD:EE:03",
        ]))
    ));
    // Открываем «Сопряжённые»: вторая строка раздела.
    menu.move_sel(1);
    assert_eq!(menu.enter(), Outcome::Pushed);
    menu.move_sel(1);
    let before = menu
        .current()
        .list
        .selected_item()
        .expect("строка")
        .id
        .clone();
    assert_eq!(before, "bluetooth/device/AA:BB:CC:DD:EE:02");

    let position = menu.position();
    let fresh = menu.refresh_dynamic(key).expect("обновление");
    assert!(menu.apply_dynamic(
        key,
        fresh,
        Ok(bluetooth_nodes(&[
            "AA:BB:CC:DD:EE:02",
            "AA:BB:CC:DD:EE:01",
            "AA:BB:CC:DD:EE:03",
        ]))
    ));
    menu.restore(&position);
    assert_eq!(menu.depth(), 2, "остались на уровне устройств");
    assert_eq!(
        menu.current().list.selected_item().expect("строка").id,
        "bluetooth/device/AA:BB:CC:DD:EE:02",
        "курсор остался на том же устройстве"
    );
    assert_eq!(
        menu.current().list.selected(),
        Some(0),
        "и переехал на новое место в списке"
    );
}

/// Устаревший ответ провайдера Bluetooth не перетирает свежий снимок.
#[test]
fn stale_bluetooth_result_is_ignored() {
    let key = ProviderKey::BLUETOOTH;
    let mut menu = Menu::new(vec![Node::dynamic("b", "Bluetooth", key)]);
    menu.enter();
    let (_, generation) = menu.take_dynamic_requests()[0];
    let fresh = menu.refresh_dynamic(key).expect("обновление");
    assert!(!menu.apply_dynamic(key, generation, Ok(bluetooth_nodes(&["AA:BB:CC:DD:EE:09"]))));
    assert!(menu.apply_dynamic(key, fresh, Ok(bluetooth_nodes(&["AA:BB:CC:DD:EE:01"]))));
    let paired = menu
        .current()
        .list
        .items()
        .iter()
        .find(|item| item.title == "Сопряжённые")
        .expect("список есть");
    assert_eq!(paired.kind, ItemKind::Submenu);
}

/// Ошибка даёт строку сообщения и пункт обновления; Enter по нему
/// запускает новое поколение.
#[test]
fn dynamic_error_shows_message_and_refresh_row() {
    let mut menu = dynamic_menu();
    menu.move_sel(1);
    menu.enter();
    let key = ProviderKey::new("test-audio");
    let (_, generation) = menu.take_dynamic_requests()[0];
    assert!(menu.apply_dynamic(key, generation, Err("демон молчит".to_string())));
    assert_eq!(titles(&menu), ["демон молчит", "Обновить"]);
    menu.move_sel(1);
    assert_eq!(menu.enter(), Outcome::Changed);
    let pending = menu.take_dynamic_requests();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].1, generation + 1);
    assert_eq!(titles(&menu), ["Загрузка…"]);
}

/// Пустой готовый список — одна строка, а не пустой уровень.
#[test]
fn dynamic_empty_ready_is_one_row() {
    let mut menu = dynamic_menu();
    menu.move_sel(1);
    menu.enter();
    let key = ProviderKey::new("test-audio");
    let (_, generation) = menu.take_dynamic_requests()[0];
    assert!(menu.apply_dynamic(key, generation, Ok(Vec::new())));
    assert_eq!(menu.frame().items.len(), 1);
}

/// Недоступный раздел скрывается обычным `visible`, как отсутствующие
/// программы у статических пунктов.
#[test]
fn dynamic_section_hides_through_visible() {
    let menu = Menu::new(vec![
        Node::dynamic("a", "Звук", ProviderKey::AUDIO).when(|| false),
        Node::toggle("dnd", "Не беспокоить", || true, |_| {}),
    ]);
    assert_eq!(titles(&menu), ["Не беспокоить"]);
}

/// Динамические состояния рисуются существующим рендером карточки.
#[test]
fn dynamic_states_render_with_the_plain_card_renderer() {
    for result in [
        None,
        Some(Ok(ready_nodes())),
        Some(Ok(Vec::new())),
        Some(Err("демон молчит".to_string())),
    ] {
        let mut menu = dynamic_menu();
        menu.move_sel(1);
        menu.enter();
        if let Some(result) = result {
            let key = ProviderKey::new("test-audio");
            let (_, generation) = menu.take_dynamic_requests()[0];
            assert!(menu.apply_dynamic(key, generation, result));
        }
        menu.set_page(10);
        let pixmap = render_menu_card(&menu);
        assert!(pixmap.width() > 0 && pixmap.height() > 0);
    }
}

fn nested_nodes(state: &str) -> Vec<Node> {
    vec![Node::submenu(
        "p",
        "Устройства",
        vec![Node::info("d", state).with_id("dev/1")],
    )]
}

/// Путь окна — apply_dynamic без position/restore, пока открыт вложенный уровень.
#[test]
fn nested_level_follows_a_fresh_snapshot() {
    let key = ProviderKey::BLUETOOTH;
    let mut menu = Menu::new(vec![Node::dynamic("b", "Bluetooth", key)]);
    menu.enter();
    let (_, g) = menu.take_dynamic_requests()[0];
    assert!(menu.apply_dynamic(key, g, Ok(nested_nodes("не подключено"))));
    assert_eq!(menu.enter(), Outcome::Pushed); // вошли в «Устройства»
    assert_eq!(menu.depth(), 2);
    let fresh = menu.refresh_dynamic(key).expect("refresh");
    assert!(menu.apply_dynamic(key, fresh, Ok(nested_nodes("подключено"))));
    let shown = menu.current().list.items()[0].title.clone();
    assert_eq!(shown, "подключено", "вложенный уровень не обновился");
}

#[test]
fn vanished_nested_node_returns_to_the_live_level() {
    let key = ProviderKey::BLUETOOTH;
    let mut menu = Menu::new(vec![Node::dynamic("b", "Bluetooth", key)]);
    menu.enter();
    let (_, g) = menu.take_dynamic_requests()[0];
    menu.apply_dynamic(key, g, Ok(nested_nodes("не подключено")));
    menu.enter();
    let fresh = menu.refresh_dynamic(key).expect("refresh");
    menu.apply_dynamic(key, fresh, Ok(vec![Node::info("x", "СОВСЕМ НОВЫЙ СПИСОК")]));
    assert_eq!(
        menu.depth(),
        1,
        "исчезнувший узел возвращает на живой уровень"
    );
    let shown = menu.current().list.items()[0].title.clone();
    assert_eq!(shown, "СОВСЕМ НОВЫЙ СПИСОК", "родитель не обновился");
}

#[test]
fn busy_marker_survives_a_plain_snapshot() {
    let key = ProviderKey::new("test-wifi");
    let mut menu = Menu::new(vec![Node::dynamic("w", "Wi-Fi", key)]);
    menu.enter();
    let (_, g) = menu.take_dynamic_requests()[0];
    menu.apply_dynamic(key, g, Ok(wifi_nodes("net0", "net1")));
    menu.move_sel(1);
    assert_eq!(menu.enter(), Outcome::Changed); // net0 busy
    let _ = menu.take_pending_commands();
    // приходит чужой снимок (например, ручной rescan), без restore — как в окне
    let fresh = menu.refresh_dynamic(key).expect("refresh");
    assert!(menu.apply_dynamic(key, fresh, Ok(wifi_nodes("net0", "net1"))));
    let t = menu
        .current()
        .list
        .items()
        .iter()
        .find(|i| i.id == "wifi/net/net0")
        .unwrap()
        .title
        .clone();
    assert!(t.contains(busy_note(Language::Ru)), "метка потеряна: {t}");
}

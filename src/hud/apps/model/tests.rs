use super::*;
use std::path::PathBuf;

fn app(id: &str, name: &str) -> App {
    App {
        id: id.into(),
        name: name.into(),
        name_plain: name.into(),
        generic: String::new(),
        keywords: Vec::new(),
        exec: "true".into(),
        icon: String::new(),
        terminal: false,
        dbus_activatable: false,
        path: PathBuf::from(format!("{id}.desktop")),
    }
}

fn model(apps: &[(&str, &str)], pinned: &[&str], page: usize) -> Model {
    let apps: Vec<App> = apps.iter().map(|(id, name)| app(id, name)).collect();
    let pinned: Vec<String> = pinned.iter().map(|id| (*id).to_string()).collect();
    Model::from_apps(apps, pinned, page, Language::Ru)
}

fn names(model: &Model) -> Vec<&str> {
    model
        .rows()
        .iter()
        .map(|row| match row {
            Row::Section(title) => title.as_str(),
            Row::App(index) => model.apps()[*index].name.as_str(),
            Row::Blank => "·",
        })
        .collect()
}

#[test]
fn an_empty_list_starts_without_a_selection() {
    let model = model(&[], &[], 10);
    assert_eq!(model.total(), 0);
    assert_eq!(model.selected(), usize::MAX);
    assert!(model.selected_app().is_none());
    assert!(model.visible().is_empty());
}

#[test]
fn pinned_apps_get_their_own_section_first() {
    let model = model(
        &[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")],
        &["c", "a"],
        10,
    );
    assert_eq!(
        names(&model),
        vec!["Закреплённые", "Gamma", "Alpha", "Все приложения", "Beta"]
    );
}

/// Порядок в секции — порядок закрепления, а не алфавит: так список
/// собирают осмысленно, «первым тем, чем пользуешься».
#[test]
fn the_pinned_section_keeps_the_pin_order() {
    let model = model(
        &[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")],
        &["b", "a"],
        10,
    );
    let names = names(&model);
    let beta = names.iter().position(|name| *name == "Beta").expect("Beta");
    let alpha = names
        .iter()
        .position(|name| *name == "Alpha")
        .expect("Alpha");
    assert!(beta < alpha, "Beta закреплён первым и идёт первым");
}

/// Удалённое приложение не должно оставлять после себя пустой заголовок
/// секции: на экране была бы строка, за которой ничего нет.
#[test]
fn a_stale_pin_leaves_no_empty_section() {
    let model = model(&[("a", "Alpha")], &["ghost"], 10);
    assert_eq!(names(&model), vec!["Alpha"]);
}

#[test]
fn one_section_is_enough_when_nothing_is_pinned() {
    let model = model(&[("a", "Alpha"), ("b", "Beta")], &[], 10);
    assert_eq!(names(&model), vec!["Alpha", "Beta"]);
}

#[test]
fn a_query_drops_the_sections() {
    let mut model = model(&[("a", "Alpha"), ("b", "Beta")], &["b"], 10);
    // Запрос подбирается так, чтобы подходило ровно одно приложение: иначе
    // проверка на секции зависела бы от того, где внутри слова встретилась
    // буква запроса.
    model.type_char('l');
    assert_eq!(names(&model), vec!["Alpha"], "поиск ищет по всем секциям");
    assert!(model.searching());
}

#[test]
fn typing_resets_the_selection_to_the_first_hit() {
    let mut model = model(&[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")], &[], 10);
    model.select(2);
    model.type_char('a');
    assert_eq!(
        model.selected(),
        0,
        "курсор после буквы — на первом найденном"
    );
    assert_eq!(model.top(), 0);
}

#[test]
fn backspace_and_clear_restore_the_full_list() {
    let mut model = model(&[("a", "Alpha"), ("b", "Beta")], &[], 10);
    model.type_char('b');
    assert_eq!(model.total(), 1);
    model.backspace();
    assert_eq!(model.total(), 2);
    model.type_char('x');
    assert_eq!(model.total(), 0);
    assert_eq!(
        model.selected(),
        usize::MAX,
        "пустой результат — выбора нет"
    );
    model.clear_query();
    assert_eq!(model.total(), 2);
    assert_eq!(model.selected(), 0);
}

#[test]
fn a_clear_on_an_empty_query_changes_nothing() {
    let mut model = model(&[("a", "Alpha")], &[], 10);
    model.clear_query();
    assert!(!model.searching());
    model.backspace();
    assert!(!model.searching());
}

#[test]
fn control_characters_never_enter_the_query() {
    let mut model = model(&[("a", "Alpha")], &[], 10);
    model.type_char('\n');
    model.type_char('\t');
    assert!(!model.searching());
}

/// Заголовки секций нельзя выбрать: стрелка перешагивает их молча.
#[test]
fn movement_skips_section_headers() {
    let mut model = model(&[("a", "Alpha"), ("b", "Beta")], &["a"], 10);
    assert_eq!(names(&model)[0], "Закреплённые");
    assert_eq!(model.selected(), 1, "выбор сразу на первом приложении");
    model.move_sel(1);
    assert_eq!(model.selected(), 3, "заголовок «Все приложения» пропущен");
    model.move_sel(1);
    assert_eq!(model.selected(), 3, "за нижним краем списка ничего нет");
    model.move_sel(-1);
    assert_eq!(model.selected(), 1);
    model.move_sel(-1);
    assert_eq!(model.selected(), 1, "перед первым приложением ничего нет");
}

#[test]
fn paging_keeps_the_relative_position() {
    let apps: Vec<App> = (0..30).map(|i| app(&format!("app{i}"), "App")).collect();
    let mut model = Model::from_apps(apps, Vec::new(), 10, Language::Ru);
    model.select(2);
    model.page(1);
    assert_eq!(
        model.selected(),
        12,
        "третья строка + страница = тринадцатая"
    );
    model.page(-1);
    assert_eq!(model.selected(), 2);
    model.page(-1);
    assert_eq!(
        model.selected(),
        0,
        "вверх доходит до начала и там останавливается"
    );
}

#[test]
fn the_window_scrolls_only_when_the_selection_leaves_it() {
    let mut model = model(&[("a", "A"), ("b", "B"), ("c", "C")], &[], 2);
    model.select(2);
    assert_eq!(
        model.top(),
        1,
        "третья строка в окне из двух требует прокрутки"
    );
    model.select(1);
    assert_eq!(
        model.top(),
        1,
        "строка всё ещё видима — список не дёргается"
    );
    model.select(0);
    assert_eq!(model.top(), 0);
    assert_eq!(model.visible().len(), 2);
}

#[test]
fn jumps_land_on_apps_not_headers() {
    let mut model = model(&[("a", "A"), ("b", "B")], &["b"], 10);
    model.jump(true);
    assert_eq!(model.selected(), 3, "последнее приложение, а не заголовок");
    model.jump(false);
    assert_eq!(model.selected(), 1);
}

/// Ctrl+↑↓ двигают только закреплённые: строку из «все приложения» и
/// выдачу поиска трогать нечего, и до файла дело не доходит.
#[test]
fn reordering_is_only_for_pinned_apps() {
    let mut model = model(&[("a", "A"), ("b", "B")], &["a"], 10);
    model.select(3);
    assert!(!model.move_pinned(-1), "строка из «все приложения»");
    model.type_char('b');
    model.select(0);
    assert!(
        !model.move_pinned(1),
        "в выдаче поиска порядок задаёт запрос"
    );
}

/// Открепление оставляет вид на месте: на освободившейся строке
/// оказывается следующее закреплённое, а прокрутка не прыгает в начало.
#[test]
fn unpinning_keeps_the_row_and_the_scroll() {
    let mut model = model(&[("a", "A"), ("b", "B"), ("c", "C")], &["a", "b", "c"], 2);
    // Строки: заголовок, A, B, C, заголовок, остальные.
    model.select(2);
    let top = model.top();
    assert!(top > 0, "список прокручен до B");
    // Имитация открепления B: список пересобран без неё.
    model.pinned.retain(|id| id != "b");
    model.rebuild();
    model.keep_row(2, top);
    assert_eq!(
        model
            .app_at(&model.rows()[model.selected()])
            .map(|a| a.name.as_str()),
        Some("C"),
        "на месте B стоит C"
    );
    assert_eq!(model.top(), top, "прокрутка осталась на месте");
}

/// Если ушло последнее закреплённое, на его строке теперь заголовок
/// «Все приложения»: выбор поднимается к соседу выше, а не уезжает вниз.
#[test]
fn unpinning_the_last_pinned_steps_up_to_the_neighbour() {
    let mut model = model(
        &[("a", "A"), ("b", "B"), ("c", "C"), ("d", "D")],
        &["a", "b", "c"],
        10,
    );
    // Строки: «Закреплённые», A, B, C, «Все приложения», D.
    model.select(3); // C, последняя закреплённая
    model.pinned.retain(|id| id != "c");
    model.rebuild();
    model.keep_row(3, 0);
    assert_eq!(
        model
            .app_at(&model.rows()[model.selected()])
            .map(|a| a.name.as_str()),
        Some("B"),
        "на месте заголовка выбор поднялся к B"
    );
}

#[test]
fn the_last_page_is_not_padded_with_empty_rows() {
    let mut model = model(&[("a", "A"), ("b", "B"), ("c", "C")], &[], 10);
    model.jump(true);
    assert_eq!(
        model.visible().len(),
        3,
        "в окне ровно столько строк, сколько есть"
    );
    assert_eq!(model.top(), 0);
}

/// Закрепление пишет в файл пользователя, поэтому модель с ним в тестах не
/// проверяется: проверяется разбор списка и следование выбора.
#[test]
fn the_selection_follows_a_row_that_moves_between_sections() {
    let model = model(&[("a", "A"), ("b", "B")], &[], 10);
    let mut moved = model;
    // Ручная имитация: строка «B» переехала в закреплённые, список
    // пересобран, выбор обязан уехать за ней.
    moved.pinned.push("b".into());
    moved.rebuild();
    let names = names(&moved);
    let beta = names.iter().position(|name| *name == "B").expect("Beta");
    moved.select(beta);
    assert_eq!(moved.selected(), beta);
    assert!(moved.is_pinned("b"));
    assert_eq!(moved.pinned(), ["b".to_string()]);
}

/// Счётчик подвала считает приложения, а не строки: заголовки секций в
/// счёт не идут ни в позицию, ни в итог.
#[test]
fn the_app_count_ignores_section_headers() {
    let model = model(
        &[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")],
        &["a", "b"],
        10,
    );
    // Две секции: заголовок, два закреплённых, заголовок, одно прочее.
    assert_eq!(model.total(), 5);
    assert_eq!(model.app_count(), 3);
    assert_eq!(model.app_position(0), None, "заголовок позиции не имеет");
    assert_eq!(model.app_position(1), Some(1));
    assert_eq!(model.app_position(2), Some(2));
    assert_eq!(model.app_position(3), None, "заголовок позиции не имеет");
    assert_eq!(model.app_position(4), Some(3));
    assert_eq!(model.app_position(99), None);
}

/// При поиске секций нет, и позиция совпадает с номером строки.
#[test]
fn a_filtered_list_positions_by_apps_only() {
    let mut model = model(&[("a", "Alpha"), ("b", "Beta")], &["b"], 10);
    model.type_char('l');
    assert_eq!(model.total(), 1);
    assert_eq!(model.app_count(), 1);
    assert_eq!(model.app_position(0), Some(1));
}

/// Секция «Закреплённые» помечается по строкам, а не по индексу: её
/// содержимое меняется при закреплении, а заголовок — нет.
#[test]
fn the_pinned_section_is_marked_row_by_row() {
    let model = model(&[("a", "Alpha"), ("b", "Beta")], &["a"], 10);
    let marks: Vec<bool> = (0..model.total())
        .map(|row| model.in_pinned_section(row))
        .collect();
    // Строки: заголовок «Закреплённые», Alpha, заголовок «Все приложения»,
    // Beta. Помечены заголовок первой секции и её содержимое.
    assert_eq!(
        marks,
        vec![true, true, false, false],
        "заголовок и содержимое секции помечены, вторая секция — нет"
    );
    assert!(
        !model.in_pinned_section(99),
        "за пределами списка секций нет"
    );
}

/// Название секции из языка: помечение не должно зависеть от того, как
/// строка называется в списке.
#[test]
fn the_pinned_section_is_found_in_both_languages() {
    for lang in [Language::Ru, Language::En] {
        let model = Model::from_apps(
            vec![app("a", "Alpha"), app("b", "Beta")],
            vec!["a".to_string()],
            10,
            lang,
        );
        let first_app = model.first_app_row();
        assert!(
            model.in_pinned_section(first_app),
            "{lang:?}: первое приложение в закреплённых"
        );
    }
}

#[test]
fn section_titles_follow_the_language() {
    let apps: Vec<App> = vec![app("a", "A")];
    let model = Model::from_apps(apps, vec!["a".to_string()], 10, Language::En);
    let names: Vec<&str> = model
        .rows()
        .iter()
        .map(|row| match row {
            Row::Section(title) => title.as_str(),
            Row::App(index) => model.apps()[*index].name.as_str(),
            Row::Blank => "·",
        })
        .collect();
    assert_eq!(names[0], "Pinned");
}

// ───────────────────────── плитки ─────────────────────────

/// Сетка не рисует заголовки секций: закреплённые просто идут первыми.
#[test]
fn tiles_put_pinned_first_and_drop_the_headers() {
    let mut model = model(
        &[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")],
        &["c", "a"],
        10,
    );
    model.toggle_view();
    assert_eq!(model.view(), ViewMode::Tiles);
    assert_eq!(
        names(&model),
        vec!["Gamma", "Alpha", "·", "·", "Beta"],
        "ряд закреплённых добит пустыми клетками, чтобы блок кончался на границе ряда"
    );
    assert!(
        model
            .rows()
            .iter()
            .all(|row| !matches!(row, Row::Section(_))),
        "в сетке заголовков нет"
    );
    assert_eq!(model.tiles_break(), Some(GRID_COLS));
}

/// Пустые клетки в хвосте ряда закреплённых не выбираются: стрелки едут
/// к приложениям, а порядок обхода не зависит от добивки.
#[test]
fn the_padding_cells_are_not_selectable() {
    let apps: Vec<App> = (0..10).map(|i| app(&format!("app{i}"), "App")).collect();
    let pinned: Vec<String> = (0..5).map(|i| format!("app{i}")).collect();
    let mut model = Model::from_apps(apps, pinned, 10, Language::Ru);
    model.toggle_view();
    // Пять закреплённых: второй ряд — одна плитка и три пустые клетки.
    assert_eq!(model.tiles_break(), Some(2 * GRID_COLS));
    model.select(GRID_COLS - 1);
    model.move_sel(1);
    assert_eq!(
        model.selected(),
        3 * GRID_COLS - 1,
        "вниз с последней колонки — через пустую клетку туда же в следующем ряду"
    );
    model.select(GRID_COLS);
    model.move_col(1);
    assert_eq!(
        model.selected(),
        GRID_COLS,
        "вправо от единственной плитки ряда — никуда, пустые клетки не выбираются"
    );
    model.move_sel(1);
    assert_eq!(
        model.selected(),
        2 * GRID_COLS,
        "вниз — на первое незакреплённое"
    );
}

/// Вбок выбор идёт по плиткам ряда и не перескакивает на соседний ряд;
/// вниз-вверх — через ряд целиком.
#[test]
fn grid_navigation_keeps_the_column() {
    let apps: Vec<App> = (0..10).map(|i| app(&format!("app{i}"), "App")).collect();
    let mut model = Model::from_apps(apps, Vec::new(), 10, Language::Ru);
    model.toggle_view();
    assert_eq!(model.selected(), 0);
    assert_eq!(
        model.tiles_break(),
        None,
        "закреплённых нет — делить нечего"
    );
    model.move_col(1);
    assert_eq!(model.selected(), 1);
    model.move_col(-1);
    assert_eq!(model.selected(), 0);
    model.move_col(-1);
    assert_eq!(model.selected(), 0, "левее первой колонки ничего нет");
    model.move_sel(1);
    assert_eq!(model.selected(), GRID_COLS, "вниз — на ряд ниже");
    model.move_sel(-1);
    assert_eq!(model.selected(), 0, "вверх — на ряд выше");
    model.select(3);
    model.move_col(1);
    assert_eq!(model.selected(), 3, "правее последней колонки ничего нет");
    model.select(8);
    model.move_sel(1);
    assert_eq!(model.selected(), 8, "ниже последнего ряда ничего нет");
}

/// Смена вида не должна уводить выбор с приложения, которое человек нашёл.
#[test]
fn toggling_the_view_keeps_the_selected_app() {
    let mut model = model(&[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")], &["a"], 10);
    model.select_app("b");
    model.toggle_view();
    assert_eq!(
        model.selected_app().map(|app| app.name.as_str()),
        Some("Beta")
    );
    assert_eq!(model.top(), 0);
    model.toggle_view();
    assert_eq!(
        model.selected_app().map(|app| app.name.as_str()),
        Some("Beta")
    );
    assert_eq!(model.view(), ViewMode::List);
}

/// Прокрутка сетки идёт рядами: верх всегда кратен числу колонок, иначе
/// первая строка окна была бы неполной.
#[test]
fn the_tile_grid_scrolls_by_whole_rows() {
    let apps: Vec<App> = (0..19).map(|i| app(&format!("app{i}"), "App")).collect();
    let mut model = Model::from_apps(apps, Vec::new(), 10, Language::Ru);
    model.toggle_view();
    assert_eq!(model.visible().len(), GRID_COLS * MAX_TILE_ROWS);
    model.select(18);
    assert_eq!(model.top(), GRID_COLS, "последний ряд подтянул окно");
    assert_eq!(model.top() % GRID_COLS, 0);
    model.select(3);
    assert_eq!(model.top(), 0, "верхний ряд снова виден");
}

/// Страница в сетке — это окно целиком, а не десять плиток списка.
#[test]
fn a_page_in_the_grid_moves_by_the_whole_window() {
    let apps: Vec<App> = (0..40).map(|i| app(&format!("app{i}"), "App")).collect();
    let mut model = Model::from_apps(apps, Vec::new(), 10, Language::Ru);
    model.toggle_view();
    model.page(1);
    assert_eq!(model.selected(), GRID_COLS * MAX_TILE_ROWS);
    model.page(-1);
    assert_eq!(model.selected(), 0);
}

/// Открепление в сетке тоже оставляет выбор на месте: на освободившейся
/// плитке оказывается следующее приложение, а прокрутка не прыгает.
#[test]
fn unpinning_a_tile_keeps_the_cell() {
    let pinned: Vec<String> = (0..5).map(|i| format!("app{i}")).collect();
    let apps: Vec<App> = (0..10).map(|i| app(&format!("app{i}"), "App")).collect();
    let mut model = Model::from_apps(apps, pinned, 10, Language::Ru);
    model.toggle_view();
    model.select(2);
    let top = model.top();
    model.pinned.retain(|id| id != "app2");
    model.rebuild();
    model.keep_row(2, top);
    assert_eq!(
        model.selected_app().map(|app| app.id.as_str()),
        Some("app3"),
        "на месте откреплённой плитки стоит следующая"
    );
    assert_eq!(model.top(), top, "окно осталось на месте");
}

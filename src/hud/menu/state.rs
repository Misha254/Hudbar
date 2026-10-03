//! Состояние меню: стек уровней и операции над ним.
//!
//! Меню — это стек: корень, поверх него подменю, поверх того ещё одно. Каждый
//! уровень держит свой `ItemList` со своим запросом и выбором, поэтому
//! возвращение назад возвращает вид именно этого уровня, а не соседнего.
//!
//! Операции не знают про Wayland и про отрисовку: `enter` возвращает
//! `Outcome`, и окно в M4 решает по нему, что показать. Это позволяет
//! проверять всю навигацию в тестах без композитора.

use super::item::{Item, ItemKind, ItemList};
use super::tree::{Node, NodeKind, clamp_number, level_rows, search_rows, step_number};

/// Что изменилось в меню — окну нужно это, чтобы решить, что делать дальше.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Ничего не изменилось: пустой список или пункт, который нечего открывать.
    Idle,
    /// Открыт вложенный уровень.
    Pushed,
    /// Возврат на уровень выше.
    Popped,
    /// Меню закрыто: Esc на корне.
    Closed,
    /// Значение пункта изменилось, строка перерисована.
    Changed,
    /// Открыт выбор значения (Choice, Number, Picker) — в M4 это подменю.
    Value,
}

/// Сообщение в правой части подвала: вместо счётчика «1/8».
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Изменение применено.
    Applied(String),
    /// Не получилось.
    Failed(String),
}

impl Status {
    pub fn text(&self) -> &str {
        match self {
            Status::Applied(text) | Status::Failed(text) => text,
        }
    }

    /// Ошибка ли это: подвал красит точку другим цветом.
    pub fn is_failed(&self) -> bool {
        matches!(self, Status::Failed(_))
    }
}

/// Один уровень стека.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    /// Заголовок уровня: попадает в крошки.
    pub title: String,
    /// Строки уровня со своим запросом и выбором.
    pub list: ItemList,
    /// Глубина: 0 — корень.
    pub depth: usize,
}

impl Level {
    /// Пересобирает строки из узлов и сохраняет выбор, если он выжил.
    pub fn rebuild(&mut self, nodes: &[Node]) {
        let previous = self.list.selected_item().map(|item| item.title.clone());
        let query = self.list.query().to_string();
        let rows = self.rows_for(nodes);
        self.list.set_items(rows);
        self.set_query(&query);
        if let Some(title) = previous
            && let Some(index) = self
                .list
                .items()
                .iter()
                .position(|item| item.title == title)
        {
            self.list.select(index);
        }
    }

    /// Строки уровня: на корне с непустым запросом это поиск по всем листьям,
    /// иначе — видимые дети узла.
    pub fn rows_for(&self, nodes: &[Node]) -> Vec<Item> {
        if self.depth == 0 && !self.list.query().is_empty() {
            search_rows(nodes, self.depth)
        } else {
            level_rows(nodes, self.depth)
        }
    }

    fn set_query(&mut self, query: &str) {
        self.list.clear_query();
        for ch in query.chars() {
            self.list.type_char(ch);
        }
    }
}

/// Меню целиком.
#[derive(Clone, Debug)]
pub struct Menu {
    /// Дерево пунктов. Живёт в меню, а не приходит снаружи: уровни
    /// пересобираются из него же.
    root: Vec<Node>,
    levels: Vec<Level>,
    status: Option<Status>,
}

/// Корневой уровень с набором строк.
pub const ROOT_TITLE: &str = "HUD";

impl Menu {
    /// Новое меню по дереву. Открыт корень.
    pub fn new(root: Vec<Node>) -> Self {
        let list = ItemList::new(level_rows(&root, 0));
        Self {
            root,
            levels: vec![Level {
                title: ROOT_TITLE.to_string(),
                list,
                depth: 0,
            }],
            status: None,
        }
    }

    /// Открытое подменю: нужно, когда окно рисует заголовок уровня.
    pub fn opened_submenu(&self) -> Option<&str> {
        let level = self.levels.last()?;
        (level.depth > 0).then_some(level.title.as_str())
    }

    /// Все уровни, корень первым.
    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// Текущий уровень.
    pub fn current(&self) -> &Level {
        self.levels.last().expect("в меню всегда есть корень")
    }

    pub fn current_mut(&mut self) -> &mut Level {
        self.levels.last_mut().expect("в меню всегда есть корень")
    }

    /// Глубина стека: 0 — корень.
    pub fn depth(&self) -> usize {
        self.levels.len() - 1
    }

    /// Крошки для шапки: «HUD › Стиль». Последний уровень ярче.
    pub fn trail(&self) -> Vec<&str> {
        let mut trail = Vec::with_capacity(self.levels.len() + 1);
        trail.push(ROOT_TITLE);
        for level in &self.levels[1..] {
            trail.push(&level.title);
        }
        trail
    }

    /// Дерево пунктов.
    pub fn root(&self) -> &[Node] {
        &self.root
    }

    /// Статус в подвале.
    pub fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    /// Ставит статус и снимает его через `clear_status`.
    pub fn set_status(&mut self, status: Status) {
        self.status = Some(status);
    }

    pub fn clear_status(&mut self) {
        self.status = None;
    }

    /// Сборка строк текущего уровня заново: значения поменялись, значит
    /// подписи справа устарели.
    pub fn refresh(&mut self) {
        self.rebuild_current();
    }

    /// Задать число строк, влезающих в карточку, всем уровням: окно знает
    /// свою высоту, а не модель.
    pub fn set_page(&mut self, page: usize) {
        for level in &mut self.levels {
            level.list.set_page(page);
        }
    }

    /// Сдвиг выбора на текущем уровне.
    pub fn move_sel(&mut self, delta: isize) {
        self.current_mut().list.move_sel(delta);
    }

    /// Прокрутка текущего уровня.
    pub fn scroll(&mut self, lines: isize) {
        self.current_mut().list.scroll(lines);
    }

    /// Символ в запрос текущего уровня. На корне запрос переключает список на
    /// глобальный поиск, поэтому уровень пересобирается.
    pub fn type_char(&mut self, ch: char) {
        let depth = self.depth();
        self.current_mut().list.type_char(ch);
        if depth == 0 {
            self.rebuild_current();
        }
    }

    /// Удаление символа из запроса.
    pub fn backspace(&mut self) {
        let depth = self.depth();
        self.current_mut().list.backspace();
        if depth == 0 {
            self.rebuild_current();
        }
    }

    /// Очистка запроса.
    pub fn clear_query(&mut self) {
        let depth = self.depth();
        self.current_mut().list.clear_query();
        if depth == 0 {
            self.rebuild_current();
        }
    }

    /// Запрос текущего уровня.
    pub fn query(&self) -> &str {
        self.current().list.query()
    }

    /// Возврат на уровень выше. На корне возвращает `false` — это сигнал
    /// закрыть меню, а не пустой переход.
    pub fn back(&mut self) -> Outcome {
        if self.depth() == 0 {
            self.status = None;
            return Outcome::Closed;
        }
        self.levels.pop();
        Outcome::Popped
    }

    /// Esc: на корне закрывает, выше — возвращает на уровень назад.
    pub fn escape(&mut self) -> Outcome {
        self.back()
    }

    /// Enter на выбранной строке.
    pub fn enter(&mut self) -> Outcome {
        let depth = self.depth();
        let index = self.current().list.selected();
        let Some(item) = self.current().list.items().get(index).cloned() else {
            return Outcome::Idle;
        };
        // Строка из глобального поиска пришла с путём: Enter открывает её
        // подменю и выбирает в нём, а не выполняет действие сразу.
        if depth == 0 && item.caption.is_some() {
            return self.jump_to_leaf(&item);
        }
        match item.kind {
            ItemKind::Submenu => {
                if self.push(&item.title) {
                    Outcome::Pushed
                } else {
                    Outcome::Idle
                }
            }
            ItemKind::Leaf => self.jump_to_leaf(&item),
            ItemKind::Toggle { get, set } => {
                set(!get());
                self.after_change();
                Outcome::Changed
            }
            ItemKind::Choice { get, set, options } => {
                let next = (get() + 1) % options.len().max(1);
                set(next);
                self.after_change();
                Outcome::Changed
            }
            ItemKind::Action(action) => match action.perform() {
                Ok(()) => {
                    self.status = Some(Status::Applied(action.describe()));
                    Outcome::Changed
                }
                Err(error) => {
                    self.status = Some(Status::Failed(error));
                    Outcome::Idle
                }
            },
            ItemKind::Picker(_) | ItemKind::Number { .. } => {
                if depth == 0 {
                    // Число и пикер на корне выглядели бы странно: их значения
                    // меняются стрелками, а не Enter.
                    Outcome::Idle
                } else {
                    Outcome::Value
                }
            }
        }
    }

    /// Стрелки влево-вправо на числовом пункте: значение в пределах границ.
    pub fn adjust_value(&mut self, direction: i32) -> Outcome {
        let Some(item) = self.current().list.selected_item().cloned() else {
            return Outcome::Idle;
        };
        let ItemKind::Number {
            min,
            max,
            step,
            get,
            set,
            ..
        } = item.kind
        else {
            return Outcome::Idle;
        };
        let next = step_number(get(), min, max, step, direction);
        set(next);
        self.after_change();
        Outcome::Changed
    }

    /// Открывает подменю по заголовку. Пустой уровень не создаётся: иначе в
    /// стеке окажется карточка без строк и без заголовка.
    fn push(&mut self, title: &str) -> bool {
        let depth = self.depth();
        let nodes = self.nodes_at(depth);
        let children = nodes
            .iter()
            .find(|node| node.title == title)
            .and_then(Node::children)
            .unwrap_or_default();
        if children.is_empty() {
            return false;
        }
        let list = ItemList::new(level_rows(children, depth + 1));
        self.levels.push(Level {
            title: title.to_string(),
            list,
            depth: depth + 1,
        });
        true
    }

    /// Узлы уровня по глубине: корень — это `root`, остальные уровни
    /// достаются спуском по заголовкам уже открытых подменю.
    fn nodes_at(&self, depth: usize) -> Vec<Node> {
        let mut nodes: &[Node] = &self.root;
        for level in self.levels.get(1..=depth).unwrap_or_default() {
            let Some(parent) = nodes.iter().find(|node| node.title == level.title) else {
                return Vec::new();
            };
            let Some(children) = parent.children() else {
                return Vec::new();
            };
            nodes = children;
        }
        nodes.to_vec()
    }

    /// Переход к найденному листу: открывает его подменю и выбирает строку.
    fn jump_to_leaf(&mut self, item: &Item) -> Outcome {
        let Some(caption) = item.caption.as_deref() else {
            return Outcome::Idle;
        };
        let path: Vec<&str> = caption
            .trim_end_matches('›')
            .split('›')
            .map(str::trim)
            .collect();
        self.levels.truncate(1);
        for title in &path {
            self.push(title);
        }
        let target = item.title.clone();
        if let Some(level) = self.levels.last()
            && let Some(index) = level
                .list
                .items()
                .iter()
                .position(|row| row.title == target)
        {
            self.current_mut().list.select(index);
        }
        Outcome::Pushed
    }

    fn after_change(&mut self) {
        self.refresh();
    }

    /// Пересобирает строки текущего уровня из дерева: значения поменялись,
    /// значит подписи справа устарели.
    fn rebuild_current(&mut self) {
        let depth = self.depth();
        let nodes = self.nodes_at(depth);
        self.current_mut().rebuild(&nodes);
    }
}

/// Итог текущего состояния: строки, счётчик и состояние подвала.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Крошки.
    pub trail: Vec<String>,
    /// Видимые строки.
    pub items: Vec<Item>,
    /// Выбранный индекс.
    pub selected: usize,
    /// Индекс первой видимой строки.
    pub top: usize,
    /// Запрос.
    pub query: String,
    /// Всего строк до фильтра.
    pub total: usize,
    /// Статус подвала.
    pub status: Option<Status>,
}

impl Menu {
    /// Снимок текущего уровня для отрисовки.
    pub fn frame(&self) -> Frame {
        let level = self.current();
        Frame {
            trail: self.trail().into_iter().map(str::to_string).collect(),
            items: level.list.items().to_vec(),
            selected: level.list.selected(),
            top: level.list.top(),
            query: level.list.query().to_string(),
            total: level.list.len(),
            status: self.status.clone(),
        }
    }
}

/// Список всех `get`-функций узлов дерева: проверка в тестах, что у каждого
/// контрола есть значение.
pub fn control_count(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .filter(|node| node.is_visible())
        .map(|node| match node.children() {
            Some(children) => control_count(children),
            None => 1,
        })
        .sum()
}

/// Проверка, что дерево пригодно для меню: у каждого узла есть глиф и
/// заголовок, у подменю — непустые дети.
pub fn validate(nodes: &[Node]) -> Result<(), String> {
    for node in nodes {
        if node.title.trim().is_empty() {
            return Err("узел без заголовка".to_string());
        }
        if let NodeKind::Submenu(children) = &node.kind
            && children.is_empty()
        {
            return Err(format!("подменю «{}» пустое", node.title));
        }
        if let NodeKind::Choice { options, .. } = &node.kind
            && options.is_empty()
        {
            return Err(format!("у «{}» пустой список вариантов", node.title));
        }
        if let Some(children) = node.children() {
            validate(children)?;
        }
    }
    Ok(())
}

/// Значение числа узла: тесты границ без окна.
pub fn number_value(node: &Node) -> Option<f32> {
    match &node.kind {
        NodeKind::Number {
            min,
            max,
            step,
            get,
            ..
        } => Some(clamp_number(get(), *min, *max, *step)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::action::{Action, Live};
    use super::super::item::Origin;
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
                vec![
                    Node::choice("theme", "Тема", &["Обычная", "Pixel"], || 0, |_| {}),
                    Node::number("height", "Высота", 20.0, 48.0, 1.0, "px", || 27.0, |_| {}),
                ],
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
        assert_eq!(titles(&menu), ["Тема", "Высота"]);

        assert_eq!(menu.back(), Outcome::Popped);
        assert_eq!(menu.depth(), 0);
        assert_eq!(menu.opened_submenu(), None);
        assert_eq!(titles(&menu), ["Приложения", "Стиль", "Не беспокоить"]);
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
        menu.type_char('т');
        assert_eq!(menu.query(), "т");
        menu.back();
        assert_eq!(menu.query(), "", "запрос не должен утекать между уровнями");
    }

    #[test]
    fn root_search_spans_all_leaves_and_shows_the_path() {
        let mut menu = menu();
        for ch in "тем".chars() {
            menu.type_char(ch);
        }
        let frame = menu.frame();
        assert_eq!(frame.items.len(), 1);
        assert_eq!(frame.items[0].title, "Тема");
        assert_eq!(frame.items[0].caption.as_deref(), Some("Стиль ›"));
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
        assert_eq!(frame.items[0].origin, Origin { depth: 0, index: 2 });
    }

    #[test]
    fn search_inside_a_level_stays_inside_that_level() {
        let mut menu = menu();
        menu.move_sel(1);
        menu.enter();
        for ch in "тем".chars() {
            menu.type_char(ch);
        }
        assert_eq!(menu.query(), "тем");
        assert_eq!(titles(&menu), ["Тема"], "поиск уровня не лезет в корень");

        menu.back();
        assert_eq!(titles(&menu), ["Приложения", "Стиль", "Не беспокоить"]);
    }

    #[test]
    fn clearing_the_root_query_restores_the_sections() {
        let mut menu = menu();
        for ch in "тем".chars() {
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
        for ch in "тем".chars() {
            menu.type_char(ch);
        }
        assert_eq!(menu.enter(), Outcome::Pushed);
        assert_eq!(menu.depth(), 1);
        assert_eq!(menu.opened_submenu(), Some("Стиль"));
        assert_eq!(titles(&menu), ["Тема", "Высота"]);
        assert_eq!(
            menu.current().list.selected(),
            0,
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
    fn selection_and_scroll_stay_inside_the_level() {
        let mut menu = menu();
        menu.set_page(2);
        menu.move_sel(99);
        assert_eq!(menu.current().list.selected(), 2);
        menu.move_sel(-99);
        assert_eq!(menu.current().list.selected(), 0);

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
        let mut menu = Menu::new(vec![Node::action(
            "x",
            "Терминал",
            Action::Run("kitty", &[]),
        )]);
        assert_eq!(menu.enter(), Outcome::Idle);
        let status = menu.status().expect("статус после неудачи");
        assert!(status.is_failed());
        assert!(status.text().contains("M4"));
        menu.clear_status();
        assert!(menu.status().is_none());
    }

    #[test]
    fn back_from_the_root_clears_the_status() {
        let mut menu = menu();
        menu.set_status(Status::Applied("тема: pixel".to_string()));
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
        assert_eq!(control_count(&tree()), 4);
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
        assert_eq!(frame.selected, 0);
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
}

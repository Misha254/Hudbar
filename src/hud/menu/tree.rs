//! Дерево меню: узлы, условия видимости и превращение в строки.
//!
//! Дерево — единственное описание меню, и в нём нет ни значений, ни координат.
//! Значение строки получается вызовом `get` в момент сборки списка, а условие
//! `visible` решается там же: скрытый пункт не попадает ни в один список и ни
//! в поиск, поэтому «призрачных» строк в меню не бывает.
//!
//! Иерархия нужна для двух вещей: крошек в шапке и глобального поиска. Поиск на
//! корне идёт по всем листьям дерева и показывает путь «Стиль › Тема», чтобы
//! было видно, где найденное. На любом другом уровне поиск ограничен текущим
//! списком: искать «систему» из «Панели» бессмысленно.

use super::action::Action;
use super::item::{Item, ItemKind, Origin};

/// Что находится в узле. `PartialEq` здесь не нужен и невозможен: сравнение
/// `fn`-указателей бессмысленно, а дерево всегда сравнивают по заголовкам.
#[derive(Clone, Debug)]
pub enum NodeKind {
    /// Вложенный уровень со своими узлами.
    Submenu(Vec<Node>),
    /// Одноразовое действие.
    Action(Action),
    /// Переключатель.
    Toggle { get: fn() -> bool, set: fn(bool) },
    /// Выбор из списка вариантов.
    Choice {
        options: &'static [&'static str],
        get: fn() -> usize,
        set: fn(usize),
    },
    /// Число в границах.
    Number {
        min: f32,
        max: f32,
        step: f32,
        unit: &'static str,
        get: fn() -> f32,
        set: fn(f32),
    },
    /// Внешний выбор: обои, приложение, звук.
    Picker(&'static str),
}

/// Узел меню.
#[derive(Clone, Debug)]
pub struct Node {
    /// Глиф Nerd Font.
    pub icon: &'static str,
    /// Название пункта.
    pub title: String,
    /// Что находится внутри.
    pub kind: NodeKind,
    /// Условие показа. `None` — пункт виден всегда. Функция, а не флаг:
    /// пункт «Не беспокоить» появляется только когда dunst вообще есть.
    pub visible: Option<fn() -> bool>,
}

impl Node {
    /// Подменю с указанным заголовком.
    pub fn submenu(icon: &'static str, title: &str, children: Vec<Node>) -> Self {
        Self {
            icon,
            title: title.to_string(),
            kind: NodeKind::Submenu(children),
            visible: None,
        }
    }

    /// Действие.
    pub fn action(icon: &'static str, title: &str, action: Action) -> Self {
        Self {
            icon,
            title: title.to_string(),
            kind: NodeKind::Action(action),
            visible: None,
        }
    }

    /// Переключатель.
    pub fn toggle(icon: &'static str, title: &str, get: fn() -> bool, set: fn(bool)) -> Self {
        Self {
            icon,
            title: title.to_string(),
            kind: NodeKind::Toggle { get, set },
            visible: None,
        }
    }

    /// Выбор из списка.
    pub fn choice(
        icon: &'static str,
        title: &str,
        options: &'static [&'static str],
        get: fn() -> usize,
        set: fn(usize),
    ) -> Self {
        Self {
            icon,
            title: title.to_string(),
            kind: NodeKind::Choice { options, get, set },
            visible: None,
        }
    }

    /// Число в границах. Аргументов восемь, но каждый назван по-своему и
    /// разбивать границы на отдельный тип значило бы прятать их от читателя
    /// дерева: `Number{min, max, step, unit, get, set}` читается целиком.
    #[allow(clippy::too_many_arguments)]
    pub fn number(
        icon: &'static str,
        title: &str,
        min: f32,
        max: f32,
        step: f32,
        unit: &'static str,
        get: fn() -> f32,
        set: fn(f32),
    ) -> Self {
        Self {
            icon,
            title: title.to_string(),
            kind: NodeKind::Number {
                min,
                max,
                step,
                unit,
                get,
                set,
            },
            visible: None,
        }
    }

    /// Внешний выбор.
    pub fn picker(icon: &'static str, title: &str, id: &'static str) -> Self {
        Self {
            icon,
            title: title.to_string(),
            kind: NodeKind::Picker(id),
            visible: None,
        }
    }

    /// Добавляет условие показа.
    pub fn when(mut self, visible: fn() -> bool) -> Self {
        self.visible = Some(visible);
        self
    }

    /// Виден ли узел сейчас.
    pub fn is_visible(&self) -> bool {
        self.visible.is_none_or(|check| check())
    }

    /// Дочерние узлы, если это подменю.
    pub fn children(&self) -> Option<&[Node]> {
        match &self.kind {
            NodeKind::Submenu(children) => Some(children),
            _ => None,
        }
    }

    /// Является ли узел листом: его нельзя открыть глубже.
    pub fn is_leaf(&self) -> bool {
        !matches!(self.kind, NodeKind::Submenu(_))
    }
}

/// Значение строки из `get`-функции узла. Пустая строка — значения нет.
pub fn node_value(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Submenu(_) | NodeKind::Action(_) | NodeKind::Picker(_) => String::new(),
        NodeKind::Toggle { get, .. } => if get() { "вкл" } else { "выкл" }.to_string(),
        NodeKind::Choice { options, get, .. } => {
            options.get(get()).copied().unwrap_or("—").to_string()
        }
        NodeKind::Number {
            min,
            max,
            step,
            unit,
            get,
            ..
        } => {
            let value = clamp_number(get(), *min, *max, *step);
            if value.fract().abs() < f32::EPSILON {
                format!("{}{unit}", value.round() as i64)
            } else {
                format!("{value:.1}{unit}")
            }
        }
    }
}

/// Приводит число к границам и к кратности шага. Единственное место, где
/// решается, куда можно увеличивать значение, поэтому `min`/`max` нельзя
/// обойти ни одной командой.
pub fn clamp_number(value: f32, min: f32, max: f32, step: f32) -> f32 {
    if min > max {
        return min;
    }
    let bounded = value.clamp(min, max);
    if step <= 0.0 {
        return bounded;
    }
    let steps = ((bounded - min) / step).round();
    let snapped = min + steps * step;
    snapped.clamp(min, max)
}

/// Следующее значение числа по направлению: `+1` — увеличить, `-1` — уменьшить.
pub fn step_number(value: f32, min: f32, max: f32, step: f32, direction: i32) -> f32 {
    let bounded = clamp_number(value, min, max, step);
    if direction >= 0 {
        clamp_number(bounded + step.abs(), min, max, step)
    } else {
        clamp_number(bounded - step.abs(), min, max, step)
    }
}

/// Поведение строки из поведения узла.
pub fn item_kind(kind: &NodeKind) -> ItemKind {
    match kind {
        NodeKind::Submenu(_) => ItemKind::Submenu,
        NodeKind::Action(action) => ItemKind::Action(action.clone()),
        NodeKind::Toggle { get, set } => ItemKind::Toggle {
            get: *get,
            set: *set,
        },
        NodeKind::Choice { options, get, set } => ItemKind::Choice {
            options,
            get: *get,
            set: *set,
        },
        NodeKind::Number {
            min,
            max,
            step,
            unit,
            get,
            set,
        } => ItemKind::Number {
            min: *min,
            max: *max,
            step: *step,
            unit,
            get: *get,
            set: *set,
        },
        NodeKind::Picker(id) => ItemKind::Picker(id),
    }
}

/// Строка уровня из узла. `index` — позиция среди видимых узлов: по ней
/// `Enter` находит источник, если строка пришла из глобального поиска.
pub fn row_for(node: &Node, depth: usize, index: usize) -> Item {
    Item {
        icon: node.icon,
        title: node.title.clone(),
        value: node_value(&node.kind),
        caption: None,
        kind: item_kind(&node.kind),
        origin: Origin { depth, index },
    }
}

/// Строки одного уровня: видимые дети узла.
pub fn level_rows(nodes: &[Node], depth: usize) -> Vec<Item> {
    nodes
        .iter()
        .filter(|node| node.is_visible())
        .enumerate()
        .map(|(index, node)| row_for(node, depth, index))
        .collect()
}

/// Путь к узлу в дереве: заголовки от корня до родителя.
pub fn path_to(nodes: &[Node], target: &Node) -> Option<Vec<String>> {
    for node in nodes.iter().filter(|node| node.is_visible()) {
        if node.title == target.title {
            return Some(Vec::new());
        }
        if let Some(children) = node.children() {
            let mut tail = path_to(children, target)?;
            let mut path = vec![node.title.clone()];
            path.append(&mut tail);
            return Some(path);
        }
    }
    None
}

/// Все листья дерева вместе с путём до них. `path` — заголовки родителей,
/// они же become caption строки в глобальном поиске.
pub fn leaves_with_path(
    nodes: &[Node],
    path: &mut Vec<String>,
    out: &mut Vec<(Node, Vec<String>)>,
) {
    for node in nodes.iter().filter(|node| node.is_visible()) {
        match node.children() {
            Some(children) => {
                path.push(node.title.clone());
                leaves_with_path(children, path, out);
                path.pop();
            }
            None => out.push((node.clone(), path.clone())),
        }
    }
}

/// Строки глобального поиска: листья с путём до источника.
///
/// Строка сохраняет иконку и поведение узла: иначе найденный тумблер
/// выглядел бы как обычный пункт и потерял кружок, а иконка пропала бы вовсе.
/// Путь при этом остаётся в `caption` — по нему Enter понимает, что строка
/// пришла из поиска и её надо открыть по-настоящему.
pub fn search_rows(nodes: &[Node], depth: usize) -> Vec<Item> {
    let mut path = Vec::new();
    let mut leaves = Vec::new();
    leaves_with_path(nodes, &mut path, &mut leaves);
    leaves
        .into_iter()
        .enumerate()
        .map(|(index, (node, path))| {
            let caption = (!path.is_empty()).then(|| format!("{} ›", path.join(" › ")));
            let row = row_for(&node, depth, index);
            Item { caption, ..row }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::action::Live;
    use super::*;

    fn tree() -> Vec<Node> {
        vec![
            Node::submenu(
                "a",
                "Стиль",
                vec![
                    Node::choice("t", "Тема", &["Обычная", "Pixel"], || 0, |_| {}),
                    Node::picker("w", "Обои", "wallpaper"),
                ],
            ),
            Node::toggle("d", "Не беспокоить", || true, |_| {}).when(|| false),
            Node::number("h", "Высота", 20.0, 48.0, 1.0, "px", || 27.0, |_| {}),
        ]
    }

    #[test]
    fn hidden_nodes_do_not_reach_the_level() {
        let rows = level_rows(&tree(), 0);
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(titles, ["Стиль", "Высота"]);
    }

    #[test]
    fn hidden_nodes_do_not_reach_the_global_search() {
        let rows = search_rows(&tree(), 0);
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Тема", "Обои", "Высота"],
            "скрытый переключатель не должен находиться"
        );
    }

    #[test]
    fn submenu_nodes_have_no_children_and_are_not_leaves() {
        let nodes = tree();
        let style = &nodes[0];
        assert!(!style.is_leaf());
        assert_eq!(style.children().map(<[Node]>::len), Some(2));
        assert!(nodes[2].is_leaf());
    }

    #[test]
    fn values_come_from_get_functions() {
        let nodes = tree();
        let rows = level_rows(&nodes, 0);
        assert_eq!(rows[0].value, "", "у подменю значения нет");
        assert_eq!(rows[1].value, "27px", "у числа есть значение с единицей");

        let style = level_rows(nodes[0].children().unwrap(), 1);
        assert_eq!(style[0].value, "Обычная");
        assert_eq!(style[1].value, "", "у пикера вместо значения миниатюра");
    }

    #[test]
    fn toggle_value_reads_the_getter() {
        assert_eq!(
            node_value(&NodeKind::Toggle {
                get: || true,
                set: |_| {}
            }),
            "вкл"
        );
        assert_eq!(
            node_value(&NodeKind::Toggle {
                get: || false,
                set: |_| {}
            }),
            "выкл"
        );
    }

    #[test]
    fn search_rows_carry_the_path_as_caption() {
        let rows = search_rows(&tree(), 0);
        assert_eq!(rows[0].caption.as_deref(), Some("Стиль ›"));
        assert_eq!(rows[0].title, "Тема");
        assert_eq!(rows[0].value, "Обычная");
        assert_eq!(rows[2].caption, None, "корень без родителей");
    }

    #[test]
    fn path_lookup_finds_the_parent_chain() {
        let nodes = tree();
        let target = Node::picker("w", "Обои", "wallpaper");
        assert_eq!(path_to(&nodes, &target), Some(vec!["Стиль".to_string()]));
        let missing = Node::action("x", "Нет", Action::Live(Live::DndToggle));
        assert_eq!(path_to(&nodes, &missing), None, "чужого узла в дереве нет");
    }

    #[test]
    fn numbers_are_clamped_to_bounds_and_step() {
        assert_eq!(clamp_number(5.0, 20.0, 48.0, 1.0), 20.0, "ниже минимума");
        assert_eq!(clamp_number(99.0, 20.0, 48.0, 1.0), 48.0, "выше максимума");
        assert_eq!(clamp_number(27.4, 20.0, 48.0, 1.0), 27.0, "шаг вниз");
        assert_eq!(clamp_number(27.6, 20.0, 48.0, 1.0), 28.0, "шаг вверх");
        assert_eq!(clamp_number(27.0, 20.0, 48.0, 0.0), 27.0, "нулевой шаг");
        assert_eq!(
            clamp_number(27.0, 48.0, 20.0, 1.0),
            48.0,
            "перепутанные границы"
        );
    }

    #[test]
    fn stepping_never_leaves_the_range() {
        assert_eq!(step_number(47.0, 20.0, 48.0, 1.0, 1), 48.0);
        assert_eq!(
            step_number(48.0, 20.0, 48.0, 1.0, 1),
            48.0,
            "потолок держит"
        );
        assert_eq!(step_number(20.0, 20.0, 48.0, 1.0, -1), 20.0, "пол держит");
        assert_eq!(step_number(28.0, 20.0, 48.0, 4.0, 1), 32.0, "шаг вверх");
        assert_eq!(step_number(28.0, 20.0, 48.0, 4.0, -1), 24.0, "шаг вниз");
        assert_eq!(
            step_number(30.0, 20.0, 48.0, 4.0, 1),
            36.0,
            "30 не кратно шагу: сперва округление, потом шаг"
        );
    }

    #[test]
    fn submenu_rows_carry_submenu_kind() {
        let rows = level_rows(&tree(), 0);
        assert!(rows[0].kind.is_submenu());
        assert_eq!(rows[0].origin, Origin { depth: 0, index: 0 });
        assert_eq!(rows[1].origin, Origin { depth: 0, index: 1 });
    }

    #[test]
    fn item_kind_mirrors_node_kind() {
        let nodes = tree();
        assert!(item_kind(&nodes[0].kind).is_submenu());
        assert!(item_kind(&nodes[1].kind).is_toggle());
        match item_kind(&nodes[2].kind) {
            ItemKind::Number {
                min,
                max,
                step,
                unit,
                ..
            } => {
                assert_eq!((min, max, step, unit), (20.0, 48.0, 1.0, "px"));
            }
            other => panic!("ожидалось число, получено {other:?}"),
        }
    }
}

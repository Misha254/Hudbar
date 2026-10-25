use super::super::item::ItemList;
use super::super::system::{ProviderKey, ProviderSlot, SlotState, slot_nodes};
use super::super::tree::{Node, NodeKind, level_rows};
use super::{Level, LevelKey, Menu};

impl Menu {
    /// Открывает подменю по заголовку. Пустой уровень не создаётся: иначе в
    /// стеке окажется карточка без строк и без заголовка. Динамический
    /// раздел открывается особым путём: уровень сразу получает «Загрузка…»,
    /// а запрос уходит в очередь worker-потока.
    pub(crate) fn push(&mut self, identity: &str) -> bool {
        self.push_key(&LevelKey::Node(identity.to_string()))
    }

    /// Открыть уровень по личности, а не по заголовку. Единственный путь и в
    /// обычный Enter, и в восстановление после `retree`.
    pub(crate) fn push_key(&mut self, key: &LevelKey) -> bool {
        let depth = self.depth();
        let nodes = self.nodes_at(depth);
        let node = match key {
            LevelKey::Node(identity) => nodes
                .iter()
                .find(|node| node.identity() == identity)
                .or_else(|| nodes.iter().find(|node| node.title == *identity)),
            LevelKey::Dynamic(provider) => nodes
                .iter()
                .find(|node| matches!(&node.kind, NodeKind::Dynamic { key } if key == provider)),
        };
        let Some(node) = node else {
            return false;
        };
        match &node.kind {
            NodeKind::Submenu(children) if !children.is_empty() => {
                let lang = self.lang;
                let mut list = ItemList::new(level_rows(children, depth + 1, lang));
                list.set_page(super::super::view::rows_shown(list.len()));
                let (title, identity) = (
                    node.crumb.clone().unwrap_or_else(|| node.title.clone()),
                    node.identity().to_string(),
                );
                self.levels.push(Level {
                    title,
                    list,
                    depth: depth + 1,
                    key: LevelKey::Node(identity),
                });
                true
            }
            NodeKind::Dynamic { key } => self.push_dynamic(&node.title, *key),
            _ => false,
        }
    }

    /// Открывает динамический уровень: слот заводится при первом входе,
    /// запрос — только из пустого состояния, готовый snapshot — из кэша.
    fn push_dynamic(&mut self, title: &str, key: ProviderKey) -> bool {
        let depth = self.depth();
        let lang = self.lang;
        let slot = self
            .dynamics
            .entry(key)
            .or_insert_with(|| ProviderSlot::new(key));
        if let Some(generation) = slot.request() {
            self.pending.push((key, generation));
        }
        let nodes = slot_nodes(key, slot.state(), lang);
        let mut list = ItemList::new(level_rows(&nodes, depth + 1, lang));
        list.set_page(super::super::view::rows_shown(list.len()));
        self.levels.push(Level {
            title: title.to_string(),
            list,
            depth: depth + 1,
            key: LevelKey::Dynamic(key),
        });
        true
    }

    /// Узлы уровня по глубине: корень — это `root`, остальные уровни
    /// достаются спуском по заголовкам уже открытых подменю. Динамический
    /// уровень не имеет детей в статическом дереве: его строки лежат в
    /// слоте провайдера, поэтому спуск через него идёт по `dynamics`.
    pub(crate) fn nodes_at(&self, depth: usize) -> Vec<Node> {
        self.nodes_at_opt(depth)
            .map(<[Node]>::to_vec)
            .unwrap_or_default()
    }

    /// Как `nodes_at`, но отличает «узла больше нет» (`None`) от «детей нет»
    /// и отдаёт срез, а не копию.
    ///
    /// Спуск идёт по ссылкам: раньше здесь клонировался весь `root` и затем
    /// копия нужного уровня, то есть на каждый выход в дерево копировалось всё
    /// поддерево целиком. `Node` в вызывающих не мутируется — собирается
    /// срез строк, — поэтому копия тут не нужна.
    pub(crate) fn nodes_at_opt(&self, depth: usize) -> Option<&[Node]> {
        let mut nodes: &[Node] = &self.root;
        for level in self.levels.get(1..=depth).unwrap_or_default() {
            match &level.key {
                LevelKey::Node(identity) => {
                    let parent = nodes.iter().find(|node| node.identity() == identity)?;
                    nodes = parent.children()?;
                }
                LevelKey::Dynamic(key) => match self.dynamics.get(key).map(ProviderSlot::state) {
                    Some(SlotState::Ready(snapshot)) => nodes = snapshot.as_slice(),
                    _ => return None,
                },
            }
        }
        Some(nodes)
    }

    /// Пересобирает динамический уровень и всё, что открыто над ним. Окно
    /// применяет снимок прямо, без `position/restore`, пока пользователь может
    /// стоять во вложенном подменю: без этого и оно, и родитель после «назад»
    /// показывали бы устаревшие строки. Если вложенный узел исчез из нового
    /// снимка, меню возвращается на последний живой уровень.
    pub(crate) fn rebuild_open_levels(&mut self, key: ProviderKey) {
        let lang = self.lang;
        let Some(start) = self.levels.iter().position(|l| l.dynamic() == Some(key)) else {
            return;
        };
        let mut index = start;
        while index < self.levels.len() {
            let nodes = if index == start {
                self.dynamics
                    .get(&key)
                    .map(|slot| slot_nodes(key, slot.state(), lang))
                    .unwrap_or_default()
            } else {
                // Срез достаётся до mutable-доступа к уровню: держать
                // заимствование дерева и одновременно пересобирать уровень
                // нельзя, а копия здесь означала бы клон поддерева уровня.
                let Some(found) = self
                    .nodes_at_opt(self.levels[index].depth)
                    .map(<[Node]>::to_vec)
                else {
                    self.levels.truncate(index);
                    break;
                };
                found
            };
            self.levels[index].rebuild(&nodes, lang);
            self.mark_busy_level(index);
            index += 1;
        }
    }
}

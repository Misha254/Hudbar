use super::super::item::{Item, ItemList};
use super::super::settings::Language;
use super::super::system::ProviderKey;
use super::super::tree::{Node, level_rows, search_rows};

/// Один уровень стека.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    /// Заголовок уровня: попадает в крошки.
    pub title: String,
    /// Строки уровня со своим запросом и выбором.
    pub list: ItemList,
    /// Глубина: 0 — корень.
    pub depth: usize,
    /// Личность уровня: по ней путь и выбор строки переживают `retree`.
    pub key: LevelKey,
}

/// Личность уровня меню. Заголовок для этого не годится: он меняется при
/// смене языка и приходит из снимка провайдера, а путь должен восстанавливаться
/// в том же разделе. Статичные подменю опознаются по id узла (без id — по
/// заголовку), динамические — по ключу провайдера: тот же механием потом
/// пользуются Wi-Fi и Bluetooth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LevelKey {
    /// Статичное подменю по личности узла.
    Node(String),
    /// Динамический раздел по ключу провайдера.
    Dynamic(ProviderKey),
}

impl Level {
    /// Ключ провайдера, если уровень динамический.
    pub fn dynamic(&self) -> Option<ProviderKey> {
        match &self.key {
            LevelKey::Dynamic(key) => Some(*key),
            LevelKey::Node(_) => None,
        }
    }

    /// Пересобирает строки из узлов и сохраняет выбор, если он выжил.
    pub fn rebuild(&mut self, nodes: &[Node], lang: Language) {
        let previous = self.list.selected_item().map(|item| item.id.clone());
        let query = self.list.query().to_string();
        let rows = self.rows_for(nodes, lang);
        self.list.set_items(rows);
        // Высота страницы задаётся по длине списка, а список у динамического
        // раздела меняется на месте: Bluetooth начинал с пяти строк (сети
        // скрыты), а после обхода становился длиннее. Со страницей в пять
        // строк список прокручивался, а карточка рисовала его целиком — и под
        // последней строкой оставалась пустота.
        self.list
            .set_page(super::super::view::rows_shown(self.list.len()));
        self.set_query(&query);
        if let Some(id) = previous
            && let Some(index) = self.list.items().iter().position(|item| item.id == id)
        {
            self.list.select(index);
        }
    }

    /// Строки уровня: на корне с непустым запросом это поиск по всем листьям,
    /// иначе — видимые дети узла.
    pub fn rows_for(&self, nodes: &[Node], lang: Language) -> Vec<Item> {
        if self.depth == 0 && !self.list.query().is_empty() {
            search_rows(nodes, self.depth, lang)
        } else {
            level_rows(nodes, self.depth, lang)
        }
    }

    fn set_query(&mut self, query: &str) {
        self.list.clear_query();
        for ch in query.chars() {
            self.list.type_char(ch);
        }
    }
}

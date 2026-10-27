use super::super::item::Item;
use super::{LevelKey, Menu, Outcome};

impl Menu {
    /// Открытое положение меню: уровни по личностям и выбранная строка
    /// каждого из них. Снимок положения, а не часть состояния: его читает
    /// окно перед `retree` и отдаёт обратно в [`Menu::restore`].
    pub fn position(&self) -> Position {
        Position {
            levels: self
                .levels
                .iter()
                .skip(1)
                .map(|level| level.key.clone())
                .collect(),
            rows: self
                .levels
                .iter()
                .map(|level| {
                    level
                        .list
                        .selected_item()
                        .map(|item| item.id.clone())
                        .unwrap_or_default()
                })
                .collect(),
        }
    }

    /// Вернуть меню на снятое положение: поднять уровни по личностям и выбрать
    /// в каждом ту же строку по id. Меню при этом на корне — так его и
    /// оставляет `retree`. Если уровень или строка исчезли (смена языка,
    /// пропавший скрипт), останавливаемся на ближайшем живом уровне, а не
    /// падаем и не выбираем наугад.
    pub fn restore(&mut self, position: &Position) {
        self.levels.truncate(1);
        for key in &position.levels {
            if !self.push_key(key) {
                break;
            }
        }
        for (level, row) in self.levels.iter_mut().zip(position.rows.iter()) {
            level.list.clear_selection();
            if row.is_empty() {
                continue;
            }
            if let Some(index) = level
                .list
                .items()
                .iter()
                .position(|item| item.id == *row && item.kind.is_selectable())
            {
                level.list.select(index);
            }
        }
        self.mark_busy();
    }

    /// Переход к найденному листу: открывает его подменю и выбирает строку.
    pub(crate) fn jump_to_leaf(&mut self, item: &Item) -> Outcome {
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
                .position(|row| row.title == target && row.kind.is_selectable())
        {
            self.current_mut().list.select(index);
        }
        Outcome::Pushed
    }
}

/// Положение меню: открытые уровни и выбранная строка каждого из них.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Position {
    /// Личности уровней от корня, без него самого.
    pub levels: Vec<LevelKey>,
    /// Выбранная строка каждого уровня по id, корень включительно. Пустая
    /// строка — уровня нет или он пуст.
    pub rows: Vec<String>,
}

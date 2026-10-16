use super::super::system::{Generation, ProviderKey, ProviderSlot, SlotState, slot_nodes};
use super::super::tree::Node;
use super::Menu;

impl Menu {
    /// Забирает очередь запросов для worker-потоков цикла окна. Очередь, а
    /// не один запрос: подряд можно открыть два раздела, и ни один запрос
    /// не должен потеряться.
    pub fn take_dynamic_requests(&mut self) -> Vec<(ProviderKey, Generation)> {
        std::mem::take(&mut self.pending)
    }

    /// Состояние слота раздела: нужно циклу окна, чтобы решить, запускать
    /// ли выборку, и тестам.
    pub fn dynamic_state(&self, key: ProviderKey) -> Option<&SlotState<Vec<Node>>> {
        self.dynamics.get(&key).map(ProviderSlot::state)
    }

    /// Принять результат worker-потока. Чужое поколение — `false`, уровень
    /// не трогаем. Своё — обновляем слот и, если раздел открыт, пересобираем
    /// его строки один раз.
    pub fn apply_dynamic(
        &mut self,
        key: ProviderKey,
        generation: Generation,
        result: Result<Vec<Node>, String>,
    ) -> bool {
        let lang = self.lang;
        let (accepted, nodes) = match self.dynamics.get_mut(&key) {
            Some(slot) => {
                let accepted = slot.apply(generation, result);
                let nodes = accepted.then(|| slot_nodes(key, slot.state(), lang));
                (accepted, nodes)
            }
            None => (false, None),
        };
        if accepted && nodes.is_some() {
            self.rebuild_open_levels(key);
        }
        accepted
    }

    /// Явное обновление раздела: новое поколение и запрос в очередь, кроме
    /// недоступного. Готовый снимок при этом остаётся на экране: после
    /// действия «Загрузка…» мигала бы вместо только что прочитанного списка.
    /// Если снимка ещё не было — уровень покажет «Загрузка…», это честно.
    pub fn refresh_dynamic(&mut self, key: ProviderKey) -> Option<Generation> {
        let generation = self.dynamics.get_mut(&key)?.refresh_soft()?;
        self.pending.push((key, generation));
        self.rebuild_open_levels(key);
        Some(generation)
    }
}

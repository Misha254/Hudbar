use super::super::item::Item;
use super::super::settings::Language;
use super::super::system::ProviderKey;
use super::{Menu, Outcome, Status};
use std::collections::HashSet;

/// Отметка на строке, чья команда ещё выполняется worker-ом.
pub fn busy_note(lang: Language) -> &'static str {
    match lang {
        Language::Ru => "выполняется…",
        Language::En => "working…",
    }
}

/// Команда для worker-потока: что выполнить и какой раздел обновить после.
///
/// Команды идут списком, а не по одной: пресет раскладки дисплеев — это
/// несколько вызовов `niri msg output` подряд, и держать для этого отдельное
/// действие в `Action` значило бы продублировать всю очередь. Выполняются
/// по порядку, срыв на первой ошибке: дальше идти смысла нет, потому что
/// следующая команда опиралась бы на не сделанную.
#[derive(Clone, Debug)]
pub struct CommandJob {
    /// Поколение команды: для отбрасывания устаревших результатов.
    pub generation: u64,
    /// Команды по порядку: программа, owned-аргументы, секретный stdin.
    pub commands: Vec<super::super::system::CommandSpec>,
    /// Чей слот обновить после успешного выполнения.
    pub refresh: ProviderKey,
    /// Личность строки, из которой команду запустили: по ней уровень
    /// снимает отметку «выполняется», когда worker ответит.
    pub row: String,
}

impl Menu {
    /// Кладёт команду в очередь worker-а: сама она звук и сеть ждут в
    /// subprocess, а меню живёт в UI-потоке. Строка помечается занятой до
    /// ответа, а статус показывает, что именно ушло.
    pub(crate) fn queue_command(
        &mut self,
        item: &Item,
        commands: Vec<super::super::system::CommandSpec>,
        refresh: super::super::system::ProviderKey,
    ) -> Outcome {
        self.command_generation += 1;
        let generation = self.command_generation;
        let describe = commands
            .iter()
            .map(|command| command.log_line())
            .collect::<Vec<_>>()
            .join(" · ");
        self.busy.insert(item.id.clone());
        self.pending_commands.push(CommandJob {
            generation,
            commands,
            refresh,
            row: item.id.clone(),
        });
        self.status = Some(Status::Applied(describe));
        self.rebuild_current();
        Outcome::Changed
    }

    /// Снять отметку «выполняется» со строки: worker ответил, и она снова
    /// обычная. Строки, которых уже нет в списке, просто игнорируются.
    pub fn clear_busy_row(&mut self, row: &str) {
        if self.busy.remove(row) {
            self.rebuild_current();
        }
    }

    /// Строки с выполняющимися командами: нужно окну для теста и отчётам.
    pub fn busy_rows(&self) -> Vec<String> {
        let mut rows: Vec<String> = self.busy.iter().cloned().collect();
        rows.sort();
        rows
    }

    /// Забирает очередь команд для worker-потоков: что выполнить и какой
    /// раздел обновить. Цикл окна запускает их через `spawn_fetch`.
    pub fn take_pending_commands(&mut self) -> Vec<CommandJob> {
        std::mem::take(&mut self.pending_commands)
    }

    /// Навесить отметки «выполняется» на строки текущего уровня. Вызывается
    /// после каждой пересборки уровня: и списка, и после `restore`.
    pub(crate) fn mark_busy(&mut self) {
        let last = self.levels.len().saturating_sub(1);
        self.mark_busy_level(last);
    }

    pub(crate) fn mark_busy_level(&mut self, index: usize) {
        if self.busy.is_empty() {
            return;
        }
        let busy = self.busy.clone();
        self.mark_busy_at(index, &busy);
    }

    /// Отметка «выполняется» на занятых строках текущего уровня. Живёт в
    /// модели, а не в провайдере: провайдер о команде, запущенной из его
    /// строки, ничего не знает.
    fn mark_busy_at(&mut self, index: usize, busy: &HashSet<String>) {
        let note = busy_note(self.lang);
        let items: Vec<Item> = self.levels[index]
            .list
            .items()
            .iter()
            .map(|item| {
                if !busy.contains(&item.id) || item.title.contains(note) {
                    return item.clone();
                }
                let mut item = item.clone();
                item.title = format!("{} · {note}", item.title);
                item
            })
            .collect();
        self.levels[index].list.replace_items(items);
    }
}

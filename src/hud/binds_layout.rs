//! Общая раскладка окон биндов: плоский список биндов и фильтрация по поиску.
//! Только std: ни логики рисования, ни cosmic-text — чистая арифметика списка.

use super::bind_data::Entry;

/// Разворачивает группы в один список, сохраняя их порядок.
/// Заголовков групп в окне нет: полосы-разделители съедали бы высоту и висели
/// над первой строкой каждой группы, а толку не давали.
pub fn entries_from_groups(groups: &[(String, Vec<Entry>)], query: &str) -> Vec<Entry> {
    entries_from_groups_any(groups, &[query])
}

/// То же, но запросов несколько: бинд проходит, если совпал хотя бы один.
/// Нужен yazi, где клавиши показаны в QWERTY, а пользователь печатает в русской
/// раскладке: запрос ищем и как набран, и в транслитерации.
pub fn entries_from_groups_any(groups: &[(String, Vec<Entry>)], queries: &[&str]) -> Vec<Entry> {
    let qs: Vec<String> = queries.iter().map(|q| q.to_lowercase()).collect();
    groups
        .iter()
        .flat_map(|(_, entries)| entries.iter())
        .filter(|e| qs.iter().any(|q| e.matches(q)))
        .cloned()
        .collect()
}

/// Группы, в которых что-то нашлось, с сохранением порядка и имён.
/// Запрос, совпавший с названием группы, оставляет её целиком — иначе поиск
/// по «навигации» показывал бы пару случайных биндов вместо всей секции.
/// Пустой запрос возвращает группы как есть.
pub fn groups_matching_any(
    groups: &[(String, Vec<Entry>)],
    queries: &[&str],
) -> Vec<(String, Vec<Entry>)> {
    let qs: Vec<String> = queries.iter().map(|q| q.to_lowercase()).collect();
    groups
        .iter()
        .filter_map(|(name, entries)| {
            if qs.iter().any(|q| name.to_lowercase().contains(q)) {
                return Some((name.clone(), entries.clone()));
            }
            let kept: Vec<Entry> = entries
                .iter()
                .filter(|e| qs.iter().any(|q| e.matches(q)))
                .cloned()
                .collect();
            (!kept.is_empty()).then(|| (name.clone(), kept))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, desc: &str) -> Entry {
        Entry::new(key.to_string(), desc.to_string())
    }

    fn sample() -> Vec<(String, Vec<Entry>)> {
        vec![
            (
                "Приложения".to_string(),
                vec![
                    entry("super+Return", "Терминал"),
                    entry("super+c", "Календарь"),
                ],
            ),
            ("Фокус".to_string(), vec![entry("super+H", "Влево")]),
            ("Пустая".to_string(), vec![entry("super+X", "Скриншот")]),
        ]
    }

    fn keys(items: &[Entry]) -> Vec<&str> {
        items.iter().map(|e| e.key.as_str()).collect()
    }

    #[test]
    fn keeps_group_order_and_drops_group_names() {
        let items = entries_from_groups(&sample(), "");

        // порядок групп сохранён, заголовков в списке нет
        assert_eq!(
            keys(&items),
            ["super+Return", "super+c", "super+H", "super+X"]
        );
    }

    #[test]
    fn query_matching_ignores_case_and_filters() {
        let groups = sample();
        assert_eq!(entries_from_groups(&groups, "ТЕРМИНАЛ").len(), 1);
        assert_eq!(keys(&entries_from_groups(&groups, "календ")), ["super+c"]);
        assert!(entries_from_groups(&groups, "нет такого").is_empty());
    }

    #[test]
    fn any_query_matches_either_alternative() {
        let groups = sample();

        // как в yazi: описание по-русски, а совпадение ищем и по транслитерации
        let items = entries_from_groups_any(&groups, &["dktdj", "влево"]);
        assert_eq!(keys(&items), ["super+H"]);
        // ни одна альтернатива не совпала — пусто
        assert!(entries_from_groups_any(&groups, &["щод", "нет"]).is_empty());
    }

    #[test]
    fn group_filter_keeps_names_and_drops_empty_groups() {
        let groups = sample();

        // запрос по названию группы оставляет её целиком
        let kept = groups_matching_any(&groups, &["прилож"]);
        let names: Vec<&str> = kept.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Приложения"]);
        assert_eq!(keys(&kept[0].1), ["super+Return", "super+c"]);

        // совпадение только по бинду: группа остаётся, остальные отпадают
        let kept = groups_matching_any(&groups, &["календ"]);
        assert_eq!(keys(&kept[0].1), ["super+c"]);

        // пустой запрос — все группы как есть
        let kept = groups_matching_any(&groups, &[""]);
        assert_eq!(kept.len(), 3);
    }
}

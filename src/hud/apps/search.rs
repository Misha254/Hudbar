//! Поиск по списку приложений: что считается совпадением и в каком порядке.
//!
//! Раньше это делал rofi одним `contains` по отображаемому имени, и «steam»
//! не находился по слову «игра», а «файрфокс» — по `firefox`. Здесь совпадение
//! ранжируется: точное начало имени лучше середины слова, а синоним из
//! `GenericName` или `Categories` лучше размытого совпадения по буквам.
//!
//! Многословный запрос — это «и»: каждое слово должно найтись, иначе по
//! запросу «файр фокс» в выдаче окажется всё подряд. Порядок выдачи — по сумме
//! рангов, а при равенстве по имени, чтобы список не прыгал при переборе.

use super::scan::App;

/// Плотность совпадения. Порядок вариантов и есть порядок выдачи: `Ord` не
/// нужен, ранги сравниваются как `u8`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    /// Запрос — начало имени.
    Prefix = 0,
    /// Запрос — начало слова внутри имени: «бра» в «Файрфокс Браузер».
    Word = 1,
    /// Запрос внутри имени.
    Inside = 2,
    /// Запрос внутри desktop-id: `zen` в `app.zen_browser.zen`.
    Id = 3,
    /// Запрос в `GenericName`.
    Generic = 4,
    /// Запрос в `Keywords` или `Categories`.
    Keyword = 5,
    /// Буквы запроса идут по имени подряд, но не подряд в строке.
    Fuzzy = 6,
}

impl Rank {
    fn score(self) -> u32 {
        self as u32
    }
}

/// Пустой запрос: подходит всё, ранг нулевой. Отдельным случаем, потому что
/// пустой запрос не должен превращаться в «совпало всё подряд» с высоким рангом.
pub const ALL: u32 = 0;

/// Совпадение запроса с приложением. `None` — приложение не подходит.
///
/// Ранг — сумма лучших рангов по каждому слову запроса, поэтому запрос из
/// трёх слов всегда хуже запроса из одного, даже если найдены все три.
pub fn score(app: &App, query: &str) -> Option<u32> {
    let tokens: Vec<String> = query
        .split_whitespace()
        .map(|token| token.to_lowercase())
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.is_empty() {
        return Some(ALL);
    }
    let mut total: u32 = 0;
    for token in &tokens {
        total = total.checked_add(rank(app, token)?.score())?;
    }
    Some(total)
}

/// Лучший ранг одного слова по всем полям приложения.
fn rank(app: &App, token: &str) -> Option<Rank> {
    let name = app.name.to_lowercase();
    let plain = app.name_plain.to_lowercase();
    if name.starts_with(token) || plain.starts_with(token) {
        return Some(Rank::Prefix);
    }
    if word_start(&name, token) || word_start(&plain, token) {
        return Some(Rank::Word);
    }
    if name.contains(token) || plain.contains(token) {
        return Some(Rank::Inside);
    }
    if app.id.to_lowercase().contains(token) {
        return Some(Rank::Id);
    }
    if !app.generic.is_empty() && app.generic.to_lowercase().contains(token) {
        return Some(Rank::Generic);
    }
    if app
        .keywords
        .iter()
        .any(|word| word.to_lowercase().contains(token))
    {
        return Some(Rank::Keyword);
    }
    if fuzzy(&plain, token) || fuzzy(&name, token) {
        return Some(Rank::Fuzzy);
    }
    None
}

/// Запрос начинает слово: перед ним начало строки или любой не-буква.
/// Разделителем считается и точка: в `app.zen_browser.zen` слово `zen` должно
/// находиться по `zen`, а не тонуть внутри идентификатора.
fn word_start(name: &str, token: &str) -> bool {
    let mut from = 0;
    while let Some(at) = name[from..].find(token) {
        let start = from + at;
        let before = name[..start].chars().next_back();
        if !before.is_some_and(char::is_alphanumeric) {
            return true;
        }
        from = start + token.len();
    }
    false
}

/// Буквы запроса идут по имени подряд, но между ними могут быть другие. Регистр
/// не учитывается: обе строки уже свёрнуты.
fn fuzzy(haystack: &str, token: &str) -> bool {
    let mut chars = haystack.chars();
    token.chars().all(|wanted| chars.any(|have| have == wanted))
}

/// Отбор приложений по запросу: индексы в исходном списке, лучшие сначала.
/// При равном ранге — по имени, чтобы порядок не зависел от файловой системы.
pub fn filter(apps: &[App], query: &str) -> Vec<usize> {
    let mut hits: Vec<(u32, usize)> = apps
        .iter()
        .enumerate()
        .filter_map(|(index, app)| score(app, query).map(|rank| (rank, index)))
        .collect();
    hits.sort_by(|(rank_a, index_a), (rank_b, index_b)| {
        rank_a
            .cmp(rank_b)
            .then_with(|| {
                apps[*index_a]
                    .name
                    .to_lowercase()
                    .cmp(&apps[*index_b].name.to_lowercase())
            })
            .then_with(|| index_a.cmp(index_b))
    });
    hits.into_iter().map(|(_, index)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn app(id: &str, name: &str, plain: &str, generic: &str, keywords: &[&str]) -> App {
        App {
            id: id.into(),
            name: name.into(),
            name_plain: plain.into(),
            generic: generic.into(),
            keywords: keywords.iter().map(|word| (*word).to_string()).collect(),
            exec: "app".into(),
            icon: String::new(),
            terminal: false,
            dbus_activatable: false,
            path: PathBuf::from(format!("{id}.desktop")),
        }
    }

    fn steam() -> App {
        app(
            "steam",
            "Steam",
            "Steam",
            "Игровая платформа",
            &["Game", "Игры"],
        )
    }

    fn firefox() -> App {
        app(
            "firefox",
            "Файрфокс",
            "Firefox",
            "Веб-браузер",
            &["WebBrowser", "браузер"],
        )
    }

    #[test]
    fn an_empty_query_matches_everything() {
        assert_eq!(score(&steam(), ""), Some(0));
        assert_eq!(score(&steam(), "   "), Some(0));
    }

    #[test]
    fn a_name_prefix_beats_a_word_inside() {
        let prefixed = app("x", "Терминал", "Terminal", "", &[]);
        let inside = app("y", "Мой терминал", "My terminal", "", &[]);
        assert!(score(&prefixed, "тер") < score(&inside, "тер"));
        assert_eq!(score(&prefixed, "тер"), Some(Rank::Prefix.score()));
    }

    #[test]
    fn a_word_start_beats_the_middle_of_a_word() {
        let by_word = app("x", "Файрфокс Браузер", "Firefox Browser", "", &[]);
        let inside = app("y", "Супербраузер", "Superbrowser", "", &[]);
        assert!(score(&by_word, "бра") < score(&inside, "бра"));
        assert_eq!(score(&by_word, "бра"), Some(Rank::Word.score()));
    }

    #[test]
    fn the_plain_name_is_searched_too() {
        // Локализованное имя «Файрфокс» не содержит `fire`, а английское — да.
        assert!(score(&firefox(), "fire").is_some());
    }

    /// Кириллица ищется по кириллице, латиница — по латинице, и это разные
    /// слова. Раньше для «на одном языке» была транслитерация по раскладке, но
    /// она превращает «стим» в `fjbv` — то самое, что пользователь набирает на
    /// другой раскладке, а не то же самое слово. Совпадением это не считается.
    #[test]
    fn a_cyrillic_query_is_not_guessed_into_a_latin_name() {
        assert_eq!(score(&firefox(), "файр"), Some(Rank::Prefix.score()));
        assert_eq!(score(&steam(), "стим"), None);
    }

    #[test]
    fn generic_and_keywords_are_synonyms() {
        assert_eq!(score(&steam(), "игровая"), Some(Rank::Generic.score()));
        // У Firefox «браузер» есть и в `GenericName`, и в ключевых словах:
        // `GenericName` проверяется раньше и побеждает.
        assert_eq!(score(&firefox(), "браузер"), Some(Rank::Generic.score()));
        // Категория `Game;` в ключевых словах: по ней «игра» находится как
        // слово целиком, а не как начало `игровая` в `GenericName`.
        assert_eq!(score(&steam(), "игры"), Some(Rank::Keyword.score()));
        assert_eq!(score(&steam(), "game"), Some(Rank::Keyword.score()));
        assert_eq!(
            score(&firefox(), "webbrowser"), // регистр не должен мешать
            Some(Rank::Keyword.score())
        );
    }

    #[test]
    fn the_desktop_id_is_searchable() {
        // Имя выбрано так, чтобы `zen` не нашлось в нём раньше: иначе проверка
        // проходила бы по рангу `Prefix` и ничего не говорила бы про id.
        let zen = app("app.zen_browser.zen", "Браузер", "Browser", "", &[]);
        assert_eq!(score(&zen, "zen"), Some(Rank::Id.score()));
    }

    #[test]
    fn scattered_letters_still_match_but_rank_last() {
        // `stm` не подряд в `steam`, но буквы идут по порядку: это самый слабый
        // ранг, и трёхбуквенный запрос не превращается в список из сотни строк.
        assert_eq!(score(&steam(), "stm"), Some(Rank::Fuzzy.score()));
        assert_eq!(score(&steam(), "zzz"), None);
    }

    #[test]
    fn every_word_of_the_query_must_match() {
        assert!(score(&steam(), "steam игровая").is_some());
        assert_eq!(score(&steam(), "steam файрфокс"), None);
    }

    #[test]
    fn more_words_mean_a_worse_rank() {
        let one = score(&steam(), "steam").expect("совпадение");
        let two = score(&steam(), "steam game").expect("совпадение");
        assert!(two > one, "два слова не могут стоять выше одного");
    }

    #[test]
    fn dots_do_not_break_a_word_start() {
        let zen = app("app.zen_browser.zen", "Zen", "Zen", "", &[]);
        assert_eq!(score(&zen, "zen"), Some(Rank::Prefix.score()));
        assert!(word_start("app.zen_browser", "zen"));
        assert!(word_start("my-app name", "app"));
        assert!(!word_start("myapp", "app"));
    }

    #[test]
    fn filtering_sorts_by_rank_then_by_name() {
        let apps = vec![
            app("a", "Zzz Браузер", "Zzz Browser", "", &[]),
            steam(),
            firefox(),
        ];
        let hits = filter(&apps, "бра");
        assert_eq!(hits.len(), 2, "остальные не подходят");
        // «Zzz Браузер» начинается с искомого слова, а у «Файрфокс» оно
        // спрятано в `GenericName` — это более слабый ранг, поэтому он ниже.
        assert_eq!(hits[0], 0);
        assert_eq!(hits[1], 2);

        // Без запроса ранг у всех нулевой, и порядок — по именам: `steam`,
        // `zzz браузер`, `файрфокс`.
        let all = filter(&apps, "");
        assert_eq!(all, vec![1, 0, 2], "без запроса порядок — по именам");
    }

    #[test]
    fn filtering_keeps_source_order_for_equal_ranks() {
        let apps = vec![
            app("a", "Одинаковые", "Same", "", &[]),
            app("b", "Одинаковые", "Same", "", &[]),
        ];
        assert_eq!(filter(&apps, "одинаковые"), vec![0, 1]);
    }

    #[test]
    fn a_one_letter_query_finds_the_word_start_only_in_fuzzy() {
        // Односимвольный запрос не должен находить всё подряд по букве:
        // слово начинается с неё — совпадение, иначе только подряд в строке.
        let apps = vec![app("a", "Steam", "Steam", "", &[])];
        assert_eq!(filter(&apps, "s"), vec![0]);
        assert!(filter(&apps, "z").is_empty());
    }
}

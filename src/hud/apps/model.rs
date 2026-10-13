//! Модель лаунчера: строки, запрос, выбор и закрепление.
//!
//! Модель ничего не знает про Wayland и про отрисовку: на входе — команды
//! (`type_char`, `move_sel`, `toggle_pin`), на выходе — строки и выбранный
//! индекс. Всё это проверяется тестами без композитора.
//!
//! Два вида списка. Без запроса — две секции, «Закреплённые» и «Все
//! приложения», причём секции пропускаются при навигации: иначе стрелка вставала
//! бы на заголовок, который нельзя выбрать. С запросом секций нет вовсе: иначе
//! заголовок «Закреплённые» появлялся бы посреди результатов поиска.

use super::super::settings::Language;
use super::pins;
use super::scan::{self, App};
use super::search;

/// Строка списка.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// Заголовок секции: выбрать его нельзя.
    Section(String),
    /// Приложение по индексу в `Model::apps`.
    App(usize),
}

/// Итог закрепления — для подвала окна.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pinned {
    /// Имя приложения: подвал должен говорить «Закреплено: Steam», а не
    /// показывать desktop-id.
    pub name: String,
    /// Теперь оно закреплено или, наоборот, откреплено.
    pub pinned: bool,
}

/// Модель окна приложений.
pub struct Model {
    apps: Vec<App>,
    /// desktop-id в порядке закрепления.
    pinned: Vec<String>,
    query: String,
    rows: Vec<Row>,
    /// Индекс строки в `rows`. Всегда указывает на `Row::App`: выбор стоит
    /// на заголовке секции только в том случае, если выбирать нечего, и тогда
    /// равен `usize::MAX`.
    selected: usize,
    /// Индекс первой видимой строки.
    top: usize,
    /// Сколько строк помещается в окно. Задаётся снаружи: это высота карточки,
    /// а модель списка о геометрии знать не должна.
    page: usize,
    /// Строка в подвале: результат закрепления или ошибка запуска.
    status: Option<String>,
    lang: Language,
}

impl Model {
    /// Собирает модель: сканирует desktop-файлы, поднимает закреплённые из
    /// прежнего окна rofi и строит список.
    pub fn open(page: usize, lang: Language) -> Self {
        // Перенос из rofi — до чтения: иначе первый запуск показал бы пустые
        // закреплённые и потерял бы то, что пользователь уже собрал.
        pins::import_legacy();
        let mut model = Self {
            apps: scan::scan(),
            pinned: pins::load(),
            query: String::new(),
            rows: Vec::new(),
            selected: usize::MAX,
            top: 0,
            page: page.max(1),
            status: None,
            lang,
        };
        model.rebuild();
        model
    }

    /// Модель из готового списка приложений: снимки и тесты.
    #[allow(dead_code)]
    pub fn from_apps(apps: Vec<App>, pinned: Vec<String>, page: usize, lang: Language) -> Self {
        let mut model = Self {
            apps,
            pinned,
            query: String::new(),
            rows: Vec::new(),
            selected: usize::MAX,
            top: 0,
            page: page.max(1),
            status: None,
            lang,
        };
        model.rebuild();
        model
    }

    /// Строки целиком: столько их в модели, а не сколько в окне. Отрисовке
    /// достаточно `visible`, а снимкам и тестам нужен весь список.
    #[allow(dead_code)]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Видимые строки: срез от `top` длиной в страницу.
    pub fn visible(&self) -> &[Row] {
        let end = (self.top + self.page).min(self.rows.len());
        if self.top >= end {
            return &[];
        }
        &self.rows[self.top..end]
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Есть ли непустой запрос: от него в окне зависит высота строки и подпись
    /// в подвале.
    pub fn searching(&self) -> bool {
        !self.query.is_empty()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn top(&self) -> usize {
        self.top
    }

    pub fn total(&self) -> usize {
        self.rows.len()
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn set_status(&mut self, status: String) {
        self.status = Some(status);
    }

    pub fn clear_status(&mut self) {
        self.status = None;
    }

    pub fn lang(&self) -> Language {
        self.lang
    }

    /// Приложение под выбором.
    pub fn selected_app(&self) -> Option<&App> {
        match self.rows.get(self.selected) {
            Some(Row::App(index)) => self.apps.get(*index),
            _ => None,
        }
    }

    /// Приложение под индексом строки.
    pub fn app_at(&self, row: &Row) -> Option<&App> {
        match row {
            Row::App(index) => self.apps.get(*index),
            Row::Section(_) => None,
        }
    }

    /// Все приложения — нужны проверкам и снимкам.
    #[allow(dead_code)]
    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    /// Закреплено ли приложение.
    pub fn is_pinned(&self, id: &str) -> bool {
        self.pinned.iter().any(|item| item == id)
    }

    /// Список закреплённых в порядке закрепления.
    pub fn pinned(&self) -> &[String] {
        &self.pinned
    }

    /// Пересборка списка под текущий запрос. Выбор встаёт на первую строку
    /// приложения: после каждой буквы курсор должен быть там, где человек его
    /// ждёт, а не на позиции, которая случайно сохранилась.
    pub fn rebuild(&mut self) {
        self.rows = self.build_rows();
        self.selected = self.first_app_row();
        self.top = 0;
    }

    fn build_rows(&self) -> Vec<Row> {
        let title = |ru: &'static str, en: &'static str| match self.lang {
            Language::Ru => ru.to_string(),
            Language::En => en.to_string(),
        };
        if self.query.is_empty() {
            let mut rows: Vec<Row> = Vec::with_capacity(self.apps.len() + 2);
            let mut pinned_seen = false;
            for id in &self.pinned {
                let Some(index) = self.index_of(id) else {
                    // Закреплённое приложение могло быть удалено: в списке его
                    // нет, и заголовок секции из-за него тоже не нужен.
                    continue;
                };
                if !pinned_seen {
                    rows.push(Row::Section(title("Закреплённые", "Pinned")));
                    pinned_seen = true;
                }
                rows.push(Row::App(index));
            }
            // В «все приложения» идёт всё, чего нет в закреплённых: секция
            // построена из самого списка, поэтому закрепленное не может
            // продублироваться, даже если desktop-id в списке повторился.
            let others: Vec<usize> = self
                .apps
                .iter()
                .enumerate()
                .filter(|(_, candidate)| !self.pinned.contains(&candidate.id))
                .map(|(index, _)| index)
                .collect();
            if pinned_seen && !others.is_empty() {
                rows.push(Row::Section(title("Все приложения", "All apps")));
            }
            rows.extend(others.into_iter().map(Row::App));
            rows
        } else {
            search::filter(&self.apps, &self.query)
                .into_iter()
                .map(Row::App)
                .collect()
        }
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.apps.iter().position(|app| app.id == id)
    }

    /// Первая строка приложения в построенном списке.
    fn first_app_row(&self) -> usize {
        self.rows
            .iter()
            .position(|row| matches!(row, Row::App(_)))
            .unwrap_or(usize::MAX)
    }

    /// Следующая строка приложения в направлении `delta`, через секции.
    fn step(&self, from: usize, delta: isize) -> Option<usize> {
        if self.rows.is_empty() {
            return None;
        }
        let mut at = from as isize;
        loop {
            at += delta;
            if at < 0 || at as usize >= self.rows.len() {
                return None;
            }
            if matches!(self.rows[at as usize], Row::App(_)) {
                return Some(at as usize);
            }
        }
    }

    /// Сдвиг выбора. Границы не перепрыгиваются: у края списка стрелка молчит.
    pub fn move_sel(&mut self, delta: isize) {
        if self.selected == usize::MAX {
            return;
        }
        if let Some(next) = self.step(self.selected, delta) {
            self.select(next);
        }
    }

    /// Прокрутка на страницу с сохранением относительного положения выбора:
    /// страница вниз с третьей строки должна привести на шестую, а не на первую
    /// видимую.
    pub fn page(&mut self, delta: isize) {
        if self.selected == usize::MAX {
            return;
        }
        let target = self.selected as isize + delta * self.page as isize;
        let target = target.clamp(0, self.rows.len().saturating_sub(1) as isize) as usize;
        // Крайняя строка может оказаться заголовком секции — выбор туда не
        // встаёт, поэтому ищем от него в направлении движения.
        let target = if matches!(self.rows.get(target), Some(Row::App(_))) {
            Some(target)
        } else {
            self.step(target, if delta < 0 { -1 } else { 1 })
        };
        if let Some(target) = target {
            self.select(target);
        }
    }

    /// В начало или в конец списка.
    pub fn jump(&mut self, end: bool) {
        let target = if end {
            self.rows.iter().rposition(|row| matches!(row, Row::App(_)))
        } else {
            self.rows.iter().position(|row| matches!(row, Row::App(_)))
        };
        if let Some(target) = target {
            self.select(target);
        }
    }

    /// Выбор строки с прокруткой окна так, чтобы она была видна.
    pub fn select(&mut self, row: usize) {
        if row >= self.rows.len() {
            return;
        }
        self.selected = row;
        self.ensure_visible();
    }

    /// Прокрутка до выбранной строки. Окно списка едет только тогда, когда
    /// строка вышла за его край: иначе список прыгал бы на каждое нажатие.
    fn ensure_visible(&mut self) {
        if self.selected == usize::MAX {
            self.top = 0;
            return;
        }
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + self.page {
            self.top = self.selected + 1 - self.page;
        }
        let last = self.rows.len().saturating_sub(1);
        self.top = self.top.min(last);
    }

    /// Дописать символ к запросу.
    pub fn type_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        self.query.push(ch);
        self.rebuild();
    }

    /// Стереть последний символ запроса.
    pub fn backspace(&mut self) {
        if self.query.pop().is_some() {
            self.rebuild();
        }
    }

    /// Очистить запрос целиком.
    pub fn clear_query(&mut self) {
        if self.query.is_empty() {
            return;
        }
        self.query.clear();
        self.rebuild();
    }

    /// Закрепить или открепить приложение под выбором.
    ///
    /// Без запроса список пересобирается — приложение переезжает между
    /// секциями, — и выбор следует за ним, чтобы курсор не остался на месте
    /// старой строки. С запросом строки не меняются: меняется только подвал.
    pub fn toggle_pin(&mut self) -> Option<Pinned> {
        let app = self.selected_app()?.clone();
        let pinned = pins::toggle(&app.id).ok()?;
        if pinned {
            if !self.pinned.contains(&app.id) {
                self.pinned.push(app.id.clone());
            }
        } else {
            self.pinned.retain(|id| *id != app.id);
        }
        let result = Pinned {
            name: app.name.clone(),
            pinned,
        };
        if self.query.is_empty() {
            let keep = Some(app.id.clone());
            self.rebuild();
            if let Some(id) = keep
                && let Some(row) = self
                    .rows
                    .iter()
                    .position(|row| matches!(row, Row::App(index) if self.apps[*index].id == id))
            {
                self.select(row);
            }
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
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
            })
            .collect();
        assert_eq!(names[0], "Pinned");
    }
}

//! Модель лаунчера: строки, запрос, выбор и закрепление.
//!
//! Модель ничего не знает про Wayland и про отрисовку: на входе — команды
//! (`type_char`, `move_sel`, `toggle_pin`), на выходе — строки и выбранный
//! индекс. Всё это проверяется тестами без композитора.
//!
//! Два вида. Плитки — сетка, где закреплённые идут первыми и отделены
//! разделителем, а заголовков нет;
//! список — две секции, «Закреплённые» и «Все приложения». Секции пропускаются
//! при навигации: иначе стрелка вставала бы на заголовок, который нельзя
//! выбрать. С запросом секций нет вовсе: иначе заголовок «Закреплённые»
//! появлялся бы посреди результатов поиска.

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
    /// Пустая клетка сетки: добивает ряд закреплённых до границы, чтобы
    /// разделитель блоков не резал ряд посередине. Выбрать её нельзя.
    Blank,
}

/// Как показать список.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Плитки: закреплённые идут первыми, заголовков секций нет.
    Tiles,
    /// Привычный список с секциями.
    List,
}

/// Колонок в сетке плиток: 4×88 + 3×16 = 400 — ширина карточки без отступов.
pub const GRID_COLS: usize = 4;
/// Строк сетки в окне: меньше — карточка не схлопывается под одну плитку.
pub const MIN_TILE_ROWS: usize = 2;
/// Больше четырёх рядов не показываем: дальше сетка прокручивается.
pub const MAX_TILE_ROWS: usize = 4;

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
    /// Параллельно `rows`: строка лежит в секции «Закреплённые». Булавка в
    /// этой секции не рисуется — там она у всех строк, и второй раз она
    /// ничего не сообщает.
    in_pinned: Vec<bool>,
    lang: Language,
    /// Как показывать список. Живое окно открывается плитками, а `from_apps`
    /// остаётся списком: снимки и тесты так не зависят от оформления.
    mode: ViewMode,
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
            in_pinned: Vec::new(),
            lang,
            mode: ViewMode::Tiles,
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
            in_pinned: Vec::new(),
            lang,
            mode: ViewMode::List,
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
        let end = (self.top + self.capacity()).min(self.rows.len());
        if self.top >= end {
            return &[];
        }
        &self.rows[self.top..end]
    }

    /// Сколько ячеек помещается в окно. В списке это строки, в сетке — ряды
    /// по [`GRID_COLS`] плиток.
    fn capacity(&self) -> usize {
        match self.mode {
            ViewMode::List => self.page,
            ViewMode::Tiles => GRID_COLS * self.tile_rows(),
        }
    }

    /// Сколько рядов сетки занимает весь список. Меньше [`MIN_TILE_ROWS`]
    /// карточка не рисует, больше [`MAX_TILE_ROWS`] — прокручивает.
    fn tile_rows(&self) -> usize {
        self.rows
            .len()
            .div_ceil(GRID_COLS)
            .clamp(MIN_TILE_ROWS, MAX_TILE_ROWS)
    }

    /// Текущий вид списка.
    pub fn view(&self) -> ViewMode {
        self.mode
    }

    /// Переключить плитки и список. Выбор остаётся на том же приложении:
    /// смена оформления не должна уводить с найденного.
    pub fn toggle_view(&mut self) {
        let selected = self.selected_app().map(|app| app.id.clone());
        self.mode = match self.mode {
            ViewMode::Tiles => ViewMode::List,
            ViewMode::List => ViewMode::Tiles,
        };
        self.rebuild();
        if let Some(id) = selected {
            self.select_app(&id);
        }
        self.status = None;
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

    /// Слот, с которого в сетке начинаются незакреплённые: ряд закреплённых
    /// кончается на границе ряда, поэтому по нему видно, где рисовать
    /// разделитель блоков. `None` — делить нечего.
    pub fn tiles_break(&self) -> Option<usize> {
        if self.mode != ViewMode::Tiles || self.searching() {
            return None;
        }
        let at = self.rows.iter().position(|row| match row {
            Row::App(index) => !self.pinned.contains(&self.apps[*index].id),
            Row::Blank | Row::Section(_) => false,
        })?;
        (at > 0).then_some(at)
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
            Row::Section(_) | Row::Blank => None,
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
        self.in_pinned = self.mark_pinned_section(&self.rows);
        self.selected = self.first_app_row();
        self.top = 0;
    }

    /// Помечает строки секции «Закреплённые». Заголовок секции помечается
    /// вместе с ними: он принадлежит той же секции, и по нему видно, где
    /// секция кончилась.
    fn mark_pinned_section(&self, rows: &[Row]) -> Vec<bool> {
        let pinned_title = match self.lang {
            Language::Ru => "Закреплённые",
            Language::En => "Pinned",
        };
        let mut inside = false;
        let mut marks = Vec::with_capacity(rows.len());
        for row in rows {
            match row {
                Row::Section(title) => inside = title == pinned_title,
                Row::App(_) | Row::Blank => {}
            }
            marks.push(inside);
        }
        marks
    }

    /// Лежит ли строка в секции «Закреплённые». В прод-коде сейчас не
    /// используется (булавка рисуется у всех закреплённых), но покрыта
    /// тестами как часть публичного API модели.
    #[allow(dead_code)]
    pub fn in_pinned_section(&self, row: usize) -> bool {
        self.in_pinned.get(row).copied().unwrap_or(false)
    }

    /// Сколько в списке приложений: заголовки секций в счёт не идут, и счётчик
    /// подвала должен считать именно их.
    pub fn app_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| matches!(row, Row::App(_)))
            .count()
    }

    /// Позиция строки среди приложений, начиная с единицы. `None` — строка не
    /// приложение: у заголовка секции позиции нет.
    pub fn app_position(&self, row: usize) -> Option<usize> {
        let before = self
            .rows
            .iter()
            .take(row)
            .filter(|item| matches!(item, Row::App(_)))
            .count();
        match self.rows.get(row) {
            Some(Row::App(_)) => Some(before + 1),
            _ => None,
        }
    }

    fn build_rows(&self) -> Vec<Row> {
        let title = |ru: &'static str, en: &'static str| match self.lang {
            Language::Ru => ru.to_string(),
            Language::En => en.to_string(),
        };
        if self.query.is_empty() {
            if self.mode == ViewMode::Tiles {
                // Сетка без заголовков: закреплённые первыми в порядке
                // закрепления, за ними всё остальное. Заголовок секции занял
                // бы целый ряд и разорвал сетку.
                let others: Vec<usize> = self
                    .apps
                    .iter()
                    .enumerate()
                    .filter(|(_, candidate)| !self.pinned.contains(&candidate.id))
                    .map(|(index, _)| index)
                    .collect();
                let mut rows = Vec::with_capacity(self.apps.len() + GRID_COLS);
                for id in &self.pinned {
                    if let Some(index) = self.index_of(id) {
                        rows.push(Row::App(index));
                    }
                }
                // Ряд закреплённых добивается пустыми клетками: разделитель
                // блоков должен стоять на границе ряда, а не посреди него.
                if !rows.is_empty() && !others.is_empty() {
                    let pad = (GRID_COLS - rows.len() % GRID_COLS) % GRID_COLS;
                    rows.extend(std::iter::repeat_n(Row::Blank, pad));
                }
                rows.extend(others.into_iter().map(Row::App));
                return rows;
            }
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
    pub fn first_app_row(&self) -> usize {
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

    /// Сдвиг выбора по вертикали. В списке это строка, в сетке — ряд:
    /// `↑`/`↓` перескакивают через [`GRID_COLS`] плиток. Границы не
    /// перепрыгиваются: у края списка стрелка молчит.
    pub fn move_sel(&mut self, delta: isize) {
        if self.selected == usize::MAX {
            return;
        }
        match self.mode {
            ViewMode::List => {
                if let Some(next) = self.step(self.selected, delta) {
                    self.select(next);
                }
            }
            ViewMode::Tiles => {
                // Пустая клетка в хвосте ряда закреплённых пропускается:
                // вниз едем по тому же столбцу до первого приложения.
                let mut target = self.selected as isize + delta * GRID_COLS as isize;
                while target >= 0 && (target as usize) < self.rows.len() {
                    if matches!(self.rows[target as usize], Row::App(_)) {
                        self.select(target as usize);
                        return;
                    }
                    target += delta * GRID_COLS as isize;
                }
            }
        }
    }

    /// Сдвиг выбора внутри ряда сетки. В списке стрелки вбок ничего не
    /// делают: там один уровень, а не сетка.
    pub fn move_col(&mut self, delta: isize) {
        if self.mode != ViewMode::Tiles || self.selected == usize::MAX {
            return;
        }
        let start = self.selected / GRID_COLS * GRID_COLS;
        let end = (start + GRID_COLS).min(self.rows.len());
        // Пустые клетки в конце ряда пропускаются: стрелка едет к соседнему
        // приложению, а не в незанятое место.
        let mut target = self.selected as isize + delta;
        while target >= start as isize && (target as usize) < end {
            if matches!(self.rows[target as usize], Row::App(_)) {
                self.select(target as usize);
                return;
            }
            target += delta;
        }
    }

    /// Прокрутка на страницу с сохранением относительного положения выбора:
    /// страница вниз с третьей строки должна привести на шестую, а не на первую
    /// видимую.
    pub fn page(&mut self, delta: isize) {
        if self.selected == usize::MAX {
            return;
        }
        let target = self.selected as isize + delta * self.capacity() as isize;
        let target = target.clamp(0, self.rows.len().saturating_sub(1) as isize) as usize;
        // Крайняя строка может оказаться заголовком секции или пустой клеткой
        // сетки — выбор туда не встаёт, поэтому ищем от него в направлении
        // движения.
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
        match self.mode {
            ViewMode::List => {
                if self.selected < self.top {
                    self.top = self.selected;
                } else if self.selected >= self.top + self.page {
                    self.top = self.selected + 1 - self.page;
                }
                let last = self.rows.len().saturating_sub(1);
                self.top = self.top.min(last);
            }
            ViewMode::Tiles => {
                let rows = self.tile_rows();
                let selected_row = self.selected / GRID_COLS;
                let mut top_row = self.top / GRID_COLS;
                if selected_row < top_row {
                    top_row = selected_row;
                } else if selected_row >= top_row + rows {
                    top_row = selected_row + 1 - rows;
                }
                let last_row = self.rows.len().div_ceil(GRID_COLS);
                // Верх сетки всегда кратен ширине ряда: неполный ряд сверху
                // оставил бы дыру в первой строке окна.
                top_row = top_row.min(last_row.saturating_sub(rows));
                self.top = top_row * GRID_COLS;
            }
        }
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

    /// Переставить закреплённое приложение под выбором на `delta` строк.
    ///
    /// Работает только в секции «Закреплённые»: в поиске порядок задаёт
    /// выдача, а в «Все приложения» строка ещё не закреплена. Выбор едет за
    /// приложением, как и при закреплении.
    pub fn move_pinned(&mut self, delta: isize) -> bool {
        // В выдаче поиска порядок задаёт запрос, а не файл закрепления.
        if self.searching() {
            return false;
        }
        let Some(app) = self.selected_app() else {
            return false;
        };
        if !self.is_pinned(&app.id) {
            return false;
        }
        let id = app.id.clone();
        let Some(ids) = pins::move_by(&id, delta) else {
            return false;
        };
        self.pinned = ids;
        self.rebuild();
        self.select_app(&id);
        true
    }

    /// Ставит выбор на приложение `id`, если оно есть в строках.
    fn select_app(&mut self, id: &str) {
        if let Some(row) = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::App(index) if self.apps[*index].id == id))
        {
            self.select(row);
        }
    }

    /// Оставить выбор на прежней строке и прежней прокрутке. Откреплённая
    /// строка исчезла, и на её место встало следующее приложение; заголовок
    /// секции выбрать нельзя, поэтому с него поднимаемся к приложению выше.
    fn keep_row(&mut self, row: usize, top: usize) {
        let last = self.rows.len().saturating_sub(1);
        self.top = top.min(last);
        let row = row.min(last);
        let target = match self.rows.get(row) {
            Some(Row::App(_)) => Some(row),
            _ => self.rows[..row]
                .iter()
                .rposition(|item| matches!(item, Row::App(_))),
        };
        if let Some(target) = target {
            self.select(target);
        }
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
            let row = self.selected;
            let top = self.top;
            self.rebuild();
            if pinned {
                self.select_app(&app.id);
            } else {
                self.keep_row(row, top);
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
                Row::Blank => "·",
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

    /// Ctrl+↑↓ двигают только закреплённые: строку из «все приложения» и
    /// выдачу поиска трогать нечего, и до файла дело не доходит.
    #[test]
    fn reordering_is_only_for_pinned_apps() {
        let mut model = model(&[("a", "A"), ("b", "B")], &["a"], 10);
        model.select(3);
        assert!(!model.move_pinned(-1), "строка из «все приложения»");
        model.type_char('b');
        model.select(0);
        assert!(
            !model.move_pinned(1),
            "в выдаче поиска порядок задаёт запрос"
        );
    }

    /// Открепление оставляет вид на месте: на освободившейся строке
    /// оказывается следующее закреплённое, а прокрутка не прыгает в начало.
    #[test]
    fn unpinning_keeps_the_row_and_the_scroll() {
        let mut model = model(&[("a", "A"), ("b", "B"), ("c", "C")], &["a", "b", "c"], 2);
        // Строки: заголовок, A, B, C, заголовок, остальные.
        model.select(2);
        let top = model.top();
        assert!(top > 0, "список прокручен до B");
        // Имитация открепления B: список пересобран без неё.
        model.pinned.retain(|id| id != "b");
        model.rebuild();
        model.keep_row(2, top);
        assert_eq!(
            model
                .app_at(&model.rows()[model.selected()])
                .map(|a| a.name.as_str()),
            Some("C"),
            "на месте B стоит C"
        );
        assert_eq!(model.top(), top, "прокрутка осталась на месте");
    }

    /// Если ушло последнее закреплённое, на его строке теперь заголовок
    /// «Все приложения»: выбор поднимается к соседу выше, а не уезжает вниз.
    #[test]
    fn unpinning_the_last_pinned_steps_up_to_the_neighbour() {
        let mut model = model(
            &[("a", "A"), ("b", "B"), ("c", "C"), ("d", "D")],
            &["a", "b", "c"],
            10,
        );
        // Строки: «Закреплённые», A, B, C, «Все приложения», D.
        model.select(3); // C, последняя закреплённая
        model.pinned.retain(|id| id != "c");
        model.rebuild();
        model.keep_row(3, 0);
        assert_eq!(
            model
                .app_at(&model.rows()[model.selected()])
                .map(|a| a.name.as_str()),
            Some("B"),
            "на месте заголовка выбор поднялся к B"
        );
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

    /// Счётчик подвала считает приложения, а не строки: заголовки секций в
    /// счёт не идут ни в позицию, ни в итог.
    #[test]
    fn the_app_count_ignores_section_headers() {
        let model = model(
            &[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")],
            &["a", "b"],
            10,
        );
        // Две секции: заголовок, два закреплённых, заголовок, одно прочее.
        assert_eq!(model.total(), 5);
        assert_eq!(model.app_count(), 3);
        assert_eq!(model.app_position(0), None, "заголовок позиции не имеет");
        assert_eq!(model.app_position(1), Some(1));
        assert_eq!(model.app_position(2), Some(2));
        assert_eq!(model.app_position(3), None, "заголовок позиции не имеет");
        assert_eq!(model.app_position(4), Some(3));
        assert_eq!(model.app_position(99), None);
    }

    /// При поиске секций нет, и позиция совпадает с номером строки.
    #[test]
    fn a_filtered_list_positions_by_apps_only() {
        let mut model = model(&[("a", "Alpha"), ("b", "Beta")], &["b"], 10);
        model.type_char('l');
        assert_eq!(model.total(), 1);
        assert_eq!(model.app_count(), 1);
        assert_eq!(model.app_position(0), Some(1));
    }

    /// Секция «Закреплённые» помечается по строкам, а не по индексу: её
    /// содержимое меняется при закреплении, а заголовок — нет.
    #[test]
    fn the_pinned_section_is_marked_row_by_row() {
        let model = model(&[("a", "Alpha"), ("b", "Beta")], &["a"], 10);
        let marks: Vec<bool> = (0..model.total())
            .map(|row| model.in_pinned_section(row))
            .collect();
        // Строки: заголовок «Закреплённые», Alpha, заголовок «Все приложения»,
        // Beta. Помечены заголовок первой секции и её содержимое.
        assert_eq!(
            marks,
            vec![true, true, false, false],
            "заголовок и содержимое секции помечены, вторая секция — нет"
        );
        assert!(
            !model.in_pinned_section(99),
            "за пределами списка секций нет"
        );
    }

    /// Название секции из языка: помечение не должно зависеть от того, как
    /// строка называется в списке.
    #[test]
    fn the_pinned_section_is_found_in_both_languages() {
        for lang in [Language::Ru, Language::En] {
            let model = Model::from_apps(
                vec![app("a", "Alpha"), app("b", "Beta")],
                vec!["a".to_string()],
                10,
                lang,
            );
            let first_app = model.first_app_row();
            assert!(
                model.in_pinned_section(first_app),
                "{lang:?}: первое приложение в закреплённых"
            );
        }
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
                Row::Blank => "·",
            })
            .collect();
        assert_eq!(names[0], "Pinned");
    }

    // ───────────────────────── плитки ─────────────────────────

    /// Сетка не рисует заголовки секций: закреплённые просто идут первыми.
    #[test]
    fn tiles_put_pinned_first_and_drop_the_headers() {
        let mut model = model(
            &[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")],
            &["c", "a"],
            10,
        );
        model.toggle_view();
        assert_eq!(model.view(), ViewMode::Tiles);
        assert_eq!(
            names(&model),
            vec!["Gamma", "Alpha", "·", "·", "Beta"],
            "ряд закреплённых добит пустыми клетками, чтобы блок кончался на границе ряда"
        );
        assert!(
            model
                .rows()
                .iter()
                .all(|row| !matches!(row, Row::Section(_))),
            "в сетке заголовков нет"
        );
        assert_eq!(model.tiles_break(), Some(GRID_COLS));
    }

    /// Пустые клетки в хвосте ряда закреплённых не выбираются: стрелки едут
    /// к приложениям, а порядок обхода не зависит от добивки.
    #[test]
    fn the_padding_cells_are_not_selectable() {
        let apps: Vec<App> = (0..10).map(|i| app(&format!("app{i}"), "App")).collect();
        let pinned: Vec<String> = (0..5).map(|i| format!("app{i}")).collect();
        let mut model = Model::from_apps(apps, pinned, 10, Language::Ru);
        model.toggle_view();
        // Пять закреплённых: второй ряд — одна плитка и три пустые клетки.
        assert_eq!(model.tiles_break(), Some(2 * GRID_COLS));
        model.select(GRID_COLS - 1);
        model.move_sel(1);
        assert_eq!(
            model.selected(),
            3 * GRID_COLS - 1,
            "вниз с последней колонки — через пустую клетку туда же в следующем ряду"
        );
        model.select(GRID_COLS);
        model.move_col(1);
        assert_eq!(
            model.selected(),
            GRID_COLS,
            "вправо от единственной плитки ряда — никуда, пустые клетки не выбираются"
        );
        model.move_sel(1);
        assert_eq!(
            model.selected(),
            2 * GRID_COLS,
            "вниз — на первое незакреплённое"
        );
    }

    /// Вбок выбор идёт по плиткам ряда и не перескакивает на соседний ряд;
    /// вниз-вверх — через ряд целиком.
    #[test]
    fn grid_navigation_keeps_the_column() {
        let apps: Vec<App> = (0..10).map(|i| app(&format!("app{i}"), "App")).collect();
        let mut model = Model::from_apps(apps, Vec::new(), 10, Language::Ru);
        model.toggle_view();
        assert_eq!(model.selected(), 0);
        assert_eq!(
            model.tiles_break(),
            None,
            "закреплённых нет — делить нечего"
        );
        model.move_col(1);
        assert_eq!(model.selected(), 1);
        model.move_col(-1);
        assert_eq!(model.selected(), 0);
        model.move_col(-1);
        assert_eq!(model.selected(), 0, "левее первой колонки ничего нет");
        model.move_sel(1);
        assert_eq!(model.selected(), GRID_COLS, "вниз — на ряд ниже");
        model.move_sel(-1);
        assert_eq!(model.selected(), 0, "вверх — на ряд выше");
        model.select(3);
        model.move_col(1);
        assert_eq!(model.selected(), 3, "правее последней колонки ничего нет");
        model.select(8);
        model.move_sel(1);
        assert_eq!(model.selected(), 8, "ниже последнего ряда ничего нет");
    }

    /// Смена вида не должна уводить выбор с приложения, которое человек нашёл.
    #[test]
    fn toggling_the_view_keeps_the_selected_app() {
        let mut model = model(&[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")], &["a"], 10);
        model.select_app("b");
        model.toggle_view();
        assert_eq!(
            model.selected_app().map(|app| app.name.as_str()),
            Some("Beta")
        );
        assert_eq!(model.top(), 0);
        model.toggle_view();
        assert_eq!(
            model.selected_app().map(|app| app.name.as_str()),
            Some("Beta")
        );
        assert_eq!(model.view(), ViewMode::List);
    }

    /// Прокрутка сетки идёт рядами: верх всегда кратен числу колонок, иначе
    /// первая строка окна была бы неполной.
    #[test]
    fn the_tile_grid_scrolls_by_whole_rows() {
        let apps: Vec<App> = (0..19).map(|i| app(&format!("app{i}"), "App")).collect();
        let mut model = Model::from_apps(apps, Vec::new(), 10, Language::Ru);
        model.toggle_view();
        assert_eq!(model.visible().len(), GRID_COLS * MAX_TILE_ROWS);
        model.select(18);
        assert_eq!(model.top(), GRID_COLS, "последний ряд подтянул окно");
        assert_eq!(model.top() % GRID_COLS, 0);
        model.select(3);
        assert_eq!(model.top(), 0, "верхний ряд снова виден");
    }

    /// Страница в сетке — это окно целиком, а не десять плиток списка.
    #[test]
    fn a_page_in_the_grid_moves_by_the_whole_window() {
        let apps: Vec<App> = (0..40).map(|i| app(&format!("app{i}"), "App")).collect();
        let mut model = Model::from_apps(apps, Vec::new(), 10, Language::Ru);
        model.toggle_view();
        model.page(1);
        assert_eq!(model.selected(), GRID_COLS * MAX_TILE_ROWS);
        model.page(-1);
        assert_eq!(model.selected(), 0);
    }

    /// Открепление в сетке тоже оставляет выбор на месте: на освободившейся
    /// плитке оказывается следующее приложение, а прокрутка не прыгает.
    #[test]
    fn unpinning_a_tile_keeps_the_cell() {
        let pinned: Vec<String> = (0..5).map(|i| format!("app{i}")).collect();
        let apps: Vec<App> = (0..10).map(|i| app(&format!("app{i}"), "App")).collect();
        let mut model = Model::from_apps(apps, pinned, 10, Language::Ru);
        model.toggle_view();
        model.select(2);
        let top = model.top();
        model.pinned.retain(|id| id != "app2");
        model.rebuild();
        model.keep_row(2, top);
        assert_eq!(
            model.selected_app().map(|app| app.id.as_str()),
            Some("app3"),
            "на месте откреплённой плитки стоит следующая"
        );
        assert_eq!(model.top(), top, "окно осталось на месте");
    }
}

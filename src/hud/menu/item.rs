//! Строка меню: что в ней лежит и как список себя ведёт.
//!
//! Список в окнах биндов и список меню — одна и та же задача: отфильтровать,
//! держать выбор в границах и не дать прокрутке уехать за край. Здесь это
//! сделано один раз и без привязки к Wayland: на входе кандидаты и запрос, на
//! выходе — видимые строки, выбранный индекс и положение верхней строки.
//!
//! Значение справа не хранится в строке. Строка помнит только `get`-функцию
//! узла, а текст значения получает при сборке списка: сменилась тема —
//! пересобрали список, и вторая копия значения не осталась врать.

use super::action::Action;
use super::bind_data;

/// Что показывает строка справа и что делает по Enter.
#[derive(Clone, Debug)]
pub enum ItemKind {
    /// Открывает вложенный список.
    Submenu,
    /// Результат глобального поиска: раскрывать нечего, Enter только
    /// переходит к источнику.
    Leaf,
    /// Одноразовое действие.
    Action(Action),
    /// Вкл/выкл. `set` получает новое состояние.
    Toggle { get: fn() -> bool, set: fn(bool) },
    /// Выбор из списка: индекс хранится целиком, наружу отдаётся подпись.
    Choice {
        options: &'static [&'static str],
        get: fn() -> usize,
        set: fn(usize),
    },
    /// Число в границах: значение приводится к `[min, max]` и к шагу.
    Number {
        min: f32,
        max: f32,
        step: f32,
        unit: &'static str,
        get: fn() -> f32,
        set: fn(f32),
    },
    /// Внешний выбор: обои, приложение. Идентификатор — на будущее для M4.
    Picker(&'static str),
    /// Громкость устройства одной строкой: полоса, mute по Enter, шаг
    /// стрелками. Отдельные строки «тише» и «громче» занимали по две на
    /// устройство и не показывали, о чём речь: при звуке и микрофоне
    /// подписи были одинаковыми.
    Volume {
        /// Команда уменьшения громкости.
        down: super::system::CommandSpec,
        /// Команда увеличения.
        up: super::system::CommandSpec,
        /// Переключение mute.
        mute: super::system::CommandSpec,
    },
}

impl ItemKind {
    /// Есть ли у пункта вложенный уровень.
    pub fn is_submenu(&self) -> bool {
        matches!(self, ItemKind::Submenu)
    }

    /// Требует ли пункт значения справа. У `Action` и `Leaf` его нет: там
    /// справа либо стрелка подменю, либо ничего.
    pub fn has_value(&self) -> bool {
        matches!(
            self,
            ItemKind::Toggle { .. } | ItemKind::Choice { .. } | ItemKind::Number { .. }
        )
    }

    /// Переключатель ли это: только у тумблера справа круг.
    pub fn is_toggle(&self) -> bool {
        matches!(self, ItemKind::Toggle { .. })
    }

    /// Выбор из списка ли это: только у него значение смотрится по индексу.
    pub fn is_choice(&self) -> bool {
        matches!(self, ItemKind::Choice { .. })
    }

    /// Внешний выбор ли это: у него вместо значения рисуется миниатюра.
    pub fn is_picker(&self) -> bool {
        matches!(self, ItemKind::Picker(_))
    }
}

/// Откуда строка взята: глубина уровня и индекс в его списке. Нужен глобальному
/// поиску, чтобы по Enter не открыть найденное, а перейти к нему.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
    pub depth: usize,
    pub index: usize,
}

/// Одна строка меню.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Глиф Nerd Font. Пустая строка — значка нет.
    pub icon: &'static str,
    /// Личность строки: id узла, а без id — заголовок. По ней выбор
    /// переживает пересборку уровня, где заголовок уже мог поменяться.
    pub id: String,
    /// Название пункта.
    pub title: String,
    /// Значение справа. Пусто, если значения нет.
    pub value: String,
    /// Вторая строка в режиме поиска: путь «Стиль › Тема».
    pub caption: Option<String>,
    /// Синонимы для поиска: взяты из узла, чтобы «Не беспокоить»
    /// находилось по «dnd» без «(DND)» в названии.
    pub keywords: &'static [&'static str],
    /// Уровень для полосы: проценты 0..=100. `None` — полосы нет.
    pub level: Option<u8>,
    /// Устройство заглушено: показывается значком у полосы.
    pub muted: bool,
    /// Поведение строки.
    pub kind: ItemKind,
    /// Место строки в дереве.
    pub origin: Origin,
}

impl Item {
    /// Строка в одну линию: без подписи снизу.
    pub fn single_line(&self) -> bool {
        self.caption.is_none()
    }

    /// Совпадает ли строка с запросом. Сравниваются название и путь, поэтому
    /// «уведом» находит и «Не беспокоить» в разделе «Система», и «Не
    /// беспокоить» в разделе «Уведомления».
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let needle = query.to_lowercase();
        let latin = bind_data::transliterate(query).to_lowercase();
        let title = self.title.to_lowercase();
        let title_latin = bind_data::transliterate(&self.title).to_lowercase();
        // Пустая транслитерация не должна совпасть ни с чем: подстрока «»
        // есть в любой строке, и русский запрос без маппинга нашёл бы всё.
        let latin_hit = !latin.is_empty() && title_latin.contains(&latin);
        if title.contains(&needle) || latin_hit {
            return true;
        }
        if self
            .keywords
            .iter()
            .any(|word| word.to_lowercase().contains(&needle))
        {
            return true;
        }
        self.caption.as_ref().is_some_and(|caption| {
            let caption = caption.to_lowercase();
            let caption_latin = bind_data::transliterate(&caption).to_lowercase();
            let latin_hit = !latin.is_empty() && caption_latin.contains(&latin);
            caption.contains(&needle) || latin_hit
        })
    }
}

/// Список строк одного уровня: кандидаты, фильтр, выбор и прокрутка.
///
/// Кандидаты и видимые строки хранятся отдельно: фильтр должен уметь
/// пересобрать видимую часть из полного набора, иначе после очистки запроса
/// список не вернётся в исходный вид.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemList {
    all: Vec<Item>,
    items: Vec<Item>,
    query: String,
    selected: usize,
    top: usize,
    page: usize,
}

impl ItemList {
    /// Сколько строк влезает в карточку. Значение по умолчанию — минимум
    /// мокапа, окно задаёт своё.
    pub const DEFAULT_PAGE: usize = 8;

    pub fn new(items: Vec<Item>) -> Self {
        let mut list = Self {
            all: items,
            // Страница по умолчанию обязана быть непустой: при `page = 0`
            // условие «выбранная строка ниже окна» выполняется всегда, и
            // `ensure_visible` уводит верх списка на строку вниз. Список
            // начинался бы со второй строки.
            page: Self::DEFAULT_PAGE,
            ..Self::default()
        };
        list.refilter();
        list
    }

    /// Видимые строки.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Все строки уровня до фильтра.
    pub fn candidates(&self) -> &[Item] {
        &self.all
    }

    /// Текущий запрос.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Индекс выбранной строки в видимой части.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Верхняя видимая строка.
    pub fn top(&self) -> usize {
        self.top
    }

    /// Сколько строк помещается в карточку.
    pub fn page(&self) -> usize {
        self.page
    }

    /// Задаёт высоту карточки в строках.
    pub fn set_page(&mut self, page: usize) {
        self.page = page.max(1);
        self.ensure_visible();
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Выбранная строка, если список не пуст.
    pub fn selected_item(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }

    /// Строки, попадающие в окно карточки.
    pub fn window(&self) -> &[Item] {
        let end = (self.top + self.page).min(self.items.len());
        &self.items[self.top.min(end)..end]
    }

    /// Пересобирает видимую часть под текущий запрос. Выбор старается уцелеть:
    /// если выбранная строка выжила, она остаётся выбранной, иначе выбирается
    /// ближайшая по прежней позиции.
    pub fn refilter(&mut self) {
        let previous = self.items.get(self.selected).map(|item| item.origin);
        let previous_pos = self.selected;
        self.items = self
            .all
            .iter()
            .filter(|item| item.matches(&self.query))
            .cloned()
            .collect();
        self.selected = match previous {
            Some(origin) => self
                .items
                .iter()
                .position(|item| item.origin == origin)
                .unwrap_or(previous_pos.min(self.items.len().saturating_sub(1))),
            None => 0,
        };
        self.clamp_scroll();
        self.ensure_visible();
    }

    /// Заменяет набор строк уровня и сразу применяет запрос.
    pub fn set_items(&mut self, items: Vec<Item>) {
        self.all = items;
        self.selected = 0;
        self.top = 0;
        self.refilter();
    }

    /// Заменить видимые строки, сохранив выбор и прокрутку. Отличие от
    /// `set_items` в том, что выбор не сбрасывается: список перерисовывают
    /// под тем же курсором (отметка «выполняется» на занятой строке).
    pub fn replace_items(&mut self, items: Vec<Item>) {
        self.all = items;
        let selected = self.selected.min(self.all.len().saturating_sub(1));
        self.selected = selected;
        self.refilter();
    }

    /// Добавляет символ к запросу и перефильтровывает.
    pub fn type_char(&mut self, ch: char) {
        if !ch.is_control() {
            self.query.push(ch);
            self.refilter();
        }
    }

    /// Удаляет последний символ запроса.
    pub fn backspace(&mut self) {
        if self.query.pop().is_some() {
            self.refilter();
        }
    }

    /// Очищает запрос целиком.
    pub fn clear_query(&mut self) {
        if !self.query.is_empty() {
            self.query.clear();
            self.refilter();
        }
    }

    /// Выбирает строку по индексу видимой части.
    pub fn select(&mut self, index: usize) {
        if self.items.is_empty() {
            self.selected = 0;
            self.top = 0;
            return;
        }
        self.selected = index.min(self.items.len() - 1);
        self.ensure_visible();
    }

    /// Сдвиг выбора на `delta` строк. Список зациклен: вниз с последней
    /// строки — на первую, вверх с первой — на последнюю. Удержание стрелки
    /// листает по кругу, пока клавишу не отпустишь: повторы шлёт композитор,
    /// а круг замыкается здесь. Большие шаги тоже идут по модулю длины.
    pub fn move_sel(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
        self.ensure_visible();
    }

    /// Прокрутка колесом или PageUp/PageDown: двигает верхнюю строку, выбор
    /// остаётся тем же индексом.
    pub fn scroll(&mut self, lines: isize) {
        if self.items.is_empty() {
            return;
        }
        let last = self.max_top();
        self.top = if lines < 0 {
            self.top.saturating_sub(lines.unsigned_abs())
        } else {
            self.top.saturating_add(lines as usize)
        }
        .min(last);
        self.clamp_selection();
    }

    /// Максимальное положение верхней строки: последняя страница заполнена
    /// целиком, если строк мало.
    pub fn max_top(&self) -> usize {
        self.items.len().saturating_sub(self.page)
    }

    /// Доводит выбор в видимую часть списка.
    pub fn ensure_visible(&mut self) {
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + self.page {
            self.top = self.selected + 1 - self.page;
        }
        self.top = self.top.min(self.max_top());
        self.clamp_selection();
    }

    fn clamp_scroll(&mut self) {
        self.top = self.top.min(self.max_top());
    }

    fn clamp_selection(&mut self) {
        if self.items.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.items.len() - 1);
        }
    }
}

/// Сравнение только формы: адреса `get`/`set` указывателей не уникальны, но
/// два разных числа с одинаковыми границами в тестах должны считаться одним и
/// тем же пунктом. Поэтому `fn`-поля не участвуют в сравнении, а числа,
/// варианты и единицы — участвуют.
impl PartialEq for ItemKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ItemKind::Submenu, ItemKind::Submenu) => true,
            (ItemKind::Leaf, ItemKind::Leaf) => true,
            (ItemKind::Action(left), ItemKind::Action(right)) => left == right,
            (ItemKind::Toggle { .. }, ItemKind::Toggle { .. }) => true,
            (ItemKind::Choice { options: left, .. }, ItemKind::Choice { options: right, .. }) => {
                left == right
            }
            (
                ItemKind::Number {
                    min: min1,
                    max: max1,
                    step: step1,
                    unit: unit1,
                    ..
                },
                ItemKind::Number {
                    min: min2,
                    max: max2,
                    step: step2,
                    unit: unit2,
                    ..
                },
            ) => (min1, max1, step1, unit1) == (min2, max2, step2, unit2),
            (ItemKind::Picker(left), ItemKind::Picker(right)) => left == right,
            // Команды громкости собираются заново при каждой пересборке
            // уровня, и их равенство ничего не значит для формы строки.
            (ItemKind::Volume { .. }, ItemKind::Volume { .. }) => true,
            _ => false,
        }
    }
}

impl Eq for ItemKind {}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, depth: usize, index: usize) -> Item {
        Item {
            level: None,
            muted: false,
            icon: "",
            id: title.to_string(),
            title: title.to_string(),
            value: String::new(),
            caption: None,
            keywords: &[],
            kind: ItemKind::Leaf,
            origin: Origin { depth, index },
        }
    }

    fn list(titles: &[&str]) -> ItemList {
        ItemList::new(
            titles
                .iter()
                .enumerate()
                .map(|(index, title)| item(title, 0, index))
                .collect(),
        )
    }

    /// Список, только что собранный, обязан показывать первую строку: иначе
    /// окно открывалось бы прокрученным на один пункт.
    #[test]
    fn fresh_list_starts_at_the_first_row() {
        let list = list(&["one", "two", "three"]);
        assert_eq!(list.top(), 0, "список не должен быть прокручен");
        assert_eq!(list.selected(), 0);
        assert_eq!(list.page(), ItemList::DEFAULT_PAGE);
    }

    #[test]
    fn empty_list_has_no_selection() {
        let mut list = ItemList::new(Vec::new());
        assert!(list.is_empty());
        assert_eq!(list.selected_item(), None);
        list.move_sel(1);
        list.scroll(3);
        assert_eq!(list.selected(), 0);
        assert_eq!(list.top(), 0);
    }

    /// Удержание стрелки листает список по кругу: вниз с последней строки —
    /// на первую, вверх с первой — на последнюю.
    #[test]
    fn selection_wraps_around_both_edges() {
        let mut list = list(&["a", "b", "c"]);
        list.set_page(2);
        list.move_sel(-1);
        assert_eq!(list.selected(), 2, "вверх с первой — на последнюю");

        list.move_sel(1);
        assert_eq!(list.selected(), 0, "вниз с последней — на первую");

        list.move_sel(10);
        assert_eq!(list.selected(), 1, "большой шаг тоже по кругу: 10 % 3");

        list.select(0);
        list.move_sel(-10);
        assert_eq!(list.selected(), 2, "вверх по кругу: -10 % 3");
    }

    #[test]
    fn selection_scrolls_into_view() {
        let mut list = list(&["a", "b", "c", "d", "e"]);
        list.set_page(2);
        list.move_sel(4);
        assert_eq!(list.selected(), 4);
        assert_eq!(list.top(), 3, "нижняя строка должна стать нижней в окне");
        assert_eq!(list.window().len(), 2);

        list.move_sel(-4);
        assert_eq!(list.top(), 0);
    }

    #[test]
    fn scroll_stops_at_edges_and_keeps_selection() {
        let mut list = list(&["a", "b", "c", "d", "e"]);
        list.set_page(2);
        list.scroll(100);
        assert_eq!(list.top(), list.max_top());
        assert_eq!(list.top(), 3);

        list.scroll(-100);
        assert_eq!(list.top(), 0);

        list.select(1);
        list.scroll(1);
        assert_eq!(list.selected(), 1, "колесо двигает вид, а не выбор");
    }

    #[test]
    fn typing_filters_and_clearing_restores() {
        let mut list = list(&["Apps", "Panel", "Style", "Notifications"]);
        for ch in "yle".chars() {
            list.type_char(ch);
        }
        assert_eq!(list.query(), "yle");
        assert_eq!(list.items().len(), 1);
        assert_eq!(list.items()[0].title, "Style");

        for _ in 0..3 {
            list.backspace();
        }
        assert_eq!(list.query(), "");
        assert_eq!(list.items().len(), 4);
    }

    #[test]
    fn control_characters_never_enter_the_query() {
        let mut list = list(&["Apps"]);
        list.type_char('\n');
        list.type_char('\t');
        assert_eq!(list.query(), "");
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn russian_layout_matches_latin_titles() {
        let mut list = list(&["Notifications", "Panel"]);
        for ch in "щ".chars() {
            list.type_char(ch);
        }
        assert_eq!(list.query(), "щ");
        assert_eq!(list.items().len(), 1, "«щ» должен найти Notifications");
        assert_eq!(list.items()[0].title, "Notifications");
    }

    #[test]
    fn query_matches_the_caption_path() {
        let mut row = item("Не беспокоить", 1, 0);
        row.caption = Some("Уведомления ›".to_string());
        let mut list = ItemList::new(vec![row]);
        list.type_char('у');
        assert_eq!(list.len(), 1, "поиск должен смотреть и в путь");

        let mut list = ItemList::new(vec![item("Не беспокоить", 1, 0)]);
        list.type_char('у');
        assert_eq!(list.len(), 0, "без подписи тот же запрос ничего не находит");
    }

    #[test]
    fn filter_keeps_the_selected_row_when_it_survives() {
        let mut list = list(&["Apps", "Panel", "Style"]);
        list.select(2);
        for ch in "nel".chars() {
            list.type_char(ch);
        }
        assert_eq!(list.items()[0].title, "Panel");
        assert_eq!(
            list.selected(),
            list.items()
                .iter()
                .position(|item| item.title == "Style")
                .unwrap_or(0)
        );
    }

    #[test]
    fn empty_result_keeps_query_for_the_hint() {
        let mut list = list(&["Apps"]);
        for ch in "zzz".chars() {
            list.type_char(ch);
        }
        assert!(list.is_empty());
        assert_eq!(list.query(), "zzz");
        assert!(list.selected_item().is_none());
        assert!(list.window().is_empty());
    }

    #[test]
    fn clearing_a_query_is_idempotent() {
        let mut list = list(&["Apps"]);
        list.clear_query();
        list.clear_query();
        assert_eq!(list.query(), "");
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn page_size_is_never_zero() {
        let mut list = list(&["a", "b"]);
        list.set_page(0);
        assert_eq!(list.page(), 1);
    }

    #[test]
    fn kind_reports_what_the_row_needs() {
        let toggle = ItemKind::Toggle {
            get: || true,
            set: |_| {},
        };
        assert!(toggle.is_toggle());
        assert!(toggle.has_value());
        assert!(!toggle.is_submenu());

        let choice = ItemKind::Choice {
            options: &["a"],
            get: || 0,
            set: |_| {},
        };
        assert!(choice.is_choice());
        assert!(!choice.is_toggle());

        assert!(ItemKind::Submenu.is_submenu());
        assert!(!ItemKind::Action(Action::Niri(&["msg"])).has_value());
        assert!(ItemKind::Picker("wallpaper").is_picker());
    }
}

//! Состояние меню: стек уровней и операции над ним.
//!
//! Меню — это стек: корень, поверх него подменю, поверх того ещё одно. Каждый
//! уровень держит свой `ItemList` со своим запросом и выбором, поэтому
//! возвращение назад возвращает вид именно этого уровня, а не соседнего.
//!
//! Операции не знают про Wayland и про отрисовку: `enter` возвращает
//! `Outcome`, и окно в M4 решает по нему, что показать. Это позволяет
//! проверять всю навигацию в тестах без композитора.

use super::action::Action;
use super::item::{Item, ItemKind, ItemList};
use super::settings::Language;
use super::system::{Generation, ProviderKey, ProviderSlot, slot_nodes};
use super::tree::{Node, NodeKind, clamp_number, level_rows, step_number};
use std::collections::{HashMap, HashSet};

/// Что изменилось в меню — окну нужно это, чтобы решить, что делать дальше.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Ничего не изменилось: пустой список или пункт, который нечего открывать.
    Idle,
    /// Открыт вложенный уровень.
    Pushed,
    /// Возврат на уровень выше.
    Popped,
    /// Меню закрыто: Esc на корне.
    Closed,
    /// Значение пункта изменилось, строка перерисована.
    Changed,
    /// Открыт выбор значения (Choice, Number, Picker) — в M4 это подменю.
    Value,
}

/// Сообщение в правой части подвала: вместо счётчика «1/8».
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Изменение применено.
    Applied(String),
    /// Не получилось.
    Failed(String),
}

impl Status {
    pub fn text(&self) -> &str {
        match self {
            Status::Applied(text) | Status::Failed(text) => text,
        }
    }

    /// Ошибка ли это: подвал красит точку другим цветом.
    pub fn is_failed(&self) -> bool {
        matches!(self, Status::Failed(_))
    }
}

/// Меню целиком.
///
/// Без `Clone`: внутри живёт [`super::secret::SecretInput`] с буфером пароля,
/// и обычное `.clone()` меню размножало бы пароль вместе с остальным
/// состоянием. Кому нужен снимок — берут [`Frame`], там только маска.
#[derive(Debug)]
pub struct Menu {
    /// Дерево пунктов. Живёт в меню, а не приходит снаружи: уровни
    /// пересобираются из него же.
    root: Vec<Node>,
    levels: Vec<Level>,
    status: Option<Status>,
    action_runner: fn(&Action) -> Result<(), String>,
    /// Язык подписей и значений: берётся из `settings.json` при старте.
    lang: Language,
    /// Слоты динамических разделов: snapshot и поколения по ключу. Живут
    /// дольше открытого уровня: повторный вход берёт готовый snapshot.
    dynamics: HashMap<ProviderKey, ProviderSlot<Vec<Node>>>,
    /// Запросы, ждущие worker-потока: цикл окна забирает их через
    /// `take_dynamic_requests` и запускает выборку, не блокируя ввод.
    pending: Vec<(ProviderKey, Generation)>,
    /// Команды, которые должен выполнить worker цикла окна: смена громкости,
    /// mute, выбор устройства. Исполнитель действий здесь не вызывается —
    /// модель только описывает команду, потоки заводит цикл.
    pending_commands: Vec<CommandJob>,
    /// Монотонный счётчик команд: одинаковый раздел подряд получает разные
    /// поколения, и устаревший результат не перетирает свежий.
    command_generation: u64,
    /// Активный ввод секрета: пароль сети. Живёт отдельным полем и не имеет
    /// отношения к запросу уровня — по запросу пароль искался бы в списке и
    /// уехал бы в кадр.
    secret_input: Option<super::secret::SecretInput>,
    /// Строки с выполняющейся командой. Подключение к точке доступа идёт
    /// секунды, а снимок обновится только после него: без отметки строка
    /// выглядела бы готовой, хотя ничего ещё не произошло.
    busy: HashSet<String>,
}

fn run_action(action: &Action) -> Result<(), String> {
    action.perform()
}

/// Корневой уровень с набором строк.
pub const ROOT_TITLE: &str = "HUD";

impl Menu {
    /// Новое меню по дереву. Открыт корень.
    pub fn new(root: Vec<Node>) -> Self {
        Self::with_action_runner(root, run_action)
    }

    /// Создаёт меню с исполнителем действий. Живое окно использует настоящий
    /// `Action::perform`, а тесты передают заглушку и не запускают GUI-команды.
    pub fn with_action_runner(
        root: Vec<Node>,
        action_runner: fn(&Action) -> Result<(), String>,
    ) -> Self {
        let mut list = ItemList::new(level_rows(&root, 0, Language::Ru));
        list.set_page(super::view::rows_shown(list.len()));
        Self {
            root,
            levels: vec![Level {
                title: ROOT_TITLE.to_string(),
                list,
                depth: 0,
                key: LevelKey::Node(ROOT_TITLE.to_string()),
            }],
            status: None,
            action_runner,
            lang: Language::Ru,
            dynamics: HashMap::new(),
            pending: Vec::new(),
            pending_commands: Vec::new(),
            command_generation: 0,
            busy: HashSet::new(),
            secret_input: None,
        }
    }

    /// Язык подписей текущего меню.
    pub fn lang(&self) -> Language {
        self.lang
    }

    /// Меняет язык и пересобирает строки: подписи «вкл» и заголовки дерева
    /// берутся уже из нового языка.
    pub fn set_lang(&mut self, lang: Language) {
        self.lang = lang;
        self.refresh();
    }

    /// Заменяет дерево целиком и возвращает меню на корень. Нужно смене
    /// языка: заголовки узлов хранятся в дереве, а не пересчитываются.
    /// Слоты динамических разделов и очередь команд НЕ сбрасываются: тумблер
    /// модуля не должен перезапрашивать `wpctl`, а смена языка — терять
    /// только что загруженный список устройств. Путь восстанавливает
    /// вызывающий через [`Menu::restore`].
    pub fn retree(&mut self, root: Vec<Node>) {
        self.root = root;
        self.levels.truncate(1);
        self.status = None;
        self.refresh();
    }

    /// Открытое подменю: нужно, когда окно рисует заголовок уровня.
    pub fn opened_submenu(&self) -> Option<&str> {
        let level = self.levels.last()?;
        (level.depth > 0).then_some(level.title.as_str())
    }

    /// Все уровни, корень первым.
    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// Текущий уровень.
    pub fn current(&self) -> &Level {
        self.levels.last().expect("в меню всегда есть корень")
    }

    pub fn current_mut(&mut self) -> &mut Level {
        self.levels.last_mut().expect("в меню всегда есть корень")
    }

    /// Глубина стека: 0 — корень.
    pub fn depth(&self) -> usize {
        self.levels.len() - 1
    }

    /// Крошки для шапки: «HUD › Стиль». Последний уровень ярче.
    pub fn trail(&self) -> Vec<&str> {
        let mut trail = Vec::with_capacity(self.levels.len() + 1);
        trail.push(ROOT_TITLE);
        for level in &self.levels[1..] {
            trail.push(&level.title);
        }
        trail
    }

    /// Дерево пунктов.
    pub fn root(&self) -> &[Node] {
        &self.root
    }

    /// Статус в подвале.
    pub fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    /// Ставит статус и снимает его через `clear_status`.
    pub fn set_status(&mut self, status: Status) {
        self.status = Some(status);
    }

    pub fn clear_status(&mut self) {
        self.status = None;
    }

    /// Сборка строк текущего уровня заново: значения поменялись, значит
    /// подписи справа устарели.
    pub fn refresh(&mut self) {
        self.rebuild_current();
    }

    /// Задать число строк, влезающих в карточку, всем уровням: окно знает
    /// свою высоту, а не модель.
    pub fn set_page(&mut self, page: usize) {
        for level in &mut self.levels {
            level.list.set_page(page);
        }
    }

    /// Сдвиг выбора на текущем уровне.
    pub fn move_sel(&mut self, delta: isize) {
        self.current_mut().list.move_sel(delta);
    }

    /// Прокрутка текущего уровня.
    pub fn scroll(&mut self, lines: isize) {
        self.current_mut().list.scroll(lines);
    }

    /// Символ в запрос текущего уровня. На корне запрос переключает список на
    /// глобальный поиск, поэтому уровень пересобирается.
    pub fn type_char(&mut self, ch: char) {
        let depth = self.depth();
        self.current_mut().list.type_char(ch);
        if depth == 0 {
            self.rebuild_current();
        }
    }

    /// Удаление символа из запроса.
    pub fn backspace(&mut self) {
        let depth = self.depth();
        self.current_mut().list.backspace();
        if depth == 0 {
            self.rebuild_current();
        }
    }

    /// Очистка запроса.
    pub fn clear_query(&mut self) {
        let depth = self.depth();
        self.current_mut().list.clear_query();
        if depth == 0 {
            self.rebuild_current();
        }
    }

    /// Запрос текущего уровня.
    pub fn query(&self) -> &str {
        self.current().list.query()
    }

    /// Возврат на уровень выше. На корне возвращает `false` — это сигнал
    /// закрыть меню, а не пустой переход.
    pub fn back(&mut self) -> Outcome {
        if self.depth() == 0 {
            self.status = None;
            return Outcome::Closed;
        }
        self.levels.pop();
        Outcome::Popped
    }

    /// Esc: на корне закрывает, выше — возвращает на уровень назад.
    pub fn escape(&mut self) -> Outcome {
        self.back()
    }

    /// Enter на выбранной строке.
    pub fn enter(&mut self) -> Outcome {
        let depth = self.depth();
        let Some(item) = self.current().list.selected_item().cloned() else {
            return Outcome::Idle;
        };
        if !item.kind.is_activatable() {
            return Outcome::Idle;
        }
        // Строка из глобального поиска пришла с путём: Enter открывает её
        // подменю и выбирает в нём, а не выполняет действие сразу.
        if depth == 0 && item.caption.is_some() {
            return self.jump_to_leaf(&item);
        }
        match item.kind.clone() {
            ItemKind::Submenu => {
                if self.push(&item.title) {
                    Outcome::Pushed
                } else {
                    Outcome::Idle
                }
            }
            ItemKind::Leaf => self.jump_to_leaf(&item),
            ItemKind::Toggle { get, set } => {
                set(!get());
                self.after_change();
                Outcome::Changed
            }
            ItemKind::Choice { get, set, options } => {
                let next = (get() + 1) % options.len().max(1);
                set(next);
                self.after_change();
                Outcome::Changed
            }
            ItemKind::Action(Action::SecretInput(target)) => {
                // Команда не составляется и тем более не запускается: пароля
                // ещё нет. Открывается ввод, и до `Enter` в нём nmcli не
                // запускается вообще.
                self.open_secret_input(target);
                Outcome::Value
            }
            ItemKind::Action(Action::Back) => {
                // «Отмена» в вопросе подтверждения: тот же возврат, что и
                // `Esc`, поэтому строка работает и на корне — там она просто
                // закрывает меню, как и должен `Esc`.
                self.back()
            }
            ItemKind::Action(Action::RefreshDynamic(key)) => {
                // Обновление выполняется здесь, а не исполнителем: слоты
                // живут в меню. Статус — подтверждение, уровень покажет
                // «Загрузка…» до ответа worker-потока.
                self.refresh_dynamic(key);
                self.status = Some(Status::Applied(
                    Action::RefreshDynamic(key).describe(self.lang),
                ));
                Outcome::Changed
            }
            ItemKind::Action(action @ (Action::VpnMode(_) | Action::VpnNode(_))) => {
                // Переключение может частично выполниться (например, первый
                // PUT прошёл, второй нет). Перечитываем состояние и при успехе,
                // и при ошибке, чтобы строка режима и выбранный узел не врали.
                let result = (self.action_runner)(&action);
                self.refresh_dynamic(ProviderKey::VPN);
                match result {
                    Ok(()) => {
                        self.status = Some(Status::Applied(action.describe(self.lang)));
                        Outcome::Changed
                    }
                    Err(error) => {
                        self.status = Some(Status::Failed(error));
                        Outcome::Idle
                    }
                }
            }
            ItemKind::Action(Action::RefreshAndRunAll { commands, refresh }) => {
                // Пресет: команда одна по смыслу, но по факту их несколько, и
                // все они должны уйти одним нажатием и одним обновлением
                // снимка. Порядок и «сорваться на первой ошибке» — забота
                // worker-потока.
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
            ItemKind::Action(Action::RefreshAndRun { command, refresh }) => {
                // Команда не выполняется здесь: enter вызывается в UI-потоке,
                // а звук и сеть ждут subprocess. Кладём в очередь, цикл окна
                // запустит worker и после успеха обновит слот. Строка, из
                // которой нажали, помечается занятой до ответа worker-а.
                self.command_generation += 1;
                let generation = self.command_generation;
                let describe = command.log_line();
                self.busy.insert(item.id.clone());
                self.pending_commands.push(CommandJob {
                    generation,
                    commands: vec![command],
                    refresh,
                    row: item.id.clone(),
                });
                self.status = Some(Status::Applied(describe));
                // Отметка «выполняется» видна сразу, а не после следующей
                // пересборки: иначе нажатая строка мигнула бы как обычно
                // выбранная, пока worker ещё даже не начал.
                self.rebuild_current();
                Outcome::Changed
            }
            ItemKind::Action(action) => match (self.action_runner)(&action) {
                Ok(()) => {
                    self.status = Some(Status::Applied(action.describe(self.lang)));
                    Outcome::Changed
                }
                Err(error) => {
                    self.status = Some(Status::Failed(error));
                    Outcome::Idle
                }
            },
            ItemKind::Volume { mute, .. } => {
                // Enter на полоске — это mute: громкость ею не меняется, а
                // отдельной строки ради переключения больше нет.
                self.queue_command(&item, vec![mute], super::system::ProviderKey::AUDIO)
            }
            ItemKind::Picker(_) | ItemKind::Number { .. } => {
                if depth == 0 {
                    // Число и пикер на корне выглядели бы странно: их значения
                    // меняются стрелками, а не Enter.
                    Outcome::Idle
                } else {
                    Outcome::Value
                }
            }
            ItemKind::Header | ItemKind::Info => Outcome::Idle,
        }
    }

    /// `Shift+↑`/`Shift+↓` над строкой «порядок модулей»: двигает этот
    /// модуль внутри его зоны (перенос через границу запрещён). На любой
    /// другой строке — no-op.
    pub fn nudge_module(&mut self, delta: i32) -> Outcome {
        let Some(item) = self.current().list.selected_item().cloned() else {
            return Outcome::Idle;
        };
        let ItemKind::Action(action) = item.kind else {
            return Outcome::Idle;
        };
        let Action::MoveModule { module, .. } = action else {
            return Outcome::Idle;
        };
        match (self.action_runner)(&Action::MoveModule { module, delta }) {
            Ok(()) => {
                self.status = Some(Status::Applied(action.describe(self.lang)));
                self.after_change();
                Outcome::Changed
            }
            Err(error) => {
                self.status = Some(Status::Failed(error));
                Outcome::Idle
            }
        }
    }

    /// `Shift+стрелки` на строке громкости: шаг вниз или вверх.
    pub fn adjust_volume(&mut self, direction: i32) -> Outcome {
        let Some(item) = self.current().list.selected_item().cloned() else {
            return Outcome::Idle;
        };
        let ItemKind::Volume { down, up, .. } = item.kind.clone() else {
            return Outcome::Idle;
        };
        let command = if direction < 0 { down } else { up };
        self.queue_command(&item, vec![command], super::system::ProviderKey::AUDIO)
    }

    /// Стрелки влево-вправо на числовом пункте: значение в пределах границ.
    pub fn adjust_value(&mut self, direction: i32) -> Outcome {
        let Some(item) = self.current().list.selected_item().cloned() else {
            return Outcome::Idle;
        };
        let ItemKind::Number {
            min,
            max,
            step,
            get,
            set,
            ..
        } = item.kind
        else {
            return Outcome::Idle;
        };
        let next = step_number(get(), min, max, step, direction);
        set(next);
        self.after_change();
        Outcome::Changed
    }

    fn after_change(&mut self) {
        self.refresh();
    }

    /// Пересобирает строки текущего уровня из дерева: значения поменялись,
    /// значит подписи справа устарели. Динамический уровень пересобирается
    /// из своего слота, а не из статического дерева.
    pub(crate) fn rebuild_current(&mut self) {
        let lang = self.lang;
        if let Some(key) = self.current().dynamic() {
            let nodes = self
                .dynamics
                .get(&key)
                .map(|slot| slot_nodes(key, slot.state(), lang))
                .unwrap_or_default();
            self.current_mut().rebuild(&nodes, lang);
        } else {
            let depth = self.depth();
            let nodes = self.nodes_at(depth);
            self.current_mut().rebuild(&nodes, lang);
        }
        self.mark_busy();
    }
}

/// Итог текущего состояния: строки, счётчик и состояние подвала.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Крошки.
    pub trail: Vec<String>,
    /// Видимые строки.
    pub items: Vec<Item>,
    /// Выбранный индекс. `Some` всегда указывает на выбираемую строку.
    pub selected: Option<usize>,
    /// Индекс первой видимой строки.
    pub top: usize,
    /// Запрос.
    pub query: String,
    /// Всего строк до фильтра.
    pub total: usize,
    /// Статус подвала.
    pub status: Option<Status>,
    /// Ввод секрета, если он открыт: только маска и заголовок, без пароля.
    pub secret: Option<super::secret::SecretFrame>,
    /// Язык подписей хрома: отрисовка берёт подсказки по нему.
    pub lang: Language,
}

impl Menu {
    /// Снимок текущего уровня для отрисовки.
    pub fn frame(&self) -> Frame {
        let level = self.current();
        Frame {
            trail: self.trail().into_iter().map(str::to_string).collect(),
            items: level.list.items().to_vec(),
            selected: level.list.selected(),
            top: level.list.top(),
            query: level.list.query().to_string(),
            total: level.list.len(),
            status: self.status.clone(),
            secret: self
                .secret_input
                .as_ref()
                .map(|input| input.frame(self.lang)),
            lang: self.lang,
        }
    }
}

/// Список всех `get`-функций узлов дерева: проверка в тестах, что у каждого
/// контрола есть значение.
pub fn control_count(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .filter(|node| node.is_visible() && !matches!(node.kind, NodeKind::Header | NodeKind::Info))
        .map(|node| match node.children() {
            Some(children) => control_count(children),
            None => 1,
        })
        .sum()
}

/// Проверка, что дерево пригодно для меню: у каждого узла есть глиф и
/// заголовок, у подменю — непустые дети.
pub fn validate(nodes: &[Node]) -> Result<(), String> {
    for node in nodes {
        if node.title.trim().is_empty() {
            return Err("узел без заголовка".to_string());
        }
        if let NodeKind::Submenu(children) = &node.kind
            && children.is_empty()
        {
            return Err(format!("подменю «{}» пустое", node.title));
        }
        if let NodeKind::Choice { options, .. } = &node.kind
            && options.is_empty()
        {
            return Err(format!("у «{}» пустой список вариантов", node.title));
        }
        if let Some(children) = node.children() {
            validate(children)?;
        }
    }
    Ok(())
}

/// Значение числа узла: тесты границ без окна.
pub fn number_value(node: &Node) -> Option<f32> {
    match &node.kind {
        NodeKind::Number {
            min,
            max,
            step,
            get,
            ..
        } => Some(clamp_number(get(), *min, *max, *step)),
        _ => None,
    }
}

mod commands;
mod dynamic;
mod levels;
mod position;
mod secret;
mod stack;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use commands::{CommandJob, busy_note};
#[allow(unused_imports)]
pub use levels::{Level, LevelKey};
#[allow(unused_imports)]
pub use position::Position;

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
use super::system::{Generation, ProviderKey, ProviderSlot, SlotState, slot_nodes};
use super::tree::{Node, NodeKind, clamp_number, level_rows, search_rows, step_number};
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

/// Отметка на строке, чья команда ещё выполняется worker-ом.
pub fn busy_note(lang: Language) -> &'static str {
    match lang {
        Language::Ru => "выполняется…",
        Language::En => "working…",
    }
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
    pub commands: Vec<super::system::CommandSpec>,
    /// Чей слот обновить после успешного выполнения.
    pub refresh: ProviderKey,
    /// Личность строки, из которой команду запустили: по ней уровень
    /// снимает отметку «выполняется», когда worker ответит.
    pub row: String,
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
        let list = ItemList::new(level_rows(&root, 0, Language::Ru));
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
        let index = self.current().list.selected();
        let Some(item) = self.current().list.items().get(index).cloned() else {
            return Outcome::Idle;
        };
        // Строка из глобального поиска пришла с путём: Enter открывает её
        // подменю и выбирает в нём, а не выполняет действие сразу.
        if depth == 0 && item.caption.is_some() {
            return self.jump_to_leaf(&item);
        }
        match item.kind {
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
                self.status = Some(Status::Applied(Action::RefreshDynamic(key).describe()));
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
                        self.status = Some(Status::Applied(action.describe()));
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
                    self.status = Some(Status::Applied(action.describe()));
                    Outcome::Changed
                }
                Err(error) => {
                    self.status = Some(Status::Failed(error));
                    Outcome::Idle
                }
            },
            ItemKind::Picker(_) | ItemKind::Number { .. } => {
                if depth == 0 {
                    // Число и пикер на корне выглядели бы странно: их значения
                    // меняются стрелками, а не Enter.
                    Outcome::Idle
                } else {
                    Outcome::Value
                }
            }
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
                self.status = Some(Status::Applied(action.describe()));
                self.after_change();
                Outcome::Changed
            }
            Err(error) => {
                self.status = Some(Status::Failed(error));
                Outcome::Idle
            }
        }
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

    /// Открывает подменю по заголовку. Пустой уровень не создаётся: иначе в
    /// стеке окажется карточка без строк и без заголовка. Динамический
    /// раздел открывается особым путём: уровень сразу получает «Загрузка…»,
    /// а запрос уходит в очередь worker-потока.
    fn push(&mut self, identity: &str) -> bool {
        self.push_key(&LevelKey::Node(identity.to_string()))
    }

    /// Открыть уровень по личности, а не по заголовку. Единственный путь и в
    /// обычный Enter, и в восстановление после `retree`.
    fn push_key(&mut self, key: &LevelKey) -> bool {
        let depth = self.depth();
        let nodes = self.nodes_at(depth);
        let node = match key {
            LevelKey::Node(identity) => nodes
                .iter()
                .find(|node| node.identity() == identity)
                .or_else(|| nodes.iter().find(|node| node.title == *identity)),
            LevelKey::Dynamic(provider) => nodes
                .iter()
                .find(|node| matches!(&node.kind, NodeKind::Dynamic { key } if key == provider)),
        };
        let Some(node) = node else {
            return false;
        };
        match &node.kind {
            NodeKind::Submenu(children) if !children.is_empty() => {
                let lang = self.lang;
                let list = ItemList::new(level_rows(children, depth + 1, lang));
                let (title, identity) = (node.title.clone(), node.identity().to_string());
                self.levels.push(Level {
                    title,
                    list,
                    depth: depth + 1,
                    key: LevelKey::Node(identity),
                });
                true
            }
            NodeKind::Dynamic { key } => self.push_dynamic(&node.title, *key),
            _ => false,
        }
    }

    /// Открывает динамический уровень: слот заводится при первом входе,
    /// запрос — только из пустого состояния, готовый snapshot — из кэша.
    fn push_dynamic(&mut self, title: &str, key: ProviderKey) -> bool {
        let depth = self.depth();
        let lang = self.lang;
        let slot = self
            .dynamics
            .entry(key)
            .or_insert_with(|| ProviderSlot::new(key));
        if let Some(generation) = slot.request() {
            self.pending.push((key, generation));
        }
        let nodes = slot_nodes(key, slot.state(), lang);
        let list = ItemList::new(level_rows(&nodes, depth + 1, lang));
        self.levels.push(Level {
            title: title.to_string(),
            list,
            depth: depth + 1,
            key: LevelKey::Dynamic(key),
        });
        true
    }

    /// Забирает очередь запросов для worker-потоков цикла окна. Очередь, а
    /// не один запрос: подряд можно открыть два раздела, и ни один запрос
    /// не должен потеряться.
    pub fn take_dynamic_requests(&mut self) -> Vec<(ProviderKey, Generation)> {
        std::mem::take(&mut self.pending)
    }

    /// Открыт ли ввод секрета.
    pub fn secret_active(&self) -> bool {
        self.secret_input.is_some()
    }

    /// Текущий ввод секрета: нужно окну для отрисовки и тестам.
    pub fn secret_input(&self) -> Option<&super::secret::SecretInput> {
        self.secret_input.as_ref()
    }

    /// Открыть ввод секрета для цели. Запрос уровня не трогается: он остаётся
    /// ровно тем, чем был до открытия пароля.
    pub fn open_secret_input(&mut self, target: super::secret::SecretTarget) {
        let lang = self.lang;
        self.secret_input = Some(super::secret::SecretInput::new(target, lang));
        self.status = None;
    }

    /// Дописать символ к паролю. Любой печатный Unicode: пароль не латиница.
    pub fn type_secret_char(&mut self, ch: char) {
        if let Some(input) = &mut self.secret_input {
            input.push(ch);
        }
    }

    /// Стереть последний символ пароля.
    pub fn secret_backspace(&mut self) {
        if let Some(input) = &mut self.secret_input {
            input.erase();
        }
    }

    /// Очистить пароль целиком (`Ctrl+U`).
    pub fn secret_clear(&mut self) {
        if let Some(input) = &mut self.secret_input {
            input.clear();
        }
    }

    /// Отменить ввод: буфер выбрасывается, команда не составляется, меню
    /// остаётся на том же уровне с той же выбранной строкой.
    pub fn cancel_secret_input(&mut self) -> Outcome {
        match self.secret_input.take() {
            Some(_) => Outcome::Value,
            None => Outcome::Idle,
        }
    }

    /// Подтвердить ввод: собрать команду с `CmdArg::Secret`, поставить её в
    /// очередь worker-а и пометить строку занятой.
    ///
    /// UI-буфер пароля после этого пуст: `SecretInput` разобран на части и
    /// выброшен. Но пароль не «исчез полностью»: он временно живёт внутри
    /// `CommandSpec`, затем в `CommandJob` очереди и, наконец, в `argv`
    /// дочернего процесса (виден через `ps`). От журналов, статусов, `Debug`
    /// и кадров он при этом скрыт — см. `CmdArg::Secret`.
    ///
    /// Пустой пароль не запрещается: правила паролей у NetworkManager свои,
    /// и выдумывать тут нечего.
    pub fn submit_secret_input(&mut self) -> Outcome {
        let Some(input) = self.secret_input.take() else {
            return Outcome::Idle;
        };
        let (target, password) = input.into_parts();
        let command = target.command(&password);
        drop(password);
        self.command_generation += 1;
        let generation = self.command_generation;
        let row = target.row_id().to_string();
        self.busy.insert(row.clone());
        self.pending_commands.push(CommandJob {
            generation,
            commands: vec![command],
            refresh: target.provider_key(),
            row,
        });
        // Статус берётся из `log_line`, где секрет уже `<redacted>`: пароль в
        // подвале появиться не может.
        self.status = Some(Status::Applied(
            self.pending_commands
                .last()
                .and_then(|job| {
                    job.commands
                        .last()
                        .map(super::system::CommandSpec::log_line)
                })
                .unwrap_or_default(),
        ));
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

    /// Узлы уровня по глубине: корень — это `root`, остальные уровни
    /// достаются спуском по заголовкам уже открытых подменю. Динамический
    /// уровень не имеет детей в статическом дереве: его строки лежат в
    /// слоте провайдера, поэтому спуск через него идёт по `dynamics`.
    fn nodes_at(&self, depth: usize) -> Vec<Node> {
        self.nodes_at_opt(depth)
            .map(<[Node]>::to_vec)
            .unwrap_or_default()
    }

    /// Как `nodes_at`, но отличает «узла больше нет» (`None`) от «детей нет»
    /// и отдаёт срез, а не копию.
    ///
    /// Спуск идёт по ссылкам: раньше здесь клонировался весь `root` и затем
    /// копия нужного уровня, то есть на каждый выход в дерево копировалось всё
    /// поддерево целиком. `Node` в вызывающих не мутируется — собирается
    /// срез строк, — поэтому копия тут не нужна.
    fn nodes_at_opt(&self, depth: usize) -> Option<&[Node]> {
        let mut nodes: &[Node] = &self.root;
        for level in self.levels.get(1..=depth).unwrap_or_default() {
            match &level.key {
                LevelKey::Node(identity) => {
                    let parent = nodes.iter().find(|node| node.identity() == identity)?;
                    nodes = parent.children()?;
                }
                LevelKey::Dynamic(key) => match self.dynamics.get(key).map(ProviderSlot::state) {
                    Some(SlotState::Ready(snapshot)) => nodes = snapshot.as_slice(),
                    _ => return None,
                },
            }
        }
        Some(nodes)
    }

    /// Пересобирает динамический уровень и всё, что открыто над ним. Окно
    /// применяет снимок прямо, без `position/restore`, пока пользователь может
    /// стоять во вложенном подменю: без этого и оно, и родитель после «назад»
    /// показывали бы устаревшие строки. Если вложенный узел исчез из нового
    /// снимка, меню возвращается на последний живой уровень.
    fn rebuild_open_levels(&mut self, key: ProviderKey) {
        let lang = self.lang;
        let Some(start) = self.levels.iter().position(|l| l.dynamic() == Some(key)) else {
            return;
        };
        let mut index = start;
        while index < self.levels.len() {
            let nodes = if index == start {
                self.dynamics
                    .get(&key)
                    .map(|slot| slot_nodes(key, slot.state(), lang))
                    .unwrap_or_default()
            } else {
                // Срез достаётся до mutable-доступа к уровню: держать
                // заимствование дерева и одновременно пересобирать уровень
                // нельзя, а копия здесь означала бы клон поддерева уровня.
                let Some(found) = self
                    .nodes_at_opt(self.levels[index].depth)
                    .map(<[Node]>::to_vec)
                else {
                    self.levels.truncate(index);
                    break;
                };
                found
            };
            self.levels[index].rebuild(&nodes, lang);
            self.mark_busy_level(index);
            index += 1;
        }
    }

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
            if row.is_empty() {
                continue;
            }
            if let Some(index) = level.list.items().iter().position(|item| &item.id == row) {
                level.list.select(index);
            }
        }
        self.mark_busy();
    }

    /// Переход к найденному листу: открывает его подменю и выбирает строку.
    fn jump_to_leaf(&mut self, item: &Item) -> Outcome {
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
                .position(|row| row.title == target)
        {
            self.current_mut().list.select(index);
        }
        Outcome::Pushed
    }

    fn after_change(&mut self) {
        self.refresh();
    }

    /// Пересобирает строки текущего уровня из дерева: значения поменялись,
    /// значит подписи справа устарели. Динамический уровень пересобирается
    /// из своего слота, а не из статического дерева.
    fn rebuild_current(&mut self) {
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

    /// Навесить отметки «выполняется» на строки текущего уровня. Вызывается
    /// после каждой пересборки уровня: и списка, и после `restore`.
    fn mark_busy(&mut self) {
        let last = self.levels.len().saturating_sub(1);
        self.mark_busy_level(last);
    }

    fn mark_busy_level(&mut self, index: usize) {
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

/// Положение меню: открытые уровни и выбранная строка каждого из них.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Position {
    /// Личности уровней от корня, без него самого.
    pub levels: Vec<LevelKey>,
    /// Выбранная строка каждого уровня по id, корень включительно. Пустая
    /// строка — уровня нет или он пуст.
    pub rows: Vec<String>,
}

/// Итог текущего состояния: строки, счётчик и состояние подвала.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Крошки.
    pub trail: Vec<String>,
    /// Видимые строки.
    pub items: Vec<Item>,
    /// Выбранный индекс.
    pub selected: usize,
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
        .filter(|node| node.is_visible())
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

#[cfg(test)]
mod tests {
    use super::super::action::{Action, Live};
    use super::super::item::Origin;
    use super::super::snapshot::render_menu_card;
    use super::super::tree::Node;
    use super::*;

    fn tree() -> Vec<Node> {
        vec![
            Node::submenu(
                "apps",
                "Приложения",
                vec![Node::action("term", "Терминал", Action::Run("kitty", &[]))],
            ),
            Node::submenu(
                "style",
                "Стиль",
                vec![
                    Node::choice("theme", "Тема", &["Обычная", "Pixel"], || 0, |_| {}),
                    Node::number("height", "Высота", 20.0, 48.0, 1.0, "px", || 27.0, |_| {}),
                ],
            ),
            Node::toggle("dnd", "Не беспокоить", || true, |_| {}),
        ]
    }

    fn menu() -> Menu {
        Menu::new(tree())
    }

    fn titles(menu: &Menu) -> Vec<String> {
        menu.current()
            .list
            .items()
            .iter()
            .map(|item| item.title.clone())
            .collect()
    }

    #[test]
    fn new_menu_opens_the_root() {
        let menu = menu();
        assert_eq!(menu.depth(), 0);
        assert_eq!(menu.trail(), vec!["HUD"]);
        assert_eq!(menu.opened_submenu(), None);
        assert_eq!(titles(&menu), ["Приложения", "Стиль", "Не беспокоить"]);
    }

    #[test]
    fn enter_pushes_a_level_and_back_pops_it() {
        let mut menu = menu();
        menu.move_sel(1);
        assert_eq!(menu.enter(), Outcome::Pushed);
        assert_eq!(menu.depth(), 1);
        assert_eq!(menu.opened_submenu(), Some("Стиль"));
        assert_eq!(menu.trail(), vec!["HUD", "Стиль"]);
        assert_eq!(titles(&menu), ["Тема", "Высота"]);

        assert_eq!(menu.back(), Outcome::Popped);
        assert_eq!(menu.depth(), 0);
        assert_eq!(menu.opened_submenu(), None);
        assert_eq!(titles(&menu), ["Приложения", "Стиль", "Не беспокоить"]);
    }

    #[test]
    fn escape_on_the_root_closes_the_menu() {
        let mut menu = menu();
        assert_eq!(menu.escape(), Outcome::Closed);
    }

    #[test]
    fn escape_inside_a_submenu_returns_one_level() {
        let mut menu = menu();
        menu.move_sel(1);
        menu.enter();
        assert_eq!(menu.escape(), Outcome::Popped);
        assert_eq!(menu.depth(), 0);
        assert_eq!(menu.escape(), Outcome::Closed);
    }

    #[test]
    fn each_level_keeps_its_own_query_and_selection() {
        let mut menu = menu();
        menu.move_sel(1);
        menu.enter();
        menu.type_char('т');
        assert_eq!(menu.query(), "т");
        menu.back();
        assert_eq!(menu.query(), "", "запрос не должен утекать между уровнями");
    }

    #[test]
    fn root_search_spans_all_leaves_and_shows_the_path() {
        let mut menu = menu();
        for ch in "тем".chars() {
            menu.type_char(ch);
        }
        let frame = menu.frame();
        assert_eq!(frame.items.len(), 1);
        assert_eq!(frame.items[0].title, "Тема");
        assert_eq!(frame.items[0].caption.as_deref(), Some("Стиль ›"));
        assert_eq!(frame.total, 1);
    }

    #[test]
    fn root_search_finds_leaves_from_other_branches() {
        let mut menu = menu();
        for ch in "выс".chars() {
            menu.type_char(ch);
        }
        let frame = menu.frame();
        assert_eq!(frame.items.len(), 1);
        assert_eq!(frame.items[0].title, "Высота");
        assert_eq!(frame.items[0].origin, Origin { depth: 0, index: 2 });
    }

    #[test]
    fn search_inside_a_level_stays_inside_that_level() {
        let mut menu = menu();
        menu.move_sel(1);
        menu.enter();
        for ch in "тем".chars() {
            menu.type_char(ch);
        }
        assert_eq!(menu.query(), "тем");
        assert_eq!(titles(&menu), ["Тема"], "поиск уровня не лезет в корень");

        menu.back();
        assert_eq!(titles(&menu), ["Приложения", "Стиль", "Не беспокоить"]);
    }

    #[test]
    fn clearing_the_root_query_restores_the_sections() {
        let mut menu = menu();
        for ch in "тем".chars() {
            menu.type_char(ch);
        }
        assert_eq!(menu.frame().items.len(), 1);
        menu.clear_query();
        assert_eq!(titles(&menu), ["Приложения", "Стиль", "Не беспокоить"]);
    }

    #[test]
    fn backspace_shrinks_the_query_and_the_result() {
        let mut menu = menu();
        for ch in "высота".chars() {
            menu.type_char(ch);
        }
        assert_eq!(menu.frame().items.len(), 1);
        menu.backspace();
        assert_eq!(menu.query(), "высот");
        menu.backspace();
        menu.backspace();
        assert_eq!(menu.query(), "выс");
        assert_eq!(menu.frame().items.len(), 1);
        menu.clear_query();
        menu.type_char('ф');
        assert_eq!(menu.query(), "ф");
        assert_eq!(
            menu.frame().items.len(),
            0,
            "буквы «ф» нет ни в одном названии и ни в одном пути"
        );
        assert_eq!(menu.enter(), Outcome::Idle);
    }

    #[test]
    fn enter_on_a_search_result_opens_its_own_level() {
        let mut menu = menu();
        for ch in "тем".chars() {
            menu.type_char(ch);
        }
        assert_eq!(menu.enter(), Outcome::Pushed);
        assert_eq!(menu.depth(), 1);
        assert_eq!(menu.opened_submenu(), Some("Стиль"));
        assert_eq!(titles(&menu), ["Тема", "Высота"]);
        assert_eq!(
            menu.current().list.selected(),
            0,
            "найденная строка выбрана"
        );
    }

    #[test]
    fn hidden_nodes_are_absent_from_every_level_and_from_search() {
        let mut menu = Menu::new(vec![
            Node::toggle("dnd", "Не беспокоить", || false, |_| {}).when(|| false),
            Node::action("x", "Терминал", Action::Live(Live::DndToggle)),
        ]);
        assert_eq!(titles(&menu), ["Терминал"]);
        for ch in "беспокоить".chars() {
            menu.type_char(ch);
        }
        assert!(menu.frame().items.is_empty());
    }

    #[test]
    fn selection_wraps_around_the_level_while_scroll_stays_clamped() {
        let mut menu = menu();
        menu.set_page(2);
        menu.move_sel(99);
        assert_eq!(menu.current().list.selected(), 0, "99 % 3 — круг");
        menu.move_sel(-1);
        assert_eq!(menu.current().list.selected(), 2, "вверх с первой — вниз");

        menu.scroll(99);
        assert_eq!(menu.current().list.top(), 1, "нижняя страница");
        menu.scroll(-99);
        assert_eq!(menu.current().list.top(), 0);
    }

    #[test]
    fn toggle_reports_a_change_and_rebuilds_the_row() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static ON: AtomicBool = AtomicBool::new(true);
        fn get() -> bool {
            ON.load(Ordering::Relaxed)
        }
        fn set(on: bool) {
            ON.store(on, Ordering::Relaxed);
        }
        let mut menu = Menu::new(vec![Node::toggle("dnd", "Не беспокоить", get, set)]);
        assert_eq!(menu.frame().items[0].value, "вкл");
        assert_eq!(menu.enter(), Outcome::Changed);
        assert_eq!(menu.frame().items[0].value, "выкл", "строка пересобралась");
        ON.store(true, Ordering::Relaxed);
    }

    #[test]
    fn numbers_never_leave_their_bounds() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static VALUE: AtomicU32 = AtomicU32::new(27);
        fn get() -> f32 {
            VALUE.load(Ordering::Relaxed) as f32
        }
        fn set(value: f32) {
            VALUE.store(value.round() as u32, Ordering::Relaxed);
        }
        let mut menu = Menu::new(vec![Node::submenu(
            "panel",
            "Панель",
            vec![Node::number("h", "Высота", 20.0, 48.0, 1.0, "px", get, set)],
        )]);
        menu.enter();
        assert_eq!(menu.frame().items[0].value, "27px");

        for _ in 0..40 {
            menu.adjust_value(1);
        }
        assert_eq!(menu.frame().items[0].value, "48px", "потолок держит");
        for _ in 0..60 {
            menu.adjust_value(-1);
        }
        assert_eq!(menu.frame().items[0].value, "20px", "пол держит");
    }

    #[test]
    fn arrows_on_a_row_without_a_number_do_nothing() {
        let mut menu = menu();
        assert_eq!(menu.adjust_value(1), Outcome::Idle);
        assert_eq!(menu.adjust_value(-1), Outcome::Idle);
    }

    #[test]
    fn a_stub_action_reports_failure_in_the_footer() {
        fn fail_action(_: &Action) -> Result<(), String> {
            Err("M4 test action failed".to_string())
        }

        let mut menu = Menu::with_action_runner(
            vec![Node::action("x", "Терминал", Action::Run("kitty", &[]))],
            fail_action,
        );
        assert_eq!(menu.enter(), Outcome::Idle);
        let status = menu.status().expect("статус после неудачи");
        assert!(status.is_failed());
        assert!(status.text().contains("M4"));
        menu.clear_status();
        assert!(menu.status().is_none());
    }

    #[test]
    fn vpn_selection_refreshes_its_dynamic_snapshot_after_success() {
        fn succeed(_: &Action) -> Result<(), String> {
            Ok(())
        }

        let mut menu = Menu::with_action_runner(
            vec![Node::dynamic("network", "VPN", ProviderKey::VPN)],
            succeed,
        );
        assert_eq!(menu.enter(), Outcome::Pushed);
        let (key, generation) = menu
            .take_dynamic_requests()
            .pop()
            .expect("первый snapshot запрошен");
        assert_eq!(key, ProviderKey::VPN);
        menu.apply_dynamic(
            key,
            generation,
            Ok(vec![Node::action(
                "server",
                "🇺🇸 США, Атланта",
                Action::VpnNode("🇺🇸 США, Атланта".to_string()),
            )]),
        );

        assert_eq!(menu.enter(), Outcome::Changed);

        let (key, generation) = menu
            .take_dynamic_requests()
            .pop()
            .expect("snapshot обновляется");
        assert!(menu.status().is_some_and(|status| !status.is_failed()));
        menu.apply_dynamic(
            key,
            generation,
            Ok(vec![Node::info("server", "обновлённый выбор")]),
        );
        assert_eq!(menu.frame().items[0].title, "обновлённый выбор");
    }

    #[test]
    fn a_failed_vpn_mode_change_still_refreshes_partial_controller_state() {
        fn fail(_: &Action) -> Result<(), String> {
            Err("второй селектор не ответил".to_string())
        }

        let mut menu = Menu::with_action_runner(
            vec![Node::dynamic("network", "VPN", ProviderKey::VPN)],
            fail,
        );
        assert_eq!(menu.enter(), Outcome::Pushed);
        let (key, generation) = menu
            .take_dynamic_requests()
            .pop()
            .expect("первый snapshot запрошен");
        menu.apply_dynamic(
            key,
            generation,
            Ok(vec![Node::action(
                "mode",
                "Всё через прокси",
                Action::VpnMode(super::super::vpn::VpnMode::All),
            )]),
        );

        assert_eq!(menu.enter(), Outcome::Idle);

        assert_eq!(
            menu.take_dynamic_requests().len(),
            1,
            "перечитать частичное состояние"
        );
        assert!(menu.status().is_some_and(|status| status.is_failed()));
    }

    #[test]
    fn nudge_moves_only_the_module_order_row() {
        use std::sync::atomic::{AtomicI32, Ordering};
        static LAST_DELTA: AtomicI32 = AtomicI32::new(0);
        fn record_or_idle(action: &Action) -> Result<(), String> {
            if let Action::MoveModule { delta, .. } = action {
                LAST_DELTA.store(*delta, Ordering::SeqCst);
            }
            Ok(())
        }
        use super::super::settings::Module;

        let mut menu = Menu::with_action_runner(
            vec![
                Node::action("a", "Терминал", Action::Run("kitty", &[])),
                Node::action(
                    "b",
                    "audio",
                    Action::MoveModule {
                        module: Module::Audio,
                        delta: 1,
                    },
                ),
            ],
            record_or_idle,
        );
        // Строка без move: не двигает порядок, статус Idle.
        menu.move_sel(0);
        assert_eq!(menu.nudge_module(1), Outcome::Idle);
        assert_eq!(LAST_DELTA.load(Ordering::SeqCst), 0);
        // Строка порядка модулей: Enter-equivalent и Shift в обе стороны.
        menu.move_sel(1);
        assert_eq!(menu.nudge_module(1), Outcome::Changed);
        assert_eq!(LAST_DELTA.load(Ordering::SeqCst), 1);
        assert_eq!(menu.nudge_module(-1), Outcome::Changed);
        assert_eq!(LAST_DELTA.load(Ordering::SeqCst), -1);
    }

    #[test]
    fn back_from_the_root_clears_the_status() {
        let mut menu = menu();
        menu.set_status(Status::Applied("тема: pixel".to_string()));
        assert!(menu.status().is_some());
        assert_eq!(menu.back(), Outcome::Closed);
        assert!(menu.status().is_none());
    }

    #[test]
    fn empty_submenu_is_not_pushed() {
        let mut menu = Menu::new(vec![Node::submenu("empty", "Пусто", Vec::new())]);
        assert_eq!(
            menu.enter(),
            Outcome::Idle,
            "пустое подменю открывать некуда"
        );
        assert_eq!(menu.depth(), 0);
        assert_eq!(titles(&menu), ["Пусто"]);
    }

    #[test]
    fn validate_catches_broken_trees() {
        assert!(validate(&tree()).is_ok());
        assert!(validate(&[Node::submenu("a", "Пусто", Vec::new())]).is_err());
        assert!(validate(&[Node::action("a", "  ", Action::Live(Live::DndToggle))]).is_err());
        assert!(
            validate(&[Node::choice("a", "Тема", &[], || 0, |_| {})]).is_err(),
            "пустой список вариантов — ошибка"
        );
    }

    #[test]
    fn control_count_sees_leaves_only() {
        assert_eq!(control_count(&tree()), 4);
    }

    #[test]
    fn number_value_of_a_leaf_is_bounded() {
        let nodes = [Node::number(
            "h",
            "Высота",
            20.0,
            48.0,
            1.0,
            "px",
            || 99.0,
            |_| {},
        )];
        assert_eq!(number_value(&nodes[0]), Some(48.0));
    }

    #[test]
    fn frame_reports_the_counter_parts() {
        let mut menu = menu();
        menu.set_page(5);
        let frame = menu.frame();
        assert_eq!(frame.trail, vec!["HUD".to_string()]);
        assert_eq!(frame.total, 3);
        assert_eq!(frame.selected, 0);
        assert_eq!(frame.top, 0);
        assert_eq!(frame.query, "");
        assert_eq!(frame.status, None);
    }

    #[test]
    fn deep_nesting_keeps_every_level() {
        let mut menu = Menu::new(vec![Node::submenu(
            "a",
            "Панель",
            vec![Node::submenu(
                "b",
                "Модули",
                vec![Node::submenu(
                    "c",
                    "Глубоко",
                    vec![Node::action("d", "Конец", Action::Niri(&["msg"]))],
                )],
            )],
        )]);
        for expected in ["Панель", "Модули", "Глубоко"] {
            menu.enter();
            assert_eq!(menu.opened_submenu(), Some(expected));
        }
        assert_eq!(menu.depth(), 3);
        assert_eq!(menu.trail(), vec!["HUD", "Панель", "Модули", "Глубоко"]);
        assert_eq!(menu.enter(), Outcome::Idle, "лист не открывается");
        assert_eq!(menu.opened_submenu(), Some("Глубоко"));
    }

    fn dynamic_tree() -> Vec<Node> {
        vec![
            Node::submenu(
                "panel",
                "Панель",
                vec![Node::toggle("dnd", "Не беспокоить", || true, |_| {})],
            ),
            Node::dynamic("audio", "Звук", ProviderKey::new("test-audio")),
        ]
    }

    fn dynamic_menu() -> Menu {
        Menu::new(dynamic_tree())
    }

    fn ready_nodes() -> Vec<Node> {
        vec![
            Node::toggle("v", "Громкость", || true, |_| {}),
            Node::action("m", "Устройство", Action::Run("true", &[])),
        ]
    }

    /// Вход в динамический раздел сразу показывает загрузку и ставит один
    /// запрос в очередь worker-потока.
    #[test]
    fn dynamic_enter_shows_loading_and_queues_one_request() {
        let mut menu = dynamic_menu();
        menu.move_sel(1);
        assert_eq!(menu.enter(), Outcome::Pushed);
        assert_eq!(menu.depth(), 1);
        assert_eq!(menu.opened_submenu(), Some("Звук"));
        assert_eq!(titles(&menu), ["Загрузка…"]);
        let key = ProviderKey::new("test-audio");
        assert!(matches!(menu.dynamic_state(key), Some(SlotState::Loading)));
        let pending = menu.take_dynamic_requests();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0, key);
        assert!(menu.take_dynamic_requests().is_empty(), "очередь забрана");
    }

    /// Динамический вход рисуется той же стрелкой подменю: рендер не менялся.
    #[test]
    fn dynamic_entry_looks_like_a_submenu() {
        let menu = dynamic_menu();
        let row = menu
            .frame()
            .items
            .into_iter()
            .find(|item| item.title == "Звук")
            .expect("вход в раздел");
        assert!(row.kind.is_submenu());
        assert_eq!(row.value, "", "у входа значения нет");
    }

    /// Готовый ответ один раз перестраивает уровень; повторный вход берёт
    /// кэш и нового запроса не ставит.
    #[test]
    fn dynamic_ready_rebuilds_once_and_caches() {
        let mut menu = dynamic_menu();
        menu.move_sel(1);
        menu.enter();
        let key = ProviderKey::new("test-audio");
        let (pending_key, generation) = menu.take_dynamic_requests()[0];
        assert_eq!(pending_key, key);
        assert!(menu.apply_dynamic(key, generation, Ok(ready_nodes())));
        assert_eq!(titles(&menu), ["Громкость", "Устройство"]);
        menu.back();
        // Возврат не трогает выбор корня: курсор уже стоит на «Звуке», и
        // двигать его не надо. Со списком-кольцом лишний шаг вниз ушёл бы на
        // первый пункт и открыл бы не тот раздел.
        assert_eq!(menu.enter(), Outcome::Pushed);
        assert_eq!(titles(&menu), ["Громкость", "Устройство"]);
        assert!(
            menu.take_dynamic_requests().is_empty(),
            "кэш: нового запроса нет"
        );
    }

    /// Устаревший ответ не перетирает уровень и не трогает состояние.
    #[test]
    fn dynamic_stale_result_is_ignored() {
        let mut menu = dynamic_menu();
        menu.move_sel(1);
        menu.enter();
        let key = ProviderKey::new("test-audio");
        let (_, old) = menu.take_dynamic_requests()[0];
        let fresh = menu.refresh_dynamic(key).expect("обновление");
        assert!(!menu.apply_dynamic(key, old, Ok(ready_nodes())));
        assert_eq!(titles(&menu), ["Загрузка…"]);
        assert!(menu.apply_dynamic(key, fresh, Ok(ready_nodes())));
        assert_eq!(titles(&menu), ["Громкость", "Устройство"]);
    }

    /// Снимок раздела с личностями строк: заголовок громкости меняется от
    /// снимка к снимку, а личность остаётся — на этом и держится курсор.
    fn audio_nodes(volume: u8) -> Vec<Node> {
        vec![
            Node::info("v", &format!("Звук: {volume}% вкл")),
            Node::action(
                "−",
                "Тише на 5%",
                Action::RefreshAndRun {
                    command: super::super::system::CommandSpec::new("wpctl"),
                    refresh: ProviderKey::new("test-audio"),
                },
            )
            .with_id("audio/volume/output/-"),
            Node::submenu("s", "Устройства вывода", vec![Node::info("d", "Speaker")])
                .with_id("audio/devices/sinks"),
        ]
    }

    /// Путь и курсор переживают `retree`: снимок положения снимается до
    /// пересборки дерева и возвращается после. Заголовок строки при этом
    /// меняется — если бы курсор искался по нему, он прыгнул бы в начало.
    #[test]
    fn position_and_restore_keep_the_path_and_the_selected_row() {
        let mut menu = dynamic_menu();
        menu.move_sel(1);
        menu.enter();
        let key = ProviderKey::new("test-audio");
        let (_, generation) = menu.take_dynamic_requests()[0];
        assert!(menu.apply_dynamic(key, generation, Ok(audio_nodes(40))));
        menu.move_sel(1);
        let position = menu.position();

        // Ровно то, что делает окно после действия: дерево пересобрано,
        // снимок провайдера уже с другой громкостью.
        menu.retree(dynamic_tree());
        assert_eq!(menu.depth(), 0, "retree возвращает на корень");
        let key = ProviderKey::new("test-audio");
        let fresh = menu.refresh_dynamic(key).expect("мягкое обновление");
        assert!(
            menu.apply_dynamic(key, fresh, Ok(audio_nodes(45))),
            "свежий снимок принят"
        );
        menu.restore(&position);

        assert_eq!(menu.depth(), 1, "путь восстановлен");
        assert_eq!(menu.opened_submenu(), Some("Звук"));
        let row = menu
            .current()
            .list
            .selected_item()
            .expect("выбранная строка");
        assert_eq!(row.id, "audio/volume/output/-");
        assert_eq!(row.title, "Тише на 5%");
    }

    /// Refresh после действия не прячет готовый снимок: уровень продолжает
    /// показывать прежние строки, пока worker не ответил.
    #[test]
    fn refresh_after_an_action_keeps_the_previous_rows() {
        let mut menu = dynamic_menu();
        menu.move_sel(1);
        menu.enter();
        let key = ProviderKey::new("test-audio");
        let (_, generation) = menu.take_dynamic_requests()[0];
        assert!(menu.apply_dynamic(key, generation, Ok(audio_nodes(40))));
        let jobs = menu.take_pending_commands();
        assert!(jobs.is_empty(), "команд ещё не было");

        let fresh = menu.refresh_dynamic(key).expect("мягкое обновление");
        assert_eq!(
            titles(&menu),
            ["Звук: 40% вкл", "Тише на 5%", "Устройства вывода"]
        );
        assert!(!menu.apply_dynamic(key, generation, Ok(audio_nodes(99))));
        assert!(menu.apply_dynamic(key, fresh, Ok(audio_nodes(45))));
        assert_eq!(
            titles(&menu),
            ["Звук: 45% вкл", "Тише на 5%", "Устройства вывода"]
        );
    }

    /// Команда динамического раздела уходит в очередь worker-а, а слот
    /// раздела запрашивается отдельно — UI-поток ничего не ждёт.
    #[test]
    fn run_command_is_queued_with_its_own_generation() {
        let mut menu = dynamic_menu();
        menu.move_sel(1);
        menu.enter();
        let key = ProviderKey::new("test-audio");
        let (_, generation) = menu.take_dynamic_requests()[0];
        assert!(menu.apply_dynamic(key, generation, Ok(audio_nodes(40))));
        menu.move_sel(1);
        assert_eq!(menu.enter(), Outcome::Changed);
        let jobs = menu.take_pending_commands();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].refresh, key);
        assert!(jobs[0].generation > 0);
        assert!(menu.take_pending_commands().is_empty(), "очередь забрана");
    }

    /// Сети Wi-Fi: личность строки по SSID, поэтому пересканирование и
    /// пересборка дерева не уводят курсор на другую сеть.
    fn wifi_nodes(current: &str, other: &str) -> Vec<Node> {
        let connect = || Action::RefreshAndRun {
            command: super::super::system::CommandSpec::new("nmcli")
                .arg("connection")
                .arg("up"),
            refresh: ProviderKey::new("test-wifi"),
        };
        vec![
            Node::action("n", &format!("{other} · 78% · защищённая"), connect())
                .with_id(&format!("wifi/net/{other}")),
            Node::action("n", &format!("{current} · 70% · защищённая"), connect())
                .with_id(&format!("wifi/net/{current}")),
            Node::info("p", "Обновить"),
        ]
    }

    /// Путь и курсор переживают отметку «выполняется» и пересборку дерева:
    /// сначала нажали на сеть, потом скан показал другой порядок.
    #[test]
    fn busy_row_and_cursor_survive_a_rescan() {
        let key = ProviderKey::new("test-wifi");
        let mut menu = Menu::new(vec![Node::dynamic("w", "Wi-Fi", key)]);
        assert_eq!(menu.enter(), Outcome::Pushed);
        let (_, generation) = menu.take_dynamic_requests()[0];
        assert!(menu.apply_dynamic(key, generation, Ok(wifi_nodes("net0", "net1"))));
        menu.move_sel(1);
        let before = menu
            .current()
            .list
            .selected_item()
            .expect("строка выбрана")
            .id
            .clone();
        assert_eq!(before, "wifi/net/net0", "второй строкой идёт текущая сеть");

        // Команда поставлена в очередь: строка помечена, но ещё не ушла.
        assert_eq!(menu.enter(), Outcome::Changed);
        let jobs = menu.take_pending_commands();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].row, before);
        assert_eq!(menu.busy_rows(), vec!["wifi/net/net0".to_string()]);
        let busy_title = menu
            .current()
            .list
            .selected_item()
            .expect("строка на месте")
            .title
            .clone();
        assert!(busy_title.contains(busy_note(Language::Ru)), "{busy_title}");

        // Пересборка дерева и другой порядок сетей не должны сбить курсор.
        let position = menu.position();
        menu.retree(vec![Node::dynamic("w", "Wi-Fi", key)]);
        let fresh = menu.refresh_dynamic(key).expect("мягкое обновление");
        assert!(menu.apply_dynamic(key, fresh, Ok(wifi_nodes("net1", "net0"))));
        menu.restore(&position);
        let row = menu.current().list.selected_item().expect("строка есть");
        assert_eq!(row.id, "wifi/net/net0", "курсор остался на той же сети");
        assert_eq!(
            menu.current().list.selected(),
            0,
            "и переехал на её новое место"
        );
        assert!(
            row.title.contains(busy_note(Language::Ru)),
            "отметка держится: {}",
            row.title
        );

        let id = row.id.clone();
        menu.clear_busy_row(&id);
        assert!(menu.busy_rows().is_empty());
        let row = menu.current().list.selected_item().expect("строка есть");
        assert!(!row.title.contains(busy_note(Language::Ru)));
    }

    /// Меню на разделе Wi-Fi со строкой защищённой сети без профиля: её
    /// действие открывает ввод пароля, а не запускает nmcli.
    fn wifi_password_menu() -> Menu {
        let key = ProviderKey::new("test-wifi");
        let tree = vec![Node::dynamic("w", "Wi-Fi", key)];
        let mut menu = Menu::new(tree);
        assert_eq!(menu.enter(), Outcome::Pushed);
        let (_, generation) = menu.take_dynamic_requests()[0];
        let row_id = "wifi/net/Дом \\ принтер".to_string();
        let nodes = vec![
            Node::action(
                "n",
                "Дом \\ принтер · 61% · защищённая",
                Action::SecretInput(super::super::secret::SecretTarget::wifi(
                    "Дом \\ принтер",
                    &row_id,
                )),
            )
            .with_id(&row_id),
            Node::info("i", "Обновить"),
        ];
        assert!(menu.apply_dynamic(key, generation, Ok(nodes)));
        menu
    }

    fn type_secret(menu: &mut Menu, text: &str) {
        for ch in text.chars() {
            menu.type_secret_char(ch);
        }
    }

    /// Защищённая сеть без профиля открывает ввод, и до `Enter` в нём не
    /// выполняется ни одной команды.
    #[test]
    fn protected_network_opens_the_password_input_without_running_anything() {
        let mut menu = wifi_password_menu();
        menu.type_char('Д');
        assert_eq!(menu.query(), "Д", "запрос уровня не должен уехать в пароль");
        assert!(
            menu.take_pending_commands().is_empty(),
            "поиск не запускает команды"
        );
        menu.clear_query();
        assert_eq!(menu.enter(), Outcome::Value);
        assert!(menu.secret_active());
        assert_eq!(menu.query(), "", "ввод не трогает запрос");
        assert!(
            menu.take_pending_commands().is_empty(),
            "пароль ещё не введён"
        );
        let input = menu.secret_input().expect("ввод открыт");
        assert_eq!(input.target.subject(), "Дом \\ принтер");
        assert_eq!(input.target.row_id(), "wifi/net/Дом \\ принтер");
        assert!(input.is_empty());
    }

    /// Печатные символы, включая юникод и пробелы, идут в буфер; `Backspace`
    /// удаляет по символу, а не по байту.
    #[test]
    fn secret_input_types_unicode_and_erases_by_character() {
        let mut menu = wifi_password_menu();
        menu.enter();
        type_secret(&mut menu, "Пароль 123");
        let input = menu.secret_input().expect("ввод открыт");
        assert_eq!(input.value, "Пароль 123", "пробелы внутри сохраняются");
        assert_eq!(input.len(), 10);
        assert_eq!(input.masked(), "••••••••••");
        menu.secret_backspace();
        assert_eq!(menu.secret_input().expect("ввод").value, "Пароль 12");
        type_secret(&mut menu, "ы");
        assert_eq!(menu.secret_input().expect("ввод").value, "Пароль 12ы");
        menu.secret_backspace();
        assert_eq!(menu.secret_input().expect("ввод").value, "Пароль 12");
        assert_eq!(menu.query(), "", "запрос уровня пуст");
    }

    /// `Ctrl+U` очищает пароль, но не трогает цель и не закрывает ввод.
    #[test]
    fn ctrl_u_clears_the_password_but_keeps_the_input_open() {
        let mut menu = wifi_password_menu();
        menu.enter();
        type_secret(&mut menu, "тихийпароль");
        menu.secret_clear();
        let input = menu.secret_input().expect("ввод открыт");
        assert!(input.is_empty());
        assert_eq!(input.target.row_id(), "wifi/net/Дом \\ принтер");
        assert_eq!(input.masked(), "");
    }

    /// Подтверждение собирает команду с секретным аргументом, кладёт её в
    /// очередь worker-а, помечает строку занятой и выбрасывает буфер.
    #[test]
    fn submit_queues_a_secret_command_and_clears_the_buffer() {
        let mut menu = wifi_password_menu();
        menu.enter();
        type_secret(&mut menu, "тихийпароль");
        assert_eq!(menu.submit_secret_input(), Outcome::Changed);
        assert!(!menu.secret_active(), "ввод закрыт");
        assert!(
            menu.secret_input().is_none(),
            "буфер не должен переживать отправку"
        );
        assert_eq!(menu.query(), "", "запрос не задет");
        let jobs = menu.take_pending_commands();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].row, "wifi/net/Дом \\ принтер");
        // Цель знает свой раздел: это настоящий WIFI, а не ключ тестового
        // дерева, по которому построены строки.
        assert_eq!(jobs[0].refresh, ProviderKey::WIFI);
        let line = jobs[0].commands[0].log_line();
        assert!(line.contains("nmcli device wifi connect"), "{line}");
        assert!(
            !line.contains("тихийпароль"),
            "пароль ушёл в очередь: {line}"
        );
        assert!(jobs[0].commands[0].has_secrets());
        assert_eq!(
            menu.busy_rows(),
            vec!["wifi/net/Дом \\ принтер".to_string()],
            "строка помечена занятой"
        );
        let status = menu.status().expect("статус есть").clone();
        assert!(!status.text().contains("тихийпароль"), "{status:?}");
        assert!(
            menu.current()
                .list
                .selected_item()
                .expect("строка")
                .title
                .contains(busy_note(Language::Ru))
        );
    }

    /// Отмена не запускает ничего, пароль пропадает, уровень и выбор на месте.
    #[test]
    fn escape_cancels_the_input_and_keeps_the_row_and_the_query() {
        let mut menu = wifi_password_menu();
        menu.type_char('h');
        menu.enter();
        type_secret(&mut menu, "не_сохраняй");
        let position = menu.position();
        assert_eq!(menu.cancel_secret_input(), Outcome::Value);
        assert!(!menu.secret_active());
        assert!(
            menu.take_pending_commands().is_empty(),
            "отмена без команды"
        );
        assert_eq!(menu.query(), "h", "запрос уровня не тронут");
        assert!(menu.busy_rows().is_empty());
        let current = menu.current().list.selected_item().expect("строка");
        assert_eq!(current.id, "wifi/net/Дом \\ принтер");
        assert_eq!(menu.position(), position, "положение меню не изменилось");
        // Повторная отмена без ввода — no-op, а не паника.
        assert_eq!(menu.cancel_secret_input(), Outcome::Idle);
    }

    /// Кадр в режиме ввода содержит только маску: пароля в нём нет ни в
    /// заголовке, ни в запросе, ни в статусе.
    #[test]
    fn frame_carries_only_the_mask() {
        let mut menu = wifi_password_menu();
        menu.enter();
        type_secret(&mut menu, "p@ss:word");
        let frame = menu.frame();
        assert_eq!(frame.query, "", "запрос пуст");
        let secret = frame.secret.clone().expect("ввод виден в кадре");
        assert_eq!(secret.subject, "Дом \\ принтер");
        assert_eq!(secret.masked, "•••••••••");
        assert_eq!(secret.len, 9);
        let everything = format!("{frame:?}");
        for text in [
            frame.query.as_str(),
            secret.masked.as_str(),
            secret.subject.as_str(),
        ] {
            assert!(!text.contains("p@ss"), "{text}");
        }
        assert!(!everything.contains("p@ss"), "пароль не попал в кадр");
        menu.submit_secret_input();
        let after = menu.frame();
        assert!(after.secret.is_none(), "после отправки ввода в кадре нет");
        assert!(
            !format!("{after:?}").contains("p@ss"),
            "и в кадре после отправки пароля нет"
        );
    }

    /// Пустой пароль отправляется как есть: правил паролей тут не изобретаем.
    #[test]
    fn empty_password_is_sent_to_nmcli_unchanged() {
        let mut menu = wifi_password_menu();
        menu.enter();
        assert_eq!(menu.submit_secret_input(), Outcome::Changed);
        let jobs = menu.take_pending_commands();
        assert_eq!(jobs.len(), 1);
        let (_, args) = jobs[0].commands[0].argv();
        assert_eq!(args.last().map(String::as_str), Some(""));
        assert!(jobs[0].commands[0].has_secrets());
    }

    /// Строка сети остаётся выбранной и после отправки команды, и после
    /// прихода свежего снимка: курсор идёт по личности, а не по позиции.
    #[test]
    fn cursor_and_path_survive_the_password_flow() {
        let mut menu = wifi_password_menu();
        menu.enter();
        type_secret(&mut menu, "pw");
        let position = menu.position();
        menu.submit_secret_input();
        let key = ProviderKey::new("test-wifi");
        let fresh = menu.refresh_dynamic(key).expect("мягкое обновление");
        assert!(menu.apply_dynamic(key, fresh, Ok(wifi_nodes("net0", "net1"))));
        menu.restore(&position);
        assert_eq!(menu.depth(), 1, "мы на уровне Wi-Fi");
        assert_eq!(menu.opened_submenu(), Some("Wi-Fi"));
        assert_eq!(
            menu.current().list.selected().min(1),
            0,
            "выбранная строка восстановлена"
        );
    }

    /// Async-цепочка подключения с паролем: Enter ничего не запускает, в
    /// очередь попадает одна команда, устаревший ответ worker-а не
    /// перетирает свежий, а отметка «выполняется» снимается по `row`.
    #[test]
    fn password_connect_runs_through_the_worker_queue() {
        let mut menu = wifi_password_menu();
        menu.enter();
        type_secret(&mut menu, "pw");
        assert_eq!(menu.submit_secret_input(), Outcome::Changed);
        let jobs = menu.take_pending_commands();
        assert_eq!(jobs.len(), 1, "Enter не блокирует и не дублирует команду");
        let row = jobs[0].row.clone();

        let key = ProviderKey::new("test-wifi");
        let first = menu.refresh_dynamic(key).expect("обновление после команды");
        let stale = first - 1;
        assert!(
            !menu.apply_dynamic(key, stale, Ok(Vec::new())),
            "устаревший ответ отброшен"
        );
        let second = menu.refresh_dynamic(key).expect("второе обновление");
        assert!(menu.apply_dynamic(key, second, Ok(wifi_nodes("net0", "net1"))));

        // Ответ пришёл: строка перестаёт быть занятой.
        menu.clear_busy_row(&row);
        assert!(menu.busy_rows().is_empty());
        assert_eq!(menu.depth(), 1, "меню осталось на уровне Wi-Fi");
    }

    /// Даже если действие кто-то попробует выполнить напрямую, пароля у
    /// исполнителя нет: команда составляется только из буфера ввода.
    #[test]
    fn secret_input_action_is_a_no_op_for_the_runner() {
        use super::super::action::RecordingRunner;
        let target = super::super::secret::SecretTarget::wifi("Дом", "wifi/net/Дом");
        let action = Action::SecretInput(target);
        let runner = RecordingRunner::new();
        action.perform_with(&runner).expect("выполнение не падает");
        assert!(
            runner.calls().is_empty(),
            "исполнитель не запускает nmcli: {:?}",
            runner.calls()
        );
        assert!(!action.closes_menu(), "меню остаётся открытым");
        assert!(action.command().is_none());
        assert_eq!(action.describe(), "Дом");
    }

    /// Глобальный поиск находит «Wi-Fi» и по `Enter` открывает раздел, а не
    /// ввод пароля: пароль начинается только со строки сети.
    #[test]
    fn search_opens_the_wifi_section_without_entering_a_password() {
        let key = ProviderKey::new("test-wifi");
        let tree = vec![
            Node::submenu("s", "Система", vec![Node::dynamic("w", "Wi-Fi", key)]),
            Node::submenu(
                "z",
                "Захват",
                vec![Node::toggle("c", "Микрофон", || true, |_| {})],
            ),
        ];
        let mut menu = Menu::new(tree);
        for ch in "Wi-Fi".chars() {
            menu.type_char(ch);
        }
        let rows = menu.frame().items;
        let hit = rows
            .iter()
            .find(|item| item.caption.is_some())
            .expect("найден раздел с путём");
        assert!(hit.title.contains("Wi-Fi"));
        assert_eq!(menu.enter(), Outcome::Pushed);
        assert!(!menu.secret_active(), "поиск в пароль не превращается");
        assert_eq!(menu.depth(), 1);
        assert_eq!(menu.opened_submenu(), Some("Система"));
    }

    /// Список Bluetooth-устройств как строки уровня: identity по MAC, действий
    /// нет — только подменю и пункт обновления.
    fn bluetooth_nodes(order: &[&str]) -> Vec<Node> {
        let mut rows = vec![Node::info("b", "Bluetooth: вкл")];
        rows.push(Node::submenu(
            "p",
            "Сопряжённые",
            order
                .iter()
                .map(|mac| {
                    Node::info("d", &format!("Устройство {mac}"))
                        .with_id(&format!("bluetooth/device/{mac}"))
                })
                .collect(),
        ));
        rows.push(Node::submenu(
            "f",
            "Найденные",
            vec![Node::info("d", "Нет устройств")],
        ));
        rows.push(Node::action(
            "u",
            "Обновить",
            Action::RefreshDynamic(ProviderKey::BLUETOOTH),
        ));
        rows
    }

    /// Курсор в разделе Bluetooth переживает refresh: устройство выбрано по
    /// MAC, а не по позиции, поэтому перестановка списка его не сдвигает.
    #[test]
    fn bluetooth_cursor_follows_the_mac_across_a_refresh() {
        let key = ProviderKey::BLUETOOTH;
        let mut menu = Menu::new(vec![Node::dynamic("b", "Bluetooth", key)]);
        assert_eq!(menu.enter(), Outcome::Pushed);
        let (_, generation) = menu.take_dynamic_requests()[0];
        assert!(menu.apply_dynamic(
            key,
            generation,
            Ok(bluetooth_nodes(&[
                "AA:BB:CC:DD:EE:01",
                "AA:BB:CC:DD:EE:02",
                "AA:BB:CC:DD:EE:03",
            ]))
        ));
        // Открываем «Сопряжённые»: вторая строка раздела.
        menu.move_sel(1);
        assert_eq!(menu.enter(), Outcome::Pushed);
        menu.move_sel(1);
        let before = menu
            .current()
            .list
            .selected_item()
            .expect("строка")
            .id
            .clone();
        assert_eq!(before, "bluetooth/device/AA:BB:CC:DD:EE:02");

        let position = menu.position();
        let fresh = menu.refresh_dynamic(key).expect("обновление");
        assert!(menu.apply_dynamic(
            key,
            fresh,
            Ok(bluetooth_nodes(&[
                "AA:BB:CC:DD:EE:02",
                "AA:BB:CC:DD:EE:01",
                "AA:BB:CC:DD:EE:03",
            ]))
        ));
        menu.restore(&position);
        assert_eq!(menu.depth(), 2, "остались на уровне устройств");
        assert_eq!(
            menu.current().list.selected_item().expect("строка").id,
            "bluetooth/device/AA:BB:CC:DD:EE:02",
            "курсор остался на том же устройстве"
        );
        assert_eq!(
            menu.current().list.selected(),
            0,
            "и переехал на новое место в списке"
        );
    }

    /// Устаревший ответ провайдера Bluetooth не перетирает свежий снимок.
    #[test]
    fn stale_bluetooth_result_is_ignored() {
        let key = ProviderKey::BLUETOOTH;
        let mut menu = Menu::new(vec![Node::dynamic("b", "Bluetooth", key)]);
        menu.enter();
        let (_, generation) = menu.take_dynamic_requests()[0];
        let fresh = menu.refresh_dynamic(key).expect("обновление");
        assert!(!menu.apply_dynamic(key, generation, Ok(bluetooth_nodes(&["AA:BB:CC:DD:EE:09"]))));
        assert!(menu.apply_dynamic(key, fresh, Ok(bluetooth_nodes(&["AA:BB:CC:DD:EE:01"]))));
        let paired = menu
            .current()
            .list
            .items()
            .iter()
            .find(|item| item.title == "Сопряжённые")
            .expect("список есть");
        assert_eq!(paired.kind, ItemKind::Submenu);
    }

    /// Ошибка даёт строку сообщения и пункт обновления; Enter по нему
    /// запускает новое поколение.
    #[test]
    fn dynamic_error_shows_message_and_refresh_row() {
        let mut menu = dynamic_menu();
        menu.move_sel(1);
        menu.enter();
        let key = ProviderKey::new("test-audio");
        let (_, generation) = menu.take_dynamic_requests()[0];
        assert!(menu.apply_dynamic(key, generation, Err("демон молчит".to_string())));
        assert_eq!(titles(&menu), ["демон молчит", "Обновить"]);
        menu.move_sel(1);
        assert_eq!(menu.enter(), Outcome::Changed);
        let pending = menu.take_dynamic_requests();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1, generation + 1);
        assert_eq!(titles(&menu), ["Загрузка…"]);
    }

    /// Пустой готовый список — одна строка, а не пустой уровень.
    #[test]
    fn dynamic_empty_ready_is_one_row() {
        let mut menu = dynamic_menu();
        menu.move_sel(1);
        menu.enter();
        let key = ProviderKey::new("test-audio");
        let (_, generation) = menu.take_dynamic_requests()[0];
        assert!(menu.apply_dynamic(key, generation, Ok(Vec::new())));
        assert_eq!(menu.frame().items.len(), 1);
    }

    /// Недоступный раздел скрывается обычным `visible`, как отсутствующие
    /// программы у статических пунктов.
    #[test]
    fn dynamic_section_hides_through_visible() {
        let menu = Menu::new(vec![
            Node::dynamic("a", "Звук", ProviderKey::AUDIO).when(|| false),
            Node::toggle("dnd", "Не беспокоить", || true, |_| {}),
        ]);
        assert_eq!(titles(&menu), ["Не беспокоить"]);
    }

    /// Динамические состояния рисуются существующим рендером карточки.
    #[test]
    fn dynamic_states_render_with_the_plain_card_renderer() {
        for result in [
            None,
            Some(Ok(ready_nodes())),
            Some(Ok(Vec::new())),
            Some(Err("демон молчит".to_string())),
        ] {
            let mut menu = dynamic_menu();
            menu.move_sel(1);
            menu.enter();
            if let Some(result) = result {
                let key = ProviderKey::new("test-audio");
                let (_, generation) = menu.take_dynamic_requests()[0];
                assert!(menu.apply_dynamic(key, generation, result));
            }
            menu.set_page(10);
            let pixmap = render_menu_card(&menu, false);
            assert!(pixmap.width() > 0 && pixmap.height() > 0);
        }
    }

    fn nested_nodes(state: &str) -> Vec<Node> {
        vec![Node::submenu(
            "p",
            "Устройства",
            vec![Node::info("d", state).with_id("dev/1")],
        )]
    }

    /// Путь окна — apply_dynamic без position/restore, пока открыт вложенный уровень.
    #[test]
    fn nested_level_follows_a_fresh_snapshot() {
        let key = ProviderKey::BLUETOOTH;
        let mut menu = Menu::new(vec![Node::dynamic("b", "Bluetooth", key)]);
        menu.enter();
        let (_, g) = menu.take_dynamic_requests()[0];
        assert!(menu.apply_dynamic(key, g, Ok(nested_nodes("не подключено"))));
        assert_eq!(menu.enter(), Outcome::Pushed); // вошли в «Устройства»
        assert_eq!(menu.depth(), 2);
        let fresh = menu.refresh_dynamic(key).expect("refresh");
        assert!(menu.apply_dynamic(key, fresh, Ok(nested_nodes("подключено"))));
        let shown = menu.current().list.items()[0].title.clone();
        assert_eq!(shown, "подключено", "вложенный уровень не обновился");
    }

    #[test]
    fn vanished_nested_node_returns_to_the_live_level() {
        let key = ProviderKey::BLUETOOTH;
        let mut menu = Menu::new(vec![Node::dynamic("b", "Bluetooth", key)]);
        menu.enter();
        let (_, g) = menu.take_dynamic_requests()[0];
        menu.apply_dynamic(key, g, Ok(nested_nodes("не подключено")));
        menu.enter();
        let fresh = menu.refresh_dynamic(key).expect("refresh");
        menu.apply_dynamic(key, fresh, Ok(vec![Node::info("x", "СОВСЕМ НОВЫЙ СПИСОК")]));
        assert_eq!(
            menu.depth(),
            1,
            "исчезнувший узел возвращает на живой уровень"
        );
        let shown = menu.current().list.items()[0].title.clone();
        assert_eq!(shown, "СОВСЕМ НОВЫЙ СПИСОК", "родитель не обновился");
    }

    #[test]
    fn busy_marker_survives_a_plain_snapshot() {
        let key = ProviderKey::new("test-wifi");
        let mut menu = Menu::new(vec![Node::dynamic("w", "Wi-Fi", key)]);
        menu.enter();
        let (_, g) = menu.take_dynamic_requests()[0];
        menu.apply_dynamic(key, g, Ok(wifi_nodes("net0", "net1")));
        menu.move_sel(1);
        assert_eq!(menu.enter(), Outcome::Changed); // net0 busy
        let _ = menu.take_pending_commands();
        // приходит чужой снимок (например, ручной rescan), без restore — как в окне
        let fresh = menu.refresh_dynamic(key).expect("refresh");
        assert!(menu.apply_dynamic(key, fresh, Ok(wifi_nodes("net0", "net1"))));
        let t = menu
            .current()
            .list
            .items()
            .iter()
            .find(|i| i.id == "wifi/net/net0")
            .unwrap()
            .title
            .clone();
        assert!(t.contains(busy_note(Language::Ru)), "метка потеряна: {t}");
    }
}

//! Раскладка окна настроек HUD: модули бара, строки, попадания и переход фокуса.
//! Только арифметика, без cosmic-text и Wayland — как `binds_layout`.

use super::config::Config;
pub use super::settings::{Module, NotificationPosition, Zone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Overview,
    Panel,
    Appearance,
    Notifications,
    Controls,
    Wallpaper,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::Overview,
        Section::Panel,
        Section::Appearance,
        Section::Notifications,
        Section::Controls,
        Section::Wallpaper,
    ];

    /// Подпись раздела в сайдбаре.
    pub fn label(self) -> &'static str {
        match self {
            Section::Overview => "Обзор",
            Section::Panel => "Панель",
            Section::Appearance => "Внешний вид",
            Section::Notifications => "Уведомления",
            Section::Controls => "Управление",
            Section::Wallpaper => "Обои",
        }
    }

    /// Заголовок раздела в правой колонке.
    pub fn title(self) -> &'static str {
        self.label()
    }

    pub fn index(self) -> usize {
        Section::ALL
            .iter()
            .position(|item| *item == self)
            .unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Section {
        Section::ALL[index.min(Section::ALL.len() - 1)]
    }

    /// Готов ли раздел: у незаконченных показывается заглушка.
    pub fn is_ready(self) -> bool {
        !matches!(self, Section::Wallpaper)
    }

    /// Раздел, с которого окно открывается. `HUD_SETTINGS_SECTION=panel`
    /// нужно для снимков и проверок: без ручной раскладки по клавишам.
    pub fn from_env() -> Section {
        match std::env::var("HUD_SETTINGS_SECTION").as_deref() {
            Ok("panel") => Section::Panel,
            Ok("appearance") => Section::Appearance,
            Ok("notifications") => Section::Notifications,
            Ok("controls") => Section::Controls,
            Ok("wallpaper") => Section::Wallpaper,
            _ => Section::Overview,
        }
    }

    /// Подпись заглушки для ещё не сделанных разделов.
    pub fn stub_note(self) -> &'static str {
        match self {
            Section::Wallpaper => "Выбор обоев и схемы matugen",
            _ => "",
        }
    }
}

/// Ускоренный повтор клавиши в сайдбаре: после задержки первый шаг, дальше
/// интервал сокращается до минимального. Свой таймер вместо автоповтора
/// композитора: SCTK без фичи `calloop` повторы не генерирует, а нам нужен ещё
/// и разгон.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Repeat {
    /// Задержка перед первым повтором, мс.
    pub delay_ms: u32,
    /// Интервал первого повтора, мс.
    pub interval_ms: u32,
    /// Нижняя граница интервала, мс.
    pub min_interval_ms: u32,
    /// Множитель интервала на каждом шаге.
    pub factor: f32,
}

impl Repeat {
    pub const fn new(delay_ms: u32, interval_ms: u32, min_interval_ms: u32, factor: f32) -> Self {
        Self {
            delay_ms,
            interval_ms,
            min_interval_ms,
            factor,
        }
    }

    /// Рабочий вариант для листания разделов.
    pub const fn sections() -> Self {
        Self::new(320, 150, 45, 0.82)
    }

    /// Следующий интервал: с каждым шагом он короче, но не короче минимума.
    pub fn next_interval(&mut self) -> u32 {
        let current = self.interval_ms;
        self.interval_ms =
            ((self.interval_ms as f32 * self.factor) as u32).max(self.min_interval_ms);
        current
    }
}

/// Зона фокуса: сайдбар или содержимое. Без явной зоны геометрический поиск
/// соседа не мог надёжно увести фокус в левую колонку.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// Пункт сайдбара по индексу в `Section::ALL`.
    Nav(usize),
    /// Контрол правой колонки.
    Content(Control),
}

/// Состояние навигации: активный раздел и где сейчас фокус. Чистые функции —
/// их можно прогнать тестами без Wayland.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nav {
    pub section: Section,
    pub focus: Focus,
}

impl Nav {
    pub fn new(section: Section) -> Self {
        Self {
            section,
            focus: Focus::Nav(section.index()),
        }
    }

    pub fn focus_nav(&self) -> bool {
        matches!(self.focus, Focus::Nav(_))
    }

    /// Первый контрол содержимого раздела — вход в правую колонку.
    pub fn first_content(&self, rows: &[Row]) -> Option<Focus> {
        content_controls(rows)
            .first()
            .map(|control| Focus::Content(*control))
    }

    /// Перевод фокуса. `←` из самой левой колонки уходит в сайдбар, `→` из
    /// сайдбара возвращает в содержимое, `↑↓` в сайдбаре идут по пунктам.
    pub fn move_focus(&mut self, rows: &[Row], dx: i32, dy: i32) -> bool {
        let before = (self.focus, self.section);
        match self.focus {
            Focus::Nav(index) => {
                if dy != 0 {
                    // В сайдбаре стрелки сразу открывают соседний раздел: при
                    // удержании клавиши разделы листаются подряд, без Enter.
                    // У края список перескакивает на противоположный конец, а не
                    // упирается.
                    let count = Section::ALL.len() as i32;
                    let next = (index as i32 + dy).rem_euclid(count) as usize;
                    self.focus = Focus::Nav(next);
                    self.section = Section::from_index(next);
                } else if dx > 0
                    && let Some(content) = self.first_content(rows)
                {
                    self.focus = content;
                }
            }
            Focus::Content(control) => {
                if dx < 0 && is_leftmost(rows, control) {
                    self.focus = Focus::Nav(self.section.index());
                } else if let Some(next) = neighbour(rows, control, dx, dy) {
                    self.focus = Focus::Content(next);
                }
            }
        }
        (self.focus, self.section) != before
    }

    /// Переключение зоны фокуса клавишей Tab.
    pub fn cycle_zone(&mut self, rows: &[Row], backwards: bool) -> bool {
        let before = self.focus;
        match self.focus {
            Focus::Nav(_) => {
                if let Some(content) = self.first_content(rows) {
                    self.focus = content;
                }
            }
            Focus::Content(_) => {
                self.focus = Focus::Nav(self.section.index());
            }
        }
        let _ = backwards;
        self.focus != before
    }

    /// Enter или Space: в сайдбаре открывает раздел, в содержимом возвращает
    /// контрол, который нужно применить.
    pub fn activate(&mut self) -> Option<Control> {
        match self.focus {
            Focus::Nav(index) => {
                self.section = Section::from_index(index);
                None
            }
            Focus::Content(control) => Some(control),
        }
    }
}

/// Контролы содержимого раздела. Пункты сайдбара и кнопка закрытия — обвязка:
/// они есть на любом разделе и в содержимое не входят.
pub fn content_controls(rows: &[Row]) -> Vec<Control> {
    rows.iter()
        .filter_map(Row::control)
        .filter(|control| !matches!(control, Control::Nav(_) | Control::Close))
        .collect()
}

/// Самый левый контрол колонки: из него `←` уводит в сайдбар.
fn is_leftmost(rows: &[Row], control: Control) -> bool {
    let Some(rect) = rows
        .iter()
        .find(|row| row.control() == Some(control))
        .and_then(Row::rect)
    else {
        return false;
    };
    content_controls(rows)
        .iter()
        .filter_map(|other| {
            rows.iter()
                .find(|row| row.control() == Some(*other))
                .and_then(Row::rect)
        })
        .all(|other| other.x >= rect.x - 1.0)
}

/// Подписи модулей для окна. Сами модули живут в `settings::Module`, чтобы
/// не было двух перечислений, которые могут разойтись.
impl Module {
    pub fn label(self) -> &'static str {
        match self {
            Module::Tray => "Трей",
            Module::Weather => "Погода",
            Module::Webcam => "Вебка",
            Module::Clock => "Часы",
            Module::Recorder => "Запись",
            Module::Battery => "Батарея",
            Module::System => "Система",
            Module::Audio => "Звук",
            Module::Network => "Сеть",
            Module::Dnd => "Не беспокоить",
        }
    }

    /// Подпись зоны, в которой модуль стоит на панели.
    pub fn zone_label(self) -> &'static str {
        match self.zone() {
            Zone::Left => "Слева",
            Zone::Center => "По центру",
            Zone::Right => "Справа",
        }
    }

    /// Можно ли двигать модуль внутри зоны. У `tray` переключатель один,
    /// переставлять нечего.
    pub fn movable(self) -> bool {
        self.zone() != Zone::Left
    }
}

pub const HEIGHT_MIN: u32 = 24;
pub const HEIGHT_MAX: u32 = 48;

/// Пределы и шаг высоты: `hud-setting` и панель читают те же границы.
pub fn clamp_height(value: i64) -> u32 {
    value.clamp(HEIGHT_MIN as i64, HEIGHT_MAX as i64) as u32
}

/// Новая высота или `None`, если шаг ничего не меняет.
pub fn step_height(current: u32, delta: i32) -> Option<u32> {
    let next = clamp_height(current as i64 + delta as i64);
    (next != current).then_some(next)
}

/// Подпись и шрифт карточки оформления.
pub fn theme_card(pixel: bool) -> (&'static str, &'static str, &'static str) {
    if pixel {
        ("Pixel", "Minecraft Rus и чёткие границы", "Minecraft Rus")
    } else {
        (
            "Обычный",
            "Плавный системный стиль",
            "JetBrainsMono Nerd Font",
        )
    }
}

/// Интерактивный элемент окна.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Nav(Section),
    /// Карточка оформления: `true` — pixel.
    Theme(bool),
    Toggle(Module),
    /// Переключатель видимости модуля в разделе «Панель».
    Switch(Module),
    /// Кнопка ▲▼: перестановка модуля внутри своей зоны.
    Move(Module, i32),
    /// Кнопка высоты бара: −1 или +1.
    Height(i32),
    HeightSlider,
    NotificationFont(i32),
    NotificationLineHeight(i32),
    NotificationPosition(NotificationPosition),
    /// Переход в другой раздел из «Обзора».
    Goto(Section),
    Language,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }
}

/// Подсказки в подвале окна: показываются все, раскладываются равномерно.
pub const HINTS: [&str; 5] = [
    "↑↓ раздел",
    "← сайдбар",
    "Space применить",
    "L язык",
    "Esc закрыть",
];

/// Строка окна: либо элемент, либо заголовок/разделитель/подпись.
/// Позиции живут здесь же, чтобы рисование и hit-test не разъезжались.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Nav {
        section: Section,
        rect: Rect,
    },
    /// Заглушка ещё не сделанного раздела: название и пояснение.
    Stub {
        title: &'static str,
        note: &'static str,
        y: f32,
    },
    Theme {
        pixel: bool,
        rect: Rect,
    },
    Toggle {
        module: Module,
        rect: Rect,
    },
    /// Подпись зоны на панели: название и пояснение, кликами не перехватывается.
    ZoneLabel {
        zone: Zone,
        title: &'static str,
        hint: &'static str,
        rect: Rect,
    },
    /// Строка модуля в разделе «Панель»: переключатель и кнопки ▲▼.
    ModuleRow {
        module: Module,
        line: usize,
        rect: Rect,
        switch: Rect,
    },
    /// Кнопка перестановки модуля внутри зоны: `dir` — −1 вверх, +1 вниз.
    Move {
        module: Module,
        line: usize,
        dir: i32,
        rect: Rect,
    },
    /// Подпись «Высота панели» и её шкала: не кликаются, кликают кнопки.
    HeightLabel {
        y: f32,
        scale: Rect,
    },
    Height {
        dir: i32,
        rect: Rect,
    },
    NotificationFont {
        dir: i32,
        rect: Rect,
    },
    NotificationLineHeight {
        dir: i32,
        rect: Rect,
    },
    NotificationPosition {
        position: NotificationPosition,
        rect: Rect,
    },
    /// Строка сводки в «Обзоре»: подпись слева, значение справа, кликается как
    /// ссылка в соответствующий раздел.
    Summary {
        label: &'static str,
        value: String,
        section: Section,
        rect: Rect,
    },
    /// Ссылка на раздел из «Обзора» или из «Управления».
    Goto {
        section: Section,
        rect: Rect,
    },
    /// Горячая клавиша из niri: только чтение, править здесь ничего нельзя.
    Hotkey {
        keys: String,
        desc: String,
        rect: Rect,
    },
    Close {
        rect: Rect,
    },
    Header {
        text: &'static str,
        y: f32,
    },
    Rule {
        y: f32,
    },
    Status {
        y: f32,
    },
    Footer {
        y: f32,
    },
    Language {
        rect: Rect,
    },
}

impl Row {
    pub fn rect(&self) -> Option<Rect> {
        match self {
            Row::Theme { rect, .. }
            | Row::Nav { rect, .. }
            | Row::Toggle { rect, .. }
            | Row::Height { rect, .. }
            | Row::HeightLabel { scale: rect, .. }
            | Row::NotificationFont { rect, .. }
            | Row::NotificationLineHeight { rect, .. }
            | Row::NotificationPosition { rect, .. }
            | Row::Summary { rect, .. }
            | Row::Goto { rect, .. }
            | Row::Hotkey { rect, .. }
            | Row::Language { rect }
            | Row::Close { rect } => Some(*rect),
            Row::ModuleRow { switch, .. } => Some(*switch),
            Row::Move { rect, .. } => Some(*rect),
            _ => None,
        }
    }

    pub fn control(&self) -> Option<Control> {
        match self {
            Row::Nav { section, .. } => Some(Control::Nav(*section)),
            Row::Theme { pixel, .. } => Some(Control::Theme(*pixel)),
            Row::Toggle { module, .. } => Some(Control::Toggle(*module)),
            Row::ModuleRow { module, .. } => Some(Control::Switch(*module)),
            Row::Move { module, dir, .. } => Some(Control::Move(*module, *dir)),
            Row::Height { dir, .. } => Some(Control::Height(*dir)),
            Row::HeightLabel { .. } => Some(Control::HeightSlider),
            Row::NotificationFont { dir, .. } => Some(Control::NotificationFont(*dir)),
            Row::NotificationLineHeight { dir, .. } => Some(Control::NotificationLineHeight(*dir)),
            Row::NotificationPosition { position, .. } => {
                Some(Control::NotificationPosition(*position))
            }
            Row::Summary { section, .. } | Row::Goto { section, .. } => {
                Some(Control::Goto(*section))
            }
            Row::Language { .. } => Some(Control::Language),
            Row::Close { .. } => Some(Control::Close),
            Row::Hotkey { .. } => None,
            _ => None,
        }
    }
}

// Геометрия окна — единственный источник истины. Рисование, hit-test и
// отладочная отрисовка берут числа отсюда, поэтому подпись и её контрол всегда
// лежат в одной полосе и не могут разъехаться.
//
// У каждой секции своя полоса (band) и явные вертикальные отступы: заголовок
// секции, её содержимое, разделитель и следующая секция не слипаются.
pub const WIDTH: f32 = 1100.0;
pub const HEIGHT: f32 = 740.0;
/// Отступ контента от левого края; заканчивается за rail навигации.
pub const PAD_X: f32 = 280.0;
/// Отступ контента справа.
pub const PAD_R: f32 = 24.0;
/// Ширина одной колонки тумблеров и карточек оформления.
pub const COL_W: f32 = 390.0;
const COL_GAP: f32 = 16.0;
/// Высота строки модуля и кнопки: одинаковая для текста и контролов.
pub const ROW_H: f32 = 32.0;
const ITEM_GAP: f32 = 8.0;
const STEP_W: f32 = 40.0;

/// Высота полосы заголовка секции.
pub const HEADER_H: f32 = 22.0;

// Шапка окна и верхние кнопки.
const TOP_BTN_W: f32 = 86.0;
const TOP_BTN_H: f32 = 32.0;
const TOP_BTN_Y: f32 = 24.0;
/// Отступ заголовка окна от левого края.
pub const TITLE_X: f32 = 36.0;

// Боковая навигация.
const RAIL_X: f32 = 18.0;
const RAIL_Y: f32 = 92.0;
const RAIL_W: f32 = 236.0;
const NAV_TOP: f32 = 140.0;
// Шаг и высота подобраны так, чтобы шесть разделов поместились над
// разделителем: последний пункт кончается на 420 px.
const NAV_STEP: f32 = 48.0;
const NAV_ITEM_H: f32 = 40.0;
pub const RAIL_RULE_Y: f32 = 436.0;
pub const RAIL_STATUS_Y: f32 = 464.0;
const RAIL_STATUS_STEP: f32 = 30.0;

// Правая колонка. Каждая цифра — либо верх полосы, либо её высота; между
// секциями явные зазоры.
const HEADER_Y: f32 = 100.0;
const CARD_Y: f32 = 132.0;
const CARD_H: f32 = 116.0;
/// Полоса заголовка карточки: чекбокс, имя темы и метка «применено» — одна строка.
const CARD_TITLE_Y: f32 = 150.0;
const CARD_TITLE_H: f32 = 26.0;
const CARD_DESC_Y: f32 = 184.0;
const CARD_DESC_H: f32 = 20.0;
const CARD_FONT_Y: f32 = 210.0;
const CARD_FONT_H: f32 = 20.0;
const CARD_PAD: f32 = 16.0;

// Раздел «Обзор»: сводка в две колонки и ссылки на разделы.
const SUMMARY_HEADER_Y: f32 = 132.0;
const SUMMARY_Y: f32 = 156.0;
const SECTIONS_HEADER_Y: f32 = 316.0;
const SECTIONS_Y: f32 = 340.0;

// Раздел «Панель»: три зоны в ряд, у каждой подпись и своя колонка строк.
const PANEL_HEADER_Y: f32 = 132.0;
/// Верх полосы подписи зоны.
const ZONE_LABEL_Y: f32 = 160.0;
const ZONE_LABEL_H: f32 = 22.0;
/// Верх первой строки модуля.
const ZONE_ROWS_Y: f32 = 188.0;
/// Ширина колонки зоны: три зоны в ряд с промежутками.
const ZONE_W: f32 = 336.0;
const ZONE_GAP: f32 = 16.0;
/// Ширина кнопок ▲▼ у строки модуля.
const MOVE_BTN_W: f32 = 34.0;
const MOVE_BTN_H: f32 = 32.0;
const RULE_HEIGHT_Y: f32 = 440.0;
const HEIGHT_HEADER_Y: f32 = 470.0;
const HEIGHT_ROW_Y: f32 = 490.0;
const HEIGHT_ROW_H: f32 = 40.0;

// Раздел «Внешний вид»: две карточки тем и переключатель языка.
const LANGUAGE_RULE_Y: f32 = 276.0;
const LANGUAGE_HEADER_Y: f32 = 306.0;
const LANGUAGE_Y: f32 = 328.0;

// Раздел «Уведомления».
const NOTIFY_HEADER_Y: f32 = 132.0;
const NOTIFY_FONT_RULE_Y: f32 = 156.0;
const NOTIFY_FONT_Y: f32 = 180.0;
const NOTIFY_LINE_RULE_Y: f32 = 244.0;
const NOTIFY_LINE_Y: f32 = 268.0;
const NOTIFY_POSITION_RULE_Y: f32 = 332.0;
const NOTIFY_POSITION_HEADER_Y: f32 = 362.0;
const NOTIFY_POSITION_Y: f32 = 386.0;

// Раздел «Управление»: список горячих клавиш из niri, только чтение.
// Биндов больше сотни, поэтому список идёт в две колонки и листается.
const HOTKEY_HEADER_Y: f32 = 132.0;
const HOTKEY_Y: f32 = 156.0;
const HOTKEY_ROW_STEP: f32 = ROW_H + 4.0;
/// Сколько строк помещается в колонку до строки статуса.
const HOTKEY_ROWS: usize = 13;
/// Сколько горячих клавиш видно на странице: две колонки.
pub const HOTKEY_PAGE: usize = HOTKEY_ROWS * 2;

const TRACK_H: f32 = 6.0;
const TRACK_W: f32 = 520.0;

/// Заглушка для ещё не сделанных разделов.
const STUB_Y: f32 = 300.0;

const STATUS_Y: f32 = 676.0;
const FOOTER_Y: f32 = 700.0;
const FOOTER_H: f32 = 26.0;

/// Прямоугольник пункта навигации по его индексу в `Section::ALL`.
pub fn nav_rect(index: usize) -> Rect {
    Rect::new(
        RAIL_X + 10.0,
        NAV_TOP + index as f32 * NAV_STEP,
        RAIL_W - 20.0,
        NAV_ITEM_H,
    )
}

/// Прямоугольник всей боковой панели.
pub fn rail_rect() -> Rect {
    Rect::new(RAIL_X, RAIL_Y, RAIL_W, HEIGHT - RAIL_Y - 24.0)
}

/// Прямоугольник горизонтального разделителя правой колонки.
pub fn rule_rect(y: f32) -> Rect {
    Rect::new(PAD_X, y, WIDTH - PAD_X - PAD_R, 1.0)
}

/// Прямоугольник полосы заголовка секции (и строки статуса — та же высота).
pub fn header_rect(y: f32) -> Rect {
    Rect::new(PAD_X, y - HEADER_H / 2.0, WIDTH - PAD_X - PAD_R, HEADER_H)
}

/// Прямоугольник полосы заголовка окна: слева название, справа язык и закрытие.
pub fn title_rect() -> Rect {
    Rect::new(
        TITLE_X,
        TOP_BTN_Y,
        WIDTH - TITLE_X - PAD_R - TOP_BTN_W - 12.0,
        TOP_BTN_H,
    )
}

/// Прямоугольник карточки оформления: `false` — обычная тема, `true` — pixel.
pub fn card_rect(pixel: bool) -> Rect {
    let index = if pixel { 1 } else { 0 };
    Rect::new(
        PAD_X + index as f32 * (COL_W + COL_GAP),
        CARD_Y,
        COL_W,
        CARD_H,
    )
}

/// Полоса заголовка карточки: чекбокс, название и метка применения.
pub fn card_title_rect(pixel: bool) -> Rect {
    let card = card_rect(pixel);
    Rect::new(
        card.x + CARD_PAD,
        CARD_TITLE_Y,
        card.w - CARD_PAD * 2.0,
        CARD_TITLE_H,
    )
}

/// Полоса описания темы.
pub fn card_desc_rect(pixel: bool) -> Rect {
    let card = card_rect(pixel);
    Rect::new(
        card.x + CARD_PAD,
        CARD_DESC_Y,
        card.w - CARD_PAD * 2.0,
        CARD_DESC_H,
    )
}

/// Полоса имени шрифта темы.
pub fn card_font_rect(pixel: bool) -> Rect {
    let card = card_rect(pixel);
    Rect::new(
        card.x + CARD_PAD,
        CARD_FONT_Y,
        card.w - CARD_PAD * 2.0,
        CARD_FONT_H,
    )
}

/// Прямоугольник строки модуля по колонке и строке сетки.
pub fn toggle_rect(column: usize, line: usize) -> Rect {
    Rect::new(
        PAD_X + column as f32 * (COL_W + COL_GAP),
        SUMMARY_Y + line as f32 * (ROW_H + ITEM_GAP),
        COL_W,
        ROW_H,
    )
}

/// Строка сводки в «Обзоре».
pub fn summary_rect(index: usize) -> Rect {
    Rect::new(
        PAD_X + (index % 2) as f32 * (COL_W + COL_GAP),
        SUMMARY_Y + (index / 2) as f32 * (ROW_H + ITEM_GAP),
        COL_W,
        ROW_H,
    )
}

/// Ссылка на раздел: список разделов, каждый открывается по клику.
pub fn goto_rect(index: usize) -> Rect {
    Rect::new(
        PAD_X + (index % 2) as f32 * (COL_W + COL_GAP),
        SECTIONS_Y + (index / 2) as f32 * (ROW_H + ITEM_GAP),
        COL_W,
        ROW_H,
    )
}

/// Переключатель языка в разделе «Внешний вид».
pub fn language_row_rect() -> Rect {
    Rect::new(PAD_X, LANGUAGE_Y, COL_W, ROW_H)
}

/// Строка горячей клавиши из niri: две колонки, индекс уже с учётом прокрутки.
pub fn hotkey_rect(index: usize) -> Rect {
    let width = (WIDTH - PAD_X - PAD_R - HOTKEY_COL_GAP) / 2.0;
    Rect::new(
        PAD_X + (index / HOTKEY_ROWS) as f32 * (width + HOTKEY_COL_GAP),
        HOTKEY_Y + (index % HOTKEY_ROWS) as f32 * HOTKEY_ROW_STEP,
        width,
        ROW_H,
    )
}

const HOTKEY_COL_GAP: f32 = 16.0;

/// Индекс зоны в `Zone::ALL` — он же порядок колонок на панели «Панель».
pub fn zone_column(zone: Zone) -> usize {
    Zone::ALL.iter().position(|item| *item == zone).unwrap_or(0)
}

/// Подпись зоны: «Слева», «По центру», «Справа».
pub fn zone_title(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "Слева",
        Zone::Center => "По центру",
        Zone::Right => "Справа",
    }
}

/// Пояснение под подписью зоны: что с её модулями можно делать.
pub fn zone_hint(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "только переключатель",
        Zone::Center => "порядок меняется здесь",
        Zone::Right => "порядок меняется здесь",
    }
}

/// Полоса подписи зоны: заголовок и пояснение под ним.
pub fn zone_label_rect(zone: Zone) -> Rect {
    Rect::new(zone_x(zone), ZONE_LABEL_Y, ZONE_W, ZONE_LABEL_H)
}

/// Левая граница колонки зоны.
pub fn zone_x(zone: Zone) -> f32 {
    PAD_X + zone_column(zone) as f32 * (ZONE_W + ZONE_GAP)
}

/// Строка модуля внутри зоны. `line` — позиция внутри своей зоны, а не
/// сквозная: так порядок в зоне виден буквально.
pub fn module_rect(zone: Zone, line: usize) -> Rect {
    Rect::new(
        zone_x(zone),
        ZONE_ROWS_Y + line as f32 * (ROW_H + ITEM_GAP),
        ZONE_W,
        ROW_H,
    )
}

/// Кнопка перестановки ▲/▼ у строки модуля.
pub fn move_rect(module: Module, line: usize, up: bool) -> Rect {
    let row = module_rect(module.zone(), line);
    Rect::new(
        if up {
            row.right() - MOVE_BTN_W
        } else {
            row.right() - MOVE_BTN_W * 2.0 - 6.0
        },
        row.y + (ROW_H - MOVE_BTN_H) / 2.0,
        MOVE_BTN_W,
        MOVE_BTN_H,
    )
}

/// Где в строке живёт переключатель видимости: левая часть, кнопки ▲▼ — справа.
pub fn switch_rect(module: Module, line: usize) -> Rect {
    let row = module_rect(module.zone(), line);
    let buttons = if module.movable() {
        MOVE_BTN_W * 2.0 + 12.0
    } else {
        0.0
    };
    Rect::new(
        row.x + 8.0,
        row.y + 4.0,
        (row.w - 16.0 - buttons).max(40.0),
        ROW_H - 8.0,
    )
}

/// Полоса строки высоты панели: слайдер, значение, подпись и кнопки шага.
pub fn height_row_rect() -> Rect {
    Rect::new(PAD_X, HEIGHT_ROW_Y, WIDTH - PAD_X - PAD_R, HEIGHT_ROW_H)
}

/// Дорожка слайдера высоты — по центру своей полосы.
pub fn height_track_rect() -> Rect {
    let row = height_row_rect();
    Rect::new(row.x, row.y + (row.h - TRACK_H) / 2.0, TRACK_W, TRACK_H)
}

/// Прямоугольник кнопки шага высоты: `dir` — −1 или +1.
pub fn height_step_rect(dir: i32) -> Rect {
    let row = height_row_rect();
    // 0 — «−» у левого края, 1 — «+» у правого; между ними зазор 8 px.
    let index = if dir < 0 { 0.0 } else { 1.0 };
    Rect::new(
        row.right() - (2.0 - index) * STEP_W - (1.0 - index) * 8.0,
        row.y,
        STEP_W,
        row.h,
    )
}

pub fn notification_step_rect(y: f32, dir: i32) -> Rect {
    Rect::new(
        WIDTH - PAD_R - (if dir < 0 { STEP_W * 2.0 + 8.0 } else { STEP_W }),
        y,
        STEP_W,
        HEIGHT_ROW_H,
    )
}

pub fn notification_position_rect(index: usize) -> Rect {
    let column = index % 2;
    let row = index / 2;
    Rect::new(
        PAD_X + column as f32 * (COL_W + COL_GAP),
        NOTIFY_POSITION_Y + row as f32 * (ROW_H + ITEM_GAP),
        COL_W,
        ROW_H,
    )
}

/// Прямоугольник значения высоты и подписи диапазона.
pub fn height_value_rect() -> Rect {
    let track = height_track_rect();
    Rect::new(track.right() + 20.0, track.y - 11.0, 46.0, 28.0)
}

pub fn height_hint_rect() -> Rect {
    let value = height_value_rect();
    Rect::new(value.right() + 12.0, value.y, 92.0, value.h)
}

/// Прямоугольник кнопки закрытия в шапке.
pub fn close_rect() -> Rect {
    Rect::new(WIDTH - PAD_R - TOP_BTN_W, TOP_BTN_Y, TOP_BTN_W, TOP_BTN_H)
}

/// Полоса заглушки незаконченного раздела.
pub fn stub_rect(y: f32) -> Rect {
    Rect::new(PAD_X, y, WIDTH - PAD_X - PAD_R, 26.0)
}

/// Прямоугольник полосы подсказок внизу окна.
pub fn footer_rect() -> Rect {
    Rect::new(PAD_X, FOOTER_Y, WIDTH - PAD_X - PAD_R, FOOTER_H)
}

/// Включён ли отладочный режим раскладки: `HUD_DEBUG_LAYOUT=1`.
pub fn debug_layout() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("HUD_DEBUG_LAYOUT").as_deref() == Ok("1"))
}

/// Все полосы правой колонки и сайдбара — для отладочной отрисовки.
pub fn debug_rects() -> Vec<Rect> {
    let mut rects = vec![
        title_rect(),
        close_rect(),
        rail_rect(),
        header_rect(HEADER_Y),
        header_rect(STATUS_Y),
        footer_rect(),
        height_row_rect(),
        rule_rect(RULE_HEIGHT_Y),
        language_row_rect(),
    ];
    for index in 0..Section::ALL.len() {
        rects.push(nav_rect(index));
    }
    for pixel in [false, true] {
        rects.push(card_rect(pixel));
        rects.push(card_title_rect(pixel));
        rects.push(card_desc_rect(pixel));
        rects.push(card_font_rect(pixel));
    }
    for index in 0..4 {
        rects.push(summary_rect(index));
        rects.push(goto_rect(index));
    }
    for index in 0..Module::ALL.len() {
        rects.push(module_rect(Zone::Right, index % 6));
    }
    for zone in Zone::ALL {
        rects.push(zone_label_rect(zone));
    }
    rects.push(height_track_rect());
    rects.push(height_value_rect());
    rects.push(height_hint_rect());
    rects.push(height_step_rect(-1));
    rects.push(height_step_rect(1));
    for position in NotificationPosition::ALL {
        rects.push(notification_position_rect(
            NotificationPosition::ALL
                .iter()
                .position(|item| *item == position)
                .unwrap_or(0),
        ));
    }
    rects
}

/// Равномерно распределяет подсказки по полосе: одинаковые промежутки,
/// блок центрируется. Ширины берутся из измеренного текста.
pub fn spread(total_width: f32, widths: &[f32]) -> Vec<f32> {
    if widths.is_empty() {
        return Vec::new();
    }
    let sum: f32 = widths.iter().sum();
    let count = widths.len();
    let free = (total_width - sum).max(0.0);
    let gap = if count > 1 {
        free / (count - 1) as f32
    } else {
        0.0
    };
    let mut x = 0.0;
    widths
        .iter()
        .map(|width| {
            let start = x;
            x += width + gap;
            start
        })
        .collect()
}

/// Высота строки, внутри которой центрируется всё содержимое строки модуля.
pub const ITEM_H: f32 = ROW_H;

/// Полная раскладка окна: пункты сайдбара, содержимое активного раздела и
/// обвязка (статус, подсказки, кнопки шапки). Контролы других разделов в
/// список не попадают, поэтому клик в пустой области ничего не меняет.
pub fn rows() -> Vec<Row> {
    rows_for(Section::Overview, &Config::default(), &[], 0)
}

/// Раскладка раздела. `hotkeys` нужны только «Управлению»: там окно показывает
/// горячие клавиши из niri и ничего не правит.
pub fn rows_for(
    section: Section,
    config: &Config,
    hotkeys: &[Hotkey],
    hotkey_scroll: usize,
) -> Vec<Row> {
    let mut rows = nav_rows();
    rows.push(Row::Header {
        text: section.title(),
        y: HEADER_Y,
    });
    match section {
        Section::Overview => rows.extend(overview_rows(config)),
        Section::Panel => rows.extend(panel_rows(config)),
        Section::Appearance => rows.extend(appearance_rows()),
        Section::Notifications => rows.extend(notification_rows()),
        Section::Controls => rows.extend(controls_rows(hotkeys, hotkey_scroll)),
        Section::Wallpaper => rows.push(Row::Stub {
            title: section.label(),
            note: section.stub_note(),
            y: STUB_Y,
        }),
    }
    rows.push(Row::Status { y: STATUS_Y });
    rows.push(Row::Footer { y: FOOTER_Y });
    rows.push(Row::Close { rect: close_rect() });
    rows
}

/// Горячая клавиша из конфига niri. Окно только показывает её, поэтому структура
/// простая: сочетание и расшифровка.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub keys: String,
    pub desc: String,
}

/// Раздел «Обзор»: короткая сводка и переходы в разделы. Контролов здесь нет —
/// всё настраивается в своих разделах, иначе значения пришлось бы дублировать.
fn overview_rows(config: &Config) -> Vec<Row> {
    let enabled = Module::ALL
        .iter()
        .filter(|module| config.module_enabled(**module))
        .count();
    let theme = match config.theme {
        Some(super::config::Theme::Pixel) => "Pixel",
        Some(super::config::Theme::Normal) => "Обычный",
        None => "не задана",
    };
    let language = match config.language {
        super::settings::Language::Ru => "Русский",
        super::settings::Language::En => "English",
    };
    let summary = [
        ("Тема", theme.to_string(), Section::Appearance),
        (
            "Модули",
            format!("{enabled} из {}", Module::ALL.len()),
            Section::Panel,
        ),
        ("Высота", format!("{} px", config.height), Section::Panel),
        ("Язык", language.to_string(), Section::Appearance),
    ];
    let mut rows = vec![Row::Header {
        text: "Сводка",
        y: SUMMARY_HEADER_Y,
    }];
    for (index, (label, value, section)) in summary.into_iter().enumerate() {
        rows.push(Row::Summary {
            label,
            value,
            section,
            rect: summary_rect(index),
        });
    }
    rows.push(Row::Rule {
        y: SECTIONS_HEADER_Y - HEADER_H,
    });
    rows.push(Row::Header {
        text: "Разделы",
        y: SECTIONS_HEADER_Y,
    });
    for (index, target) in Section::ALL.into_iter().enumerate() {
        rows.push(Row::Goto {
            section: target,
            rect: goto_rect(index),
        });
    }
    rows
}

/// Раздел «Панель»: три зоны модулей и высота панели.
fn panel_rows(config: &Config) -> Vec<Row> {
    let mut rows = vec![Row::Header {
        text: "Модули и порядок",
        y: PANEL_HEADER_Y,
    }];
    for zone in Zone::ALL {
        rows.push(Row::ZoneLabel {
            zone,
            title: zone_title(zone),
            hint: zone_hint(zone),
            rect: zone_label_rect(zone),
        });
        // `line` — позиция внутри зоны: перестановка меняет именно её.
        for (line, module) in config.zone(zone).iter().copied().enumerate() {
            rows.push(Row::ModuleRow {
                module,
                line,
                rect: module_rect(zone, line),
                switch: switch_rect(module, line),
            });
            if module.movable() {
                for dir in [-1, 1] {
                    rows.push(Row::Move {
                        module,
                        line,
                        dir,
                        rect: move_rect(module, line, dir < 0),
                    });
                }
            }
        }
    }
    rows.push(Row::Rule { y: RULE_HEIGHT_Y });
    rows.push(Row::Header {
        text: "Высота панели",
        y: HEIGHT_HEADER_Y,
    });
    rows.push(Row::HeightLabel {
        y: HEIGHT_ROW_Y,
        scale: height_track_rect(),
    });
    for dir in [-1, 1] {
        rows.push(Row::Height {
            dir,
            rect: height_step_rect(dir),
        });
    }
    rows
}

/// Раздел «Внешний вид»: тема и язык.
fn appearance_rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for pixel in [false, true] {
        rows.push(Row::Theme {
            pixel,
            rect: card_rect(pixel),
        });
    }
    rows.push(Row::Rule { y: LANGUAGE_RULE_Y });
    rows.push(Row::Header {
        text: "Язык интерфейса",
        y: LANGUAGE_HEADER_Y,
    });
    rows.push(Row::Language {
        rect: language_row_rect(),
    });
    rows
}

/// Раздел «Уведомления»: размер шрифта, высота строки и угол экрана.
fn notification_rows() -> Vec<Row> {
    let mut rows = vec![Row::Header {
        text: "Размер",
        y: NOTIFY_HEADER_Y,
    }];
    rows.push(Row::Rule {
        y: NOTIFY_FONT_RULE_Y,
    });
    for dir in [-1, 1] {
        rows.push(Row::NotificationFont {
            dir,
            rect: notification_step_rect(NOTIFY_FONT_Y, dir),
        });
        rows.push(Row::NotificationLineHeight {
            dir,
            rect: notification_step_rect(NOTIFY_LINE_Y, dir),
        });
    }
    rows.push(Row::Rule {
        y: NOTIFY_POSITION_RULE_Y,
    });
    rows.push(Row::Header {
        text: "Угол экрана",
        y: NOTIFY_POSITION_HEADER_Y,
    });
    for (index, position) in NotificationPosition::ALL.into_iter().enumerate() {
        rows.push(Row::NotificationPosition {
            position,
            rect: notification_position_rect(index),
        });
    }
    rows
}

/// Раздел «Управление»: горячие клавиши из niri. Только чтение, листается
/// `PageUp`/`PageDown`, потому что биндов больше, чем помещается на экран.
fn controls_rows(hotkeys: &[Hotkey], scroll: usize) -> Vec<Row> {
    let mut rows = vec![Row::Header {
        text: "Горячие клавиши (niri)",
        y: HOTKEY_HEADER_Y,
    }];
    for (offset, hotkey) in hotkeys.iter().skip(scroll).take(HOTKEY_PAGE).enumerate() {
        rows.push(Row::Hotkey {
            keys: hotkey.keys.clone(),
            desc: hotkey.desc.clone(),
            rect: hotkey_rect(offset),
        });
    }
    rows
}

/// Раздел «Обои»: заглушка до Block C.
#[allow(dead_code)]
fn wallpaper_rows_removed() {}

/// Пункты сайдбара. Рисование, hit-test и отладка берут один `nav_rect`.
fn nav_rows() -> Vec<Row> {
    Section::ALL
        .iter()
        .enumerate()
        .map(|(index, section)| Row::Nav {
            section: *section,
            rect: nav_rect(index),
        })
        .collect()
}

/// Что под курсором. Координаты — логические пиксели поверхности, поэтому
/// масштаб буфера здесь не применяется.
pub fn hit(rows: &[Row], x: f32, y: f32) -> Option<Focus> {
    rows.iter().rev().find_map(|row| {
        let rect = row.rect()?;
        if !rect.contains(x, y) {
            return None;
        }
        match row.control()? {
            Control::Nav(section) => Some(Focus::Nav(section.index())),
            control => Some(Focus::Content(control)),
        }
    })
}

/// Соседний элемент: `dy` — по своей колонке, `dx` — между колонками.
/// Ищет ближайший по вертикали элемент в той же или соседней колонке,
/// поэтому сетка тумблеров, карточки и кнопки высоты ходят одним кодом.
/// Кнопки шапки (язык, закрытие) не перехватывают вертикальную навигацию.
pub fn neighbour(rows: &[Row], from: Control, dx: i32, dy: i32) -> Option<Control> {
    if dx == 0 && dy == 0 {
        return Some(from);
    }
    let from_rect = rows
        .iter()
        .find(|row| row.control() == Some(from))
        .and_then(Row::rect)?;
    let candidates: Vec<(Control, Rect)> = rows
        .iter()
        .filter_map(|row| Some((row.control()?, row.rect()?)))
        .filter(|(control, _)| *control != from)
        .collect();
    let pick = |same_column: bool| -> Option<(i32, Control)> {
        let mut best: Option<(i32, Control)> = None;
        for (control, rect) in &candidates {
            if matches!(*control, Control::Close) && dy != 0 {
                continue;
            }
            let step = if dy != 0 {
                if same_column && (rect.x - from_rect.x).abs() > 1.0 {
                    continue;
                }
                let diff = (rect.y - from_rect.y) as i32;
                if diff.signum() != dy.signum() {
                    continue;
                }
                // одинаковый y — это соседняя колонка, а не следующий ряд
                diff.abs() * 2
                    + if same_column {
                        0
                    } else {
                        (rect.x - from_rect.x).abs() as i32
                    }
            } else {
                if (rect.y - from_rect.y).abs() > ITEM_H {
                    continue;
                }
                let diff = (rect.x - from_rect.x) as i32;
                if diff.signum() != dx.signum() {
                    continue;
                }
                diff.abs() + ((rect.y - from_rect.y).abs() as i32) * 2
            };
            if best.is_none_or(|(best_step, _)| step < best_step) {
                best = Some((step, *control));
            }
        }
        best
    };
    let picked = if dy != 0 {
        pick(true).or_else(|| pick(false))
    } else {
        pick(true)
    };
    picked.map(|(_, control)| control)
}

/// Все элементы в порядке чтения — для Home/End и пересборки фокуса.
pub fn controls(rows: &[Row]) -> Vec<Control> {
    rows.iter().filter_map(Row::control).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controls_of(rows: &[Row]) -> Vec<Control> {
        controls(rows)
    }

    #[test]
    fn hotkey_list_is_two_columns_within_the_window() {
        let hotkeys: Vec<Hotkey> = (0..HOTKEY_PAGE)
            .map(|n| Hotkey {
                keys: format!("Mod+{n}"),
                desc: format!("Действие {n}"),
            })
            .collect();
        let rows = rows_for(Section::Controls, &Config::default(), &hotkeys, 0);

        assert_eq!(
            rows.iter()
                .filter(|row| matches!(row, Row::Hotkey { .. }))
                .count(),
            HOTKEY_PAGE,
            "показана ровно одна страница"
        );
        for row in &rows {
            if let Row::Hotkey { rect, .. } = row {
                assert!(
                    rect.right() <= WIDTH - PAD_R + 0.5,
                    "строка вылезла за окно"
                );
                assert!(
                    rect.y + rect.h <= STATUS_Y,
                    "строка налезает на строку статуса"
                );
            }
        }
        // вторая колонка начинается правее первой, а не ниже
        let first = hotkey_rect(0);
        let second_column = hotkey_rect(HOTKEY_ROWS);
        assert!(second_column.x > first.right());
        assert_eq!(first.y, second_column.y);
    }

    #[test]
    fn hotkey_scroll_reaches_every_entry_and_stops_at_the_edges() {
        let count = HOTKEY_PAGE * 2 + 3;
        let hotkeys: Vec<Hotkey> = (0..count)
            .map(|n| Hotkey {
                keys: format!("Mod+{n}"),
                desc: format!("Действие {n}"),
            })
            .collect();
        let pages = count.div_ceil(HOTKEY_PAGE);
        assert_eq!(pages, 3);

        let mut scroll = 0usize;
        for page in 1..pages {
            scroll = (scroll / HOTKEY_PAGE + 1).min(pages - 1) * HOTKEY_PAGE;
            let rows = rows_for(Section::Controls, &Config::default(), &hotkeys, scroll);
            let shown = rows
                .iter()
                .filter(|row| matches!(row, Row::Hotkey { .. }))
                .count();
            assert!(
                shown <= HOTKEY_PAGE,
                "страница {page} показала больше, чем помещается"
            );
            // Первая строка страницы — тот бинд, с которого она начинается.
            let first_keys = format!("Mod+{scroll}");
            assert!(
                rows.iter()
                    .any(|row| matches!(row, Row::Hotkey { keys, .. } if *keys == first_keys)),
                "страница {page} началась не с {first_keys}"
            );
        }
    }

    #[test]
    fn modules_map_to_hud_setting_keys() {
        for module in Module::ALL {
            assert_eq!(Module::from_key(module.key()), Some(module));
        }
        assert_eq!(Module::from_key("nope"), None);
        assert_eq!(Module::ALL.len(), 10);
    }

    #[test]
    fn panel_rows_keep_three_zones_and_order_controls() {
        let rows = rows_for(Section::Panel, &Config::default(), &[], 0);

        let zones = rows
            .iter()
            .filter_map(|row| match row {
                Row::ZoneLabel { zone, .. } => Some(*zone),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(zones, Zone::ALL.to_vec());

        let modules = rows
            .iter()
            .filter_map(|row| match row {
                Row::ModuleRow { module, .. } => Some(*module),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(modules, Module::ALL.to_vec());

        let moves = rows
            .iter()
            .filter_map(|row| match row {
                Row::Move { module, dir, .. } => Some((*module, *dir)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(moves.len(), (Module::ALL.len() - 1) * 2);
        assert!(!moves.iter().any(|(module, _)| *module == Module::Tray));
    }

    #[test]
    fn panel_hit_test_separates_switch_and_move_buttons() {
        let rows = rows_for(Section::Panel, &Config::default(), &[], 0);
        let switch = rows
            .iter()
            .find_map(|row| match row {
                Row::ModuleRow {
                    module: Module::Clock,
                    switch,
                    ..
                } => Some(*switch),
                _ => None,
            })
            .expect("clock switch");
        assert_eq!(
            hit(&rows, switch.x + switch.w / 2.0, switch.y + switch.h / 2.0),
            Some(Focus::Content(Control::Switch(Module::Clock)))
        );

        let move_button = rows
            .iter()
            .find_map(|row| match row {
                Row::Move {
                    module: Module::Clock,
                    dir: -1,
                    rect,
                    ..
                } => Some(*rect),
                _ => None,
            })
            .expect("clock up button");
        assert_eq!(
            hit(
                &rows,
                move_button.x + move_button.w / 2.0,
                move_button.y + move_button.h / 2.0
            ),
            Some(Focus::Content(Control::Move(Module::Clock, -1)))
        );
    }

    #[test]
    fn height_is_clamped_and_step_reports_change() {
        assert_eq!(clamp_height(10), HEIGHT_MIN);
        assert_eq!(clamp_height(999), HEIGHT_MAX);
        assert_eq!(step_height(30, 1), Some(31));
        assert_eq!(step_height(30, -1), Some(29));
        assert_eq!(step_height(HEIGHT_MAX, 1), None);
        assert_eq!(step_height(HEIGHT_MIN, -1), None);
    }

    #[test]
    fn every_section_has_its_own_controls_only() {
        // «Обзор» — только сводка и ссылки: настраивать здесь нечего, поэтому
        // дублировать контролы других разделов нельзя.
        let overview_rows = rows_for(Section::Overview, &Config::default(), &[], 0);
        let overview = content_controls(&overview_rows);
        assert!(
            overview
                .iter()
                .all(|control| matches!(control, Control::Goto(_))),
            "в обзоре должны быть только переходы: {overview:?}"
        );
        let summaries = overview_rows
            .iter()
            .filter(|row| matches!(row, Row::Summary { .. }))
            .count();
        assert_eq!(summaries, 4, "сводка: тема, модули, высота, язык");
        assert_eq!(overview.len(), Section::ALL.len() + summaries);

        let panel = content_controls(&rows_for(Section::Panel, &Config::default(), &[], 0));
        assert!(
            panel.iter().any(|c| matches!(c, Control::Switch(_))),
            "в панели должны быть переключатели модулей"
        );
        assert!(
            panel.iter().any(|c| matches!(c, Control::Height(_))),
            "высота переехала в раздел «Панель»"
        );

        let appearance =
            content_controls(&rows_for(Section::Appearance, &Config::default(), &[], 0));
        assert_eq!(
            appearance
                .iter()
                .filter(|c| matches!(c, Control::Theme(_)))
                .count(),
            2
        );
        assert_eq!(
            appearance
                .iter()
                .filter(|c| **c == Control::Language)
                .count(),
            1
        );
    }

    #[test]
    fn controls_in_a_section_never_share_a_rect() {
        let config = Config::default();
        let hotkeys = vec![
            Hotkey {
                keys: "Mod+T".to_string(),
                desc: "Показать клавиши".to_string(),
            },
            Hotkey {
                keys: "Mod+Y".to_string(),
                desc: "Бинды yazi".to_string(),
            },
        ];
        for section in Section::ALL {
            let rows = rows_for(section, &config, &hotkeys, 0);
            let mut rects: Vec<Rect> = rows.iter().filter_map(Row::rect).collect();
            let total = rects.len();
            rects.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
            rects.dedup_by(|a, b| a.x == b.x && a.y == b.y && a.w == b.w && a.h == b.h);
            assert_eq!(
                rects.len(),
                total,
                "в разделе {section:?} два контрола занимают один прямоугольник"
            );
        }
    }

    #[test]
    fn every_row_element_shares_one_center_line() {
        // Строка модуля: подпись, «вкл» и переключатель должны иметь одну
        // вертикальную середину, иначе текст уезжает под контрол.
        let rows = rows();
        for row in &rows {
            if let Row::Toggle { rect, .. } = row {
                let center = rect.y + rect.h / 2.0;
                let switch = Rect::new(rect.right() - 12.0 - 38.0, rect.y, 38.0, rect.h);
                assert_eq!(switch.y + switch.h / 2.0, center);
                assert_eq!(rect.h, ROW_H);
            }
        }

        // Карточка темы: чекбокс, название и «применено» — одна полоса.
        for pixel in [false, true] {
            let card = card_rect(pixel);
            let title = card_title_rect(pixel);
            let box_y = title.y + (title.h - 18.0) / 2.0;
            let mark = Rect::new(card.right() - 104.0, title.y, 88.0, title.h);
            assert_eq!(box_y + 9.0, title.y + title.h / 2.0);
            assert_eq!(mark.y, title.y);
            // Полосы описания и шрифта не наезжают на заголовок и друг на друга.
            let desc = card_desc_rect(pixel);
            let font = card_font_rect(pixel);
            assert!(title.y + title.h <= desc.y);
            assert!(desc.y + desc.h <= font.y);
            assert!(font.y + font.h <= card.y + card.h);
        }
    }

    #[test]
    fn height_row_elements_are_on_one_line() {
        let row = height_row_rect();
        let center = row.y + row.h / 2.0;
        let track = height_track_rect();
        let value = height_value_rect();
        let hint = height_hint_rect();
        for rect in [track, value, hint] {
            assert_eq!(rect.y + rect.h / 2.0, center);
        }
        // Кнопки шага занимают всю высоту полосы, поэтому линия совпадает.
        for dir in [-1, 1] {
            let step = height_step_rect(dir);
            assert_eq!(step.y + step.h / 2.0, center);
            assert!(step.x > track.right());
        }
        // Порядок слева направо: дорожка, значение, подпись, кнопки.
        assert!(track.right() < value.x);
        assert!(value.right() < hint.x);
        assert!(hint.right() < height_step_rect(-1).x);
        assert!(height_step_rect(-1).right() < height_step_rect(1).x);
    }

    #[test]
    fn spread_gives_equal_gaps() {
        let widths = [40.0, 60.0, 50.0];
        let offsets = spread(400.0, &widths);
        let gaps: Vec<f32> = offsets
            .windows(2)
            .zip(widths.windows(2))
            .map(|(pair, sizes)| pair[1] - (pair[0] + sizes[0]))
            .collect();
        assert_eq!(gaps.len(), 2);
        assert!((gaps[0] - gaps[1]).abs() < 0.001);
        // блок не выходит за полосу
        let last = offsets[2] + widths[2];
        assert!(last <= 400.0 + 0.001);
        assert_eq!(spread(400.0, &[]), Vec::<f32>::new());
        assert_eq!(spread(400.0, &[70.0]), vec![0.0]);
    }

    #[test]
    fn footer_hints_fit_the_window() {
        let band = footer_rect();
        let widths: Vec<f32> = HINTS
            .iter()
            .map(|hint| hint.chars().count() as f32 * 7.0)
            .collect();
        let last = spread(band.w, &widths);
        let end = last.last().copied().unwrap_or(0.0) + widths.last().copied().unwrap_or(0.0);
        assert!(end <= band.w + 0.001);
        assert!(band.y + band.h <= HEIGHT - 12.0);
        // статус и подсказки не слипаются
        assert!(STATUS_Y + HEADER_H / 2.0 < band.y);
    }

    #[test]
    fn close_button_does_not_collide_with_title() {
        let title = title_rect();
        let close = close_rect();
        assert!(title.right() <= close.x);
        // шапка выше боковой панели, чтобы не перекрываться
        assert!(title.y + title.h <= rail_rect().y);
    }

    #[test]
    fn zone_rows_stack_without_overlap() {
        let rows = rows_for(Section::Panel, &Config::default(), &[], 0);
        for zone in Zone::ALL {
            let mut rects: Vec<Rect> = rows
                .iter()
                .filter_map(|row| match row {
                    Row::ModuleRow {
                        module, line, rect, ..
                    } if module.zone() == zone && *line > 0 => Some(*rect),
                    _ => None,
                })
                .collect();
            rects.sort_by(|a, b| a.y.total_cmp(&b.y));
            for pair in rects.windows(2) {
                assert!(
                    pair[1].y >= pair[0].y + ROW_H,
                    "строки зоны {zone:?} наезжают: {:?} / {:?}",
                    pair[0],
                    pair[1]
                );
            }
        }
    }

    #[test]
    fn hit_finds_controls_and_misses_the_gaps() {
        let rows = rows_for(Section::Panel, &Config::default(), &[], 0);
        let row = rows
            .iter()
            .find_map(|row| match row {
                Row::ModuleRow {
                    module: Module::Weather,
                    switch,
                    ..
                } => Some(*switch),
                _ => None,
            })
            .expect("строка погоды");

        let center = Focus::Content(Control::Switch(Module::Weather));
        assert_eq!(hit(&rows, row.x + 4.0, row.y + 4.0), Some(center));
        assert_eq!(
            hit(&rows, row.right() - 1.0, row.y + row.h - 1.0),
            Some(center)
        );
        // пустое место под шапкой и между строками
        assert_eq!(hit(&rows, row.x + 4.0, 20.0), None);
        assert_eq!(hit(&rows, row.x + 4.0, row.y + row.h + 1.0), None);
    }

    #[test]
    fn focus_walks_columns_and_rows() {
        let rows = rows_for(Section::Appearance, &Config::default(), &[], 0);

        // карточки оформления — соседи по горизонтали
        assert_eq!(
            neighbour(&rows, Control::Theme(false), 1, 0),
            Some(Control::Theme(true))
        );
        assert_eq!(
            neighbour(&rows, Control::Theme(true), -1, 0),
            Some(Control::Theme(false))
        );
        // с карточки вниз — переключатель языка
        assert_eq!(
            neighbour(&rows, Control::Theme(false), 0, 1),
            Some(Control::Language)
        );
    }

    #[test]
    fn panel_focus_walks_zones_then_height() {
        let rows = rows_for(Section::Panel, &Config::default(), &[], 0);

        // переключатели идут по своей зоне, вбок — в соседнюю
        assert_eq!(
            neighbour(&rows, Control::Switch(Module::Tray), 1, 0),
            Some(Control::Switch(Module::Weather))
        );
        // вниз по своей зоне
        assert_eq!(
            neighbour(&rows, Control::Switch(Module::Weather), 0, 1),
            Some(Control::Switch(Module::Webcam))
        );
        // с самой нижней строки правой зоны — кнопки высоты
        assert_eq!(
            neighbour(&rows, Control::Switch(Module::Dnd), 0, 1),
            Some(Control::Height(-1))
        );
        assert_eq!(
            neighbour(&rows, Control::Height(-1), 1, 0),
            Some(Control::Height(1))
        );
    }

    /// Клик в центр каждого пункта сайдбара попадает в этот пункт и открывает
    /// его раздел. Rect берётся из `nav_rect` — тот же, что и в отрисовке.
    #[test]
    fn clicking_sidebar_item_opens_its_section() {
        for (index, section) in Section::ALL.iter().enumerate() {
            let rect = nav_rect(index);
            let rows = rows_for(*section, &Config::default(), &[], 0);
            let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);

            assert_eq!(hit(&rows, center.0, center.1), Some(Focus::Nav(index)));

            let mut nav = Nav::new(Section::Overview);
            nav.focus = Focus::Nav(index);
            assert_eq!(nav.activate(), None);
            assert_eq!(nav.section, *section);
        }
    }

    /// Hit-test сайдбара и его отрисовка используют один и тот же Rect.
    #[test]
    fn sidebar_hit_test_shares_rects_with_drawing() {
        let debug = debug_rects();
        for index in 0..Section::ALL.len() {
            let rect = nav_rect(index);
            assert!(
                debug.contains(&rect),
                "пункт {index} есть в отрисовке, но не в отладке"
            );
            let row = rows()
                .into_iter()
                .find_map(|row| match row {
                    Row::Nav { rect: nav, .. } if nav == rect => Some(nav),
                    _ => None,
                })
                .expect("пункт сайдбара в раскладке");
            assert_eq!(row, rect);
        }
    }

    /// Клавиатурная последовательность ← ↓ Enter переключает раздел.
    #[test]
    fn keyboard_sequence_switches_section() {
        let mut nav = Nav::new(Section::Overview);
        let mut rows = rows_for(nav.section, &Config::default(), &[], 0);
        // стартуем в содержимом, как после запуска окна
        nav.focus = nav.first_content(&rows).expect("фокус в содержимом");
        assert!(!nav.focus_nav());

        // ← из самой левой колонки уводит в сайдбар
        while !nav.focus_nav() {
            assert!(nav.move_focus(&rows, -1, 0), "не удалось уйти в сайдбар");
        }
        assert_eq!(nav.focus, Focus::Nav(Section::Overview.index()));

        // ↓ в сайдбаре сразу открывает соседний раздел — Enter не нужен
        assert!(nav.move_focus(&rows, 0, 1));
        assert_eq!(nav.focus, Focus::Nav(Section::Panel.index()));
        assert_eq!(nav.section, Section::Panel);

        // Enter в сайдбаре тоже открывает раздел
        assert_eq!(nav.activate(), None);
        assert_eq!(nav.section, Section::Panel);

        // содержимое нового раздела — строки трёх зон, фокус доступен.
        rows = rows_for(nav.section, &Config::default(), &[], 0);
        assert!(!rows.iter().any(|row| matches!(row, Row::Stub { .. })));
        assert!(nav.first_content(&rows).is_some());
    }

    /// У края списка переход циклический: вниз с последнего — на первый, вверх
    /// с первого — на последний.
    #[test]
    fn scrolling_wraps_around_the_ends() {
        let mut nav = Nav::new(Section::Wallpaper);
        let mut rows = rows_for(nav.section, &Config::default(), &[], 0);
        nav.focus = Focus::Nav(nav.section.index());

        // вниз с последнего раздела — на первый
        assert!(nav.move_focus(&rows, 0, 1));
        assert_eq!(nav.section, Section::Overview);
        assert_eq!(nav.focus, Focus::Nav(Section::Overview.index()));
        rows = rows_for(nav.section, &Config::default(), &[], 0);

        // вверх с первого — на последний
        assert!(nav.move_focus(&rows, 0, -1));
        assert_eq!(nav.section, Section::Wallpaper);
        assert_eq!(nav.focus, Focus::Nav(Section::Wallpaper.index()));
    }

    /// Удержание стрелки листает разделы подряд: каждый шаг меняет раздел, а
    /// фокус остаётся в сайдбаре.
    /// Интервал повтора сокращается и упирается в минимум.
    #[test]
    fn repeat_accelerates_and_stops_at_minimum() {
        let mut repeat = Repeat::sections();
        assert_eq!(repeat.delay_ms, 320);
        let mut steps = Vec::new();
        for _ in 0..8 {
            steps.push(repeat.next_interval());
        }
        // каждый следующий шаг быстрее предыдущего
        for pair in steps.windows(2) {
            assert!(pair[1] <= pair[0], "интервал должен сокращаться: {steps:?}");
        }
        // и не опускается ниже минимума
        assert!(steps.iter().all(|step| *step >= repeat.min_interval_ms));
        assert_eq!(*steps.last().unwrap(), repeat.min_interval_ms);
    }

    #[test]
    fn holding_arrows_scrolls_sections() {
        let mut nav = Nav::new(Section::Overview);
        let mut rows = rows_for(nav.section, &Config::default(), &[], 0);
        nav.focus = nav.first_content(&rows).expect("фокус в содержимом");
        while !nav.focus_nav() {
            nav.move_focus(&rows, -1, 0);
        }
        for expected in [Section::Panel, Section::Appearance, Section::Notifications] {
            assert!(nav.move_focus(&rows, 0, 1));
            assert_eq!(nav.section, expected);
            assert_eq!(nav.focus, Focus::Nav(expected.index()));
            rows = rows_for(nav.section, &Config::default(), &[], 0);
        }
        // у края список перескакивает на противоположный конец
        nav.move_focus(&rows, 0, 1);
        assert_eq!(nav.section, Section::Controls);
    }

    /// → из сайдбара возвращает фокус в содержимое, Tab ходит между зонами.
    #[test]
    fn focus_returns_from_sidebar_to_content() {
        let mut nav = Nav::new(Section::Overview);
        let rows = rows_for(nav.section, &Config::default(), &[], 0);
        nav.focus = Focus::Nav(Section::Overview.index());
        assert!(nav.move_focus(&rows, 1, 0));
        assert!(!nav.focus_nav());

        assert!(nav.cycle_zone(&rows, false));
        assert_eq!(nav.focus, Focus::Nav(Section::Overview.index()));
        assert!(nav.cycle_zone(&rows, true));
        assert!(!nav.focus_nav());
    }

    /// В незаконченных разделах нет контролов содержимого: клик в пустой
    /// области ничего не применяет.
    #[test]
    fn unfinished_sections_have_no_content_controls() {
        let hotkeys = vec![Hotkey {
            keys: "Mod+T".to_string(),
            desc: "Показать клавиши".to_string(),
        }];
        for section in Section::ALL {
            let rows = rows_for(section, &Config::default(), &hotkeys, 0);
            let content = content_controls(&rows);
            if matches!(section, Section::Controls) {
                // «Управление» только показывает горячие клавиши: править их
                // здесь нельзя, поэтому интерактивных контролов нет.
                assert!(content.is_empty());
                assert!(
                    rows.iter().any(|row| matches!(row, Row::Hotkey { .. })),
                    "раздел должен показывать горячие клавиши из niri"
                );
            } else if section.is_ready() {
                assert!(!content.is_empty(), "{section:?} должен иметь контролы");
            } else {
                assert!(content.is_empty(), "{section:?} не должен иметь контролов");
            }
        }
    }

    #[test]
    fn focus_can_reach_every_control() {
        let hotkeys = vec![Hotkey {
            keys: "Mod+T".to_string(),
            desc: "Показать клавиши".to_string(),
        }];
        for section in Section::ALL {
            let rows = rows_for(section, &Config::default(), &hotkeys, 0);
            let all = controls_of(&rows);
            let Some(start) = content_controls(&rows).first().copied() else {
                continue;
            };
            let mut reached = vec![start];
            let mut queue = vec![start];
            while let Some(current) = queue.pop() {
                for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                    if let Some(next) = neighbour(&rows, current, dx, dy)
                        && !reached.contains(&next)
                    {
                        reached.push(next);
                        queue.push(next);
                    }
                }
            }
            for control in content_controls(&rows) {
                assert!(
                    reached.contains(&control),
                    "в разделе {section:?} недостижим элемент {control:?}"
                );
            }
            let _ = all;
        }
    }
}

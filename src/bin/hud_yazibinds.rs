//! Нативное окно биндов yazi (KP_9). Замена `yazibind.sh`+rofi.
//! Каркас карточки (фон, шапка, подвал, скроллбар, подсветка) — общий
//! `card_frame`, строки с клавишей и описанием рисуются здесь же. Размер
//! окна и кегли — как у окна биндов niri: 440×452, `type_scale`.
//!
//! Отличия от окна биндов niri: источник данных — keymap.toml yazi,
//! при поиске запрос транслитерируется («копир» ищет и по «kopir»),
//! блок «Частые» при поиске скрывается, счётчик ищет знаменатель без него.

use std::os::fd::AsFd;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers, RepeatInfo},
        pointer::{AxisScroll, PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

#[allow(dead_code)]
#[path = "../hud/bind_data.rs"]
mod bind_data;
#[allow(dead_code)]
#[path = "../hud/binds_layout.rs"]
mod binds_layout;
#[allow(dead_code)]
#[path = "../hud/canvas.rs"]
mod canvas;
#[allow(dead_code)]
#[path = "../hud/card_frame.rs"]
mod card_frame;
#[allow(dead_code)]
#[path = "../hud/geom.rs"]
mod geom;

#[path = "../hud/key_display.rs"]
mod key_display;
#[path = "../hud/palette.rs"]
mod palette;
#[path = "../hud/repeat.rs"]
mod repeat;
#[allow(dead_code)]
#[path = "../hud/settings_icons.rs"]
mod settings_icons;
#[path = "../hud/shell.rs"]
mod shell;
#[allow(dead_code)]
#[path = "../hud/text.rs"]
mod text;
#[allow(dead_code)]
#[path = "../hud/ui_tokens.rs"]
mod ui_tokens;

use repeat::Repeat;

use bind_data::Entry;

const TITLE: &str = "Yazi";
const PLACEHOLDER: &str = "Поиск…";

/// Размеры окна — как у окна биндов niri: 440×452 =
/// `card_frame::card_height(9)` — 52 шапка + 9×40 строк + 40 подвал.
/// Формула живёт в `card_frame` с тестом, числа здесь — для
/// `set_min_size`/`set_max_size` и отката конфигурации.
const WIDTH: u32 = 440;
const HEIGHT: u32 = 452;
/// Подвал: подсказки слева, счётчик справа.
const HINTS: &str = "↑↓ PgUp PgDn ЛИСТАТЬ   ESC ЗАКРЫТЬ";

// Поток строк одной колонкой: заголовок группы занимает всю ширину,
// бинд занимает ячейку. Без рамок — только заливка у выбранной строки.
const COLS: usize = 1;
const ITEM_H: f32 = 30.0;
const COL_GAP: f32 = 20.0;
const ROW_GAP: f32 = 0.0;
const LIST_Y0: f32 = 52.0;
const PAD_X: f32 = 16.0;
/// Отступ после блока секции, чтобы следующая группа не примыкала вплотную.
const SECTION_GAP: f32 = 12.0;

/// Место строки в окне: заголовок секции или бинд.
struct Slot {
    row: usize,
    item: Option<usize>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// Строка списка: заголовок группы или выбираемый бинд.
#[derive(Clone, PartialEq)]
enum ListRow {
    Header(String),
    Item(usize),
}

struct YaziApp {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    /// Клиентский автоповтор удержанной стрелки: композитор повторы не шлёт.
    repeat: Repeat,
    mods: Modifiers,
    /// Painter общего текстового модуля: один на окно, кэш шейпов внутри.
    text: text::TextPainter,
    /// Отдельный painter с Nerd Font — только для глифов.
    icons: text::TextPainter,
    query: String,
    groups: Vec<(String, Vec<Entry>)>,
    items: Vec<Entry>,
    rows: Vec<ListRow>,
    selected: usize,
    /// Строка под указателем: подложка `hover_bg`, выбор не переносит.
    hover: Option<usize>,
    /// Прокрутка в логических пикселях от начала списка.
    top: f32,
    /// Остаток дробной дельты колеса: тачпад шлёт value120 меньше 120,
    /// без накопления одно деление давало step = 0 и скролл не работал.
    scroll_acc: f32,
    palette: palette::Palette,
    stamps: Vec<(PathBuf, Option<SystemTime>)>,
    width: u32,
    height: u32,
    configured: bool,
    dirty: bool,
    exit: Arc<AtomicBool>,
}

fn stamp(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

fn watch_paths() -> Vec<PathBuf> {
    let home = bind_data::home();
    vec![
        home.join(".config/yazi/keymap.toml"),
        home.join(".config/hudbar/settings.json"),
        home.join(".config/hudbar/colors.css"),
    ]
}

fn load_groups() -> Vec<(String, Vec<Entry>)> {
    bind_data::load_yazi_grouped()
}

/// Группы без блока «Частые» — только разделы keymap, без повторов.
fn strip_favorites(groups: &[(String, Vec<Entry>)]) -> &[(String, Vec<Entry>)] {
    let first = groups
        .iter()
        .position(|(name, _)| name != bind_data::YAZI_FAVORITES_GROUP)
        .unwrap_or(groups.len());
    &groups[first..]
}

/// Ширина колонки списка.
fn col_width(w: f32) -> f32 {
    (w - PAD_X * 2.0 - COL_GAP * (COLS - 1) as f32) / COLS as f32
}

/// X-координата колонки.
fn col_x(c: usize, w: f32) -> f32 {
    PAD_X + c as f32 * (col_width(w) + COL_GAP)
}

/// Сколько биндов идёт в секции, если её заголовок стоит в строке `header`.
fn section_len(rows: &[ListRow], header: usize) -> usize {
    rows[header + 1..]
        .iter()
        .take_while(|r| matches!(r, ListRow::Item(_)))
        .count()
}

/// Раскладка всего потока в координатах карточки, без прокрутки и без
/// отсечения по высоте. Секции — компактные блоки: заголовок во всю ширину,
/// бинды ниже, между блоками отступ. Одна колонка, поэтому бинды других
/// групп не встают рядом с секцией.
fn layout_slots(rows: &[ListRow], width: f32) -> Vec<Slot> {
    let col_w = col_width(width);
    let mut slots: Vec<Slot> = Vec::new();
    let mut offset = 0.0f32;
    let mut row = 0;
    while row < rows.len() {
        match &rows[row] {
            ListRow::Header(_) => {
                let hy = LIST_Y0 + offset;
                slots.push(Slot {
                    row,
                    item: None,
                    x: PAD_X,
                    y: hy,
                    w: width - PAD_X * 2.0,
                    h: ITEM_H,
                });
                let len = section_len(rows, row);
                for (n, r) in (row + 1..=row + len).enumerate() {
                    slots.push(Slot {
                        row: r,
                        item: match rows[r] {
                            ListRow::Item(i) => Some(i),
                            ListRow::Header(_) => None,
                        },
                        x: PAD_X,
                        y: hy + ITEM_H + ROW_GAP + n as f32 * (ITEM_H + ROW_GAP),
                        w: col_w,
                        h: ITEM_H,
                    });
                }
                offset += ITEM_H + len as f32 * (ITEM_H + ROW_GAP) + SECTION_GAP;
                row += 1 + len;
            }
            ListRow::Item(i) => {
                // Вне секции такого потока не бывает, но на случай —
                // как обычная строка списка.
                slots.push(Slot {
                    row,
                    item: Some(*i),
                    x: PAD_X,
                    y: LIST_Y0 + offset,
                    w: col_w,
                    h: ITEM_H,
                });
                offset += ITEM_H + ROW_GAP;
                row += 1;
            }
        }
    }
    slots
}

/// Высота потока от начала списка до его конца.
fn content_h(rows: &[ListRow], width: f32) -> f32 {
    layout_slots(rows, width)
        .iter()
        .map(|s| s.y + s.h)
        .fold(LIST_Y0, f32::max)
}

/// Данные кадра, не зависящие от Wayland: живое окно и снимок рисуют их
/// одинаковым кодом.
struct Render<'a> {
    palette: palette::Palette,
    rows: &'a [ListRow],
    items: &'a [Entry],
    selected: usize,
    top: f32,
    query: &'a str,
    hover: Option<usize>,
    /// Знаменатель счётчика при поиске — разделы keymap без блока «Частые».
    groups_total: usize,
}

/// Отрисовка карточки биндов в `card`. Живое окно даёт карточку во весь буфер,
/// снимок — по центру сцены. Каркас, шапка и подвал — из `card_frame`, строки
/// с клавишей и описанием — локальная надстройка.
fn render(
    pixmap: &mut tiny_skia::Pixmap,
    card: geom::Rect,
    r: &Render<'_>,
    painter: &mut text::TextPainter,
    icons: &mut text::TextPainter,
) {
    let ui = ui_tokens::ui_palette(r.palette);
    let scale = ui_tokens::type_scale();

    card_frame::card(pixmap, card, &ui);
    card_frame::header(
        pixmap,
        painter,
        icons,
        card,
        &ui,
        TITLE,
        r.query,
        PLACEHOLDER,
    );

    let list_y = card.y + LIST_Y0;
    let view_h = card.h - card_frame::FOOTER_H - LIST_Y0;

    if r.items.is_empty() {
        painter.paint(
            pixmap,
            "Ничего не найдено",
            scale.label,
            ui.muted,
            card_frame::center_band(
                geom::Rect::new(
                    card.x + PAD_X,
                    list_y,
                    card.w - PAD_X * 2.0,
                    view_h.min(ITEM_H),
                ),
                scale.label * 2.0,
            ),
            text::Align::Start,
        );
    }

    // Колонка клавиш: общий хелпер key_display (по самой длинной клавише
    // списка, не шире 45% карточки). Срез «XF86» внутри хелпера для
    // keymap yazi нейтрален — префикс в ключах не встречается.
    let key_w = key_display::key_column_width(r.items, painter, scale.label, card.w);

    // Поток биндов одним столбцом: клавиша слева, описание справа;
    // выделение — акцентной заливкой строки.
    let slots: Vec<Slot> = layout_slots(r.rows, card.w)
        .into_iter()
        .filter(|s| s.y >= LIST_Y0 + r.top && s.y + s.h <= LIST_Y0 + r.top + view_h)
        .map(|mut s| {
            s.y -= r.top;
            s
        })
        .collect();
    // Подсветка строк — прямая, как в меню: полоса с отступом CARD_INSET=1
    // лог. px от краёв строки. Hover — той же формы: расхождений между ними
    // не оставляем.
    let style = card_frame::RowHighlightStyle {
        rounded: false,
        inset: 1.0,
    };
    for slot in &slots {
        let rect = geom::Rect::new(card.x + slot.x, card.y + slot.y, slot.w, slot.h);
        if let ListRow::Header(label) = &r.rows[slot.row] {
            // Заголовок секции: общий в card_frame (тёмная полоса-фон,
            // акцентная метка слева, подпись акцентом).
            card_frame::section_header(pixmap, painter, rect, &ui, label);
            continue;
        }
        let Some(item) = slot.item else { continue };
        let Some(e) = r.items.get(item) else { continue };
        let selected = item == r.selected;
        if selected {
            card_frame::select_band(pixmap, rect, &ui, style);
        } else if r.hover == Some(item) {
            card_frame::hover_band(pixmap, rect, &ui, style);
        }
        let key_x = rect.x + PAD_X;
        let desc_x = key_x + key_w + PAD_X;
        let desc_w = rect.right() - desc_x - PAD_X;
        // Общая базовая линия строки: клавиша и описание садятся на одну
        // линейку письма — контур глифов отдельного текста её не крутит
        // (KB2.1.1: «gg» и «Начало списка» разъезжались на 1–3 px).
        let baseline = painter.shared_baseline(rect, scale.label);
        painter.paint_boxed_on_baseline(
            pixmap,
            key_display::display_key(e.key.as_str()),
            scale.label,
            if selected { ui.on_primary } else { ui.accent },
            card_frame::center_band(
                geom::Rect::new(key_x, rect.y, key_w, rect.h),
                scale.label * 2.0,
            ),
            text::Align::Start,
            baseline,
        );
        painter.paint_boxed_on_baseline(
            pixmap,
            e.desc.as_str(),
            scale.label,
            if selected { ui.on_primary } else { ui.text },
            card_frame::center_band(
                geom::Rect::new(desc_x, rect.y, desc_w, rect.h),
                scale.label * 2.0,
            ),
            text::Align::Start,
            baseline,
        );
    }

    // Скроллбар: трек во всю высоту списка и яркий бегунок — как в
    // привычном варианте, приглушённый одноцветный бегунок читался хуже.
    let content = (content_h(r.rows, card.w) - LIST_Y0).max(view_h);
    let overflow = content - view_h;
    if overflow > 0.5 {
        card_frame::scrollbar(
            pixmap,
            geom::Rect::new(card.x, list_y, card.w, view_h),
            &ui,
            content,
            view_h,
            r.top,
            card_frame::ScrollbarStyle {
                track: true,
                inset: 6.0,
                thumb_accent: true,
            },
        );
    }

    // Подвал со подсказками и счётчиком. Контент — счётчик, как и был:
    // без запроса — число биндов, с запросом — найденное из общего.
    let items_total = r.items.len();
    let count = if r.query.is_empty() {
        format!("{items_total}")
    } else {
        format!("{items_total} / {}", r.groups_total)
    };
    card_frame::footer(pixmap, painter, card, &ui, HINTS, Some(&count));
}

impl YaziApp {
    fn view_h(&self) -> f32 {
        self.height as f32 - card_frame::FOOTER_H - LIST_Y0
    }

    /// Строки, попавшие в окно при текущей прокрутке, уже в координатах окна.
    fn slots(&self) -> Vec<Slot> {
        // Полоса списка: от LIST_Y0 до подвала. Сдвиг на `top` — целиком
        // (без вычитания LIST_Y0), иначе список уезжал бы на шапку.
        let band_y0 = LIST_Y0 + self.top;
        let band_y1 = band_y0 + self.view_h();
        layout_slots(&self.rows, self.width as f32)
            .into_iter()
            // Только строки, целиком попавшие в полосу списка: иначе
            // первая/последняя строка наезжала на шапку или подвал.
            .filter(|s| s.y >= band_y0 && s.y + s.h <= band_y1)
            .map(|mut s| {
                s.y -= self.top;
                s
            })
            .collect()
    }

    /// Абсолютная плашка бинда — по ней курсор держится в поле зрения.
    fn slot_of(&self, item: usize) -> Option<Slot> {
        layout_slots(&self.rows, self.width as f32)
            .into_iter()
            .find(|s| s.item == Some(item))
    }

    /// Предельная прокрутка: глубже последняя строка всё равно не видна.
    fn max_top(&self) -> f32 {
        (self.content_h() - self.height as f32 + card_frame::FOOTER_H).max(0.0)
    }

    fn content_h(&self) -> f32 {
        content_h(&self.rows, self.width as f32)
    }

    /// Строк на страницу — по высоте окна.
    fn page_lines(&self) -> i32 {
        ((self.view_h() / (ITEM_H + ROW_GAP)).floor() as i32).max(1)
    }

    /// Пересобирает список и поток строк под текущий запрос: в потоке у каждой
    /// группы свой заголовок, поэтому список читается по секциям.
    fn rebuild(&mut self) {
        let prev = self.items.get(self.selected).cloned();
        let mut groups = self.groups.as_slice();
        // Блок «Частые» — это повтор биндов из своих групп, при поиске он дал бы
        // каждый найденный бинд дважды. Пустая выдача — только разделы файла.
        if !self.query.is_empty() {
            groups = strip_favorites(groups);
        }
        // Клавиши показаны в QWERTY, а пользователь печатает в русской раскладке:
        // ищем и по набранному, и по транслитерации. Иначе «щ» не находил «o».
        let latin = bind_data::transliterate(&self.query);
        let filtered = binds_layout::groups_matching_any(groups, &[&self.query, &latin]);
        self.items = filtered
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .cloned()
            .collect();
        // Поток строк: у каждой группы свой заголовок, иначе секции сливаются.
        self.rows.clear();
        let mut item = 0;
        for (name, entries) in &filtered {
            self.rows.push(ListRow::Header(name.clone()));
            for _ in entries {
                self.rows.push(ListRow::Item(item));
                item += 1;
            }
        }
        // Курсор держим за самим биндом, а не за номером строки.
        self.selected = prev
            .and_then(|p| self.items.iter().position(|e| *e == p))
            .unwrap_or(0);
        self.top = 0.0;
        self.dirty = true;
    }

    /// Общая обработка клавиши для `press_key` и `repeat_key`.
    fn on_key(&mut self, event: KeyEvent) {
        match event.keysym {
            Keysym::Escape => self.exit.store(true, Ordering::Relaxed),
            Keysym::Return | Keysym::KP_Enter => self.exit.store(true, Ordering::Relaxed),
            Keysym::Up => self.move_sel(0, -1),
            Keysym::Down => self.move_sel(0, 1),
            Keysym::Left => self.move_sel(-1, 0),
            Keysym::Right => self.move_sel(1, 0),
            Keysym::Page_Up => self.scroll_by(-self.page_lines()),
            Keysym::Page_Down => self.scroll_by(self.page_lines()),
            Keysym::Home => {
                if !self.items.is_empty() {
                    self.select(0);
                }
            }
            Keysym::End => {
                if let Some(i) = self.items.len().checked_sub(1) {
                    self.select(i);
                }
            }
            Keysym::BackSpace => {
                self.query.pop();
                self.rebuild();
            }
            _ => {
                // Ctrl и Super — служебные: на русской раскладке Alt даёт
                // AltGr-символы («<», «;»), их вводим.
                if self.mods.ctrl || self.mods.logo {
                    if (event.keysym == Keysym::u || event.keysym == Keysym::U) && self.mods.ctrl {
                        self.query.clear();
                        self.rebuild();
                    }
                    return;
                }
                if let Some(text) = &event.utf8 {
                    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !clean.is_empty() {
                        self.query.push_str(&clean);
                        self.rebuild();
                    }
                }
            }
        }
    }

    fn select(&mut self, item: usize) {
        self.selected = item;
        self.ensure_visible();
        self.dirty = true;
    }

    /// `dy` шагает по списку (вниз — следующий бинд), `dx` — к соседней строке
    /// той же визуальной строки в другой колонке.
    fn move_sel(&mut self, dx: i32, dy: i32) {
        if dy != 0 {
            let next = if dy < 0 {
                self.selected.checked_sub(1)
            } else {
                let down = self.selected + 1;
                (down < self.items.len()).then_some(down)
            };
            if let Some(next) = next {
                self.select(next);
            }
            return;
        }
        let slots = self.slots();
        let in_column = |c: usize| -> Vec<Option<usize>> {
            let x = col_x(c, self.width as f32);
            slots.iter().filter(|s| s.x == x).map(|s| s.item).collect()
        };
        let Some(cur) = slots.iter().find(|s| s.item == Some(self.selected)) else {
            return;
        };
        let from = if (cur.x - PAD_X).abs() < f32::EPSILON {
            0
        } else {
            1
        };
        let col = in_column(from);
        let Some(pos) = col.iter().position(|&i| i == Some(self.selected)) else {
            return;
        };
        let other = from as isize - dx as isize;
        if other >= 0
            && (other as usize) < COLS
            && let Some(Some(item)) = in_column(other as usize).get(pos)
            && *item != self.selected
        {
            self.select(*item);
        }
    }

    /// Держит выбранную строку в поле зрения, иначе подкручивает поток.
    fn ensure_visible(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let Some(slot) = self.slot_of(self.selected) else {
            return;
        };
        let band_y0 = LIST_Y0 + self.top;
        let band_y1 = band_y0 + self.view_h();
        let top = if slot.y < band_y0 {
            slot.y - LIST_Y0
        } else if slot.y + slot.h > band_y1 {
            slot.y + slot.h - LIST_Y0 - self.view_h()
        } else {
            self.top
        };
        self.top = top.clamp(0.0, self.max_top());
    }

    fn scroll_by(&mut self, lines: i32) {
        if self.items.is_empty() {
            return;
        }
        self.top = (self.top + lines as f32 * (ITEM_H + ROW_GAP)).clamp(0.0, self.max_top());
        // Курсор не теряем: подтягиваем к ближайшей видимой строке.
        let vis: Vec<usize> = self.slots().into_iter().filter_map(|s| s.item).collect();
        if !vis.contains(&self.selected)
            && let Some(&nearest) = vis
                .iter()
                .min_by_key(|&&i| (i as i32 - self.selected as i32).abs())
        {
            self.selected = nearest;
        }
        self.dirty = true;
    }

    fn refresh_sources(&mut self) {
        let mut changed = false;
        for (path, prev) in self.stamps.iter_mut() {
            let cur = stamp(path);
            if cur != *prev {
                *prev = cur;
                changed = true;
            }
        }
        if changed {
            self.groups = load_groups();
            self.rebuild();
            self.palette = palette::load();
            self.dirty = true;
        } else if palette::load() != self.palette {
            self.palette = palette::load();
            self.dirty = true;
        }
    }

    /// Какая строка под курсором. Заголовки не выбираются.
    fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        self.slots()
            .into_iter()
            .find(|s| y >= s.y && y <= s.y + s.h && x >= s.x && x <= s.x + s.w)
            .and_then(|s| s.item)
    }

    /// Дельта колеса в целых строках. Формула общая (`shell::axis_steps`):
    /// дробный остаток тачпада копится и отдаётся по целой строке.
    fn axis_step(&mut self, v: &AxisScroll) -> i32 {
        shell::axis_steps(&mut self.scroll_acc, v) as i32
    }

    fn draw(&mut self) {
        if !self.configured || self.width == 0 || self.height == 0 {
            return;
        }
        let pw = (self.width as f32 * text::SCALE) as u32;
        let ph = (self.height as f32 * text::SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        // Знаменатель счётчика — разделы keymap: блок «Частые» при поиске
        // скрыт, поэтому и считаем без него.
        let groups_total: usize = strip_favorites(&self.groups)
            .iter()
            .map(|(_, e)| e.len())
            .sum();
        let r = Render {
            palette: self.palette,
            rows: &self.rows,
            items: &self.items,
            selected: self.selected,
            top: self.top,
            query: &self.query,
            hover: self.hover,
            groups_total,
        };
        let card = geom::Rect::new(0.0, 0.0, self.width as f32, self.height as f32);
        render(&mut pixmap, card, &r, &mut self.text, &mut self.icons);
        // Плотность карточки как у меню: отрисовка идёт цветами с альфой
        // BG_ALPHA, в конце проход домножает карточку на CARD_ALPHA.
        card_frame::apply_card_alpha(&mut pixmap, card, card_frame::CARD_ALPHA);

        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
        else {
            // Пул занят — композитор ещё не отдал предыдущий буфер.
            // Кадр не рисуем, но и не теряем: пробуем на следующем тике.
            self.dirty = true;
            return;
        };
        let (dst, _) = canvas.as_chunks_mut::<4>();
        let (src, _) = pixmap.data().as_chunks::<4>();
        for (out, input) in dst.iter_mut().zip(src.iter()) {
            *out = [input[2], input[1], input[0], input[3]];
        }
        let surface = self.window.wl_surface();
        surface.set_buffer_scale(text::SCALE as i32);
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        let _ = buffer.attach_to(surface);
        self.window.commit();
    }
}

impl CompositorHandler for YaziApp {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for YaziApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for YaziApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(pointer) = self.pointer.take()
        {
            pointer.release();
        }
        if capability == Capability::Keyboard
            && let Some(keyboard) = self.keyboard.take()
        {
            keyboard.release();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl WindowHandler for YaziApp {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit.store(true, Ordering::Relaxed);
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        self.width = configure.new_size.0.map(|v| v.get()).unwrap_or(WIDTH);
        self.height = configure.new_size.1.map(|v| v.get()).unwrap_or(HEIGHT);
        self.configured = true;
        self.dirty = true;
    }
}

impl PointerHandler for YaziApp {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if event.surface != *self.window.wl_surface() {
                continue;
            }
            match &event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    // Курсор даёт подложку `hover_bg`, выбор не переносит:
                    // выбор — стрелки и колесо, как у окна биндов niri.
                    let idx = self.row_at(event.position.0 as f32, event.position.1 as f32);
                    if idx != self.hover {
                        self.hover = idx;
                        self.dirty = true;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.hover.take().is_some() {
                        self.dirty = true;
                    }
                }
                PointerEventKind::Press { button: 0x110, .. } => {
                    if self
                        .row_at(event.position.0 as f32, event.position.1 as f32)
                        .is_some()
                    {
                        self.exit.store(true, Ordering::Relaxed);
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let step = self.axis_step(vertical);
                    if step != 0 {
                        self.scroll_by(step);
                    }
                }
                _ => {}
            }
        }
    }
}

impl KeyboardHandler for YaziApp {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.repeat.press(&event);
        self.on_key(event);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        // Удержание стрелки должно листать, а не срабатывать один раз.
        self.on_key(event);
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.repeat.release(&event);
    }
    /// Rate и delay удержания клавиши: композитор сообщает их один раз,
    /// клиент сам выдаёт повторы, пока клавиша зажата.
    fn update_repeat_info(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        info: RepeatInfo,
    ) {
        self.repeat.set_info(info);
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.mods = modifiers;
    }
}

impl ShmHandler for YaziApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for YaziApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(YaziApp);
smithay_client_toolkit::delegate_dispatch2!(YaziApp);

/// Демо-данные снимка: две секции, 14 биндов из keymap yazi. Питают и
/// снапшот, и тесты колонки клавиш. Блока «Частые» здесь нет совсем:
/// счётчик и поиск в живом окне строго свои, снимок их не проверяет.
fn demo_groups() -> Vec<(String, Vec<Entry>)> {
    let keys = [
        (
            "Навигация",
            [
                ("j", "Вниз"),
                ("k", "Вверх"),
                ("gg", "Начало списка"),
                ("G", "Конец списка"),
                ("Return", "Открыть"),
                ("Backspace", "Каталог выше"),
                ("q", "Выход"),
            ],
        ),
        (
            "Операции с файлами",
            [
                ("Space", "Отметить"),
                ("y", "Скопировать"),
                ("x", "Вырезать"),
                ("p", "Вставить"),
                ("d", "Удалить"),
                ("r", "Переименовать"),
                (".", "Скрытые файлы"),
            ],
        ),
    ];
    keys.iter()
        .map(|(name, pairs)| {
            (
                name.to_string(),
                pairs
                    .iter()
                    .map(|(k, d)| Entry::new(k.to_string(), d.to_string()))
                    .collect(),
            )
        })
        .collect()
}

/// `hud-yazibinds-rs --snapshot <out.png>`: тот же `render`, что у живого
/// окна, но без Wayland. Демо-данных хватает на прокрутку: две секции,
/// 14 биндов, выбран пятый, список прокручен на три строки. Фон сцены —
/// как у снимков меню (menu/snapshot.rs): градиент, карточка по центру.
/// Карточка кладётся на сцену настоящим альфа-смешиванием
/// (`card_frame::blend_card`): пиксели снимка непрозрачные, как после
/// композитора в живом окне.
fn snapshot(out: &str) {
    // Цвета сцены — из menu/snapshot.rs (:35-37): верх и горизонт градиента.
    const BACKDROP_TOP: palette::Rgba = palette::Rgba(0x1a, 0x1f, 0x30, 255);
    const BACKDROP_HORIZON: palette::Rgba = palette::Rgba(0x11, 0x15, 0x24, 255);
    const SCENE_W: f32 = 1200.0;
    const SCENE_H: f32 = 800.0;

    let groups = demo_groups();
    let items = binds_layout::entries_from_groups(&groups, "");
    let mut rows: Vec<ListRow> = Vec::new();
    let mut item = 0;
    for (name, entries) in &groups {
        rows.push(ListRow::Header(name.clone()));
        for _ in entries {
            rows.push(ListRow::Item(item));
            item += 1;
        }
    }
    let r = Render {
        palette: palette::load(),
        rows: &rows,
        items: &items,
        selected: 5,
        top: 90.0,
        query: "",
        hover: None,
        groups_total: items.len(),
    };

    let height = card_frame::card_height(card_frame::ROOT_ROWS);
    let card = geom::Rect::new(
        (SCENE_W - card_frame::CARD_W) / 2.0,
        (SCENE_H - height) / 2.0,
        card_frame::CARD_W,
        height,
    );
    let mut pixmap = tiny_skia::Pixmap::new(
        (SCENE_W * text::SCALE) as u32,
        (SCENE_H * text::SCALE) as u32,
    )
    .expect("pixmap");
    // Градиентный фон сцены — построчно, как у снимков меню.
    let width = pixmap.width();
    let scene_height = pixmap.height();
    {
        let pixels = pixmap.pixels_mut();
        for y in 0..scene_height {
            let t = y as f32 / scene_height.max(1) as f32;
            let red = (BACKDROP_TOP.0 as f32
                + (BACKDROP_HORIZON.0 as f32 - BACKDROP_TOP.0 as f32) * t)
                as u8;
            let green = (BACKDROP_TOP.1 as f32
                + (BACKDROP_HORIZON.1 as f32 - BACKDROP_TOP.1 as f32) * t)
                as u8;
            let blue = (BACKDROP_TOP.2 as f32
                + (BACKDROP_HORIZON.2 as f32 - BACKDROP_TOP.2 as f32) * t)
                as u8;
            let row = y as usize * width as usize;
            for x in 0..width as usize {
                if let Some(color) =
                    tiny_skia::PremultipliedColorU8::from_rgba(red, green, blue, 255)
                {
                    pixels[row + x] = color;
                }
            }
        }
    }
    let mut painter = text::TextPainter::new(&ui_tokens::font_name());
    let mut icons = text::TextPainter::new(settings_icons::FONT);
    // Карточка рисуется в отдельный буфер тем же кодом, что и живое окно,
    // с той же плотностью (`apply_card_alpha`), и уже полупрозрачной
    // смешивается на непрозрачную сцену — пиксели снимка выходят
    // непрозрачными, как после композитора.
    let mut card_pixmap =
        tiny_skia::Pixmap::new((card.w * text::SCALE) as u32, (card.h * text::SCALE) as u32)
            .expect("card pixmap");
    let card_rect = geom::Rect::new(0.0, 0.0, card.w, card.h);
    render(&mut card_pixmap, card_rect, &r, &mut painter, &mut icons);
    card_frame::apply_card_alpha(&mut card_pixmap, card_rect, card_frame::CARD_ALPHA);
    card_frame::blend_card(
        &mut pixmap,
        &card_pixmap,
        (card.x * text::SCALE) as i32,
        (card.y * text::SCALE) as i32,
    );
    if let Err(error) = canvas::write_png(&pixmap, std::path::Path::new(out)) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    println!("yazi {}x{} → {out}", pixmap.width(), pixmap.height());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--snapshot") {
        match args.get(1) {
            Some(out) => snapshot(out),
            None => {
                eprintln!("использование: hud-yazibinds-rs --snapshot <out.png>");
                std::process::exit(2);
            }
        }
        return;
    }

    let conn = Connection::connect_to_env().expect("wayland connection");
    let (globals, mut event_queue) = registry_queue_init(&conn).expect("registry");
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm");
    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::ServerDefault, &qh);
    window.set_title(TITLE);
    window.set_app_id("com.mihail.hud-yazibinds");
    window.set_min_size(Some((WIDTH, HEIGHT)));
    window.set_max_size(Some((WIDTH, HEIGHT)));
    window.commit();

    let exit = Arc::new(AtomicBool::new(false));
    // Буфер 440x452 при scale 2 — это ~3.2 МБ; держим четыре, иначе при быстром
    // вводе пул кончается и кадр не рисуется.
    let pool = SlotPool::new(40_000_000, &shm).expect("shm pool");
    let groups = load_groups();
    let paths = watch_paths();
    let stamps = paths.iter().map(|p| (p.clone(), stamp(p))).collect();
    let mut app = YaziApp {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        pool,
        window,
        pointer: None,
        keyboard: None,
        mods: Modifiers::default(),
        repeat: Repeat::default(),
        text: text::TextPainter::new(&ui_tokens::font_name()),
        icons: text::TextPainter::new(settings_icons::FONT),
        query: String::new(),
        groups,
        items: Vec::new(),
        rows: Vec::new(),
        selected: 0,
        hover: None,
        top: 0.0,
        scroll_acc: 0.0,
        palette: palette::load(),
        stamps,
        width: WIDTH,
        height: HEIGHT,
        configured: false,
        dirty: true,
        exit,
    };
    app.rebuild();

    while !app.exit.load(Ordering::Relaxed) {
        app.refresh_sources();
        if event_queue.flush().is_err() {
            break;
        }
        // Сон до ближайшего повтора удержанной клавиши или 250 мс: композитор
        // повторы не шлёт, клиент выдаёт их сам, поэтому спать дольше —
        // значит двигать выбор рывками по тикам сна.
        let timeout = app
            .repeat
            .wait_hint()
            .and_then(|left| u16::try_from(left.as_millis()).ok())
            .unwrap_or(250);
        shell::poll_read(&mut event_queue, conn.as_fd(), timeout);
        // Ошибка разговора с вейлендом — повод выйти, иначе цикл крутится вхолостую.
        if event_queue.dispatch_pending(&mut app).is_err() {
            break;
        }
        // Повтор удержанной стрелки виден обработчику как обычное нажатие.
        let mut repeat_fired = false;
        while let Some(event) = app.repeat.poll() {
            app.on_key(event);
            repeat_fired = true;
        }
        if repeat_fired {
            app.dirty = true;
        }
        if app.dirty {
            app.dirty = false;
            app.draw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// В демо-данных снапшота ни одна клавиша не усечена: самая длинная
    /// ниже потолка 45% ширины карточки.
    #[test]
    fn demo_keys_fit_their_column() {
        let items = binds_layout::entries_from_groups(&demo_groups(), "");
        let mut painter = text::TextPainter::new(&ui_tokens::font_name());
        let size = ui_tokens::type_scale().label;
        let key_w = key_display::key_column_width(&items, &mut painter, size, card_frame::CARD_W);
        for entry in &items {
            assert!(
                painter.text_width(key_display::display_key(entry.key.as_str()), size) <= key_w,
                "{} не влезает в колонку {key_w:.1}",
                entry.key
            );
        }
        // И потолок уважен: колонка не тянет атмосферу шире 45% карточки.
        assert!(key_w <= card_frame::CARD_W * 0.45 + f32::EPSILON);
    }

    /// Клавиша и описание демо-строки садятся на одну базовую линию:
    /// строки идут по «линейке письма», а не по центру контура глифов
    /// (KB2.1.1). Полосы centerY одинаковы, x и ширина различаются —
    /// на baseline это влиять не должно.
    #[test]
    fn demo_key_and_desc_share_baseline() {
        let items = binds_layout::entries_from_groups(&demo_groups(), "");
        let mut painter = text::TextPainter::new(&ui_tokens::font_name());
        let size = ui_tokens::type_scale().label;
        let key_w = card_frame::CARD_W * 0.45;
        let key_band =
            card_frame::center_band(geom::Rect::new(PAD_X, 0.0, key_w, ITEM_H), size * 2.0);
        let desc_band = card_frame::center_band(
            geom::Rect::new(
                PAD_X + key_w + PAD_X,
                0.0,
                card_frame::CARD_W - PAD_X * 3.0 - key_w,
                ITEM_H,
            ),
            size * 2.0,
        );
        for entry in &items {
            assert_eq!(
                painter.shared_baseline(key_band, size),
                painter.shared_baseline(desc_band, size),
                "клавиша {} и её описание разъехались по высоте",
                entry.key
            );
        }
    }
}

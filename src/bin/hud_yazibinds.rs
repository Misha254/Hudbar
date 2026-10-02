//! Нативное окно биндов yazi (KP_9). Замена `yazibind.sh`+rofi.
//! Основные бинды и раздел «Операции с файлами».

use std::collections::HashMap;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use cosmic_text::{
    Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
};
use nix::poll::{PollFd, PollFlags, poll};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
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
#[path = "../hud/palette.rs"]
mod palette;

use bind_data::Entry;

const SCALE: f32 = 2.0;
const WIDTH: u32 = 900;
const HEIGHT: u32 = 620;
const TITLE: &str = "Yazi";
/// Прозрачность фона как у hudbar: base на 90%, сквозь окно видно стол.
const BG_ALPHA: f32 = 0.9;
const PLACEHOLDER: &str = "Поиск…";

// Поток строк в две колонки, как в окне биндов niri: заголовок группы на всю
// ширину, бинд — клавиша и описание, рамок нет.
const COLS: usize = 2;
const ITEM_H: f32 = 26.0;
const COL_GAP: f32 = 28.0;
const ROW_GAP: f32 = 2.0;
const LIST_Y0: f32 = 118.0;
const PAD_X: f32 = 28.0;
const MARK_W: f32 = 2.0;
const KEY_W: f32 = 132.0;
/// Отступ после блока секции, чтобы следующая группа не примыкала вплотную.
const SECTION_GAP: f32 = 12.0;

/// Строка списка: заголовок группы или выбираемый бинд.
#[derive(Clone, Copy)]
struct Slot {
    row: usize,
    item: Option<usize>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    /// Строка — последняя в блоке секции: под ней идёт разделитель.
    section_end: bool,
}

#[derive(Clone, Copy)]
struct UiPalette {
    base: palette::Rgba,
    panel: palette::Rgba,
    idle_panel: palette::Rgba,
    border: palette::Rgba,
    accent: palette::Rgba,
    text: palette::Rgba,
    muted: palette::Rgba,
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
    mods: Modifiers,
    font_system: FontSystem,
    swash: SwashCache,
    /// Замеренный верх глифов (logical px) для (шрифт, размер, интерлиньяж).
    top_cache: HashMap<(String, u32, u32), f32>,
    font: String,
    pixel: bool,
    query: String,
    groups: Vec<(String, Vec<Entry>)>,
    items: Vec<Entry>,
    rows: Vec<ListRow>,
    selected: usize,
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

#[derive(Clone, PartialEq)]
enum ListRow {
    Header(String),
    Item(usize),
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

fn mix(a: palette::Rgba, b: palette::Rgba, t: f32) -> palette::Rgba {
    palette::Rgba(
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
        255,
    )
}

fn ui_palette(pixel: bool, p: palette::Palette) -> UiPalette {
    if pixel {
        UiPalette {
            base: palette::Rgba(0x0b, 0x10, 0x20, 255).with_a(BG_ALPHA),
            panel: palette::Rgba(0x11, 0x1b, 0x31, 255).with_a(BG_ALPHA),
            idle_panel: palette::Rgba(0x0e, 0x17, 0x2b, 255).with_a(BG_ALPHA),
            border: palette::Rgba(0x31, 0x5b, 0x9b, 255),
            accent: palette::Rgba(0x79, 0xa7, 0xff, 255),
            text: palette::Rgba(0xe8, 0xea, 0xff, 255),
            muted: palette::Rgba(0x8d, 0x9a, 0xbd, 255),
        }
    } else {
        UiPalette {
            base: p.base.with_a(BG_ALPHA),
            panel: mix(p.base, p.secondary, 0.10).with_a(BG_ALPHA),
            idle_panel: mix(p.base, p.secondary, 0.05).with_a(BG_ALPHA),
            border: mix(p.base, p.primary, 0.35),
            accent: p.primary,
            text: p.text,
            muted: p.text.with_a(0.58),
        }
    }
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

impl YaziApp {
    fn col_width(&self) -> f32 {
        (self.width as f32 - PAD_X * 2.0 - COL_GAP * (COLS - 1) as f32) / COLS as f32
    }

    fn col_x(&self, c: usize) -> f32 {
        PAD_X + c as f32 * (self.col_width() + COL_GAP)
    }

    /// Сколько биндов идёт в секции, если её заголовок стоит в строке `header`.
    fn section_len(&self, header: usize) -> usize {
        self.rows[header + 1..]
            .iter()
            .take_while(|r| matches!(r, ListRow::Item(_)))
            .count()
    }

    /// Раскладка всего потока в абсолютных координатах, без прокрутки и без
    /// отсечения по высоте окна. Секции — компактные блоки: заголовок во всю
    /// ширину, бинды по COLS в ряд начиная с левой колонки. Обе колонки после
    /// блока продолжаются с одной высоты, поэтому бинды других групп не
    /// встают рядом с секцией.
    ///
    /// Раскладка всегда от начала списка — иначе прокрутка внутрь секции
    /// переставляла её бинды в газетную колонку и список дёргался.
    fn layout_slots(&self) -> Vec<Slot> {
        let col_w = self.col_width();
        let mut slots: Vec<Slot> = Vec::new();
        let mut col_y = [0.0f32; COLS];
        let mut row = 0;
        while row < self.rows.len() {
            match &self.rows[row] {
                ListRow::Header(_) => {
                    let y = LIST_Y0 + col_y[0].max(col_y[1]);
                    slots.push(Slot {
                        row,
                        item: None,
                        x: PAD_X,
                        y,
                        w: self.width as f32 - PAD_X * 2.0,
                        h: ITEM_H,
                        section_end: false,
                    });
                    let len = self.section_len(row);
                    let block_rows = len.div_ceil(COLS);
                    let favorites = matches!(
                        &self.rows[row],
                        ListRow::Header(name) if name == bind_data::YAZI_FAVORITES_GROUP
                    );
                    for (n, r) in (row + 1..=row + len).enumerate() {
                        // «Частые» читаются как две вертикальные группы:
                        // основные операции слева, дополнительные действия справа.
                        // Остальные секции остаются компактными построчно.
                        let (c, row_in_block) = if favorites {
                            (n / block_rows, n % block_rows)
                        } else {
                            (n % COLS, n / COLS)
                        };
                        slots.push(Slot {
                            row: r,
                            item: match self.rows[r] {
                                ListRow::Item(i) => Some(i),
                                ListRow::Header(_) => None,
                            },
                            x: self.col_x(c),
                            y: y + ITEM_H + ROW_GAP + row_in_block as f32 * (ITEM_H + ROW_GAP),
                            w: col_w,
                            h: ITEM_H,
                            section_end: n + 1 == len,
                        });
                    }
                    let used = ITEM_H + block_rows as f32 * (ITEM_H + ROW_GAP) + SECTION_GAP;
                    col_y = [y - LIST_Y0 + used; COLS];
                    row += 1 + len;
                }
                ListRow::Item(_) => {
                    // Вне секции такого в keymap нет, но на случай: газеткой в
                    // колонку с меньшей высотой.
                    let c = usize::from(col_y[1] < col_y[0]);
                    let item = match self.rows[row] {
                        ListRow::Item(i) => i,
                        ListRow::Header(_) => unreachable!(),
                    };
                    slots.push(Slot {
                        row,
                        item: Some(item),
                        x: self.col_x(c),
                        y: LIST_Y0 + col_y[c],
                        w: col_w,
                        h: ITEM_H,
                        section_end: false,
                    });
                    col_y[c] += ITEM_H + ROW_GAP;
                    row += 1;
                }
            }
        }
        slots
    }

    /// Высота окна под список.
    fn view_h(&self) -> f32 {
        self.height as f32 - 8.0 - LIST_Y0
    }

    /// Нижняя кромка потока в абсолютных координатах.
    fn content_h(&self) -> f32 {
        self.layout_slots()
            .iter()
            .map(|s| s.y + s.h)
            .fold(LIST_Y0, f32::max)
    }

    /// Строки, попавшие в окно при текущей прокрутке, уже в координатах окна.
    fn slots(&self) -> Vec<Slot> {
        // Полоса списка: от LIST_Y0 до низа окна. Сдвиг на `top` — целиком
        // (без вычитания LIST_Y0), иначе список уезжал бы на шапку.
        let band_y0 = LIST_Y0 + self.top;
        let band_y1 = band_y0 + self.view_h();
        self.layout_slots()
            .into_iter()
            .filter(|s| s.y + s.h > band_y0 && s.y < band_y1)
            .map(|mut s| {
                s.y -= self.top;
                s
            })
            .collect()
    }

    /// Абсолютная плашка бинда — по ней курсор держится в поле зрения.
    fn slot_of(&self, item: usize) -> Option<Slot> {
        self.layout_slots()
            .into_iter()
            .find(|s| s.item == Some(item))
    }

    /// Строк на страницу — по высоте окна.
    fn page_lines(&self) -> i32 {
        ((self.view_h() / (ITEM_H + ROW_GAP)).floor() as i32).max(1)
    }

    /// Предельная прокрутки: глубже последняя строка всё равно не видна.
    fn max_top(&self) -> f32 {
        (self.content_h() - self.height as f32 + 8.0).max(0.0)
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
        let in_column = |c: usize| -> Vec<usize> {
            let x = self.col_x(c);
            slots
                .iter()
                .filter(|s| s.x == x)
                .filter_map(|s| s.item)
                .collect()
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
        let Some(pos) = col.iter().position(|&i| Some(i) == cur.item) else {
            return;
        };
        let other = from as isize - dx as isize;
        if other >= 0
            && (other as usize) < COLS
            && let Some(&item) = in_column(other as usize).get(pos)
            && item != self.selected
        {
            self.select(item);
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
            self.pixel = bind_data::pixel_mode();
            self.font = bind_data::configured_font();
            self.dirty = true;
        } else if palette::load() != self.palette {
            self.palette = palette::load();
            self.dirty = true;
        }
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

    /// Какая строка под курсором. Заголовки не выбираются.
    fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        self.slots()
            .into_iter()
            .find(|s| y >= s.y && y <= s.y + s.h && x >= s.x && x <= s.x + s.w)
            .and_then(|s| s.item)
    }

    fn layout(&mut self, text: &str, size: f32, line_h: f32) -> (f32, Vec<(CacheKey, i32, i32)>) {
        let mut buffer = Buffer::new_empty(Metrics::new(size * SCALE, line_h * SCALE));
        let attrs = Attrs::new().family(Family::Name(&self.font));
        let mut width: f32 = 0.0;
        let mut glyphs = Vec::new();
        {
            let mut b = buffer.borrow_with(&mut self.font_system);
            b.set_text(text, &attrs, Shaping::Advanced, None);
            b.set_size(None, None);
            b.shape_until_scroll(false);
            for run in b.layout_runs() {
                width = width.max(run.line_w);
                for glyph in run.glyphs {
                    let physical = glyph.physical((0.0, run.line_y), 1.0);
                    glyphs.push((physical.cache_key, physical.x, physical.y));
                }
            }
        }
        (width, glyphs)
    }

    fn text_width(&mut self, text: &str, size: f32) -> f32 {
        self.layout(text, size, size + 8.0).0
    }

    /// Дельта колеса в целых строках. Тачпад шлёт value120 меньше 120, поэтому
    /// дробный остаток копим и отдаём по целой строке.
    fn axis_step(&mut self, v: &AxisScroll) -> i32 {
        let delta = if v.value120 != 0 {
            v.value120 as f32 / 120.0
        } else if v.discrete != 0 {
            v.discrete as f32
        } else if v.absolute != 0.0 {
            v.absolute.signum() as f32 * 3.0
        } else {
            0.0
        };
        self.scroll_acc += delta;
        let steps = self.scroll_acc.trunc();
        self.scroll_acc -= steps;
        steps as i32
    }

    /// Фактический верх глифов относительно переданного y (logical px).
    fn visual_top(&mut self, size: f32, line_h: f32) -> f32 {
        let key = (
            self.font.clone(),
            (size * 256.0) as u32,
            (line_h * 256.0) as u32,
        );
        if let Some(&v) = self.top_cache.get(&key) {
            return v;
        }
        let mut buffer = Buffer::new_empty(Metrics::new(size * SCALE, line_h * SCALE));
        let attrs = Attrs::new().family(Family::Name(&self.font));
        let mut probes = Vec::new();
        {
            let mut b = buffer.borrow_with(&mut self.font_system);
            b.set_text("AQБДЖ019+/", &attrs, Shaping::Advanced, None);
            b.set_size(None, None);
            b.shape_until_scroll(false);
            for run in b.layout_runs() {
                for glyph in run.glyphs {
                    let physical = glyph.physical((0.0, run.line_y), 1.0);
                    probes.push((physical.cache_key, physical.x, physical.y));
                }
            }
        }
        let mut top = f32::MAX;
        for (ck, _, gy) in probes {
            if let Some(image) = self.swash.get_image_uncached(&mut self.font_system, ck)
                && matches!(image.content, SwashContent::Mask)
            {
                top = top.min(gy as f32 - image.placement.top as f32);
            }
        }
        let top = if top == f32::MAX { 0.0 } else { top / SCALE };
        self.top_cache.insert(key, top);
        top
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        text: &str,
        size: f32,
        color: palette::Rgba,
        x: f32,
        y: f32,
        line_h: f32,
    ) {
        // y — визуальный верх текста, а не baseline первой строки.
        let y0 = y - self.visual_top(size, line_h);
        let (_, glyphs) = self.layout(text, size, line_h);
        for (key, gx, gy) in glyphs {
            if let Some(image) = self.swash.get_image_uncached(&mut self.font_system, key)
                && matches!(image.content, SwashContent::Mask)
            {
                blend(
                    pixmap,
                    x as i32 + gx,
                    y0 as i32 + gy - image.placement.top,
                    image.placement.width,
                    image.placement.height,
                    color,
                    &image.data,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Обрезает по ширине, добавляя «…». Ширину меряем делением пополам:
    /// перебор по одному символу шейпил строку десятки раз на каждую строку окна.
    fn draw_text_boxed(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        text: &str,
        size: f32,
        color: palette::Rgba,
        max_width: f32,
        x: f32,
        y: f32,
        line_h: f32,
    ) {
        let max_width_px = max_width * SCALE;
        let mut shown = text.to_string();
        if self.text_width(&shown, size) > max_width_px {
            let chars: Vec<char> = shown.chars().collect();
            let (mut lo, mut hi) = (0usize, chars.len());
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                let candidate: String = chars[..mid].iter().collect();
                let candidate = format!("{candidate}…");
                if self.text_width(&candidate, size) <= max_width_px {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            shown = chars[..lo].iter().collect();
            shown.push('…');
        }
        self.draw_text(pixmap, &shown, size, color, x, y, line_h);
    }

    fn draw(&mut self) {
        if !self.configured || self.width == 0 || self.height == 0 {
            return;
        }
        let pw = (self.width as f32 * SCALE) as u32;
        let ph = (self.height as f32 * SCALE) as u32;
        let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
            return;
        };
        let p = ui_palette(self.pixel, self.palette);
        let k = SCALE;
        let title_size = if self.pixel { 20.0 } else { 21.0 };
        let row_size = if self.pixel { 12.0 } else { 13.0 };
        let search_size = if self.pixel { 12.0 } else { 13.0 };
        pixmap.fill(p.base.to_tiny());
        fill_rect(&mut pixmap, 0.0, 0.0, pw as f32, 1.5, p.accent.with_a(0.7));

        self.draw_text(
            &mut pixmap,
            TITLE,
            title_size,
            p.text,
            PAD_X * k,
            20.0 * k,
            28.0,
        );
        let items = self.items.len();
        let count = if self.query.is_empty() {
            format!("{items}")
        } else {
            // Знаменатель — разделы keymap: блок «Частые» при поиске скрыт.
            let total: usize = strip_favorites(&self.groups)
                .iter()
                .map(|(_, e)| e.len())
                .sum();
            format!("{items} / {total}")
        };
        let cw = self.text_width(&count, row_size);
        self.draw_text(
            &mut pixmap,
            &count,
            row_size,
            p.muted,
            (self.width as f32 - PAD_X) * k - cw,
            26.0 * k,
            18.0,
        );

        let sy = 56.0;
        fill_round_rect(
            &mut pixmap,
            PAD_X * k,
            sy * k,
            (self.width as f32 - PAD_X * 2.0) * k,
            38.0 * k,
            if self.pixel { 0.0 } else { 8.0 * k },
            p.idle_panel,
        );
        if self.query.is_empty() {
            self.draw_text(
                &mut pixmap,
                PLACEHOLDER,
                search_size,
                p.muted,
                (PAD_X + 14.0) * k,
                (sy + 9.0) * k,
                20.0,
            );
        } else {
            let shown = format!("{}▍", self.query);
            self.draw_text_boxed(
                &mut pixmap,
                &shown,
                search_size,
                p.text,
                self.width as f32 - PAD_X * 2.0 - 28.0,
                (PAD_X + 14.0) * k,
                (sy + 9.0) * k,
                20.0,
            );
        }

        if self.items.is_empty() {
            self.draw_text(
                &mut pixmap,
                "Ничего не найдено",
                row_size,
                p.muted,
                PAD_X * k,
                LIST_Y0 * k,
                20.0,
            );
        }

        // Поток биндов газеткой: клавиша и описание, выделение заливкой.
        let slots = self.slots();
        for slot in &slots {
            if let ListRow::Header(label) = &self.rows[slot.row] {
                let label = label.clone();
                // Заголовок секции: полоса во всю ширину плюс акцентная метка
                // слева — без них блок сливается с общим списком.
                fill_rect(
                    &mut pixmap,
                    slot.x * k,
                    slot.y * k,
                    slot.w * k,
                    (slot.h - ROW_GAP) * k,
                    p.idle_panel,
                );
                fill_rect(
                    &mut pixmap,
                    slot.x * k,
                    (slot.y + 4.0) * k,
                    3.0 * k,
                    (slot.h - ROW_GAP - 8.0) * k,
                    p.accent,
                );
                self.draw_text(
                    &mut pixmap,
                    &label,
                    row_size,
                    p.accent,
                    (slot.x + MARK_W + 8.0) * k,
                    (slot.y + 5.0) * k,
                    18.0,
                );
                continue;
            }
            let Some(item) = slot.item else { continue };
            let Some(e) = self.items.get(item).cloned() else {
                continue;
            };
            let selected = item == self.selected;
            if selected {
                fill_rect(
                    &mut pixmap,
                    slot.x * k,
                    slot.y * k,
                    slot.w * k,
                    (slot.h - ROW_GAP) * k,
                    p.panel,
                );
                fill_rect(
                    &mut pixmap,
                    slot.x * k,
                    (slot.y + 3.0) * k,
                    MARK_W * k,
                    (slot.h - ROW_GAP - 6.0) * k,
                    p.accent,
                );
            }
            // Отступ постоянный: метка занимает своё место всегда, текст не прыгает.
            let text_x = slot.x + MARK_W + 8.0;
            self.draw_text_boxed(
                &mut pixmap,
                &e.key,
                row_size,
                p.accent,
                KEY_W,
                (text_x + 2.0) * k,
                (slot.y + 5.0) * k,
                18.0,
            );
            self.draw_text_boxed(
                &mut pixmap,
                &e.desc,
                row_size,
                p.text,
                slot.x + slot.w - (text_x + KEY_W + 12.0),
                (text_x + KEY_W + 4.0) * k,
                (slot.y + 5.0) * k,
                18.0,
            );
            // Низ блока секции: тонкая линия во всю ширину отделяет группу
            // от следующих биндов, иначе строки разных групп читаются как одна.
            if slot.section_end {
                let ly = slot.y + slot.h + (SECTION_GAP - ROW_GAP) / 2.0;
                fill_rect(
                    &mut pixmap,
                    PAD_X * k,
                    ly * k,
                    (self.width as f32 - PAD_X * 2.0) * k,
                    1.0 * k,
                    p.border.with_a(0.55),
                );
            }
        }

        // Скроллбар: полоса — доля видимого от высоты потока, бегунок — по прокрутке.
        if self.max_top() > 0.5 {
            let track_h = self.view_h();
            let content = (self.content_h() - LIST_Y0).max(track_h);
            let range = self.max_top().max(1.0);
            let shown = (track_h / content).clamp(0.08, 1.0);
            let th = track_h * shown;
            let ty = LIST_Y0 + self.top / range * (track_h - th);
            let bx = self.width as f32 - 6.0;
            fill_rect(
                &mut pixmap,
                bx * k,
                LIST_Y0 * k,
                2.0 * k,
                track_h * k,
                p.border,
            );
            fill_rect(&mut pixmap, bx * k, ty * k, 2.0 * k, th * k, p.accent);
        }

        let stride = (pw * 4) as i32;
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
        else {
            // Пул занят — композитор ещё не отдал предыдущий буфер.
            self.dirty = true;
            return;
        };
        let (dst, _) = canvas.as_chunks_mut::<4>();
        let (src, _) = pixmap.data().as_chunks::<4>();
        for (out, input) in dst.iter_mut().zip(src.iter()) {
            *out = [input[2], input[1], input[0], input[3]];
        }
        let surface = self.window.wl_surface();
        surface.set_buffer_scale(SCALE as i32);
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
                // Наведение мыши не меняет выбранный бинд: подсветка остаётся
                // под клавиатурным курсором и не прыгает от движения мыши.
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {}
                PointerEventKind::Leave { .. } => {}
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
        _: KeyEvent,
    ) {
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

fn fill_rect(pixmap: &mut tiny_skia::Pixmap, x: f32, y: f32, w: f32, h: f32, color: palette::Rgba) {
    let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) else {
        return;
    };
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Shader::SolidColor(color.to_tiny()),
        anti_alias: false,
        ..Default::default()
    };
    pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
}

fn fill_round_rect(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    color: palette::Rgba,
) {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut path = tiny_skia::PathBuilder::new();
    path.move_to(x + r, y);
    path.line_to(x + w - r, y);
    path.quad_to(x + w, y, x + w, y + r);
    path.line_to(x + w, y + h - r);
    path.quad_to(x + w, y + h, x + w - r, y + h);
    path.line_to(x + r, y + h);
    path.quad_to(x, y + h, x, y + h - r);
    path.line_to(x, y + r);
    path.quad_to(x, y, x + r, y);
    path.close();
    if let Some(path) = path.finish() {
        let paint = tiny_skia::Paint {
            shader: tiny_skia::Shader::SolidColor(color.to_tiny()),
            anti_alias: true,
            ..Default::default()
        };
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }
}

fn blend(
    pixmap: &mut tiny_skia::Pixmap,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: palette::Rgba,
    mask: &[u8],
) {
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let pixels = pixmap.pixels_mut();
    for row in 0..h as i32 {
        for col in 0..w as i32 {
            let xx = x + col;
            let yy = y + row;
            if xx < 0 || yy < 0 || xx >= pw || yy >= ph {
                continue;
            }
            let alpha = mask[(row * w as i32 + col) as usize] as u32 * color.3 as u32 / 255;
            if alpha == 0 {
                continue;
            }
            let dst = &mut pixels[(yy * pw + xx) as usize];
            let inv = 255 - alpha;
            let nr = (color.0 as u32 * alpha / 255 + dst.red() as u32 * inv / 255).min(255) as u8;
            let ng = (color.1 as u32 * alpha / 255 + dst.green() as u32 * inv / 255).min(255) as u8;
            let nb = (color.2 as u32 * alpha / 255 + dst.blue() as u32 * inv / 255).min(255) as u8;
            let na = (alpha + dst.alpha() as u32 * inv / 255).min(255) as u8;
            if let Some(px) = tiny_skia::PremultipliedColorU8::from_rgba(nr, ng, nb, na) {
                *dst = px;
            }
        }
    }
}

fn main() {
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
    // Буфер 900x620 при scale 2 — это ~8.9 МБ; держим четыре, иначе при быстром
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
        font_system: FontSystem::new(),
        swash: SwashCache::new(),
        top_cache: HashMap::new(),
        font: bind_data::configured_font(),
        pixel: bind_data::pixel_mode(),
        query: String::new(),
        groups,
        items: Vec::new(),
        rows: Vec::new(),
        selected: 0,
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
        if let Some(guard) = event_queue.prepare_read() {
            let mut fds = [PollFd::new(conn.as_fd(), PollFlags::POLLIN)];
            let _ = poll(&mut fds, 250u16);
            if fds[0]
                .revents()
                .is_some_and(|flags| flags.intersects(PollFlags::POLLIN))
            {
                let _ = guard.read();
            } else {
                drop(guard);
            }
        }
        // Ошибка разговора с вейлендом — повод выйти, иначе цикл крутится вхолостую.
        if event_queue.dispatch_pending(&mut app).is_err() {
            break;
        }
        if app.dirty {
            app.dirty = false;
            app.draw();
        }
    }
}

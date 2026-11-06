//! Отрисовка окна приложений: карточка, шапка с поиском, строки с иконками
//! или сетка плиток, подвал.
//!
//! Геометрия и цвета взяты у карточки меню (`menu::view`), поэтому окно
//! выглядит частью того же HUD, а не отдельной программой: та же ширина, та же
//! высота шапки и подвала, тот же ход подсветки, тот же ползунок прокрутки.
//!
//! Отличие одно — вместо глифа Nerd Font настоящая иконка приложения. В сетке
//! подписей нет: имя выбранного приложения показывает подвал. Заглушка на месте
//! иконки — кружок: он означает «иконка ещё едет» и не выдаётся ни за букву, ни
//! за значок категории.

use super::super::menu::view::RowsHighlight;
use super::icons::{self, Loader};
use super::model::{GRID_COLS, MAX_TILE_ROWS, MIN_TILE_ROWS, Model, Row, ViewMode};

pub use super::super::canvas::{fill_rect, fill_round_rect, stroke_rect};
use super::super::geom::Rect;
/// Прозрачность карточки — общая с менем: правило одна, а две реализации
/// разъехались бы на пиксель.
pub use super::super::menu::view::apply_card_alpha;
use super::super::palette::{self, Rgba};
use super::super::settings::Language;
use super::super::settings_icons;
use super::super::text::{Align, SCALE, TextPainter};
use super::super::ui_tokens::{
    TypeScale, UiPalette, radii, radius, spacing, type_scale, ui_palette,
};

/// Ширина карточки: как у меню, окно и есть карточка.
pub const CARD_W: f32 = 440.0;
/// Шапка с названием и полем поиска.
pub const HEADER_H: f32 = 52.0;
/// Подвал с подсказками.
pub const FOOTER_H: f32 = 40.0;
/// Строка списка. Ровно как в меню: с иконкой она не выше, потому что высота
/// задана ритмом карточки, а не содержимым.
pub const ROW_H: f32 = 40.0;
/// Иконка приложения в строке.
pub const ICON: f32 = 26.0;
/// Плитка сетки: 4 колонки по 90 px плюс три зазора по 16 — ровно ширина
/// карточки без внутренних отступов.
pub const TILE: f32 = 90.0;
/// Зазор между плитками — тот же внутренний отступ карточки.
pub const TILE_GAP: f32 = spacing::LG;
/// Добавка к зазору перед рядом после разделителя секций: подпись «Все
/// приложения» не должна упираться в плитки сверху и снизу.
pub const SECTION_EXTRA: f32 = spacing::LG;
/// Иконка на плитке: крупнее строковой, но с полем под подсветку и булавку.
pub const TILE_ICON: f32 = 44.0;
/// Вертикальный отступ сетки от шапки и подвала.
pub const GRID_PAD_Y: f32 = spacing::LG;
/// Строк меньше минимума не рисуем: карточка не должна схлопываться под
/// короткий список.
pub const MIN_ROWS: usize = 5;
/// Больше десяти строк окно не показывает: список прокручивается.
pub const MAX_ROWS: usize = 10;
/// Поле поиска в шапке.
pub const SEARCH_H: f32 = 28.0;
pub const SEARCH_W: f32 = 168.0;
/// Внутренний отступ карточки.
pub const PAD: f32 = spacing::LG;
/// Отступ подсветки строки от края карточки.
pub const CARD_INSET: f32 = 1.0;
/// Ползунок прокрутки.
pub const SCROLLBAR_W: f32 = 2.0;
pub const SCROLLBAR_INSET: f32 = PAD / 2.0;
/// Прозрачность карточки: сквозь неё должен быть виден рабочий стол.
pub const CARD_ALPHA: f32 = 0.8;
/// Горизонтальный отступ между значками строки и текстом.
const GAP: f32 = spacing::MD;

/// Всё, что нужно для кадра.
pub struct AppsView<'a> {
    pub pixmap: &'a mut tiny_skia::Pixmap,
    /// Painter основного шрифта темы.
    pub painter: &'a mut TextPainter,
    /// Отдельная копия painter с Nerd Font — только для глифов.
    pub icons: &'a mut TextPainter,
    pub ui: UiPalette,
    pub scale: TypeScale,
}

/// Сколько строк рисуется при таком количестве строк в модели.
pub fn rows_shown(count: usize) -> usize {
    count.clamp(MIN_ROWS, MAX_ROWS)
}

/// Сколько рядов плиток рисуется при таком количестве приложений.
pub fn tile_rows_shown(count: usize) -> usize {
    count
        .div_ceil(GRID_COLS)
        .clamp(MIN_TILE_ROWS, MAX_TILE_ROWS)
}

/// Высота ряда плиток без добавки у разделителя.
fn grid_height(rows: usize) -> f32 {
    if rows == 0 {
        0.0
    } else {
        rows as f32 * TILE + (rows.saturating_sub(1)) as f32 * TILE_GAP
    }
}

/// Ряд сетки, перед которым стоит разделитель секций.
fn break_row(model: &Model) -> Option<usize> {
    model.tiles_break().map(|at| at / GRID_COLS)
}

/// Добавка к зазору перед видимым рядом `row` (ряд считается от `top`).
/// Разделитель, уехавший за верх окна, места в кадре не занимает.
fn section_extra(model: &Model, row: usize) -> f32 {
    let first = model.top() / GRID_COLS;
    match break_row(model) {
        Some(br) if br > first && br <= first + row => SECTION_EXTRA,
        _ => 0.0,
    }
}

/// Высота видимой сетки. Добавка у разделителя входит всегда, когда он есть:
/// иначе при прокрутке, уводящей подпись за верх, окно меняло бы высоту.
fn grid_height_in_window(model: &Model, rows: usize) -> f32 {
    let extra = if model.tiles_break().is_some() {
        SECTION_EXTRA
    } else {
        0.0
    };
    grid_height(rows) + extra
}

/// Высота карточки под текущий вид.
pub fn card_height(model: &Model) -> f32 {
    match model.view() {
        ViewMode::List => HEADER_H + rows_shown(model.visible().len()) as f32 * ROW_H + FOOTER_H,
        ViewMode::Tiles => {
            HEADER_H
                + GRID_PAD_Y
                + grid_height_in_window(model, tile_rows_shown(model.total()))
                + GRID_PAD_Y
                + FOOTER_H
        }
    }
}

/// Потолок высоты карточки: по нему считается пул буферов слоя.
pub fn max_card_height() -> f32 {
    let list = HEADER_H + MAX_ROWS as f32 * ROW_H + FOOTER_H;
    let tiles =
        HEADER_H + GRID_PAD_Y + grid_height(MAX_TILE_ROWS) + SECTION_EXTRA + GRID_PAD_Y + FOOTER_H;
    if tiles > list { tiles } else { list }
}

/// Размер слоя: окно равно карточке. Ширина постоянная, высота зависит от
/// списка — вход в поиск и смена вида меняют её.
pub fn layer_size(model: &Model) -> (u32, u32) {
    (CARD_W as u32, card_height(model).round() as u32)
}

/// Прямоугольник списка внутри карточки.
pub fn list_rect(card: Rect, model: &Model) -> Rect {
    match model.view() {
        ViewMode::List => Rect::new(
            card.x,
            card.y + HEADER_H,
            card.w,
            rows_shown(model.visible().len()) as f32 * ROW_H,
        ),
        ViewMode::Tiles => Rect::new(
            card.x + PAD,
            card.y + HEADER_H + GRID_PAD_Y,
            card.w - PAD * 2.0,
            grid_height_in_window(model, tile_rows_shown(model.total())),
        ),
    }
}

/// Прямоугольник плитки по её месту в видимом окне.
fn tile_rect(list: Rect, slot: usize, model: &Model) -> Rect {
    let row = slot / GRID_COLS;
    let col = slot % GRID_COLS;
    Rect::new(
        list.x + col as f32 * (TILE + TILE_GAP),
        list.y + row as f32 * (TILE + TILE_GAP) + section_extra(model, row),
        TILE,
        TILE,
    )
}

/// Элемент под точкой. Возвращает абсолютный индекс строки модели, а не
/// индекс видимого окна: с прокруткой это разные вещи, и клик по нижней строке
/// обязан выбрать именно её. В сетке зазор между плитками не попадает ни в одну.
pub fn row_at(model: &Model, card: Rect, x: f32, y: f32) -> Option<usize> {
    let list = list_rect(card, model);
    if !list.contains(x, y) {
        return None;
    }
    let slot = match model.view() {
        ViewMode::List => ((y - list.y) / ROW_H).floor() as usize,
        ViewMode::Tiles => {
            let step = TILE + TILE_GAP;
            let col = ((x - list.x) / step).floor() as usize;
            if col >= GRID_COLS {
                return None;
            }
            // Ряды не равномерны из-за разделителя секций: ищем ряд, в
            // который попал `y`. Зазор и подпись не принадлежат ни одной плите.
            let rows = tile_rows_shown(model.total());
            let mut found = None;
            for row in 0..rows {
                let top = list.y + row as f32 * step + section_extra(model, row);
                if y >= top && y < top + TILE {
                    found = Some(row);
                    break;
                }
            }
            let row = found?;
            let slot = row * GRID_COLS + col;
            let rect = tile_rect(list, slot, model);
            if x >= rect.right() {
                return None;
            }
            slot
        }
    };
    let index = model.top() + slot;
    // Заголовки секций и пустые клетки сетки — не приложения: клик по ним
    // ничего не выбирает.
    match model.rows().get(index) {
        Some(row) => model.app_at(row).map(|_| index),
        None => None,
    }
}

/// Прямоугольник поля поиска: прижат к правому краю шапки.
fn search_rect(header: Rect) -> Rect {
    Rect::new(
        header.right() - PAD - SEARCH_W,
        header.y + (HEADER_H - SEARCH_H) / 2.0,
        SEARCH_W,
        SEARCH_H,
    )
}

/// Полоса в центре прямоугольника по заданной высоте содержимого.
fn center_band(rect: Rect, height: f32) -> Rect {
    Rect::new(rect.x, rect.y + (rect.h - height) / 2.0, rect.w, height)
}

/// Подсветка выбранной строки: сплошная заливка акцентом — та же, что в меню.
fn highlight_band(view: &mut AppsView<'_>, rect: Rect) {
    let inner = Rect::new(
        rect.x + CARD_INSET,
        rect.y,
        rect.w - CARD_INSET * 2.0,
        rect.h,
    );
    fill_rect(
        view.pixmap,
        inner.x * SCALE,
        inner.y * SCALE,
        inner.w * SCALE,
        inner.h * SCALE,
        view.ui.accent,
    );
}

/// Рисует окно целиком и возвращает прямоугольник карточки.
///
/// `height` — текущая высота окна, а не расчётная высота карточки: во время
/// переключения вида и поиска слой едет к новой высоте, и подвал должен идти
/// вместе с ним, а не ждать последнего кадра. В покое значения совпадают.
///
/// `highlight` — скользящая полоса в строках экрана (`None`, пока подсветка не
/// начала ход), `hover` — строка под указателем: она подсвечивается, но не
/// выбирается, как и в меню.
pub fn render_window(
    view: &mut AppsView<'_>,
    model: &Model,
    loader: &Loader,
    highlight: RowsHighlight,
    height: f32,
) -> Rect {
    let frame = Rect::new(0.0, 0.0, CARD_W, height);
    card(view, frame);
    header(view, frame, model);
    match model.view() {
        ViewMode::List => rows_view(view, frame, model, loader, highlight),
        ViewMode::Tiles => tiles_view(view, frame, model, loader, highlight),
    }
    footer(view, frame, model);
    frame
}

/// Подложка карточки и рамка.
fn card(view: &mut AppsView<'_>, rect: Rect) {
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(radii::WINDOW) * SCALE,
        view.ui.panel,
    );
    stroke_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        view.ui.border,
        SCALE,
    );
}

/// Шапка: название окна слева, поле поиска справа.
fn header(view: &mut AppsView<'_>, card: Rect, model: &Model) {
    let rect = Rect::new(card.x, card.y, card.w, HEADER_H);
    let title = view.scale.section_title;
    let label = match model.lang() {
        Language::Ru => "Приложения",
        Language::En => "Applications",
    };
    view.painter.paint_boxed(
        view.pixmap,
        label,
        title,
        view.ui.text,
        center_band(
            Rect::new(rect.x + PAD, rect.y, rect.w / 2.0 - PAD, rect.h),
            title * 2.0,
        ),
        Align::Start,
    );
    search(view, rect, model);
}

/// Поле поиска: лупа, набранный текст с курсором и крестик очистки.
fn search(view: &mut AppsView<'_>, header: Rect, model: &Model) {
    let rect = search_rect(header);
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(radii::SM) * SCALE,
        view.ui.idle_panel,
    );
    stroke_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        view.ui.border_focus,
        SCALE,
    );
    view.icons.paint(
        view.pixmap,
        settings_icons::SEARCH,
        view.scale.caption,
        view.ui.muted,
        center_band(
            Rect::new(rect.x + spacing::SM, rect.y, 16.0, rect.h),
            view.scale.label * 2.0,
        ),
        Align::Center,
    );
    let clear = model.searching();
    let text_x = rect.x + spacing::SM + 16.0 + spacing::SM;
    let text_w = rect.w
        - (spacing::SM + 16.0 + spacing::SM)
        - spacing::SM
        - if clear { 16.0 + spacing::XS } else { 0.0 };
    let text_rect = center_band(
        Rect::new(text_x, rect.y, text_w.max(0.0), rect.h),
        view.scale.label * 2.0,
    );
    let query = model.query();
    if query.is_empty() {
        let placeholder = match model.lang() {
            Language::Ru => "Поиск",
            Language::En => "Search",
        };
        view.painter.paint_boxed(
            view.pixmap,
            placeholder,
            view.scale.caption,
            view.ui.text_disabled,
            text_rect,
            Align::Start,
        );
    } else {
        view.painter.paint_boxed(
            view.pixmap,
            query,
            view.scale.label,
            view.ui.text,
            text_rect,
            Align::Start,
        );
        // Курсор: окно работает только с клавиатуры, поэтому он всегда виден.
        let caret_x = text_x + view.painter.text_width(query, view.scale.label);
        fill_rect(
            view.pixmap,
            (caret_x + 1.0) * SCALE,
            (rect.y + spacing::SM) * SCALE,
            2.0 * SCALE,
            (rect.h - spacing::SM * 2.0) * SCALE,
            view.ui.accent,
        );
    }
    if clear {
        view.icons.paint(
            view.pixmap,
            settings_icons::TIMES,
            view.scale.caption,
            view.ui.muted,
            center_band(
                Rect::new(rect.right() - spacing::SM - 16.0, rect.y, 16.0, rect.h),
                view.scale.label * 2.0,
            ),
            Align::Center,
        );
    }
}

/// Список строк: заголовки секций и приложения.
fn rows_view(
    view: &mut AppsView<'_>,
    card: Rect,
    model: &Model,
    loader: &Loader,
    highlight: RowsHighlight,
) {
    let visible = model.visible();
    let rows = rows_shown(visible.len());
    let list = list_rect(card, model);
    let selected = model.selected();
    if let Some(position) = highlight.sliding
        && !visible.is_empty()
    {
        let slot = position.clamp(0.0, visible.len() as f32 - 1.0);
        highlight_band(
            view,
            Rect::new(list.x, list.y + slot * ROW_H, list.w, ROW_H),
        );
    }
    for (slot, row) in visible.iter().enumerate() {
        let rect = Rect::new(list.x, list.y + slot as f32 * ROW_H, list.w, ROW_H);
        let absolute = model.top() + slot;
        match row {
            Row::Section(title) => section_row(view, rect, title),
            Row::Blank => {}
            Row::App(_) => {
                let Some(app) = model.app_at(row) else {
                    continue;
                };
                let is_selected = absolute == selected;
                let near = match highlight.sliding {
                    Some(position) => (1.0 - (position - slot as f32).abs()).clamp(0.0, 1.0),
                    None => usize::from(is_selected) as f32,
                };
                app_row(
                    view,
                    rect,
                    app,
                    loader,
                    show_pin(model, absolute, is_selected),
                    is_selected && highlight.sliding.is_none(),
                    near,
                    !is_selected && absolute == highlight.hover.unwrap_or(usize::MAX),
                );
            }
        }
    }
    if let Some(track) = scrollbar_rect(
        list,
        card.right(),
        model.total(),
        model.top(),
        visible.len(),
    ) {
        fill_rect(
            view.pixmap,
            track.x * SCALE,
            track.y * SCALE,
            track.w * SCALE,
            track.h * SCALE,
            view.ui.muted,
        );
    }
    if model.total() == 0 {
        let notice = match model.lang() {
            Language::Ru => "Ничего не найдено",
            Language::En => "Nothing found",
        };
        view.painter.paint_boxed(
            view.pixmap,
            notice,
            view.scale.label,
            view.ui.muted,
            Rect::new(list.x, list.y, list.w, rows as f32 * ROW_H),
            Align::Center,
        );
    }
}

/// Заголовок секции: подпись без иконки, выбрать её нельзя.
fn section_row(view: &mut AppsView<'_>, rect: Rect, title: &str) {
    view.painter.paint_boxed(
        view.pixmap,
        title,
        view.scale.caption,
        view.ui.muted,
        center_band(
            Rect::new(rect.x + PAD, rect.y, rect.w - PAD * 2.0, rect.h),
            view.scale.caption * 2.0,
        ),
        Align::Start,
    );
}

/// Сетка плиток: только иконки, закреплённые идут первыми, блоки разделены
/// подписью с линией.
fn tiles_view(
    view: &mut AppsView<'_>,
    card: Rect,
    model: &Model,
    loader: &Loader,
    highlight: RowsHighlight,
) {
    let list = list_rect(card, model);
    let visible = model.visible();
    let selected = model.selected();
    for (slot, row) in visible.iter().enumerate() {
        let Some(app) = model.app_at(row) else {
            continue;
        };
        let absolute = model.top() + slot;
        let is_selected = absolute == selected;
        let hover = !is_selected && absolute == highlight.hover.unwrap_or(usize::MAX);
        tile(
            view,
            tile_rect(list, slot, model),
            app,
            loader,
            model.is_pinned(&app.id),
            is_selected,
            hover,
        );
    }
    // Граница блоков: закреплённые кончаются на границе ряда, и разделитель
    // садится в существующий зазор, не раздвигая сетку.
    let scrollbar = scrollbar_rect(
        list,
        card.right(),
        model.total(),
        model.top(),
        visible.len(),
    );
    if let Some(at) = model.tiles_break() {
        let break_row = at / GRID_COLS;
        let top_row = model.top() / GRID_COLS;
        if break_row > top_row && break_row < top_row + tile_rows_shown(model.total()) {
            let row = break_row - top_row;
            let row_y = list.y + row as f32 * (TILE + TILE_GAP) + SECTION_EXTRA;
            let band = TILE_GAP + SECTION_EXTRA;
            tiles_divider(view, list, row_y - band / 2.0, band, model.lang());
        }
    }
    if let Some(track) = scrollbar {
        fill_rect(
            view.pixmap,
            track.x * SCALE,
            track.y * SCALE,
            track.w * SCALE,
            track.h * SCALE,
            view.ui.muted,
        );
    }
    if model.total() == 0 {
        let notice = match model.lang() {
            Language::Ru => "Ничего не найдено",
            Language::En => "Nothing found",
        };
        view.painter.paint_boxed(
            view.pixmap,
            notice,
            view.scale.label,
            view.ui.muted,
            Rect::new(list.x, list.y, list.w, list.h.max(TILE)),
            Align::Center,
        );
    }
}

/// Разделитель блоков сетки: подпись и тонкая линия в зазоре между рядами.
fn tiles_divider(view: &mut AppsView<'_>, list: Rect, y: f32, band: f32, lang: Language) {
    let label = match lang {
        Language::Ru => "Все приложения",
        Language::En => "All apps",
    };
    let size = view.scale.caption;
    let width = view.painter.text_width(label, size);
    view.painter.paint_boxed(
        view.pixmap,
        label,
        size,
        view.ui.muted,
        Rect::new(list.x, y - band / 2.0, width, band),
        Align::Start,
    );
    let x = list.x + width + spacing::SM;
    fill_rect(
        view.pixmap,
        x * SCALE,
        (y - 0.5) * SCALE,
        (list.right() - x) * SCALE,
        SCALE,
        view.ui.border,
    );
}

/// Плитка приложения: иконка на подложке, булавка у закреплённых.
fn tile(
    view: &mut AppsView<'_>,
    rect: Rect,
    app: &super::scan::App,
    loader: &Loader,
    pinned: bool,
    selected: bool,
    hover: bool,
) {
    let color = if selected {
        view.ui.accent
    } else if hover {
        view.ui.hover_bg
    } else {
        view.ui.idle_panel
    };
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(radii::MD) * SCALE,
        color,
    );
    let icon = Rect::new(
        rect.x + (TILE - TILE_ICON) / 2.0,
        rect.y + (TILE - TILE_ICON) / 2.0,
        TILE_ICON,
        TILE_ICON,
    );
    match loader.get(&app.icon) {
        Some(bitmap) => icons::blit(view.pixmap, &bitmap, icon.x, icon.y, TILE_ICON),
        None => placeholder(
            view,
            icon,
            if selected { 1.0 } else { 0.0 },
            loader.has_failed(&app.icon),
        ),
    }
    if pinned {
        view.icons.paint(
            view.pixmap,
            settings_icons::PIN,
            view.scale.caption,
            if selected {
                view.ui.on_primary
            } else {
                view.ui.muted
            },
            Rect::new(rect.right() - 18.0, rect.y + 4.0, 14.0, 14.0),
            Align::Center,
        );
    }
}

/// Рисовать ли булавку на строке.
/// Булавка рисуется у каждого закреплённого приложения всегда, включая секцию
/// «Закреплённые»: это единый признак закреплённости во всём списке.
fn show_pin(model: &Model, row: usize, _selected: bool) -> bool {
    let Some(Row::App(index)) = model.rows().get(row) else {
        return false;
    };
    let Some(app) = model.apps().get(*index) else {
        return false;
    };
    model.is_pinned(&app.id)
}

/// Строка приложения: иконка, имя и метка закрепления справа.
#[allow(clippy::too_many_arguments)]
fn app_row(
    view: &mut AppsView<'_>,
    rect: Rect,
    app: &super::scan::App,
    loader: &Loader,
    pinned: bool,
    selected: bool,
    near: f32,
    hover: bool,
) {
    // Подложка под указателем — та же заливка без смены цвета текста: мышь
    // показывает, где кликнёт, но не выбирает. Выбранная строка всегда
    // акцентом — даже под курсором и при едущей полосе.
    if selected {
        highlight_band(view, rect);
    } else if hover {
        let inner = Rect::new(
            rect.x + CARD_INSET,
            rect.y,
            rect.w - CARD_INSET * 2.0,
            rect.h,
        );
        fill_rect(
            view.pixmap,
            inner.x * SCALE,
            inner.y * SCALE,
            inner.w * SCALE,
            inner.h * SCALE,
            view.ui.hover_bg,
        );
    }
    // Цвет текста смешивается с близостью к едущей полосе: строка, которую полоса
    // сейчас проходит, проявляется постепенно. Выбранный текст — тёмный
    // `on_primary`: акцентом он не читался бы на заливке акцента.
    let near = near.clamp(0.0, 1.0);
    let text_color = blend(view.ui.text, view.ui.on_primary, near);
    let icon_rect = center_band(Rect::new(rect.x + PAD, rect.y, ICON, rect.h), ICON);
    let mark_w = if pinned { 16.0 + spacing::SM } else { 0.0 };
    let label = Rect::new(
        icon_rect.right() + GAP,
        rect.y,
        rect.right() - PAD - mark_w - (icon_rect.right() + GAP),
        rect.h,
    );
    match loader.get(&app.icon) {
        Some(icon) => icons::blit(view.pixmap, &icon, icon_rect.x, icon_rect.y, ICON),
        // Провал и ожидание рисуются по-разному: провал — плитка с глифом,
        // ожидание — просто плитка.
        None => placeholder(view, icon_rect, near, loader.has_failed(&app.icon)),
    }
    view.painter.paint_boxed(
        view.pixmap,
        &app.name,
        view.scale.label,
        text_color,
        center_band(label, view.scale.label * 2.0),
        Align::Start,
    );
    if pinned {
        view.icons.paint(
            view.pixmap,
            settings_icons::PIN,
            view.scale.caption,
            blend(view.ui.muted, view.ui.on_primary, near),
            center_band(
                Rect::new(rect.right() - PAD - 16.0, rect.y, 16.0, rect.h),
                view.scale.label * 2.0,
            ),
            Align::Center,
        );
    }
}

/// Потолок смешения заглушки с цветом текста на акценте.
///
/// Плитка идёт к тому же цвету, что и текст строки, но не дальше: на полном
/// смешении она становилась белой плиткой и читалась как значок, которого нет,
/// — то есть выдавала себя за то, чего не случилось.
const PLACEHOLDER_MIX: f32 = 0.35;

/// Потолок зафиксирован намертво: это требование из L1, а не вкусовое число, и
/// сдвинуть его вверх можно только вместе с новым снимком и новой проверкой
/// контраста.
const _: () = assert!(
    PLACEHOLDER_MIX <= 0.35,
    "заглушка не должна доходить до акцента"
);

/// Заглушка на месте иконки, которой нет.
///
/// Цвет — отдельная роль `ui.placeholder`: своя альфа и свой порог контраста
/// (3:1, это графика), потому что приглушённый текст на карточке даёт 4.5:1 и
/// на строке смотрелся бы слишком заметно. Проверка роли на всех модельных
/// схемах — в тестах `ui_tokens`.
fn placeholder(view: &mut AppsView<'_>, rect: Rect, near: f32, failed: bool) {
    // В Pixel полутонов быть не должно: смешение идёт ступенями, а плитка —
    // единственное, что на строке не пиксельное. Поэтому там смесь нулевая.
    let share = placeholder_share(near);
    let color = blend(view.ui.placeholder, view.ui.on_primary, share);
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(radii::SM) * SCALE,
        color,
    );
    // Глиф только у провала: он отвечает на вопрос «иконки не будет», а пока
    // иконка едет, пустая плитка честнее — значок ещё появится.
    if let Some(glyph) = placeholder_glyph(failed) {
        view.icons.paint(
            view.pixmap,
            glyph,
            view.scale.caption,
            blend(view.ui.muted, view.ui.on_primary, share),
            rect,
            Align::Center,
        );
    }
}

/// Глиф заглушки: только при провале загрузки.
///
/// Глиф «приложения» — тот же, что у раздела «Приложения» в меню, отдельный
/// значок не нужен: смысл у него один, а второй кодпойнт читался бы как
/// «другая программа».
pub fn placeholder_glyph(failed: bool) -> Option<&'static str> {
    failed.then_some(settings_icons::APPS)
}

/// Доля акцента в заглушке: не больше потолка.
pub fn placeholder_share(near: f32) -> f32 {
    near.clamp(0.0, 1.0) * PLACEHOLDER_MIX
}

/// Полоса прокрутки в поле карточки справа. Показывается только когда список
/// длиннее окна: до этого подвал со счётчиком и так говорит, сколько строк.
/// `right` — край карточки: в сетке полоса не должна лежать поверх последней
/// колонки плиток.
fn scrollbar_rect(
    list: Rect,
    right: f32,
    total: usize,
    top: usize,
    visible: usize,
) -> Option<Rect> {
    if total <= visible || visible == 0 {
        return None;
    }
    let thumb_h = (list.h * visible as f32 / total as f32).max(SCROLLBAR_W * 2.0);
    let travel = (list.h - thumb_h).max(0.0);
    let offset = travel * (top as f32 / (total - visible) as f32).clamp(0.0, 1.0);
    Some(Rect::new(
        right - SCROLLBAR_INSET - SCROLLBAR_W,
        list.y + offset,
        SCROLLBAR_W,
        thumb_h.min(list.h),
    ))
}

/// Подсказки подвала в трёх уровнях: полные, средние и одни клавиши. Набор
/// зависит от вида: в сетке место занимают стрелки по ряду и `Tab`, в списке —
/// закрепление.
///
/// Уровни те же, что у `menu::view::layout_hints`: берётся первый влезший, и
/// подсказки сокращаются, а не обрезаются многоточием — обрезанная подсказка
/// читается как «ещё что-то есть» и вводит в заблуждение.
pub fn hint_levels(mode: ViewMode, lang: Language) -> [&'static [&'static str]; 3] {
    match (mode, lang) {
        (ViewMode::List, Language::Ru) => [
            &[
                "↑↓ выбор",
                "Enter запуск",
                "Tab плитки",
                "Ctrl+F",
                "Esc закрыть",
            ],
            &["↑↓ выбор", "Enter", "Tab", "Ctrl+F", "Esc"],
            &["Tab", "Esc"],
        ],
        (ViewMode::List, Language::En) => [
            &[
                "↑↓ select",
                "Enter launch",
                "Tab tiles",
                "Ctrl+F",
                "Esc close",
            ],
            &["↑↓ select", "Enter", "Tab", "Ctrl+F", "Esc"],
            &["Tab", "Esc"],
        ],
        (ViewMode::Tiles, Language::Ru) => [
            &[
                "стрелки",
                "Enter запуск",
                "Ctrl+F",
                "Tab список",
                "Esc закрыть",
            ],
            &["стрелки", "Enter", "Ctrl+F", "Tab", "Esc"],
            &["Tab", "Esc"],
        ],
        (ViewMode::Tiles, Language::En) => [
            &["arrows", "Enter launch", "Ctrl+F", "Tab list", "Esc close"],
            &["arrows", "Enter", "Ctrl+F", "Tab", "Esc"],
            &["Tab", "Esc"],
        ],
    }
}

/// Промежуток между подсказками. Меньше, чем `LG` у меню: подсказок тут на
/// одну больше, а карточка та же, и на `LG` четвёртая строка не влезала ни в
/// каком виде.
const HINT_GAP: f32 = spacing::MD;

/// Сколько подвала отдаётся имени выбранной плитки. Длинное имя обрезается
/// многоточием: лучше узнать приложение по началу, чем остаться без подсказок.
const NAME_MAX_W: f32 = 144.0;

/// Ширина подсказок уровня: сумма текстов и промежутков между ними.
fn hints_width(view: &mut AppsView<'_>, hints: &[&str]) -> f32 {
    let mut width = 0.0;
    for (index, hint) in hints.iter().enumerate() {
        if index > 0 {
            width += HINT_GAP;
        }
        width += view.painter.text_width(hint, view.scale.caption);
    }
    width
}

/// Подсказки, которые влезают в отведённую ширину: уровень целиком, а если не
/// влезает даже последний — по одной, пока место есть.
fn fit_hints(
    view: &mut AppsView<'_>,
    available: f32,
    mode: ViewMode,
    lang: Language,
) -> Vec<&'static str> {
    let levels = hint_levels(mode, lang);
    for level in levels {
        if hints_width(view, level) <= available {
            return level.to_vec();
        }
    }
    let shortest = levels[2];
    let mut chosen: Vec<&'static str> = Vec::new();
    let mut used = 0.0;
    for hint in shortest {
        let width = view.painter.text_width(hint, view.scale.caption);
        let step = if chosen.is_empty() {
            width
        } else {
            width + spacing::LG
        };
        if used + step > available {
            break;
        }
        used += step;
        chosen.push(hint);
    }
    chosen
}

/// Подвал: подсказки слева, статус или счётчик справа.
fn footer(view: &mut AppsView<'_>, card: Rect, model: &Model) {
    let rect = Rect::new(card.x, card.bottom() - FOOTER_H, card.w, FOOTER_H);
    fill_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        SCALE,
        view.ui.border_subtle,
    );
    let band = center_band(rect, view.scale.caption * 2.0);
    // Правая зона резервируется по ширине, а не по факту нарисованному: иначе
    // подсказки прыгали бы влево в момент появления статуса.
    let right_zone = right_zone_width(view, model);
    let available = (rect.w - PAD * 2.0 - right_zone - spacing::LG).max(0.0);
    debug_assert!(
        right_zone <= rect.w,
        "правая зона не должна быть шире подвала"
    );
    let hints = fit_hints(view, available, model.view(), model.lang());
    let mut x = rect.x + PAD;
    for hint in hints {
        let width = view.painter.text_width(hint, view.scale.caption);
        view.painter.paint_boxed(
            view.pixmap,
            hint,
            view.scale.caption,
            view.ui.muted,
            Rect::new(x, band.y, width, band.h),
            Align::Start,
        );
        x += width + spacing::LG;
    }
    // Справа: результат действия; в сетке — имя выбранной плитки (иконки
    // подписаны не бывают), в списке — счётчик «выбрано n из m».
    let (text, is_status) = right_label(model);
    let color = if is_status {
        view.ui.accent
    } else if model.view() == ViewMode::Tiles {
        view.ui.text
    } else {
        view.ui.muted
    };
    let mut right = rect.right() - PAD;
    if !text.is_empty() {
        let width = right_text_width(view, model, &text, is_status);
        right -= width;
        view.painter.paint_boxed(
            view.pixmap,
            &text,
            view.scale.caption,
            color,
            Rect::new(right, band.y, width, band.h),
            Align::End,
        );
        right -= spacing::SM;
    }
    if is_status {
        // Точка состояния перед текстом: на ошибке запуска она читается
        // быстрее, чем сама строка.
        let dot = 6.0_f32;
        right -= dot + spacing::XS;
        fill_rect(
            view.pixmap,
            right * SCALE,
            (band.y + (band.h - dot) / 2.0) * SCALE,
            dot * SCALE,
            dot * SCALE,
            view.ui.accent,
        );
    }
}

/// Текст правой зоны подвала и признак «это статус»: у статуса есть точка.
fn right_label(model: &Model) -> (String, bool) {
    if let Some(status) = model.status() {
        return (status.to_string(), true);
    }
    match model.view() {
        ViewMode::Tiles => (
            model
                .selected_app()
                .map(|app| app.name.clone())
                .unwrap_or_default(),
            false,
        ),
        ViewMode::List => (counter(model), false),
    }
}

/// Зарезервированная ширина правой зоны подвала.
///
/// Резерв считается по образцам — счётчик и короткий статус, — а не по текущей
/// строке: иначе подсказки прыгали бы в момент появления статуса. Фактическая
/// ширина берётся не ниже резерва: длинное имя приложения в статусе сузит
/// место один раз, зато подсказки не наездут на текст и не обрезаются.
fn right_zone_width(view: &mut AppsView<'_>, model: &Model) -> f32 {
    const STATUS_DOT: f32 = 6.0;
    let lang = model.lang();
    let mut zone = 0.0_f32;
    // Образцы счётчика — для обычных списков. Тысяча приложений в списке не
    // бывает, а счётчик такой длины сузил бы подвал у всех.
    for total in [45usize, 100] {
        let sample = counter_text(lang, total, total);
        zone = zone.max(view.painter.text_width(&sample, view.scale.caption));
    }
    for sample in status_samples(lang) {
        zone = zone
            .max(view.painter.text_width(sample, view.scale.caption) + STATUS_DOT + spacing::XS);
    }
    zone.max(actual_right_width(view, model))
}

/// Образцы статуса: короткие слова без имени приложения. Их хватает, чтобы
/// подвал не прыгал на обычных действиях, а длинное имя сужает место один раз.
fn status_samples(lang: Language) -> [&'static str; 2] {
    match lang {
        Language::Ru => ["Закреплено", "Откреплено"],
        Language::En => ["Pinned", "Unpinned"],
    }
}

/// Ширина того, что на самом деле нарисовано справа: статус, имя плитки или
/// счётчик плюс точка состояния.
fn actual_right_width(view: &mut AppsView<'_>, model: &Model) -> f32 {
    let (text, is_status) = right_label(model);
    if text.is_empty() {
        return 0.0;
    }
    let dot = if is_status { 6.0 + spacing::XS } else { 0.0 };
    right_text_width(view, model, &text, is_status) + dot + spacing::SM
}

/// Ширина правого текста с учётом потолка для имени плитки.
fn right_text_width(view: &mut AppsView<'_>, model: &Model, text: &str, is_status: bool) -> f32 {
    let width = view.painter.text_width(text, view.scale.caption);
    if model.view() == ViewMode::Tiles && !is_status {
        width.min(NAME_MAX_W)
    } else {
        width
    }
}

/// «Выбрано n из m»: позиция среди приложений и их число. Заголовки секций в
/// счёт не идут, при пустом результате счётчика нет.
fn counter(model: &Model) -> String {
    if model.app_count() == 0 {
        return String::new();
    }
    match model.app_position(model.selected()) {
        Some(position) => counter_text(model.lang(), position, model.app_count()),
        None => String::new(),
    }
}

/// Текст счётчика: язык из настроек, числа — позиция и всего. Отдельная
/// функция, потому что подвалу нужен ещё и образец заведомо длинного текста.
pub fn counter_text(lang: Language, position: usize, total: usize) -> String {
    match lang {
        Language::Ru => format!("выбрано {position} из {total}"),
        Language::En => format!("selected {position} of {total}"),
    }
}

/// Смешивание цвета строки по близости к едущей полосе. Отдельная функция, а не
/// `ui_tokens::mix`: та выставляет альфу в 255, а цвета текста в списке
/// полупрозрачны.
fn blend(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let channel = |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * t) as u8;
    Rgba(
        channel(a.0, b.0),
        channel(a.1, b.1),
        channel(a.2, b.2),
        channel(a.3, b.3),
    )
}

/// Палитра и шрифт окна под текущую тему.
pub fn theme(base: palette::Palette) -> (UiPalette, TypeScale) {
    (ui_palette(base), type_scale())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::model::{Model, Row};
    use crate::apps::scan::App;
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

    fn model(apps: Vec<App>, pinned: Vec<String>) -> Model {
        Model::from_apps(apps, pinned, MAX_ROWS, Language::Ru)
    }

    fn many(count: usize) -> Vec<App> {
        (0..count)
            .map(|i| app(&format!("app{i}"), &format!("App {i}")))
            .collect()
    }

    /// Модель из двух секций и 45 приложений: 5 закреплённых плюс 40 прочих.
    fn two_sections() -> Model {
        let apps = many(45);
        let pinned = vec!["app0".to_string(), "app1".to_string(), "app2".to_string()];
        model(apps, pinned)
    }

    // ───────────────────────── счётчик ─────────────────────────

    /// Заголовки секций — не приложения, поэтому «выбрано 1 из 45» считает и
    /// позицию, и итог по приложениям, а не по строкам списка.
    #[test]
    fn the_counter_counts_apps_not_rows() {
        let mut model = two_sections();
        assert_eq!(model.app_count(), 45);

        model.select(model.first_app_row());
        assert_eq!(counter(&model), "выбрано 1 из 45");

        model.jump(true);
        assert_eq!(counter(&model), "выбрано 45 из 45");
    }

    /// При поиске секций нет, и счётчик считает только то, что нашлось.
    #[test]
    fn the_counter_follows_the_filtered_list() {
        let mut model = two_sections();
        for ch in "app4".chars() {
            model.type_char(ch);
        }
        // Под этот запрос подходят только приложения с цифрой 4 в id.
        let found = model.app_count();
        assert!(
            found > 0 && found < 45,
            "запрос что-то отфильтровал: {found}"
        );
        assert_eq!(counter(&model), format!("выбрано 1 из {found}"));
        model.jump(true);
        assert_eq!(counter(&model), format!("выбрано {found} из {found}"));
    }

    #[test]
    fn the_counter_speaks_both_languages() {
        assert_eq!(counter_text(Language::Ru, 7, 45), "выбрано 7 из 45");
        assert_eq!(counter_text(Language::En, 7, 45), "selected 7 of 45");
    }

    /// На заголовке секции счётчика нет: выбрать заголовок нельзя, а пустая
    /// строка в подвале читалась бы как сбой.
    #[test]
    fn an_unselectable_row_has_no_counter() {
        let model = two_sections();
        let header = model
            .rows()
            .iter()
            .position(|row| matches!(row, Row::Section(_)))
            .expect("заголовок есть");
        assert_eq!(model.app_position(header), None);
        assert_eq!(
            counter_text(Language::Ru, 1, model.app_count()),
            "выбрано 1 из 45"
        );
        let empty =
            super::super::model::Model::from_apps(Vec::new(), Vec::new(), MAX_ROWS, Language::Ru);
        assert!(counter(&empty).is_empty(), "пустой список — пустой счётчик");
    }

    // ───────────────────────── подвал ─────────────────────────

    /// Подсказки укладываются в карточку 440 px целиком, на обоих языках и в
    /// обоих видах: если нет — окно начало бы прятать половину подсказок по
    /// умолчанию.
    #[test]
    fn the_full_hint_level_fits_the_card_in_both_languages_and_views() {
        let mut view = view_for();
        for mode in [ViewMode::List, ViewMode::Tiles] {
            for lang in [Language::Ru, Language::En] {
                let levels = hint_levels(mode, lang);
                let full = hints_width(&mut view, levels[0]);
                assert!(
                    full <= CARD_W - PAD * 2.0,
                    "{mode:?}/{lang:?}: полный уровень не влезает: {full}"
                );
                let shown = fit_hints(&mut view, CARD_W - PAD * 2.0, mode, lang);
                assert_eq!(
                    shown,
                    levels[0].to_vec(),
                    "{mode:?}/{lang:?}: в карточку влезает полный уровень, а показан не он"
                );
            }
        }
    }

    /// Когда места мало, подсказки сокращаются до следующего уровня, а не
    /// обрезаются многоточием.
    #[test]
    fn the_hints_shrink_instead_of_being_cut() {
        let mut view = view_for();
        let levels = hint_levels(ViewMode::List, Language::Ru);
        let medium = hints_width(&mut view, levels[1]);
        let tight = medium + 1.0;
        assert_eq!(
            fit_hints(&mut view, tight, ViewMode::List, Language::Ru),
            levels[1].to_vec(),
            "на ширине между уровнями берётся средний"
        );

        // Уже не влезает даже самый короткий набор — берётся то, что влезло.
        let shortest = hints_width(&mut view, levels[2]);
        let shown = fit_hints(&mut view, shortest - 4.0, ViewMode::List, Language::Ru);
        assert_eq!(shown, vec!["Tab"]);
        assert!(
            hints_width(&mut view, &shown) <= shortest - 4.0,
            "подсказки не выходят за отведённую ширину"
        );
        // Совсем без места не рисуем ничего, но и не рисуем за краем карточки.
        assert!(fit_hints(&mut view, 0.0, ViewMode::List, Language::Ru).is_empty());
    }

    /// Правая зона зарезервирована по образцам — счётчику и короткому статусу, —
    /// иначе подсказки прыгали бы при появлении статуса. Длинное имя
    /// приложения в статусе место сужает один раз, но не наезжает.
    #[test]
    fn the_right_zone_is_reserved_for_the_samples() {
        let mut view = view_for();
        let model = two_sections();
        let zone = right_zone_width(&mut view, &model);
        for sample in ["выбрано 45 из 45", "выбрано 100 из 100"] {
            let width = view.painter.text_width(sample, view.scale.caption);
            assert!(
                zone >= width,
                "счётчик «{sample}» должен помещаться: {zone} < {width}"
            );
        }
        for sample in status_samples(Language::Ru) {
            let width = view.painter.text_width(sample, view.scale.caption) + 6.0 + spacing::XS;
            assert!(
                zone >= width,
                "статус «{sample}» должен помещаться: {zone} < {width}"
            );
        }
    }

    /// Длинный статус (имя приложения) сужает подвал один раз — иначе он наехал
    /// бы на подсказки или обрезал их.
    #[test]
    fn a_long_status_shrinks_the_hints_instead_of_overlapping() {
        let mut view = view_for();
        let mut model = two_sections();
        model.set_status("Откреплено: An Anime Game Launcher".to_string());
        let zone = right_zone_width(&mut view, &model);
        let status_width = view
            .painter
            .text_width("Откреплено: An Anime Game Launcher", view.scale.caption);
        assert!(
            zone >= status_width,
            "статус должен помещаться: {zone} < {status_width}"
        );
        let available = CARD_W - PAD * 2.0 - zone - spacing::LG;
        let shown = fit_hints(&mut view, available, model.view(), model.lang());
        assert!(
            hints_width(&mut view, &shown) <= available,
            "подсказки обязаны уместиться рядом с длинным статусом"
        );
    }

    /// Обычное состояние: полный набор не влезает, средний — да, и именно он
    /// показывается. Переключение вида и закрепление на месте.
    #[test]
    fn the_footer_shows_the_medium_hints_at_card_width() {
        let mut view = view_for();
        let model = two_sections();
        let zone = right_zone_width(&mut view, &model);
        let available = CARD_W - PAD * 2.0 - zone - spacing::LG;
        let shown = fit_hints(&mut view, available, model.view(), model.lang());
        let levels = hint_levels(ViewMode::List, Language::Ru);
        assert_eq!(
            shown, levels[1],
            "средний уровень должен влезать при 440 px"
        );
        assert!(shown.contains(&"↑↓ выбор"));
        assert!(shown.contains(&"Ctrl+F"));
        assert!(shown.contains(&"Tab"));
        assert!(
            hints_width(&mut view, levels[0]) > available,
            "полный уровень на карточке не помещается — иначе проверка выше врала бы"
        );
    }

    /// Длинное имя плитки в подвале не съедает подсказки целиком: оно
    /// обрезается, а Ctrl+F и Tab остаются на месте.
    #[test]
    fn a_long_tile_name_still_leaves_the_hints() {
        let mut view = view_for();
        let mut model = model(vec![app("a", "AppImageLauncher Settings")], Vec::new());
        model.toggle_view();
        let zone = right_zone_width(&mut view, &model);
        let available = CARD_W - PAD * 2.0 - zone - spacing::LG;
        let shown = fit_hints(&mut view, available, model.view(), model.lang());
        assert!(
            shown.contains(&"Ctrl+F") && shown.contains(&"Tab"),
            "подсказки схлопнулись из-за имени: {shown:?}"
        );
    }

    // ───────────────────────── заглушка ─────────────────────────

    /// Смещение к акценту ограничено потолком: на полном смешении плитка
    /// становилась белой и читалась как значок, которого нет.
    #[test]
    fn the_placeholder_never_reaches_the_accent() {
        assert_eq!(placeholder_share(1.0), PLACEHOLDER_MIX);
        assert_eq!(placeholder_share(0.5), 0.175);
        assert_eq!(placeholder_share(0.0), 0.0);
        assert_eq!(
            placeholder_share(2.0),
            0.35,
            "смешение не выходит за потолок"
        );
    }

    /// Провал и ожидание рисуются по-разному: глиф появляется только когда
    /// иконки уже не будет.
    #[test]
    fn the_failed_placeholder_carries_a_glyph_and_the_pending_one_does_not() {
        assert_eq!(placeholder_glyph(true), Some(settings_icons::APPS));
        assert_eq!(placeholder_glyph(false), None);
    }

    /// Заглушка — графика, а не текст: порог 3:1, а не 4.5:1, и проверка роли
    /// идёт по всем модельным схемам в тестах `ui_tokens`.
    #[test]
    fn the_placeholder_keeps_the_graphic_threshold_on_the_default_scheme() {
        let ui = ui_palette(palette::Palette::default());
        let ratio = crate::ui_tokens::contrast(ui.placeholder, ui.panel);
        assert!(ratio >= 3.0, "заглушка к карточке: {ratio}");
    }

    // ───────────────────────── булавка ─────────────────────────

    /// Закреплённое приложение носит булавку везде, включая свою секцию:
    /// это единый признак закреплённости во всём списке.
    #[test]
    fn the_pin_shows_inside_the_pinned_section() {
        let mut model = two_sections();
        let pinned_row = model
            .rows()
            .iter()
            .position(|row| matches!(row, Row::App(_)))
            .expect("первое приложение");
        assert!(model.in_pinned_section(pinned_row));
        model.select(pinned_row);
        assert!(
            show_pin(&model, pinned_row, false),
            "в своей секции булавка тоже рисуется"
        );
        assert!(
            show_pin(&model, pinned_row, true),
            "на выбранной строке булавка остаётся"
        );
    }

    /// Незакреплённые строки булавку не получают нигде — ни обычные, ни
    /// выбранные. В списке с секциями закреплённое приложение живёт только в
    /// своей секции, поэтому здесь проверяется обратное: ни одна строка второй
    /// секции булавку не получает. Случай «закреплённая строка вне своей
    /// секции» — это поиск, он разобран следующим тестом.
    #[test]
    fn unpinned_rows_carry_no_pin() {
        let model = two_sections();
        let outside: Vec<usize> = model
            .rows()
            .iter()
            .enumerate()
            .filter(|(index, row)| matches!(row, Row::App(_)) && !model.in_pinned_section(*index))
            .map(|(index, _)| index)
            .collect();
        assert!(!outside.is_empty(), "вторая секция должна быть непустой");
        for row in outside {
            assert!(
                !show_pin(&model, row, false),
                "строка {row} вне закреплённых не должна носить булавку"
            );
            assert!(
                !show_pin(&model, row, true),
                "и на выбранной такой строке булавки нет: она не закреплена"
            );
        }
    }

    /// При поиске секций нет вовсе: любая закреплённая строка показывает
    /// булавку.
    #[test]
    fn the_pin_shows_while_searching() {
        let mut model = two_sections();
        for ch in "App".chars() {
            model.type_char(ch);
        }
        assert!(model.app_count() > 0);
        let pinned = model
            .rows()
            .iter()
            .position(|row| {
                let Row::App(index) = row else {
                    return false;
                };
                model.apps()[*index].id == "app0"
            })
            .expect("закреплённое среди результатов");
        assert!(
            !model.in_pinned_section(pinned),
            "при поиске секций нет вовсе"
        );
        assert!(show_pin(&model, pinned, false));
    }

    /// На заголовке секции булавки быть не может: строки-приложения там нет.
    #[test]
    fn a_section_header_has_no_pin() {
        let model = two_sections();
        let header = model
            .rows()
            .iter()
            .position(|row| matches!(row, Row::Section(_)))
            .expect("заголовок есть");
        assert!(!show_pin(&model, header, true));
    }

    // ───────────────────────── геометрия ─────────────────────────

    #[test]
    fn the_card_grows_with_the_list_but_no_further_than_the_window() {
        assert_eq!(
            rows_shown(1),
            MIN_ROWS,
            "короткий список не схлопывает карточку"
        );
        assert_eq!(rows_shown(MAX_ROWS), MAX_ROWS);
        assert_eq!(rows_shown(100), MAX_ROWS, "длинный список прокручивается");
        assert_eq!(tile_rows_shown(1), MIN_TILE_ROWS);
        assert_eq!(tile_rows_shown(100), MAX_TILE_ROWS);
        assert!(
            card_height(&model(many(100), Vec::new())) > card_height(&model(many(1), Vec::new()))
        );
    }

    #[test]
    fn the_layer_size_matches_the_card() {
        let short = model(many(1), Vec::new());
        let (width, height) = layer_size(&short);
        assert_eq!(width, CARD_W as u32);
        assert_eq!(height, card_height(&short).round() as u32);
        let tall = model(many(100), Vec::new());
        let (_, tall) = layer_size(&tall);
        assert!(tall > height);
    }

    #[test]
    fn the_search_field_is_pinned_to_the_right_of_the_header() {
        let model = model(many(1), Vec::new());
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));
        let field = search_rect(Rect::new(card.x, card.y, card.w, HEADER_H));
        assert_eq!(field.right(), card.right() - PAD);
        assert!(field.w <= SEARCH_W);
        assert!(field.y >= card.y && field.bottom() <= card.y + HEADER_H);
    }

    /// Клик должен выбирать строку окна, а не строку списка: при прокрутке это
    /// разные индексы, и ошибка здесь означала бы запуск не того приложения.
    #[test]
    fn a_click_picks_the_row_under_the_pointer() {
        let mut model = model(many(20), Vec::new());
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));

        let list = list_rect(card, &model);
        let hit = row_at(&model, card, 10.0, list.y + ROW_H * 1.5).expect("строка");
        assert_eq!(hit, 1);

        model.select(5);
        assert_eq!(model.top(), 0);
        model.select(12);
        assert_eq!(model.top(), 3);
        let hit = row_at(&model, card, 10.0, list.y + 2.0).expect("строка");
        assert_eq!(hit, 3, "первая видимая строка — это top");

        assert!(row_at(&model, card, 10.0, card.bottom() - 5.0).is_none());
        assert!(row_at(&model, card, 10.0, 2.0).is_none(), "клик в шапку");
    }

    #[test]
    fn a_click_below_the_last_row_hits_nothing() {
        let model = model(vec![app("a", "A")], Vec::new());
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));
        let list = list_rect(card, &model);
        assert!(row_at(&model, card, 10.0, list.y + ROW_H * 3.5).is_none());
    }

    /// Четыре плитки и три зазора должны ровно заполнить ширину карточки без
    /// отступов: иначе сетка съедет вправо или не влезет в последнюю колонку.
    #[test]
    fn four_tiles_fit_the_card_exactly() {
        let row = GRID_COLS as f32 * TILE + (GRID_COLS - 1) as f32 * TILE_GAP;
        assert_eq!(row, CARD_W - PAD * 2.0);
    }

    /// Клик по плитке выбирает её, клик в зазор — ничего: иначе запускалось бы
    /// приложение, по которому не попали.
    #[test]
    fn a_click_picks_the_tile_under_the_pointer() {
        let mut model = model(many(20), Vec::new());
        model.toggle_view();
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));
        let list = list_rect(card, &model);
        let step = TILE + TILE_GAP;

        let hit = row_at(&model, card, list.x + TILE / 2.0, list.y + TILE / 2.0).expect("плитка");
        assert_eq!(hit, 0);
        let hit = row_at(&model, card, list.x + step + 1.0, list.y + 1.0).expect("плитка");
        assert_eq!(hit, 1);
        let hit =
            row_at(&model, card, list.x + 3.0 * step + 1.0, list.y + step + 1.0).expect("плитка");
        assert_eq!(hit, GRID_COLS + 3);
        assert!(
            row_at(&model, card, list.x + TILE + 1.0, list.y + TILE / 2.0).is_none(),
            "зазор между плитками — не плитка"
        );
        assert!(row_at(&model, card, list.x + 1.0, 2.0).is_none(), "шапка");

        model.select(19);
        assert_eq!(model.top(), GRID_COLS);
        let hit = row_at(&model, card, list.x + 1.0, list.y + 1.0).expect("плитка");
        assert_eq!(hit, model.top(), "клик по первой видимой плите — это top");
    }

    /// Ползунок живёт в поле карточки справа и не ложится на последнюю колонку
    /// плиток.
    #[test]
    fn the_scrollbar_does_not_cover_the_tiles() {
        let mut model = model(many(40), Vec::new());
        model.toggle_view();
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));
        let list = list_rect(card, &model);
        let track = scrollbar_rect(
            list,
            card.right(),
            model.total(),
            model.top(),
            model.visible().len(),
        )
        .expect("список длиннее окна");
        assert!(track.x >= list.right(), "ползунок за сеткой");
    }

    /// Пустая клетка после ряда закреплённых не кликается: она не приложение,
    /// и запускать по ней нечего.
    #[test]
    fn a_click_on_the_padding_cell_does_nothing() {
        let pinned = vec!["app0".to_string(), "app1".to_string(), "app2".to_string()];
        let mut model = model(many(20), pinned);
        model.toggle_view();
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&model));
        let list = list_rect(card, &model);
        let x = list.x + 3.0 * (TILE + TILE_GAP) + TILE / 2.0;
        assert!(row_at(&model, card, x, list.y + TILE / 2.0).is_none());
    }

    /// Подпись «Все приложения» получает собственный зазор: ряд под ней
    /// отодвигается, а клик в этот зазор ничего не выбирает.
    #[test]
    fn the_section_label_gets_its_own_gap() {
        let pinned = vec!["app0".to_string(), "app1".to_string(), "app2".to_string()];
        let mut grid = model(many(20), pinned);
        grid.toggle_view();
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(&grid));
        let list = list_rect(card, &grid);
        let above = tile_rect(list, 0, &grid);
        let below = tile_rect(list, GRID_COLS, &grid);

        assert_eq!(
            below.y - above.y,
            TILE + TILE_GAP + SECTION_EXTRA,
            "под подписью должен быть дополнительный зазор"
        );
        assert!(
            row_at(&grid, card, list.x + TILE / 2.0, above.bottom() + 1.0).is_none(),
            "зазор с подписью — не плитка"
        );

        let mut flat = model(many(20), Vec::new());
        flat.toggle_view();
        assert_eq!(
            card_height(&grid),
            card_height(&flat) + SECTION_EXTRA,
            "разделитель удлиняет карточку ровно на свой зазор"
        );

        let height = card_height(&grid);
        grid.select(grid.total().saturating_sub(1));
        assert_eq!(
            card_height(&grid),
            height,
            "прокрутка не должна менять высоту окна"
        );
    }

    /// В сетке подпись выбранного приложения живёт в подвале: на плитках
    /// только иконки, и без подписи одинаковые значки не различить.
    #[test]
    fn the_tiles_footer_names_the_selected_app() {
        let mut model = model(vec![app("a", "Alpha"), app("b", "Beta")], Vec::new());
        model.toggle_view();
        model.select(1);
        assert_eq!(right_label(&model), ("Beta".to_string(), false));
        model.set_status("Закреплено: Beta".to_string());
        assert_eq!(right_label(&model), ("Закреплено: Beta".to_string(), true));

        // Имя плитки обрезается по потолку, статус — нет: длинный статус
        // сужает подсказки, но не теряет текст.
        let mut view = view_for();
        let long = "Очень длинное имя приложения, которое не влезет";
        assert_eq!(right_text_width(&mut view, &model, long, false), NAME_MAX_W);
        assert!(right_text_width(&mut view, &model, long, true) > NAME_MAX_W);
    }

    #[test]
    fn section_rows_do_not_break_the_row_math() {
        let mut model = model(vec![app("a", "A"), app("b", "B")], vec!["a".to_string()]);
        assert!(matches!(model.rows()[0], Row::Section(_)));
        assert_eq!(model.total(), 4);
        model.select(3);
        assert_eq!(model.selected(), 3);
    }

    #[test]
    fn blending_keeps_the_endpoints_exactly() {
        let a = Rgba(10, 20, 30, 40);
        let b = Rgba(200, 210, 220, 230);
        assert_eq!(blend(a, b, 0.0), a);
        assert_eq!(blend(a, b, 1.0), b);
        assert_eq!(blend(a, b, 0.5).3, 135, "альфа тоже смешивается");
    }

    /// Вид для измерения подсказок: окно не нужно, нужен только painter.
    fn view_for() -> AppsView<'static> {
        // `TextPainter` живёт в куче и переживает создание вида: владеть им
        // внутри структуры, которая возвращается, нельзя, поэтому вид берёт
        //painter по ссылке на «вытекшую» память — под тест это безопасно, но
        // написано явно.
        let painter: &'static mut TextPainter =
            Box::leak(Box::new(TextPainter::new(&crate::ui_tokens::font_name())));
        let icons: &'static mut TextPainter =
            Box::leak(Box::new(TextPainter::new(settings_icons::FONT)));
        let pixmap: &'static mut tiny_skia::Pixmap =
            Box::leak(Box::new(tiny_skia::Pixmap::new(1, 1).expect("буфер")));
        let (ui, scale) = theme(palette::Palette::default());
        AppsView {
            pixmap,
            painter,
            icons,
            ui,
            scale,
        }
    }
}

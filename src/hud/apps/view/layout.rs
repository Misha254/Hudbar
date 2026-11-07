use super::*;

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
pub(crate) const GAP: f32 = spacing::MD;

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
pub(crate) fn grid_height(rows: usize) -> f32 {
    if rows == 0 {
        0.0
    } else {
        rows as f32 * TILE + (rows.saturating_sub(1)) as f32 * TILE_GAP
    }
}

/// Ряд сетки, перед которым стоит разделитель секций.
pub(crate) fn break_row(model: &Model) -> Option<usize> {
    model.tiles_break().map(|at| at / GRID_COLS)
}

/// Добавка к зазору перед видимым рядом `row` (ряд считается от `top`).
/// Разделитель, уехавший за верх окна, места в кадре не занимает.
pub(crate) fn section_extra(model: &Model, row: usize) -> f32 {
    let first = model.top() / GRID_COLS;
    match break_row(model) {
        Some(br) if br > first && br <= first + row => SECTION_EXTRA,
        _ => 0.0,
    }
}

/// Высота видимой сетки. Добавка у разделителя входит всегда, когда он есть:
/// иначе при прокрутке, уводящей подпись за верх, окно меняло бы высоту.
pub(crate) fn grid_height_in_window(model: &Model, rows: usize) -> f32 {
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
pub(crate) fn tile_rect(list: Rect, slot: usize, model: &Model) -> Rect {
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
pub(crate) fn search_rect(header: Rect) -> Rect {
    Rect::new(
        header.right() - PAD - SEARCH_W,
        header.y + (HEADER_H - SEARCH_H) / 2.0,
        SEARCH_W,
        SEARCH_H,
    )
}

/// Полоса в центре прямоугольника по заданной высоте содержимого.
pub(crate) fn center_band(rect: Rect, height: f32) -> Rect {
    Rect::new(rect.x, rect.y + (rect.h - height) / 2.0, rect.w, height)
}

/// Полоса прокрутки в поле карточки справа. Показывается только когда список
/// длиннее окна: до этого подвал со счётчиком и так говорит, сколько строк.
/// `right` — край карточки: в сетке полоса не должна лежать поверх последней
/// колонки плиток.
pub(crate) fn scrollbar_rect(
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

/// Текст правой зоны подвала и признак «это статус»: у статуса есть точка.
pub(crate) fn right_label(model: &Model) -> (String, bool) {
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
pub(crate) fn right_zone_width(view: &mut AppsView<'_>, model: &Model) -> f32 {
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
pub(crate) fn status_samples(lang: Language) -> [&'static str; 2] {
    match lang {
        Language::Ru => ["Закреплено", "Откреплено"],
        Language::En => ["Pinned", "Unpinned"],
    }
}

/// Ширина того, что на самом деле нарисовано справа: статус, имя плитки или
/// счётчик плюс точка состояния.
pub(crate) fn actual_right_width(view: &mut AppsView<'_>, model: &Model) -> f32 {
    let (text, is_status) = right_label(model);
    if text.is_empty() {
        return 0.0;
    }
    let dot = if is_status { 6.0 + spacing::XS } else { 0.0 };
    right_text_width(view, model, &text, is_status) + dot + spacing::SM
}

/// Ширина правого текста с учётом потолка для имени плитки.
pub(crate) fn right_text_width(
    view: &mut AppsView<'_>,
    model: &Model,
    text: &str,
    is_status: bool,
) -> f32 {
    let width = view.painter.text_width(text, view.scale.caption);
    if model.view() == ViewMode::Tiles && !is_status {
        width.min(NAME_MAX_W)
    } else {
        width
    }
}

/// «Выбрано n из m»: позиция среди приложений и их число. Заголовки секций в
/// счёт не идут, при пустом результате счётчика нет.
pub(crate) fn counter(model: &Model) -> String {
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
pub(crate) fn blend(a: Rgba, b: Rgba, t: f32) -> Rgba {
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

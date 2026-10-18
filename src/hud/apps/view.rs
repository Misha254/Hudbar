//! Отрисовка окна приложений: карточка, шапка с поиском, строки с иконками,
//! подвал.
//!
//! Геометрия и цвета взяты у карточки меню (`menu::view`), поэтому окно
//! выглядит частью того же HUD, а не отдельной программой: та же ширина, та же
//! высота шапки и подвала, тот же ход подсветки, тот же ползунок прокрутки.
//!
//! Отличие одно — вместо глифа Nerd Font в строке настоящая иконка
//! приложения. Заглушка на её месте — кружок: он означает «иконка ещё едет»
//! и не выдаётся ни за букву, ни за значок категории.

use super::super::menu::view::RowsHighlight;
use super::icons::{self, Loader};
use super::model::{Model, Row};

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

/// Высота карточки по числу нарисованных строк.
pub fn card_height(rows: usize) -> f32 {
    HEADER_H + rows as f32 * ROW_H + FOOTER_H
}

/// Размер слоя: окно равно карточке. Ширина постоянная, высота зависит от
/// списка — вход в поиск и очистка меняют её.
pub fn layer_size(rows: usize) -> (u32, u32) {
    let shown = rows_shown(rows);
    (CARD_W as u32, card_height(shown).round() as u32)
}

/// Прямоугольник списка внутри карточки.
pub fn list_rect(card: Rect, rows: usize) -> Rect {
    Rect::new(card.x, card.y + HEADER_H, card.w, rows as f32 * ROW_H)
}

/// Строка под точкой. Возвращает абсолютный индекс строки модели, а не
/// индекс видимого окна: с прокруткой это разные вещи, и клик по нижней строке
/// обязан выбрать именно её.
pub fn row_at(model: &Model, card: Rect, rows: usize, x: f32, y: f32) -> Option<usize> {
    let list = list_rect(card, rows);
    if !list.contains(x, y) {
        return None;
    }
    let slot = ((y - list.y) / ROW_H).floor() as usize;
    let index = model.top() + slot;
    (index < model.total()).then_some(index)
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
/// `highlight` — скользящая полоса в строках экрана (`None`, пока подсветка не
/// начала ход), `hover` — строка под указателем: она подсвечивается, но не
/// выбирается, как и в меню.
pub fn render_window(
    view: &mut AppsView<'_>,
    model: &Model,
    loader: &Loader,
    highlight: RowsHighlight,
) -> Rect {
    let rows = rows_shown(model.visible().len());
    let frame = Rect::new(0.0, 0.0, CARD_W, card_height(rows));
    card(view, frame);
    header(view, frame, model);
    rows_view(view, frame, model, loader, rows, highlight);
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
        radius(radii::LG) * SCALE,
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
    rows: usize,
    highlight: RowsHighlight,
) {
    let visible = model.visible();
    let list = list_rect(card, rows);
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
    if let Some(track) = scrollbar_rect(list, model.total(), model.top()) {
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

/// Полоса прокрутки у правого края списка. Показывается только когда список
/// длиннее окна: до этого подвал со счётчиком и так говорит, сколько строк.
fn scrollbar_rect(list: Rect, total: usize, top: usize) -> Option<Rect> {
    let shown = total.min(MAX_ROWS);
    if total <= MAX_ROWS || shown == 0 {
        return None;
    }
    let thumb_h = (list.h * shown as f32 / total as f32).max(SCROLLBAR_W * 2.0);
    let travel = list.h - thumb_h;
    let offset = if total > shown {
        travel * (top as f32 / (total - shown) as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some(Rect::new(
        list.right() - SCROLLBAR_INSET - SCROLLBAR_W,
        list.y + offset,
        SCROLLBAR_W,
        thumb_h,
    ))
}

/// Подсказки подвала в трёх уровнях: полные, средние и одни клавиши.
///
/// Уровни те же, что у `menu::view::layout_hints`: берётся первый влезший, и
/// подсказки сокращаются, а не обрезаются многоточием — обрезанная подсказка
/// читается как «ещё что-то есть» и вводит в заблуждение.
pub fn hint_levels(lang: Language) -> [&'static [&'static str]; 3] {
    match lang {
        Language::Ru => [
            &[
                "↑↓ выбор",
                "Enter запуск",
                "Ctrl+P закрепить",
                "Esc закрыть",
            ],
            &["↑↓ выбор", "Enter запуск", "Ctrl+P", "Esc"],
            &["Ctrl+P", "Esc"],
        ],
        Language::En => [
            &["↑↓ select", "Enter launch", "Ctrl+P pin", "Esc close"],
            &["↑↓ select", "Enter launch", "Ctrl+P", "Esc"],
            &["Ctrl+P", "Esc"],
        ],
    }
}

/// Промежуток между подсказками. Меньше, чем `LG` у меню: подсказок тут на
/// одну больше, а карточка та же, и на `LG` четвёртая строка не влезала ни в
/// каком виде.
const HINT_GAP: f32 = spacing::MD;

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
fn fit_hints(view: &mut AppsView<'_>, available: f32, lang: Language) -> Vec<&'static str> {
    let levels = hint_levels(lang);
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
    let hints = fit_hints(view, available, model.lang());
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
    // Справа: результат действия, а пока его нет — счётчик «выбрано n из m».
    let status = model.status().unwrap_or_default();
    let counter = if status.is_empty() {
        counter(model)
    } else {
        String::new()
    };
    let mut right = rect.right() - PAD;
    for (text, color) in [(status, view.ui.accent), (counter.as_str(), view.ui.muted)] {
        if text.is_empty() {
            continue;
        }
        let width = view.painter.text_width(text, view.scale.caption);
        right -= width;
        view.painter.paint_boxed(
            view.pixmap,
            text,
            view.scale.caption,
            color,
            Rect::new(right, band.y, width, band.h),
            Align::End,
        );
        right -= spacing::SM;
    }
    if !status.is_empty() {
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

/// Ширина того, что на самом деле нарисовано справа: счётчик или статус плюс
/// точка состояния.
fn actual_right_width(view: &mut AppsView<'_>, model: &Model) -> f32 {
    let status = model.status().unwrap_or_default();
    let text = if status.is_empty() {
        counter(model)
    } else {
        status.to_string()
    };
    if text.is_empty() {
        return 0.0;
    }
    let dot = if status.is_empty() {
        0.0
    } else {
        6.0 + spacing::XS
    };
    view.painter.text_width(&text, view.scale.caption) + dot + spacing::SM
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

    /// Подсказки укладываются в карточку 440 px целиком, на обоих языках: если
    /// нет — окно начало бы прятать половину подсказок по умолчанию.
    #[test]
    fn the_full_hint_level_fits_the_card_in_both_languages() {
        let mut view = view_for();
        for lang in [Language::Ru, Language::En] {
            let levels = hint_levels(lang);
            let full = hints_width(&mut view, levels[0]);
            assert!(
                full <= CARD_W - PAD * 2.0,
                "{lang:?}: полный уровень не влезает: {full}"
            );
            let shown = fit_hints(&mut view, CARD_W - PAD * 2.0, lang);
            assert_eq!(
                shown,
                levels[0].to_vec(),
                "{lang:?}: в карточку влезает полный уровень, а показан не он"
            );
        }
    }

    /// Когда места мало, подсказки сокращаются до следующего уровня, а не
    /// обрезаются многоточием.
    #[test]
    fn the_hints_shrink_instead_of_being_cut() {
        let mut view = view_for();
        let levels = hint_levels(Language::Ru);
        let medium = hints_width(&mut view, levels[1]);
        let tight = medium + 1.0;
        assert_eq!(
            fit_hints(&mut view, tight, Language::Ru),
            levels[1].to_vec(),
            "на ширине между уровнями берётся средний"
        );

        // Уже не влезает даже самый короткий набор — берётся то, что влезло.
        let shortest = hints_width(&mut view, levels[2]);
        let shown = fit_hints(&mut view, shortest - 4.0, Language::Ru);
        assert_eq!(shown, vec!["Ctrl+P"]);
        assert!(
            hints_width(&mut view, &shown) <= shortest - 4.0,
            "подсказки не выходят за отведённую ширину"
        );
        // Совсем без места не рисуем ничего, но и не рисуем за краем карточки.
        assert!(fit_hints(&mut view, 0.0, Language::Ru).is_empty());
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
        let shown = fit_hints(&mut view, available, model.lang());
        assert!(
            hints_width(&mut view, &shown) <= available,
            "подсказки обязаны уместиться рядом с длинным статусом"
        );
    }

    /// Обычное состояние: полный набор не влезает, средний — да, и именно он
    /// показывается. Обе новые подсказки на месте.
    #[test]
    fn the_footer_shows_the_new_hints_at_card_width() {
        let mut view = view_for();
        let model = two_sections();
        let zone = right_zone_width(&mut view, &model);
        let available = CARD_W - PAD * 2.0 - zone - spacing::LG;
        let shown = fit_hints(&mut view, available, model.lang());
        let levels = hint_levels(Language::Ru);
        assert_eq!(
            shown, levels[1],
            "средний уровень должен влезать при 440 px"
        );
        assert!(shown.contains(&"↑↓ выбор"));
        assert!(shown.contains(&"Enter запуск"));
        assert!(
            hints_width(&mut view, levels[0]) > available,
            "полный уровень на карточке не помещается — иначе проверка выше врала бы"
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
        assert!(card_height(MAX_ROWS) > card_height(MIN_ROWS));
    }

    #[test]
    fn the_layer_size_matches_the_card() {
        let (width, height) = layer_size(3);
        assert_eq!(width, CARD_W as u32);
        assert_eq!(height, card_height(MIN_ROWS) as u32);
        let (_, tall) = layer_size(MAX_ROWS);
        assert!(tall > height);
    }

    #[test]
    fn the_search_field_is_pinned_to_the_right_of_the_header() {
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(MAX_ROWS));
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
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(MAX_ROWS));
        let rows = MAX_ROWS;

        let list = list_rect(card, rows);
        let hit = row_at(&model, card, rows, 10.0, list.y + ROW_H * 1.5).expect("строка");
        assert_eq!(hit, 1);

        model.select(5);
        assert_eq!(model.top(), 0);
        model.select(12);
        assert_eq!(model.top(), 3);
        let hit = row_at(&model, card, rows, 10.0, list.y + 2.0).expect("строка");
        assert_eq!(hit, 3, "первая видимая строка — это top");

        assert!(row_at(&model, card, rows, 10.0, card.bottom() - 5.0).is_none());
        assert!(
            row_at(&model, card, rows, 10.0, 2.0).is_none(),
            "клик в шапку"
        );
    }

    #[test]
    fn a_click_below_the_last_row_hits_nothing() {
        let model = model(vec![app("a", "A")], Vec::new());
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(MIN_ROWS));
        let list = list_rect(card, MIN_ROWS);
        assert!(row_at(&model, card, MIN_ROWS, 10.0, list.y + ROW_H * 3.5).is_none());
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

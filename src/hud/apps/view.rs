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

use super::super::menu::view::{RowsHighlight, selected_background};
use super::icons::{self, Loader};
use super::model::{Model, Row};

/// Прозрачность карточки — общая с менем: правило одна, а две реализации
/// разъехались бы на пиксель.
pub use super::super::menu::view::apply_card_alpha;
use super::super::palette::{self, Rgba};
use super::super::settings::Language;
use super::super::settings_icons;
use super::super::settings_ui::Rect;
use super::super::settings_view::{fill_rect, fill_round_rect, stroke_rect};
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
/// Акцентная полоса слева у выбранной строки.
pub const SEL_BAR: f32 = 3.0;
/// Ползунок прокрутки.
pub const SCROLLBAR_W: f32 = 2.0;
pub const SCROLLBAR_INSET: f32 = PAD / 2.0;
/// Прозрачность карточки: сквозь неё должен быть виден рабочий стол.
pub const CARD_ALPHA: f32 = 0.9;
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
    pub pixel: bool,
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

/// Подсветка выбранной строки: заливка и акцентная полоса слева, обе от краёв
/// карточки.
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
        selected_background(view.ui),
    );
    fill_rect(
        view.pixmap,
        inner.x * SCALE,
        inner.y * SCALE,
        SEL_BAR * SCALE,
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
        radius(view.pixel, radii::LG) * SCALE,
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
        radius(view.pixel, radii::SM) * SCALE,
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
        if slot > 0 {
            fill_rect(
                view.pixmap,
                (rect.x + PAD) * SCALE,
                rect.y * SCALE,
                (rect.w - PAD * 2.0) * SCALE,
                SCALE,
                view.ui.border_subtle,
            );
        }
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
                    model.is_pinned(&app.id),
                    is_selected && highlight.sliding.is_none(),
                    near,
                    absolute == highlight.hover.unwrap_or(usize::MAX),
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
    if selected {
        highlight_band(view, rect);
    } else if hover {
        // Подложка под указателем — та же заливка без полосы и без смены цвета
        // текста: мышь показывает, где кликнёт, но не выбирает.
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
    // сейчас проходит, проявляется постепенно.
    let near = near.clamp(0.0, 1.0);
    let text_color = blend(view.ui.text, view.ui.accent, near);
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
        None => placeholder(view, icon_rect, near),
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
            blend(view.ui.muted, view.ui.accent, near),
            center_band(
                Rect::new(rect.right() - PAD - 16.0, rect.y, 16.0, rect.h),
                view.scale.label * 2.0,
            ),
            Align::Center,
        );
    }
}

/// Заглушка на месте иконки, которая ещё едет или не нашлась вовсе.
///
/// Цвет — смесь панели с приглушённым текстом, а не `picker_thumb`: тот
/// рассчитан на круг-кнопку и на строке читался бы как белая плитка, то есть
/// как значок, которого нет.
fn placeholder(view: &mut AppsView<'_>, rect: Rect, near: f32) {
    let base = super::super::ui_tokens::mix(view.ui.panel, view.ui.muted, 0.25);
    let color = blend(base, view.ui.accent, near);
    let radius = radius(view.pixel, radii::SM) * SCALE;
    fill_round_rect(
        view.pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius,
        color,
    );
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
    let hints: [&str; 2] = match model.lang() {
        Language::Ru => ["Ctrl+P закрепить", "Esc закрыть"],
        Language::En => ["Ctrl+P pin", "Esc close"],
    };
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
    // Справа: результат действия, а пока его нет — счётчик «выбрано из строк».
    // Статус отличается ещё и точкой: подвал читается быстрее, чем читают текст
    // ошибки запуска.
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

/// «Выбрано из строк», как в меню: при пустом результате счётчик не рисуется.
fn counter(model: &Model) -> String {
    if model.total() == 0 {
        return String::new();
    }
    let selected = model.selected();
    if selected == usize::MAX {
        return String::new();
    }
    format!("{} / {}", selected + 1, model.total())
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
pub fn theme(pixel: bool, base: palette::Palette) -> (UiPalette, TypeScale) {
    (ui_palette(pixel, base), type_scale(pixel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::model::{Model, Row};
    use std::path::PathBuf;

    fn app(id: &str, name: &str) -> crate::apps::scan::App {
        crate::apps::scan::App {
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

    fn model(apps: Vec<crate::apps::scan::App>, pinned: Vec<String>) -> Model {
        Model::from_apps(apps, pinned, MAX_ROWS, Language::Ru)
    }

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
        let apps: Vec<crate::apps::scan::App> = (0..20)
            .map(|i| app(&format!("app{i}"), &format!("App {i}")))
            .collect();
        let mut model = model(apps, Vec::new());
        let card = Rect::new(0.0, 0.0, CARD_W, card_height(MAX_ROWS));
        let rows = MAX_ROWS;

        // Витрина без прокрутки: вторая строка — индекс 1.
        let list = list_rect(card, rows);
        let hit = row_at(&model, card, rows, 10.0, list.y + ROW_H * 1.5).expect("строка");
        assert_eq!(hit, 1);

        // После прокрутки вниз витрина показывает строки 5..15, и клик по первой
        // видимой строке обязан вернуть 5, а не 0.
        model.select(5);
        assert_eq!(model.top(), 0);
        model.select(12);
        assert_eq!(model.top(), 3);
        let hit = row_at(&model, card, rows, 10.0, list.y + 2.0).expect("строка");
        assert_eq!(hit, 3, "первая видимая строка — это top");

        // Клик мимо списка — попадание в подвал, это не строка.
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
        // Пустые строки под единственным приложением не selectable.
        let hit = row_at(&model, card, MIN_ROWS, 10.0, list.y + ROW_H * 3.5);
        assert!(hit.is_none(), "под списком нечего выбирать");
    }

    #[test]
    fn the_counter_counts_rows_not_pages() {
        let apps: Vec<crate::apps::scan::App> = (0..7)
            .map(|i| app(&format!("app{i}"), &format!("App {i}")))
            .collect();
        let mut model = model(apps, Vec::new());
        model.select(3);
        assert_eq!(counter(&model), "4 / 7");
        model.jump(true);
        assert_eq!(counter(&model), "7 / 7");
        // Без выбора счётчик не рисуется: показывать «0 / 0» незачем.
        let empty = super::super::super::apps::model::Model::from_apps(
            Vec::new(),
            Vec::new(),
            MAX_ROWS,
            Language::Ru,
        );
        assert!(counter(&empty).is_empty());
    }

    #[test]
    fn section_rows_do_not_break_the_row_math() {
        // Секция — обычная строка по высоте, поэтому клик и прокрутка считаются
        // одним и тем же числом строк.
        let apps = vec![app("a", "A"), app("b", "B")];
        let mut model = model(apps, vec!["a".to_string()]);
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
}

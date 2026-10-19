//! Общий каркас карточки hudbar: фон со скруглением и hairline-рамкой, шапка
//! с заголовком и полем поиска, подвал с подсказками и счётчиком, скроллбар
//! и подсветка строк. Каркас кладётся в буфер произвольного размера: живое
//! окно биндов занимает им весь буфер, снимок — по центру сцены.
//!
//! Геометрия и цвета совпадают с главным меню (`menu/view/geometry.rs`,
//! `header.rs`, `footer.rs`). Зависимости от `menu::view` нет: окна биндов —
//! обычные xdg-окна, а не слои, lib-таргета у крейта нет, и бины собирают
//! общие модули через `#[path]`. Поэтому несколько маленьких утилит
//! продублированы здесь с точной ссылкой на оригинал; тела отрисовки меню не
//! копируются — всё рисуется общими примитивами `canvas`/`text`/`ui_tokens`.

use super::canvas::{fill_rect, fill_round_rect, stroke_rect};
use super::geom::Rect;
use super::palette;
use super::settings_icons;
use super::text::{Align, SCALE, TextPainter};
use super::ui_tokens::{radii, radius, spacing, type_scale, UiPalette};

// ─────────────────────────── Геометрия ────────────────────────────

/// Ширина карточки — `CARD_W` меню (geometry.rs:11).
pub const CARD_W: f32 = 440.0;
/// Шапка — `HEADER_H` меню (geometry.rs:26).
pub const HEADER_H: f32 = 52.0;
/// Подвал — `FOOTER_H` меню (geometry.rs:28).
pub const FOOTER_H: f32 = 40.0;
/// Строка списка меню в обычном режиме — `ROW_H` (geometry.rs:13). Окна
/// биндов компактнее: у них своя высота строки.
pub const ROW_H: f32 = 40.0;
/// Строка меню в режиме поиска — `ROW_H_SEARCH` (geometry.rs:15).
pub const ROW_H_SEARCH: f32 = 52.0;
/// Минимум и максимум строк карточки (geometry.rs:22,24).
pub const MIN_ROWS: usize = 5;
pub const MAX_ROWS: usize = 10;
/// Поле поиска (geometry.rs:30-32).
pub const SEARCH_W: f32 = 168.0;
pub const SEARCH_H: f32 = 28.0;
/// Квадрат иконки в поле поиска (geometry.rs:34).
pub const ICON: f32 = 20.0;
/// Бегунок и его отступ от правого края в меню (geometry.rs:55-57). Окна
/// биндов задают свой отступ через `ScrollbarStyle`.
pub const SCROLLBAR_W: f32 = 2.0;
pub const SCROLLBAR_INSET: f32 = 8.0;
/// Прозрачность карточки — `CARD_ALPHA` меню (geometry.rs:70).
pub const CARD_ALPHA: f32 = 0.8;
/// Сколько строк на корневом экране главного меню.
pub const ROOT_ROWS: usize = 9;
/// Высота корневой карточки меню: `card_height_for(ROOT_ROWS, false)` —
/// 52 шапка + 9×40 строк + 40 подвал.
pub const ROOT_CARD_H: f32 = HEADER_H + ROOT_ROWS as f32 * ROW_H + FOOTER_H;

/// Сколько строк показывает карточка: как `rows_shown` (geometry.rs:107-109).
pub fn rows_shown(count: usize) -> usize {
    count.clamp(MIN_ROWS, MAX_ROWS)
}

/// Высота строки: как `row_height` (geometry.rs:79-81).
pub fn row_height(search: bool) -> f32 {
    if search { ROW_H_SEARCH } else { ROW_H }
}

/// Высота карточки — формула `card_height_for` меню (geometry.rs:113-115).
pub fn card_height_for(rows: usize, search: bool) -> f32 {
    HEADER_H + rows_shown(rows) as f32 * row_height(search) + FOOTER_H
}

/// Высота карточки в обычном режиме — как `card_height` (geometry.rs:118-120).
pub fn card_height(rows: usize) -> f32 {
    card_height_for(rows, false)
}

/// Полоса, отцентрованная по вертикали внутри `rect` — как `center_band`
/// (geometry.rs:148-151). Единственный расчёт вертикального центра: шапка,
/// подвал и строки идут через него и не расходятся на пиксель.
pub fn center_band(rect: Rect, height: f32) -> Rect {
    let height = height.min(rect.h);
    Rect::new(rect.x, rect.y + (rect.h - height) / 2.0, rect.w, height)
}

/// Делает пиксели карточки полупрозрачными — как `apply_card_alpha` меню
/// (geometry.rs:173-195). Буфер premultiplied, поэтому домножаются все четыре
/// канала, а не только альфа-байт.
pub fn apply_card_alpha(pixmap: &mut tiny_skia::Pixmap, card: Rect, alpha: f32) {
    let factor = (alpha.clamp(0.0, 1.0) * 255.0) as u32;
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let x0 = (card.x * SCALE).floor() as i32;
    let y0 = (card.y * SCALE).floor() as i32;
    let x1 = (card.right() * SCALE).ceil() as i32;
    let y1 = (card.bottom() * SCALE).ceil() as i32;
    let pixels = pixmap.pixels_mut();
    for y in y0.max(0)..y1.min(height) {
        for x in x0.max(0)..x1.min(width) {
            let pixel = &mut pixels[y as usize * width as usize + x as usize];
            if let Some(scaled) = tiny_skia::PremultipliedColorU8::from_rgba(
                (pixel.red() as u32 * factor / 255) as u8,
                (pixel.green() as u32 * factor / 255) as u8,
                (pixel.blue() as u32 * factor / 255) as u8,
                (pixel.alpha() as u32 * factor / 255) as u8,
            ) {
                *pixel = scaled;
            }
        }
    }
}

// ─────────────────────────── Стили ───────────────────────────

/// Спецификация скроллбара. У окон биндов — трек во всю высоту списка и
/// акцентный бегунок («вариант 1»); у меню — бегунок без трека цветом `muted`
/// с отступом `SCROLLBAR_INSET`.
pub struct ScrollbarStyle {
    pub track: bool,
    pub inset: f32,
    pub thumb_accent: bool,
}

/// Форма подсветки строки. У биндов — скруглённая заливка всего слота без
/// отступа; у меню — прямая полоса с отступом `CARD_INSET` от краёв карточки.
pub struct RowHighlightStyle {
    pub rounded: bool,
    pub inset: f32,
}

// ─────────────────────────── Каркас ───────────────────────────

/// Подложка карточки и её рамка — то же, что `MenuView::card`
/// (menu/view/mod.rs:103-122): заливка `panel` радиусом `radii::LG` и
/// hairline `border`. Пиксели снаружи скругления остаются прозрачными.
pub fn card(pixmap: &mut tiny_skia::Pixmap, rect: Rect, ui: &UiPalette) {
    fill_round_rect(
        pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(radii::LG) * SCALE,
        ui.panel,
    );
    stroke_rect(
        pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        ui.border,
        SCALE,
    );
}

/// Шапка: заголовок слева, поле поиска справа — как в меню (header.rs:36-181).
/// Зона заголовка не наезжает на поиск: ширина считается как у `crumbs_rect`.
pub fn header(
    pixmap: &mut tiny_skia::Pixmap,
    text: &mut TextPainter,
    icons: &mut TextPainter,
    card: Rect,
    ui: &UiPalette,
    title: &str,
    query: &str,
    placeholder: &str,
) {
    let scale = type_scale();
    let header_rect = Rect::new(card.x, card.y, card.w, HEADER_H);
    let title_rect = Rect::new(
        header_rect.x + spacing::LG,
        header_rect.y,
        (header_rect.w - spacing::LG * 2.0 - SEARCH_W - spacing::MD).max(0.0),
        HEADER_H,
    );
    text.paint_boxed(
        pixmap,
        title,
        scale.section_title,
        ui.text,
        center_band(title_rect, scale.section_title * 2.0),
        Align::Start,
    );
    search_field(pixmap, text, icons, header_rect, ui, query, placeholder);
}

/// Поле поиска — по `search` меню (header.rs:96-181): фон `idle_panel`,
/// рамка `border_focus` (фокус в поле всегда — окно работает с клавиатуры),
/// лупа, запрос кеглем `label`, курсор-черта 2 px акцентом. Крестика очистки
/// нет: окно биндов клики по полю не обрабатывает, запрос чистит Ctrl+U.
#[allow(clippy::too_many_arguments)]
fn search_field(
    pixmap: &mut tiny_skia::Pixmap,
    text: &mut TextPainter,
    icons: &mut TextPainter,
    header_rect: Rect,
    ui: &UiPalette,
    query: &str,
    placeholder: &str,
) {
    let scale = type_scale();
    let rect = Rect::new(
        header_rect.right() - spacing::LG - SEARCH_W,
        header_rect.y + (HEADER_H - SEARCH_H) / 2.0,
        SEARCH_W,
        SEARCH_H,
    );
    fill_round_rect(
        pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        radius(radii::SM) * SCALE,
        ui.idle_panel,
    );
    stroke_rect(
        pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        rect.h * SCALE,
        ui.border_focus,
        SCALE,
    );
    icons.paint(
        pixmap,
        settings_icons::SEARCH,
        scale.caption,
        ui.muted,
        center_band(
            Rect::new(rect.x + spacing::SM, rect.y, ICON, rect.h),
            scale.label * 2.0,
        ),
        Align::Center,
    );

    let text_x = rect.x + spacing::SM + ICON + spacing::SM;
    let text_w = rect.w - (spacing::SM + ICON + spacing::SM) - spacing::SM;
    let text_rect = center_band(
        Rect::new(text_x, rect.y, text_w.max(0.0), rect.h),
        scale.label * 2.0,
    );
    if query.is_empty() {
        text.paint_boxed(
            pixmap,
            placeholder,
            scale.caption,
            ui.text_disabled,
            text_rect,
            Align::Start,
        );
    } else {
        text.paint_boxed(
            pixmap,
            query,
            scale.label,
            ui.text,
            text_rect,
            Align::Start,
        );
        // Курсор: вертикальная черта сразу после набранного текста. Окно
        // работает только с клавиатуры, поэтому курсор всегда на виду.
        let caret_x = text_x + text.text_width(query, scale.label);
        let caret = Rect::new(
            caret_x + 1.0,
            rect.y + spacing::SM,
            2.0,
            rect.h - spacing::SM * 2.0,
        );
        fill_rect(
            pixmap,
            caret.x * SCALE,
            caret.y * SCALE,
            caret.w * SCALE,
            caret.h * SCALE,
            ui.accent,
        );
    }
}

/// Подвал: hairline-разделитель, подсказки слева, счётчик справа — как в
/// меню (footer.rs:12-97), всё по `center_band`. Подсказки одной строкой с
/// усечением: трёхуровневое сжатие меню здесь излишне — набор подсказок у
/// биндов всегда один, а вылезать за край и наезжать на счётчик нельзя.
pub fn footer(
    pixmap: &mut tiny_skia::Pixmap,
    text: &mut TextPainter,
    card: Rect,
    ui: &UiPalette,
    hints: &str,
    counter: Option<&str>,
) {
    let scale = type_scale();
    let rect = Rect::new(card.x, card.bottom() - FOOTER_H, card.w, FOOTER_H);
    fill_rect(
        pixmap,
        rect.x * SCALE,
        rect.y * SCALE,
        rect.w * SCALE,
        SCALE,
        ui.border_subtle,
    );
    let band = center_band(rect, scale.caption * 2.0);
    let counter_w = counter
        .map(|c| text.text_width(c, scale.caption))
        .unwrap_or(0.0);
    text.paint_boxed(
        pixmap,
        hints,
        scale.caption,
        ui.muted,
        Rect::new(
            rect.x + spacing::LG,
            band.y,
            (rect.w - spacing::LG * 2.0 - counter_w - spacing::XL).max(0.0),
            band.h,
        ),
        Align::Start,
    );
    if let Some(counter) = counter {
        text.paint_boxed(
            pixmap,
            counter,
            scale.caption,
            ui.muted,
            center_band(
                Rect::new(
                    rect.right() - spacing::LG - counter_w,
                    rect.y,
                    counter_w,
                    rect.h,
                ),
                scale.caption * 2.0,
            ),
            Align::End,
        );
    }
}

/// Скроллбар полосы списка. `total`/`visible`/`offset` — в одних единицах
/// (у биндов логические пиксели потока, у меню строки): отношение важнее
/// единиц. Прокрутки нет — не рисуется ничего.
pub fn scrollbar(
    pixmap: &mut tiny_skia::Pixmap,
    list: Rect,
    ui: &UiPalette,
    total: f32,
    visible: f32,
    offset: f32,
    style: ScrollbarStyle,
) {
    if total <= visible + f32::EPSILON {
        return;
    }
    let thumb_h = (list.h * visible / total).max(SCROLLBAR_W * 2.0);
    let travel = list.h - thumb_h;
    let fraction = (offset / (total - visible)).clamp(0.0, 1.0);
    let ty = list.y + travel * fraction;
    let x = list.right() - style.inset - SCROLLBAR_W;
    if style.track {
        fill_rect(
            pixmap,
            x * SCALE,
            list.y * SCALE,
            SCROLLBAR_W * SCALE,
            list.h * SCALE,
            ui.border,
        );
    }
    let thumb = if style.thumb_accent {
        ui.accent
    } else {
        ui.muted
    };
    fill_rect(
        pixmap,
        x * SCALE,
        ty * SCALE,
        SCROLLBAR_W * SCALE,
        thumb_h * SCALE,
        thumb,
    );
}

/// Заливка выбранной строки акцентом.
pub fn select_band(pixmap: &mut tiny_skia::Pixmap, row: Rect, ui: &UiPalette, style: RowHighlightStyle) {
    band(pixmap, row, style, ui.accent);
}

/// Подложка строки под указателем: `hover_bg` той же формы, что у выбора.
pub fn hover_band(pixmap: &mut tiny_skia::Pixmap, row: Rect, ui: &UiPalette, style: RowHighlightStyle) {
    band(pixmap, row, style, ui.hover_bg);
}

/// Заливка полосы строки в заданной форме: со скруглением `radii::SM` или
/// прямой, с отступом `inset` от краёв.
fn band(pixmap: &mut tiny_skia::Pixmap, row: Rect, style: RowHighlightStyle, color: palette::Rgba) {
    let row = Rect::new(
        row.x + style.inset,
        row.y,
        (row.w - style.inset * 2.0).max(0.0),
        row.h,
    );
    if style.rounded {
        fill_round_rect(
            pixmap,
            row.x * SCALE,
            row.y * SCALE,
            row.w * SCALE,
            row.h * SCALE,
            radius(radii::SM) * SCALE,
            color,
        );
    } else {
        fill_rect(
            pixmap,
            row.x * SCALE,
            row.y * SCALE,
            row.w * SCALE,
            row.h * SCALE,
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::palette::Palette;
    use super::super::ui_tokens::ui_palette;
    use super::*;

    /// Корневая карточка меню и константа совпадают: окно биндов держит
    /// размер по этой формуле, поэтому числа обязаны сходиться.
    #[test]
    fn root_card_height_matches_the_menu_formula() {
        assert_eq!(card_height(9), 452.0);
        assert_eq!(ROOT_CARD_H, 452.0);
        assert_eq!(card_height(MAX_ROWS), 492.0);
        assert_eq!(card_height_for(MAX_ROWS, true), 612.0);
    }

    /// Скроллбар с треком рисуется: трек и бегунок оставляют непустые пиксели.
    #[test]
    fn scrollbar_draws_track_and_thumb() {
        let mut pixmap = tiny_skia::Pixmap::new((CARD_W * SCALE) as u32, (360.0 * SCALE) as u32).expect("буфер");
        let ui = ui_palette(Palette::default());
        scrollbar(
            &mut pixmap,
            Rect::new(0.0, 0.0, CARD_W, 360.0),
            &ui,
            492.0,
            360.0,
            90.0,
            ScrollbarStyle { track: true, inset: 6.0, thumb_accent: true },
        );
        assert!(pixmap.data().iter().any(|byte| *byte != 0), "скроллбар нарисован");
    }

    /// Карточка закрашивает не всё: углы снаружи скругления прозрачны.
    #[test]
    fn card_leaves_outer_corners_transparent() {
        let mut pixmap = tiny_skia::Pixmap::new((CARD_W * SCALE) as u32, (200.0 * SCALE) as u32).expect("буфер");
        let ui = ui_palette(Palette::default());
        card(&mut pixmap, Rect::new(0.0, 0.0, CARD_W, 200.0), &ui);
        let corner = pixmap.pixel(0, 0).expect("угол");
        assert_eq!(corner.alpha(), 0, "угол снаружи прозрачен");
        let middle = pixmap.pixel(pixmap.width() / 2, pixmap.height() / 2).expect("центр");
        assert!(middle.alpha() > 0, "центр карточки залит");
    }
}

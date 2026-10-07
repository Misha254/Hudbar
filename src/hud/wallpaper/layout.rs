//! Раскладка окна выбора обоев: чистые прямоугольники, которые потом рисуются.
//!
//! Вынесено отдельно от отрисовки намеренно: тесты проверяют именно раскладку
//! (элемент внутри своей зоны, подпись внутри плитки), а не пиксели. Значения
//! берутся из токенов `ui_tokens`, поэтому смена темы меняет и геометрию.

use super::super::settings_ui::Rect;
use super::super::ui_tokens::spacing;

/// Ширина карточки окна.
pub const CARD_W: f32 = 960.0;
/// Высота карточки окна.
pub const CARD_H: f32 = 600.0;
/// Отступ от края карточки до содержимого.
pub const PAD: f32 = spacing::LG;
/// Высота шапки с крошками и поиском.
pub const HEADER_H: f32 = 64.0;
/// Высота подвала с подсказками и счётчиком.
pub const FOOTER_H: f32 = 48.0;
/// Толщина разделителя: hairline, а не рамка элемента.
pub const HAIRLINE: f32 = 1.0;
/// Толщина рамки активной зоны.
pub const ZONE_BORDER: f32 = 2.0;
/// Ширина колонки каталогов.
pub const FOLDERS_W: f32 = 200.0;
/// Промежуток между колонкой каталогов и основной областью.
pub const MAIN_GAP: f32 = spacing::MD;
/// Внутренний отступ зоны: между рамкой и её содержимым.
pub const ZONE_INSET: f32 = spacing::MD;
/// Число колонок и рядов сетки.
pub const GRID_COLS: usize = 4;
pub const GRID_ROWS: usize = 4;
/// Зазор между плитками сетки.
pub const CELL_GAP: f32 = spacing::SM;
/// Высота миниатюры.
pub const THUMB_H: f32 = 86.0;
/// Высота подписи под миниатюрой.
pub const LABEL_H: f32 = 20.0;
/// Промежуток между рядами сетки.
pub const ROW_GAP: f32 = spacing::SM;
/// Высота строки каталога.
pub const FOLDER_ROW_H: f32 = 28.0;
/// Промежуток между строками каталога.
pub const FOLDER_GAP: f32 = 6.0;
/// Ширина поля поиска.
pub const SEARCH_W: f32 = 240.0;
/// Высота поля поиска.
pub const SEARCH_H: f32 = 28.0;
/// Сторона кнопки очистки в поле поиска.
pub const CLEAR_BTN: f32 = 18.0;
/// Ширина резерва под ratio в подписи миниатюры. Берётся по самой длинной
/// метке (`W×H` для нестандартных пропорций), иначе ratio обрезался бы.
pub const RATIO_RESERVE: f32 = 72.0;
/// Размер бейджа ACTIVE.
pub const BADGE_W: f32 = 74.0;
pub const BADGE_H: f32 = 18.0;
/// Сторона кнопки-галочки в углу выбранной миниатюры.
pub const TICK: f32 = 18.0;

/// Три зоны окна. Активная рисуется `border_focus`, остальные — `border_subtle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Folders,
    Grid,
}

/// Рамки трёх зон внутри карточки.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zones {
    pub folders: Rect,
    pub grid: Rect,
}

/// Высота, которую занимает сетка: три ряда миниатюр с подписями.
pub fn grid_height() -> f32 {
    GRID_ROWS as f32 * (THUMB_H + LABEL_H) + (GRID_ROWS - 1) as f32 * ROW_GAP
}

/// Раскладывает окно: шапка, зоны и подвал. Все прямоугольники — логические.
pub fn zones(card: Rect) -> Zones {
    let body_top = card.y + HEADER_H;
    let body_bottom = card.y + CARD_H - FOOTER_H;

    let main_x = card.x + PAD + FOLDERS_W + MAIN_GAP;
    let main_w = card.w - PAD * 2.0 - FOLDERS_W - MAIN_GAP;

    // Сетка занимает всю высоту тела: схемы уехали в отдельное окно, и
    // освободившийся низ ушёл под более крупные миниатюры.
    Zones {
        folders: Rect::new(card.x + PAD, body_top, FOLDERS_W, body_bottom - body_top),
        grid: Rect::new(main_x, body_top, main_w, body_bottom - body_top - PAD),
    }
}

/// Прямоугольник строки каталога по её индексу. `None`, если строка вне зоны.
pub fn folder_row(zones: Zones, index: usize) -> Option<Rect> {
    let y = zones.folders.y + ZONE_INSET + index as f32 * (FOLDER_ROW_H + FOLDER_GAP);
    if y + FOLDER_ROW_H > zones.folders.bottom() {
        return None;
    }
    Some(Rect::new(
        zones.folders.x + ZONE_INSET,
        y,
        zones.folders.w - ZONE_INSET * 2.0,
        FOLDER_ROW_H,
    ))
}

/// Прямоугольник миниатюры по позиции в сетке. `None`, если за пределами зоны.
pub fn cell(zones: Zones, row: usize, col: usize) -> Option<Rect> {
    if row >= GRID_ROWS || col >= GRID_COLS {
        return None;
    }
    let inner_w = zones.grid.w - ZONE_INSET * 2.0;
    let cell_w = (inner_w - CELL_GAP * (GRID_COLS - 1) as f32) / GRID_COLS as f32;
    let x = zones.grid.x + ZONE_INSET + col as f32 * (cell_w + CELL_GAP);
    let y = zones.grid.y + ZONE_INSET + row as f32 * (THUMB_H + LABEL_H + ROW_GAP);
    Some(Rect::new(x, y, cell_w, THUMB_H))
}

/// Полоса подписи под миниатюрой: та же ширина, что у плитки.
pub fn cell_label(thumb: Rect) -> Rect {
    Rect::new(thumb.x, thumb.y + THUMB_H, thumb.w, LABEL_H)
}

/// Прямоугольник бейджа ACTIVE: правый верхний угол миниатюры.
pub fn active_badge(thumb: Rect) -> Rect {
    Rect::new(
        thumb.right() - BADGE_W - spacing::SM,
        thumb.y + spacing::SM,
        BADGE_W,
        BADGE_H,
    )
}

/// Кнопка-галочка в углу выбранной миниатюры. Угол — левый верхний: правый
/// верхний занимает бейдж текущих обоев, и на выбранном текущем файле они бы
/// наложились.
pub fn tick(thumb: Rect) -> Rect {
    Rect::new(thumb.x + spacing::SM, thumb.y + spacing::SM, TICK, TICK)
}

/// Поле поиска в шапке.
pub fn search(card: Rect) -> Rect {
    Rect::new(
        card.x + card.w - PAD - SEARCH_W,
        card.y + (HEADER_H - SEARCH_H) / 2.0,
        SEARCH_W,
        SEARCH_H,
    )
}

/// Кнопка очистки справа в поле поиска.
pub fn search_clear(search: Rect) -> Rect {
    Rect::new(
        search.right() - CLEAR_BTN - spacing::SM,
        search.y + (search.h - CLEAR_BTN) / 2.0,
        CLEAR_BTN,
        CLEAR_BTN,
    )
}

/// Строка каталога под курсором: перебор сверху, первая содержащая. `count`
/// ограничивает перебор существующими строками.
pub fn folder_at(zones: Zones, count: usize, x: f32, y: f32) -> Option<usize> {
    for index in 0..count {
        match folder_row(zones, index) {
            Some(row) if row.contains(x, y) => return Some(index),
            None => return None,
            _ => {}
        }
    }
    None
}

/// Миниатюра под курсором в терминах списка: видимая позиция плюс `top_row`.
// Подпись считается частью плитки — клик по имени тоже выбирает.
pub fn cell_at(zones: Zones, top_row: usize, len: usize, x: f32, y: f32) -> Option<usize> {
    for row in 0..GRID_ROWS {
        for col in 0..GRID_COLS {
            let index = (top_row + row) * GRID_COLS + col;
            if index >= len {
                return None;
            }
            let Some(thumb) = cell(zones, row, col) else {
                continue;
            };
            let label = cell_label(thumb);
            let full = Rect::new(thumb.x, thumb.y, thumb.w, thumb.h + label.h);
            if full.contains(x, y) {
                return Some(index);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Rect {
        Rect::new(120.0, 100.0, CARD_W, CARD_H)
    }

    /// Пункт 1 и 10: содержимое каждой зоны живёт внутри её рамки.
    #[test]
    fn every_zone_element_stays_inside_its_frame() {
        let z = zones(card());
        for (index, area) in [z.folders, z.grid].into_iter().enumerate() {
            let _ = index;
            assert!(area.w > 0.0 && area.h > 0.0, "зона не вырождена");
        }
        for index in 0..20 {
            if let Some(row) = folder_row(z, index) {
                assert!(
                    row.y >= z.folders.y && row.bottom() <= z.folders.bottom(),
                    "строка каталога {index} вышла за зону"
                );
                assert!(row.x >= z.folders.x && row.right() <= z.folders.right());
            }
        }
        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let thumb = cell(z, row, col).expect("плитка в сетке");
                assert!(
                    thumb.y >= z.grid.y && thumb.bottom() <= z.grid.bottom(),
                    "миниатюра [{row}][{col}] вышла за зону сетки"
                );
                assert!(thumb.x >= z.grid.x && thumb.right() <= z.grid.right());
            }
        }
    }

    /// Пункт 4: колонка каталогов ровно 200 px, а строка не выходит за неё.
    #[test]
    fn folder_column_is_200px_and_row_fits() {
        let z = zones(card());
        assert_eq!(FOLDERS_W, 200.0);
        assert_eq!(z.folders.w, FOLDERS_W);
        let row = folder_row(z, 0).unwrap();
        assert!(row.w <= z.folders.w, "строка шире колонки");
        // Резерва под иконку и счётчик хватает: имя получает остаток.
        let name_w = row.w - ZONE_INSET * 2.0 - 20.0 - 44.0;
        assert!(name_w > 40.0, "под имя папки осталось {name_w} px");
    }

    /// Пункт 5: кнопка очистки внутри поля поиска и на одном уровне с ним.
    #[test]
    fn search_clear_button_sits_inside_the_field() {
        let c = card();
        let field = search(c);
        let clear = search_clear(field);
        assert!(
            clear.x >= field.x && clear.right() <= field.right(),
            "крестик вышел за поле"
        );
        assert!(clear.y >= field.y && clear.bottom() <= field.bottom());
    }

    /// Пункт 7: бейдж и галочка внутри миниатюры и не друг на друге.
    #[test]
    fn badge_and_tick_fit_inside_thumbnail() {
        let z = zones(card());
        let thumb = cell(z, 0, 0).unwrap();
        let badge = active_badge(thumb);
        let tick = tick(thumb);
        assert!(badge.x >= thumb.x && badge.right() <= thumb.right());
        assert!(badge.y >= thumb.y && badge.bottom() <= thumb.bottom());
        assert!(tick.x >= thumb.x && tick.right() <= thumb.right());
        assert!(tick.y >= thumb.y && tick.bottom() <= thumb.bottom());
        assert!(
            tick.right() <= badge.x,
            "галочка и бейдж не должны накладываться: {:?} {:?}",
            tick,
            badge
        );
    }

    /// Пустая сетка и короткий список не ломают раскладку.
    #[test]
    fn degenerate_lists_do_not_break_layout() {
        let z = zones(card());
        assert_eq!(folder_row(z, 10_000), None, "строка за пределами зоны");
        assert_eq!(cell(z, GRID_ROWS, 0), None);
    }

    /// Hit-test: строка и плитка находятся, мимо — None.
    #[test]
    fn hit_test_finds_rows_and_cells() {
        let z = zones(card());
        let row = folder_row(z, 2).unwrap();
        assert_eq!(folder_at(z, 10, row.x + 1.0, row.y + 1.0), Some(2));
        assert_eq!(
            folder_at(z, 2, row.x + 1.0, row.y + 1.0),
            None,
            "третьей строки нет"
        );
        assert_eq!(
            folder_at(z, 10, z.folders.x - 50.0, z.folders.y - 50.0),
            None
        );
        let thumb = cell(z, 1, 2).unwrap();
        let index = GRID_COLS + 2;
        assert_eq!(
            cell_at(z, 0, 100, thumb.x + 1.0, thumb.y + 1.0),
            Some(index)
        );
        let label = cell_label(thumb);
        assert_eq!(
            cell_at(z, 0, 100, label.x + 1.0, label.y + 1.0),
            Some(index),
            "подпись — часть плитки"
        );
        assert_eq!(
            cell_at(z, 0, index, thumb.x + 1.0, thumb.y + 1.0),
            None,
            "за длиной списка"
        );
        let top = cell_at(z, 3, 100, thumb.x + 1.0, thumb.y + 1.0).unwrap();
        assert_eq!(top, (3 + 1) * GRID_COLS + 2, "top_row сдвигает индексы");
    }
}

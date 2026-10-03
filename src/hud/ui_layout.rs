//! Слой раскладки: вертикальный стек полос внутри области.
//!
//! Секции окна держали координаты абсолютными константами (`NOTIFY_FONT_Y` и
//! подобные), а каждая строка несла готовый `Rect`. Поменять порядок секций или
//! высоту полосы значило пересчитать числа руками, и ошибка выглядела как
//! «наезд» элементов, а не как ошибка в арифметике.
//!
//! Здесь раскладка считается сама: полосы выдаются сверху вниз, отступы между
//! ними — из токенов, а сумма высот и промежутков всегда равна позиции
//! курсора. Модуль ничего не знает про отрисовку и Wayland: на входе area и
//! числа, на выходе — `Rect`.
//!
//! Отрисовка и hit-test получают один и тот же `Rect` из строки, поэтому
//! пересечься они не могут по построению.

use super::settings_ui::{self, Rect};
use super::ui_tokens::{CONTENT_MAX_W, PAGE_PAD};

/// Высота полосы-контрола: подпись и кнопки живут на одной высоте.
pub const SETTING_ROW_H: f32 = 40.0;
/// Ширина кнопки степпера (− и +).
pub const STEP_W: f32 = 40.0;
/// Ширина поля значения в степпере.
pub const VALUE_W: f32 = 56.0;

/// Область контента страницы: отступы и ширина — из токенов, рамка окна — из
/// `settings_ui`. Ширина ограничена `CONTENT_MAX_W`: если окно когда-нибудь
/// станет шире, полоса контента перестанет растягиваться.
pub fn content_area() -> Rect {
    let x = settings_ui::PAD_X;
    let width = CONTENT_MAX_W.min(settings_ui::WIDTH - x - PAGE_PAD);
    // Верх контента — там же, где начинаются первые полосы секций. Ниже
    // заголовка окна, но выше отступа: считать это от `HEADER_Y` значило бы
    // получить 2 px, которых в раскладке нет.
    let top = settings_ui::CARD_Y;
    Rect::new(x, top, width, settings_ui::STATUS_Y - top)
}

/// Ширина колонки: как `columns`, но для одного столбца.
pub fn column_width(columns: usize, col_gap: f32) -> f32 {
    let columns = columns.max(1) as f32;
    (CONTENT_MAX_W - col_gap * (columns - 1.0)) / columns
}

/// Какой шириной обладает колонка стека.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Width {
    /// Фиксированная ширина: кнопка, поле значения.
    Fixed(f32),
    /// Доля от остатка: подпись, которая тянется на всё свободное место.
    /// Доли не обязаны давать 1.0 — важно их отношение.
    Fill(f32),
}

/// Вертикальный стек полос внутри области.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    area: Rect,
    /// Нижний край последней полосы.
    y: f32,
    gap: f32,
    /// Есть ли уже хоть одна полоса: первая начинается без зазора.
    used: bool,
}

impl Layout {
    /// Стек от верхнего края `area` с зазором `gap` между полосами. Первая
    /// полоса начинается без отступа: сверху уже есть заголовок страницы.
    pub fn new(area: Rect, gap: f32) -> Self {
        Self {
            area,
            y: area.y,
            gap,
            used: false,
        }
    }

    /// Зазор перед следующей полосой. Накопление: можно сузить полосу, затем
    /// вернуть общий зазор для секции.
    pub fn gap(&mut self, px: f32) -> &mut Self {
        self.gap = px;
        self
    }

    /// Y следующей полосы: первая начинается от верха области, каждая
    /// последующая — через зазор.
    pub fn cursor_y(&self) -> f32 {
        if self.used { self.y + self.gap } else { self.y }
    }

    /// Область, внутри которой живёт стек. Нужна вложенным раскладкам:
    /// секция собирает свою полосу и кладёт на неё свой стек.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Нижний край последней полосы.
    pub fn end_y(&self) -> f32 {
        self.y
    }

    /// Сколько осталось до низа области.
    pub fn remaining(&self) -> f32 {
        self.area.bottom() - self.cursor_y()
    }

    /// Полоса на всю ширину области.
    pub fn row(&mut self, height: f32) -> Rect {
        let y = self.cursor_y();
        self.y = y + height;
        self.used = true;
        Rect::new(self.area.x, y, self.area.w, height)
    }

    /// Полоса, разбитая на `n` равных колонок.
    pub fn columns(&mut self, n: usize, col_gap: f32, height: f32) -> Vec<Rect> {
        let n = n.max(1);
        let width = (self.area.w - col_gap * (n - 1) as f32) / n as f32;
        let y = self.cursor_y();
        self.y = y + height;
        self.used = true;
        (0..n)
            .map(|index| {
                Rect::new(
                    self.area.x + index as f32 * (width + col_gap),
                    y,
                    width,
                    height,
                )
            })
            .collect()
    }

    /// Карточка: рамка, заголовок и содержимое. Высота считается сама из
    /// высоты содержимого, поэтому добавление строки внутрь не требует
    /// пересчёта чисел. `f` получает внутреннюю область и кладёт в неё строки.
    pub fn card(&mut self, title: &str, content: f32, f: impl FnOnce(&mut Layout)) -> Rect {
        let has_title = !title.is_empty();
        let outer_height = super::settings_widgets::card_height(content, has_title);
        let outer = self.row(outer_height);
        let inner_area = super::settings_widgets::card_content_rect(outer, has_title);
        let mut inner = Layout::new(inner_area, 0.0);
        f(&mut inner);
        outer
    }

    /// Полоса из колонок смешанной ширины: `Fill` делит остаток по весам,
    /// `Fixed` забирает своё. Копейка округления уходит последней `Fill`,
    /// чтобы сумма ширин совпала с шириной области.
    pub fn split(&mut self, widths: &[Width], col_gap: f32, height: f32) -> Vec<Rect> {
        let gaps = col_gap * widths.len().saturating_sub(1) as f32;
        let free = (self.area.w - gaps).max(0.0);
        let fixed: f32 = widths
            .iter()
            .map(|width| match width {
                Width::Fixed(px) => *px,
                Width::Fill(_) => 0.0,
            })
            .sum();
        let weight: f32 = widths
            .iter()
            .map(|width| match width {
                Width::Fill(weight) => *weight,
                Width::Fixed(_) => 0.0,
            })
            .sum();
        let fill_space = (free - fixed).max(0.0);
        let mut sizes = vec![0.0_f32; widths.len()];
        let mut last_fill: Option<usize> = None;
        let mut fills = 0.0;
        for (index, width) in widths.iter().enumerate() {
            if let Width::Fill(own) = width {
                if *own > 0.0 && weight > 0.0 {
                    sizes[index] = fill_space * own / weight;
                    fills += sizes[index];
                    last_fill = Some(index);
                }
            } else if let Width::Fixed(px) = width {
                sizes[index] = *px;
            }
        }
        if let Some(index) = last_fill {
            sizes[index] += fill_space - fills;
        }
        let y = self.cursor_y();
        self.y = y + height;
        self.used = true;
        let mut x = self.area.x;
        let mut out = Vec::with_capacity(sizes.len());
        for size in sizes {
            out.push(Rect::new(x, y, size, height));
            x += size + col_gap;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::ui_tokens::{ROW_H, spacing};
    use super::*;

    fn area() -> Rect {
        Rect::new(100.0, 50.0, 600.0, 400.0)
    }

    /// Полосы не наезжают: каждая начинается ниже конца предыдущей, с зазором.
    #[test]
    fn strips_never_overlap() {
        let mut layout = Layout::new(area(), spacing::MD);
        let mut previous: Option<Rect> = None;
        for _ in 0..8 {
            let rect = layout.row(SETTING_ROW_H);
            if let Some(previous) = previous {
                assert!(
                    rect.y >= previous.bottom() + spacing::MD - 0.001,
                    "полосы слиплись: {previous:?} и {rect:?}"
                );
            }
            assert!((rect.x - area().x).abs() < 0.001, "полоса вылезла по X");
            assert!((rect.w - area().w).abs() < 0.001, "полоса не на всю ширину");
            previous = Some(rect);
        }
    }

    /// Арифметика стека: низ стека равен началу плюс все полосы и все зазоры
    /// между ними, а курсор — это ещё плюс зазор перед следующей полосой.
    #[test]
    fn cursor_equals_area_plus_strips_and_gaps() {
        let mut layout = Layout::new(area(), spacing::MD);
        let heights = [ROW_H, SETTING_ROW_H, ROW_H, SETTING_ROW_H, ROW_H];
        let mut expected = area().y;
        for (index, height) in heights.iter().enumerate() {
            if index > 0 {
                expected += spacing::MD;
            }
            let rect = layout.row(*height);
            assert!((rect.y - expected).abs() < 0.001, "полоса {index} поехала");
            expected += height;
        }
        assert!(
            (layout.end_y() - expected).abs() < 0.001,
            "низ стека разошёлся с суммой полос и зазоров"
        );
        assert!(
            (layout.cursor_y() - (expected + spacing::MD)).abs() < 0.001,
            "курсор не равен низу стека плюс зазор"
        );
    }

    /// Колонки делят ширину без остатка: сумма равна ширине области.
    #[test]
    fn columns_divide_the_width_without_remainder() {
        for n in [2usize, 3, 4] {
            let mut layout = Layout::new(area(), spacing::SM);
            let cells = layout.columns(n, spacing::MD, ROW_H);
            assert_eq!(cells.len(), n);
            let total: f32 =
                cells.iter().map(|cell| cell.w).sum::<f32>() + spacing::MD * (n - 1) as f32;
            assert!(
                (total - area().w).abs() < 0.01,
                "колонок {n}: сумма {total} != {}",
                area().w
            );
            for cell in &cells {
                assert!((cell.y - cells[0].y).abs() < 0.001, "колонки разной высоты");
                assert!(cell.bottom() <= area().bottom() + 0.001, "колонка вылезла");
            }
        }
    }

    /// `Fill` забирает остаток, `Fixed` — своё, и сумма равна ширине области.
    #[test]
    fn split_gives_the_remainder_to_fill_columns() {
        let mut layout = Layout::new(area(), spacing::SM);
        let cells = layout.split(
            &[
                Width::Fill(1.0),
                Width::Fixed(VALUE_W),
                Width::Fixed(STEP_W),
                Width::Fixed(STEP_W),
            ],
            spacing::SM,
            SETTING_ROW_H,
        );
        assert_eq!(cells.len(), 4);
        let total: f32 = cells.iter().map(|cell| cell.w).sum::<f32>() + spacing::SM * 3.0;
        assert!(
            (total - area().w).abs() < 0.01,
            "полосы не смыкаются: {total} != {}",
            area().w
        );
        assert!(
            (cells[1].w - VALUE_W).abs() < 0.01,
            "фиксированная ширина поехала"
        );
        assert!((cells[3].w - STEP_W).abs() < 0.01, "кнопка поехала");
        assert!(cells[0].w > cells[1].w, "подпись должна быть шире значения");
        // Хвост прижат к правому краю области.
        assert!(
            (cells[3].right() - area().right()).abs() < 0.01,
            "правый край не у края"
        );
    }

    /// Доли работают как доли: 1:2 делит остаток не поровну.
    #[test]
    fn fill_weights_are_ratios() {
        let mut layout = Layout::new(area(), 0.0);
        let cells = layout.split(&[Width::Fill(1.0), Width::Fill(2.0)], 0.0, ROW_H);
        let ratio = cells[1].w / cells[0].w;
        assert!((ratio - 2.0).abs() < 0.01, "доли не 1:2, а 1:{ratio}");
        assert!((cells[0].w + cells[1].w - area().w).abs() < 0.01);
    }

    /// Одна и та же раскладка — один и тот же результат.
    #[test]
    fn layout_is_deterministic() {
        let build = || {
            let mut layout = Layout::new(area(), spacing::MD);
            let mut out = vec![layout.row(SETTING_ROW_H)];
            out.extend(layout.columns(2, spacing::MD, ROW_H));
            out.extend(layout.split(
                &[Width::Fill(1.0), Width::Fixed(STEP_W)],
                spacing::SM,
                ROW_H,
            ));
            out
        };
        assert_eq!(build(), build());
    }

    /// `remaining` считает до низа области и не уходит в минус на полной
    /// раскладке: иначе секция может уехать за статус.
    #[test]
    fn remaining_counts_down_to_the_area_bottom() {
        let mut layout = Layout::new(area(), spacing::MD);
        let before = layout.remaining();
        assert!((before - (area().bottom() - area().y)).abs() < 0.001);
        layout.row(SETTING_ROW_H);
        assert!((layout.remaining() - (before - SETTING_ROW_H - spacing::MD)).abs() < 0.001);
    }

    /// `card` считает высоту сам и отдаёт рамку; содержимое кладётся внутрь
    /// по отступам и не выходит за них.
    #[test]
    fn card_counts_its_own_height() {
        let area = Rect::new(0.0, 0.0, 600.0, 400.0);
        let mut layout = Layout::new(area, spacing::MD);
        let outer = layout.card("Заголовок", 2.0 * ROW_H + spacing::SM, |inner| {
            inner.row(ROW_H);
            inner.row(ROW_H);
        });
        assert_eq!(
            outer.h,
            super::super::settings_widgets::card_height(2.0 * ROW_H + spacing::SM, true),
            "высота карточки не совпала с содержимым"
        );
        assert!(
            outer.bottom() <= area.bottom(),
            "карточка вылезла из области"
        );
    }

    /// Пилот «Уведомлений» должен целиком помещаться в область контента.
    #[test]
    fn notification_pilot_fits_the_content_area() {
        let area = content_area();
        assert!(area.w <= CONTENT_MAX_W + 0.001, "полоса шире CONTENT_MAX_W");
        assert!(area.right() <= settings_ui::WIDTH - PAGE_PAD + 0.001);
    }
}

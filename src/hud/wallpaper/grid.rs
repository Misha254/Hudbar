//! Модель сетки обоев: позиционирование, навигация и окно видимых ячеек.

/// Направления навигации.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
}

/// Сетка `cols × rows` ячеек, связанная с произвольной длиной списка.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    pub len: usize,
    pub sel: usize,
    pub top_row: usize,
}

impl Grid {
    pub fn new(cols: usize, rows: usize, len: usize) -> Self {
        Grid {
            cols,
            rows,
            len,
            sel: 0,
            top_row: 0,
        }
    }

    pub fn set_len(&mut self, len: usize) {
        self.len = len;
        if self.len == 0 {
            self.sel = 0;
            self.top_row = 0;
        } else {
            self.sel = self.sel.min(self.len - 1);
            self.ensure_visible();
        }
    }

    pub fn rows_in_data(&self) -> usize {
        self.len.div_ceil(self.cols.max(1))
    }

    /// Выбранная ячейка в последнем ряду, где есть данные.
    ///
    /// ↓ отсюда уводит в зону схем под сеткой. Замыкать кольцо по вертикали
    /// не нужно: ↑ с первого ряда и так уводит на последний, поэтому обход
    /// сетки остаётся кругом.
    pub fn in_last_row(&self) -> bool {
        let rows = self.rows_in_data();
        rows > 0 && self.sel / self.cols.max(1) == rows - 1
    }

    /// Ставит выбор на первый элемент и возвращает окно в начало. Нужен после
    /// смены набора (папка, фильтр): индекс из прошлого списка может уйти за
    /// границы, и подсветка окажется не там.
    pub fn select_first(&mut self) {
        self.sel = 0;
        self.top_row = 0;
        self.ensure_visible();
    }

    pub fn selected(&self) -> usize {
        self.sel
    }

    pub fn top_row(&self) -> usize {
        self.top_row
    }

    /// Райкировнение окна прокрутки под выбранной ячейкой.
    pub fn ensure_visible(&mut self) {
        let n_rows = self.rows_in_data();
        if n_rows == 0 {
            self.top_row = 0;
            return;
        }
        let sel_row = self.sel / self.cols.max(1);
        if sel_row < self.top_row {
            self.top_row = sel_row;
        } else if sel_row >= self.top_row + self.rows {
            self.top_row = sel_row.saturating_sub(self.rows.saturating_sub(1));
        }
        let max_top = n_rows.saturating_sub(self.rows);
        self.top_row = self.top_row.min(max_top);
    }

    /// Индексы строк (в терминах ячеек), которые сейчас видны.
    pub fn visible_cells(&self) -> Vec<usize> {
        let start = self.top_row * self.cols;
        let end = (self.top_row + self.rows) * self.cols;
        (start..end.min(self.len)).collect()
    }

    /// Ряды для предзагрузки: видимые ±1, зажаты по существующим.
    pub fn prefetch_cells(&self) -> Vec<usize> {
        let top = self.top_row.saturating_sub(1);
        let start = top * self.cols;
        let rows = self.rows + 2;
        let end = start + rows * self.cols;
        (start..end.min(self.len)).collect()
    }

    /// Прокрутка окна на `delta` рядов колесом: выбор подтягивается в вид,
    /// чтобы подсветка не уезжала за кадр. Знак как у колеса: плюс — вниз.
    pub fn scroll_rows(&mut self, delta: isize) {
        if self.len == 0 {
            return;
        }
        let max_top = self.rows_in_data().saturating_sub(self.rows);
        let next = (self.top_row as isize + delta).clamp(0, max_top as isize);
        self.top_row = next as usize;
        let cols = self.cols.max(1);
        let sel_row = self.sel / cols;
        let col = self.sel % cols;
        if sel_row < self.top_row || sel_row >= self.top_row + self.rows {
            let row = sel_row.clamp(self.top_row, self.top_row + self.rows.saturating_sub(1));
            self.sel = (row * cols + col).min(self.len.saturating_sub(1));
        }
        self.ensure_visible();
    }

    pub fn move_dir(&mut self, dir: Dir) {
        if self.len == 0 {
            return;
        }
        let cols = self.cols.max(1);
        let row = self.sel / cols;
        let col = self.sel % cols;
        match dir {
            Dir::Left => {
                if col > 0 {
                    self.sel -= 1;
                }
            }
            Dir::Right => {
                // Вправо идёт по ячейкам подряд, через конец ряда — на
                // следующий, а с самой последней ячейки — на первую: круг.
                // Влево круг не замыкается здесь специально: ← в первой
                // колонке уходит в зону папок, это навигация, а не список.
                self.sel = if self.sel + 1 < self.len {
                    self.sel + 1
                } else {
                    0
                };
            }
            Dir::Up => {
                if row > 0 {
                    self.sel -= cols;
                } else {
                    // Вверх с первого ряда — на последний в той же колонке.
                    let last_row = self.rows_in_data().saturating_sub(1);
                    self.sel = (last_row * cols + col).min(self.len - 1);
                }
            }
            Dir::Down => {
                if self.sel + cols < self.len {
                    self.sel += cols;
                } else {
                    // Вниз с последнего ряда — на первый в той же колонке.
                    // Неполный последний ряд при этом не застревает на
                    // последнем элементе, а уходит в круг.
                    self.sel = col.min(self.len - 1);
                }
            }
            Dir::PageUp => {
                self.sel = self.sel.saturating_sub(cols * self.rows);
            }
            Dir::PageDown => {
                self.sel = (self.sel + cols * self.rows).min(self.len - 1);
            }
            Dir::Home => {
                self.sel = 0;
            }
            Dir::End => {
                self.sel = self.len - 1;
            }
        }
        self.ensure_visible();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Вертикаль зациклена с сохранением колонки: ↓ с последнего ряда — на
    /// первый, ↑ с первого — на последний. Неполный ряд не застревает.
    #[test]
    fn vertical_moves_wrap_around_keeping_the_column() {
        let mut grid = Grid::new(4, 3, 5);
        grid.sel = 4; // последняя неполная строка (индекс 4), колонка 0
        grid.move_dir(Dir::Down);
        assert_eq!(grid.sel, 0, "вниз с последнего ряда — на первый");

        let mut grid = Grid::new(4, 3, 5);
        grid.move_dir(Dir::Up);
        assert_eq!(grid.sel, 4, "вверх с первого ряда — на последний");

        let mut grid = Grid::new(4, 3, 8);
        grid.sel = 3;
        grid.move_dir(Dir::Down);
        assert_eq!(
            grid.sel, 7,
            "из полного ряда ↓ — тот же столбец, последний ряд полный"
        );

        let mut grid = Grid::new(4, 3, 12);
        grid.sel = 1;
        grid.move_dir(Dir::Up);
        assert_eq!(grid.sel, 9, "вверх держит колонку: ряд 2, колонка 1");
    }

    /// → идёт по ячейкам подряд и с последней — на первую. ← в первой колонке
    /// стоит: уход в папки решает вызывающий, а не сетка.
    #[test]
    fn horizontal_moves_walk_cells_and_wrap_at_the_end() {
        let mut grid = Grid::new(4, 3, 12);
        grid.move_dir(Dir::Left);
        assert_eq!(grid.sel, 0, "← в первой колонке — стоит");
        grid.sel = 3; // конец первого ряда
        grid.move_dir(Dir::Right);
        assert_eq!(grid.sel, 4, "→ с конца ряда — на начало следующего");
        grid.move_dir(Dir::Left);
        assert_eq!(
            grid.sel, 4,
            "← в первой колонке стоит: в папки уходит вызывающий"
        );

        grid.sel = 11; // самая последняя ячейка
        grid.move_dir(Dir::Right);
        assert_eq!(grid.sel, 0, "→ с последней ячейки — на первую");

        grid.sel = 5; // середина второго ряда
        grid.move_dir(Dir::Left);
        assert_eq!(grid.sel, 4, "← внутри ряда — на ячейку назад");
    }

    /// Переключение между строками: ↓ двигает на пересечение, ↑ возвращает.
    #[test]
    fn up_and_down_between_rows() {
        let mut grid = Grid::new(4, 3, 12);
        grid.sel = 5;
        grid.move_dir(Dir::Up);
        assert_eq!(grid.sel, 1);
        grid.move_dir(Dir::Down);
        assert_eq!(grid.sel, 5);
        grid.sel = 0;
        grid.move_dir(Dir::Down);
        assert_eq!(grid.sel, 4);
    }

    /// Последний ряд — это тот, где последние данные, а не где кончилось окно.
    #[test]
    fn last_row_follows_the_data_and_ignores_a_partial_one() {
        // 10 плиток при cols=4 — это три ряда: 0-3, 4-7 и неполный 8-9.
        let mut grid = Grid::new(4, 3, 10);
        grid.sel = 9;
        assert!(
            grid.in_last_row(),
            "9 — неполный последний ряд, но он последний"
        );
        grid.sel = 8;
        assert!(grid.in_last_row(), "8 — тот же последний ряд");
        grid.sel = 7;
        assert!(!grid.in_last_row(), "7 — средний ряд, не последний");
        grid.sel = 3;
        assert!(!grid.in_last_row(), "3 — первый ряд");

        // 6 плиток при cols=4 — это два ряда: 0-3 и неполный 4-5.
        grid.set_len(6);
        grid.sel = 5;
        assert!(grid.in_last_row(), "5 — неполный последний ряд");
        grid.sel = 4;
        assert!(grid.in_last_row());
        grid.sel = 2;
        assert!(!grid.in_last_row());
    }

    #[test]
    fn last_row_on_an_empty_grid_is_never_taken() {
        let grid = Grid::new(4, 3, 0);
        assert!(!grid.in_last_row(), "пустой сетке нечего покидать");
    }

    /// PgUp/PgDn и Home/End держат границы и видимость.
    #[test]
    fn paging_is_clamped() {
        let mut grid = Grid::new(4, 3, 20);
        grid.sel = 18;
        grid.move_dir(Dir::PageDown);
        assert_eq!(grid.sel, 19);
        assert_eq!(grid.top_row, 2);

        grid.sel = 2;
        grid.move_dir(Dir::PageUp);
        assert_eq!(grid.sel, 0);

        grid.sel = 7;
        grid.move_dir(Dir::End);
        assert_eq!(grid.sel, 19);
        grid.move_dir(Dir::Home);
        assert_eq!(grid.sel, 0);
    }

    /// Scroll:ensure_visible не уходит за край, окно в точ фиксировано.
    #[test]
    fn visible_window_keeps_selection_inside_pages() {
        let mut grid = Grid::new(4, 3, 40);
        assert_eq!(
            grid.visible_cells().len(),
            12,
            "первая страница заполняет 12 ячеек"
        );
        grid.sel = 15;
        grid.ensure_visible();
        assert!(grid.visible_cells().contains(&15));
        grid.sel = 12;
        grid.ensure_visible();
        assert_eq!(
            grid.top_row, 1,
            "выделение на границе: top_row подтягивается"
        );
    }

    /// Эмпбтельный случай: список пуст или меньше ряда.
    #[test]
    fn empty_or_partial_grid_is_safe() {
        let mut grid = Grid::new(4, 3, 0);
        grid.move_dir(Dir::Down);
        assert_eq!(grid.sel, 0);
        assert_eq!(grid.visible_cells(), Vec::<usize>::new());

        let mut grid = Grid::new(4, 3, 3);
        grid.sel = 2;
        grid.move_dir(Dir::Right);
        assert_eq!(grid.sel, 0, "→ с последней ячейки — на первую");
        grid.move_dir(Dir::Down);
        assert_eq!(
            grid.sel, 0,
            "вниз в короткой сетке держит единственную колонку"
        );
    }

    /// Смена набора: выбран первый элемент, окно не осталось на старой странице.
    #[test]
    fn select_first_resets_selection_after_a_list_change() {
        let mut grid = Grid::new(4, 3, 40);
        grid.sel = 35;
        grid.ensure_visible();
        assert!(grid.top_row > 0, "окно должно уехать вниз");
        grid.set_len(3);
        grid.select_first();
        assert_eq!(grid.sel, 0);
        assert_eq!(
            grid.top_row, 0,
            "окно прокрутки осталось на старой странице"
        );
    }

    /// Пустой набор после смены: выбора нет, и навигация не паникует.
    #[test]
    fn select_first_on_an_empty_list_is_safe() {
        let mut grid = Grid::new(4, 3, 0);
        grid.select_first();
        assert_eq!(grid.sel, 0);
        grid.move_dir(Dir::End);
        assert_eq!(grid.sel, 0);
    }

    /// Колесо едет окном и подтягивает выбор в вид.
    #[test]
    fn scroll_rows_moves_the_window_and_keeps_selection_visible() {
        let mut grid = Grid::new(4, 3, 40);
        grid.scroll_rows(2);
        assert_eq!(grid.top_row, 2);
        assert!(grid.visible_cells().contains(&grid.sel));
        grid.sel = 39;
        grid.ensure_visible();
        grid.scroll_rows(-7);
        assert!(grid.visible_cells().contains(&grid.sel));
        // Пустой список и нулевой сдвиг — без паники и без сдвига.
        let mut empty = Grid::new(4, 3, 0);
        empty.scroll_rows(5);
        assert_eq!(empty.top_row, 0);
        let top = grid.top_row;
        grid.scroll_rows(0);
        assert_eq!(grid.top_row, top);
    }
}

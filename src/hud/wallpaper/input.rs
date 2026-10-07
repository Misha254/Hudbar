//! Ввод окна обоев: клавиша и мышь превращаются в команду модели.
//!
//! Модуль ничего не знает про Wayland: на входе ключ, координаты и состояние,
//! на выходе — команда или исход. Всё разбирается в тестах без композитора,
//! именно поэтому ввод вынесен из окна, как `menu::input` в меню.
//!
//! Правила из W3: стрелки по сетке; Tab / Shift+Tab — папки → сетка → схемы;
//! ← из первой колонки сетки — в папки, → из папок — в сетку; ↓ из последнего
//! ряда сетки — в схемы, ↑ из первого ряда схем — в сетку; Enter —
//! применить в сетке или выбрать схему/папку; PgUp/PgDn, Home/End; печатные
//! символы — в поиск; Esc чистит запрос, иначе закрывает. Мышь: клик по плитке
//! выбирает, второй клик по выбранной применяет; клик по папке и схеме
//! выбирает; колесо крутит ряды; hover подсвечивает без смены выбора.
//!
//! Общее правило переходов: жест в сторону соседней зоны уводит из зоны,
//! а не замыкает кольцо. ←/→ внутри схем остаются кольцом чипов — там нет
//! соседа слева или справа.

use super::super::settings_ui::Rect;
use super::grid::Dir;
use super::layout;
use super::state::{Hover, Outcome, State, Zone};

/// Клавиша в понятных терминах: те же коды XKB, что в меню, но своя команда.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Tab,
    Enter,
    Backspace,
    Escape,
    /// Печатный символ: уже с учётом раскладки.
    Char(char),
    /// Клавиша без команды.
    #[default]
    Other,
}

/// Клавиша с её состоянием.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub logo: bool,
}

/// Команда модели.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WpInput {
    Ignore,
    /// Стрелки и дальние клавиши: зону решает применение.
    Move(Dir),
    /// Tab: вперёд или назад (Shift+Tab).
    Tab(bool),
    /// Войти/выбрать в активной зоне.
    Activate,
    /// Закрыть окно.
    Close,
    /// Очистить запрос целиком.
    ClearQuery,
    /// Стереть последний символ запроса.
    Erase,
    /// Дописать символ к запросу.
    Type(char),
}

/// Коды XKB: те же значения, что в `menu::input` — стандарт X11.
pub mod xkb {
    pub const ESCAPE: u32 = 0xff1b;
    pub const BACKSPACE: u32 = 0xff08;
    pub const RETURN: u32 = 0xff0d;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const TAB: u32 = 0xff09;
    pub const LEFT: u32 = 0xff51;
    pub const UP: u32 = 0xff52;
    pub const RIGHT: u32 = 0xff53;
    pub const DOWN: u32 = 0xff54;
    pub const PAGE_UP: u32 = 0xff55;
    pub const PAGE_DOWN: u32 = 0xff56;
    pub const HOME: u32 = 0xff50;
    pub const END: u32 = 0xff57;
}

/// Превращает XKB-код и текст клавиши в [`KeyEvent`]: текст уже собран
/// композитором с учётом раскладки, код читается, когда текста нет.
pub fn event(keysym: u32, utf8: Option<&str>, ctrl: bool, shift: bool, logo: bool) -> KeyEvent {
    let key = match keysym {
        xkb::ESCAPE => Key::Escape,
        xkb::BACKSPACE => Key::Backspace,
        xkb::RETURN | xkb::KP_ENTER => Key::Enter,
        xkb::TAB => Key::Tab,
        xkb::LEFT => Key::Left,
        xkb::UP => Key::Up,
        xkb::RIGHT => Key::Right,
        xkb::DOWN => Key::Down,
        xkb::PAGE_UP => Key::PageUp,
        xkb::PAGE_DOWN => Key::PageDown,
        xkb::HOME => Key::Home,
        xkb::END => Key::End,
        _ => match utf8 {
            Some(text) => {
                let mut clean = text.chars().filter(|ch| !ch.is_control());
                match (clean.next(), clean.next()) {
                    (Some(ch), None) => Key::Char(ch),
                    _ => Key::Other,
                }
            }
            None => Key::Other,
        },
    };
    KeyEvent {
        key,
        ctrl,
        shift,
        logo,
    }
}

/// Переводит клавишу в команду модели. `query` нужен одному решению: Esc
/// сначала чистит непустой запрос и только потом закрывает окно.
pub fn translate(event: KeyEvent, query: &str) -> WpInput {
    if event.logo {
        return WpInput::Ignore;
    }
    if event.ctrl {
        return WpInput::Ignore;
    }
    match event.key {
        Key::Up => WpInput::Move(Dir::Up),
        Key::Down => WpInput::Move(Dir::Down),
        Key::Left => WpInput::Move(Dir::Left),
        Key::Right => WpInput::Move(Dir::Right),
        Key::PageUp => WpInput::Move(Dir::PageUp),
        Key::PageDown => WpInput::Move(Dir::PageDown),
        Key::Home => WpInput::Move(Dir::Home),
        Key::End => WpInput::Move(Dir::End),
        Key::Tab => WpInput::Tab(!event.shift),
        Key::Enter => WpInput::Activate,
        Key::Escape => {
            if query.is_empty() {
                WpInput::Close
            } else {
                WpInput::ClearQuery
            }
        }
        Key::Backspace => {
            if query.is_empty() {
                WpInput::Ignore
            } else {
                WpInput::Erase
            }
        }
        Key::Char(ch) => WpInput::Type(ch),
        Key::Other => WpInput::Ignore,
    }
}

/// Применяет команду к состоянию. Возвращает исход для окна: применить файл,
/// закрыть или ничего. Переходы между зонами живут здесь, а не в окне.
pub fn apply(state: &mut State, input: WpInput) -> Outcome {
    match input {
        WpInput::Ignore => Outcome::None,
        WpInput::Move(dir) => move_in_zone(state, dir),
        WpInput::Tab(forward) => {
            state.zone = match (state.zone, forward) {
                (Zone::Folders, true) => Zone::Grid,
                (Zone::Grid, true) => Zone::Schemes,
                (Zone::Schemes, true) => Zone::Folders,
                (Zone::Folders, false) => Zone::Schemes,
                (Zone::Grid, false) => Zone::Folders,
                (Zone::Schemes, false) => Zone::Grid,
            };
            Outcome::None
        }
        WpInput::Activate => state.enter(),
        WpInput::Close => Outcome::Close,
        WpInput::ClearQuery => {
            state.clear();
            state.grid.select_first();
            Outcome::None
        }
        WpInput::Erase => {
            state.backspace();
            Outcome::None
        }
        WpInput::Type(ch) => {
            state.type_char(ch);
            // Печать показывает результаты: фокус в сетку, выбор на первый.
            state.zone = Zone::Grid;
            state.grid.select_first();
            Outcome::None
        }
    }
}

/// Стрелки внутри активной зоны с двумя переходами: ← из первой колонки
/// сетки уходит в папки, → из папок — в сетку.
fn move_in_zone(state: &mut State, dir: Dir) -> Outcome {
    match state.zone {
        Zone::Folders => match dir {
            Dir::Left => Outcome::None,
            Dir::Right => {
                state.zone = Zone::Grid;
                Outcome::None
            }
            Dir::Up => {
                // Папки зациклены: вверх с первой — на последнюю.
                let len = state.folders.len();
                if len > 0 {
                    state.folder_sel = (state.folder_sel + len - 1) % len;
                }
                Outcome::None
            }
            Dir::Down => {
                // Вниз с последней — на первую.
                let len = state.folders.len();
                if len > 0 {
                    state.folder_sel = (state.folder_sel + 1) % len;
                }
                Outcome::None
            }
            Dir::PageUp => {
                state.folder_sel = state.folder_sel.saturating_sub(10);
                Outcome::None
            }
            Dir::PageDown => {
                state.folder_sel =
                    (state.folder_sel + 10).min(state.folders.len().saturating_sub(1));
                Outcome::None
            }
            Dir::Home => {
                state.folder_sel = 0;
                Outcome::None
            }
            Dir::End => {
                state.folder_sel = state.folders.len().saturating_sub(1);
                Outcome::None
            }
        },
        Zone::Grid => {
            // ← в первой колонке — не упирается в край, а уходит в папки.
            if dir == Dir::Left && state.grid.sel.is_multiple_of(state.grid.cols.max(1)) {
                state.zone = Zone::Folders;
                return Outcome::None;
            }
            // ↓ из последнего ряда — в схемы, которые нарисованы под сеткой.
            // Правило то же, что и для ← в папки: жест в сторону соседней зоны
            // уходит из зоны, а не замыкает кольцо. По вертикали кольцо не
            // теряется — ↑ с первого ряда уводит на последний.
            if dir == Dir::Down && state.grid.in_last_row() {
                state.zone = Zone::Schemes;
                return Outcome::None;
            }
            state.grid.move_dir(dir);
            Outcome::None
        }
        Zone::Schemes => {
            // ↑ с первого ряда схем — в сетку над ними. ←/→ остаются кольцом
            // чипов, а ↑ со второго ряда по-прежнему переходит на первый.
            if dir == Dir::Up && state.scheme_sel < layout::SCHEME_COLS {
                state.zone = Zone::Grid;
                return Outcome::None;
            }
            state.move_dir(dir);
            Outcome::None
        }
    }
}

/// Клик: папка выбирается сразу, плитка выбирается первым кликом и
/// применяется вторым, схема выбирается. Крестик в поиске чистит запрос.
pub fn click(state: &mut State, card: Rect, x: f32, y: f32) -> Outcome {
    let zones = layout::zones(card);
    let field = layout::search(card);
    if !state.query.is_empty() && layout::search_clear(field).contains(x, y) {
        state.clear();
        state.grid.select_first();
        return Outcome::None;
    }
    if let Some(index) = layout::folder_at(zones, state.folders.len(), x, y) {
        state.zone = Zone::Folders;
        state.select_folder(index);
        return Outcome::None;
    }
    if let Some(index) = layout::cell_at(zones, state.grid.top_row, state.filtered.len(), x, y) {
        state.zone = Zone::Grid;
        if state.grid.sel == index {
            return state.enter();
        }
        state.grid.sel = index;
        state.grid.ensure_visible();
        return Outcome::None;
    }
    if let Some(index) = layout::chip_at(zones, x, y) {
        state.zone = Zone::Schemes;
        state.set_scheme(index);
        return Outcome::None;
    }
    Outcome::None
}

/// Движение мыши: только подсветка, выбор не трогает. Возвращает истину,
// если подсветка сменилась и кадр надо перерисовать.
pub fn motion(state: &mut State, card: Rect, x: f32, y: f32) -> bool {
    let zones = layout::zones(card);
    let next = layout::folder_at(zones, state.folders.len(), x, y)
        .map(Hover::Folder)
        .or_else(|| {
            layout::cell_at(zones, state.grid.top_row, state.filtered.len(), x, y).map(Hover::Cell)
        })
        .or_else(|| layout::chip_at(zones, x, y).map(Hover::Scheme))
        .unwrap_or(Hover::None);
    if next == state.hover {
        return false;
    }
    state.hover = next;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            ..KeyEvent::default()
        }
    }

    fn shift(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            shift: true,
            ..KeyEvent::default()
        }
    }

    fn card() -> Rect {
        Rect::new(0.0, 0.0, layout::CARD_W, layout::CARD_H)
    }

    /// Временное дерево: две папки по несколько файлов.
    fn mock_state() -> State {
        let root = std::env::temp_dir().join(format!(
            "hudbar-wp-input-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("anime")).unwrap();
        std::fs::create_dir_all(root.join("nature")).unwrap();
        for name in ["one.png", "two.png", "three.png", "four.png", "five.png"] {
            std::fs::write(root.join("anime").join(name), b"x").unwrap();
        }
        for name in ["a.png", "b.png"] {
            std::fs::write(root.join("nature").join(name), b"x").unwrap();
        }
        State::open_at(&root)
    }

    /// Стрелки и дальние клавиши превращаются в движение.
    #[test]
    fn arrows_and_far_keys_move() {
        assert_eq!(translate(press(Key::Up), ""), WpInput::Move(Dir::Up));
        assert_eq!(translate(press(Key::Down), ""), WpInput::Move(Dir::Down));
        assert_eq!(translate(press(Key::Left), ""), WpInput::Move(Dir::Left));
        assert_eq!(translate(press(Key::Right), ""), WpInput::Move(Dir::Right));
        assert_eq!(
            translate(press(Key::PageUp), ""),
            WpInput::Move(Dir::PageUp)
        );
        assert_eq!(
            translate(press(Key::PageDown), ""),
            WpInput::Move(Dir::PageDown)
        );
        assert_eq!(translate(press(Key::Home), ""), WpInput::Move(Dir::Home));
        assert_eq!(translate(press(Key::End), ""), WpInput::Move(Dir::End));
    }

    /// Tab вперёд, Shift+Tab назад; Super и Ctrl не проходят.
    #[test]
    fn tab_goes_forward_and_shift_tab_goes_back() {
        assert_eq!(translate(press(Key::Tab), ""), WpInput::Tab(true));
        assert_eq!(translate(shift(Key::Tab), ""), WpInput::Tab(false));
        let logo = KeyEvent {
            key: Key::Tab,
            logo: true,
            ..KeyEvent::default()
        };
        assert_eq!(translate(logo, ""), WpInput::Ignore);
        let ctrl = KeyEvent {
            key: Key::Up,
            ctrl: true,
            ..KeyEvent::default()
        };
        assert_eq!(translate(ctrl, ""), WpInput::Ignore);
    }

    /// Esc чистит запрос, иначе закрывает; Backspace на пустом — мимо.
    #[test]
    fn escape_clears_before_closing() {
        assert_eq!(translate(press(Key::Escape), "ани"), WpInput::ClearQuery);
        assert_eq!(translate(press(Key::Escape), ""), WpInput::Close);
        assert_eq!(translate(press(Key::Backspace), "а"), WpInput::Erase);
        assert_eq!(translate(press(Key::Backspace), ""), WpInput::Ignore);
        assert_eq!(translate(press(Key::Enter), ""), WpInput::Activate);
    }

    /// Печатные символы, включая кириллицу, идут в поиск.
    #[test]
    fn printable_characters_go_into_the_query() {
        assert_eq!(translate(press(Key::Char('ж')), ""), WpInput::Type('ж'));
        assert_eq!(translate(press(Key::Char('7')), ""), WpInput::Type('7'));
    }

    /// Коды XKB читаются без текста: Tab, Enter, стрелки.
    #[test]
    fn keys_without_text_read_their_codes() {
        assert_eq!(event(xkb::TAB, None, false, false, false).key, Key::Tab);
        assert_eq!(
            event(xkb::RETURN, None, false, false, false).key,
            Key::Enter
        );
        assert_eq!(
            event(xkb::KP_ENTER, None, false, false, false).key,
            Key::Enter
        );
        assert_eq!(
            event(xkb::ESCAPE, None, false, false, false).key,
            Key::Escape
        );
        assert_eq!(event(xkb::LEFT, None, false, false, false).key, Key::Left);
    }

    /// Кириллица с русской раскладки приходит текстом и печатается.
    #[test]
    fn cyrillic_from_a_russian_layout_is_typed() {
        let key = event(0x0063, Some("с"), false, false, false);
        assert_eq!(key.key, Key::Char('с'));
        assert_eq!(translate(key, ""), WpInput::Type('с'));
    }

    /// Tab гоняет зону по кругу в обе стороны.
    #[test]
    fn tab_cycles_zones_both_ways() {
        let mut state = mock_state();
        state.zone = Zone::Folders;
        apply(&mut state, WpInput::Tab(true));
        assert_eq!(state.zone, Zone::Grid);
        apply(&mut state, WpInput::Tab(true));
        assert_eq!(state.zone, Zone::Schemes);
        apply(&mut state, WpInput::Tab(true));
        assert_eq!(state.zone, Zone::Folders);
        apply(&mut state, WpInput::Tab(false));
        assert_eq!(state.zone, Zone::Schemes);
        apply(&mut state, WpInput::Tab(false));
        assert_eq!(state.zone, Zone::Grid);
        apply(&mut state, WpInput::Tab(false));
        assert_eq!(state.zone, Zone::Folders);
    }

    /// Папки зациклены: ↑ с первой — на последнюю, ↓ с последней — на первую.
    #[test]
    fn folders_wrap_around_on_up_and_down() {
        let mut state = mock_state();
        state.select_folder(0);
        state.zone = Zone::Folders;
        assert!(state.folders.len() >= 2, "нужны хотя бы две папки");

        apply(&mut state, WpInput::Move(Dir::Up));
        assert_eq!(state.folder_sel, state.folders.len() - 1);

        apply(&mut state, WpInput::Move(Dir::Down));
        assert_eq!(state.folder_sel, 0);
    }

    /// Схемы: ←/→ идут по кольцу чипов, ↑ со второго ряда — на первый.
    #[test]
    fn schemes_ring_horizontally_and_step_up_between_rows() {
        let mut state = mock_state();
        state.zone = Zone::Schemes;
        let len = state.schemes.len();
        assert!(len > super::super::layout::SCHEME_COLS);

        state.scheme_sel = 0;
        apply(&mut state, WpInput::Move(Dir::Left));
        assert_eq!(state.scheme_sel, len - 1);
        apply(&mut state, WpInput::Move(Dir::Right));
        assert_eq!(state.scheme_sel, 0);

        // ↑ со второго ряда держит колонку и поднимает на первый ряд.
        state.scheme_sel = super::super::layout::SCHEME_COLS + 1;
        apply(&mut state, WpInput::Move(Dir::Up));
        assert_eq!(
            state.scheme_sel, 1,
            "вверх со второго ряда — первый, та же колонка"
        );
    }

    /// ↓ из последнего ряда сетки уводит в схемы, ↑ из первого ряда схем
    /// возвращает в сетку. Раньше схемы были тупиком: попасть туда мог
    /// было только Tab.
    #[test]
    fn down_from_last_row_reaches_schemes_and_up_comes_back() {
        let mut state = mock_state();
        state.select_folder(0);
        state.filtered = (0..8)
            .map(|i| PathBuf::from(format!("/tmp/wall-{i}.png")))
            .collect();
        state.grid = super::super::grid::Grid::new(4, 3, 8);
        state.zone = Zone::Grid;

        // 8 плиток при cols=4 — это два ряда, второй полностью заполнен.
        state.grid.sel = 4;
        apply(&mut state, WpInput::Move(Dir::Down));
        assert_eq!(state.zone, Zone::Schemes, "↓ из последнего ряда — в схемы");

        // Схемы лежат в два ряда, поэтому ↑ со второго ряда сперва поднимает
        // на первый, и только следующий ↑ уходит из зоны.
        state.scheme_sel = layout::SCHEME_COLS + 1;
        apply(&mut state, WpInput::Move(Dir::Up));
        assert_eq!(state.scheme_sel, 1, "↑ со второго ряда — на первый");
        assert_eq!(state.zone, Zone::Schemes, "зона ещё не покинута");
        apply(&mut state, WpInput::Move(Dir::Up));
        assert_eq!(state.zone, Zone::Grid, "↑ из первого ряда схем — в сетку");
        assert_eq!(
            state.grid.sel, 4,
            "выбор сетки сохранился, вернулись туда же"
        );
    }

    /// ↑ с первого ряда сетки по-прежнему заворачивает на последний, даже
    /// когда ↓ из него уводит в схемы: по вертикали кольцо не теряется.
    #[test]
    fn up_from_first_row_still_wraps_to_the_last() {
        let mut state = mock_state();
        state.filtered = (0..8)
            .map(|i| PathBuf::from(format!("/tmp/wall-{i}.png")))
            .collect();
        state.grid = super::super::grid::Grid::new(4, 3, 8);
        state.zone = Zone::Grid;

        state.grid.sel = 1;
        apply(&mut state, WpInput::Move(Dir::Up));
        assert_eq!(state.zone, Zone::Grid, "зона не сменилась");
        assert_eq!(state.grid.sel, 5, "вверх с первого ряда — последний ряд");
    }

    /// ↓ не из последнего ряда двигает по сетке, а не уводит в схемы.
    #[test]
    fn down_above_the_last_row_stays_in_the_grid() {
        let mut state = mock_state();
        state.filtered = (0..8)
            .map(|i| PathBuf::from(format!("/tmp/wall-{i}.png")))
            .collect();
        state.grid = super::super::grid::Grid::new(4, 3, 8);
        state.zone = Zone::Grid;

        state.grid.sel = 1;
        apply(&mut state, WpInput::Move(Dir::Down));
        assert_eq!(state.zone, Zone::Grid);
        assert_eq!(state.grid.sel, 5);
    }

    /// ← из первой колонки уходит в папки, → из папок — в сетку.
    #[test]
    fn left_from_first_column_goes_to_folders_and_right_returns() {
        let mut state = mock_state();
        state.select_folder(0);
        state.zone = Zone::Grid;
        state.grid.sel = 4; // начало второго ряда при cols=4
        apply(&mut state, WpInput::Move(Dir::Left));
        assert_eq!(state.zone, Zone::Folders, "← из первой колонки — в папки");
        assert_eq!(state.grid.sel, 4, "выбор сетки не тронут");
        apply(&mut state, WpInput::Move(Dir::Right));
        assert_eq!(state.zone, Zone::Grid, "→ из папок — в сетку");
        // Не первая колонка: ← двигает внутри сетки.
        state.grid.sel = 5;
        apply(&mut state, WpInput::Move(Dir::Left));
        assert_eq!(state.zone, Zone::Grid);
        assert_eq!(state.grid.sel, 4);
    }

    /// Enter в сетке применяет файл со схемой, в папках и схемах — выбирает.
    #[test]
    fn enter_applies_in_grid_and_selects_elsewhere() {
        let mut state = mock_state();
        state.select_folder(0);
        state.zone = Zone::Grid;
        state.filtered = vec![PathBuf::from("/tmp/wall.png")];
        state.grid = super::super::grid::Grid::new(4, 3, 1);
        let outcome = apply(&mut state, WpInput::Activate);
        match outcome {
            Outcome::Apply { path, scheme } => {
                assert_eq!(path, PathBuf::from("/tmp/wall.png"));
                assert!(!scheme.is_empty());
            }
            other => panic!("ожидали Apply, получили {other:?}"),
        }
        state.zone = Zone::Schemes;
        let before = state.current_scheme.clone();
        state.scheme_sel = 3;
        apply(&mut state, WpInput::Activate);
        assert_ne!(state.current_scheme, before, "схема выбралась");
        state.zone = Zone::Folders;
        state.folder_sel = 1;
        apply(&mut state, WpInput::Activate);
        assert_eq!(state.current_folder, state.folders[1].name);
    }

    /// Печать уводит фокус в сетку и ставит выбор на первый результат.
    #[test]
    fn typing_focuses_the_grid_on_the_first_result() {
        let mut state = mock_state();
        state.select_folder(0);
        state.zone = Zone::Folders;
        state.grid.sel = 4;
        apply(&mut state, WpInput::Type('o'));
        assert_eq!(state.zone, Zone::Grid);
        assert_eq!(state.grid.sel, 0);
        assert!(!state.query.is_empty());
    }

    /// Клик по плитке выбирает, второй клик по выбранной применяет.
    #[test]
    fn click_selects_first_and_applies_on_second() {
        let mut state = mock_state();
        state.select_folder(0);
        state.zone = Zone::Folders;
        let card = card();
        let zones = layout::zones(card);
        let thumb = layout::cell(zones, 0, 1).expect("плитка");
        let outcome = click(&mut state, card, thumb.x + 1.0, thumb.y + 1.0);
        assert_eq!(outcome, Outcome::None, "первый клик только выбирает");
        assert_eq!(state.zone, Zone::Grid);
        assert_eq!(state.grid.sel, 1);
        let outcome = click(&mut state, card, thumb.x + 1.0, thumb.y + 1.0);
        assert!(
            matches!(outcome, Outcome::Apply { .. }),
            "второй клик применяет: {outcome:?}"
        );
    }

    /// Клик по папке переключает содержимое, по схеме — выбирает её.
    #[test]
    fn click_on_folder_and_scheme_selects_them() {
        let mut state = mock_state();
        let card = card();
        let zones = layout::zones(card);
        let row = layout::folder_row(zones, 2).expect("строка");
        click(&mut state, card, row.x + 1.0, row.y + 1.0);
        assert_eq!(state.zone, Zone::Folders);
        assert_eq!(state.folder_sel, 2);
        let chip = layout::chip(zones, 4).expect("плитка схемы");
        click(&mut state, card, chip.x + 1.0, chip.y + 1.0);
        assert_eq!(state.zone, Zone::Schemes);
        assert_eq!(state.scheme_sel, 4);
        assert_eq!(state.current_scheme, state.schemes[4]);
    }

    /// Hover только подсвечивает: выбор и зона стоят.
    #[test]
    fn motion_only_highlights_without_moving_selection() {
        let mut state = mock_state();
        state.select_folder(0);
        state.zone = Zone::Grid;
        state.grid.sel = 0;
        let card = card();
        let zones = layout::zones(card);
        let thumb = layout::cell(zones, 0, 3).expect("плитка");
        assert!(motion(&mut state, card, thumb.x + 1.0, thumb.y + 1.0));
        assert_eq!(state.hover, Hover::Cell(3));
        assert_eq!(state.grid.sel, 0, "выбор не уехал за курсором");
        assert_eq!(state.zone, Zone::Grid, "зона не сменилась");
        assert!(
            !motion(&mut state, card, thumb.x + 1.0, thumb.y + 1.0),
            "повтор без смены"
        );
        assert!(motion(&mut state, card, -100.0, -100.0));
        assert_eq!(state.hover, Hover::None);
    }
}

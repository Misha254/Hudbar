//! Ввод окна приложений: клавиша превращается в команду модели.
//!
//! Таблица своя, а не общая с меню, по одной причине: в меню `Ctrl+P` — это
//! «на строку вверх» (там так работает `vim`-раскладка), а в лаунчере
//! закрепление живёт на `Ctrl+F`. Разбирать клавиши здесь, а не в окне, —
//! чтобы правила проверялись тестами без композитора.

use super::super::menu::input::{Key, KeyEvent};

/// Команда модели лаунчера.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// Ничего не делать.
    Ignore,
    /// Сдвинуть выбор на столько строк.
    Move(isize),
    /// Сдвинуть выбор на столько плиток внутри ряда сетки.
    MoveCol(isize),
    /// Прокрутить список на страницу.
    Page(isize),
    /// В начало или в конец.
    Jump(bool),
    /// Запустить выбранное приложение.
    Activate,
    /// Закрепить или открепить выбранное приложение.
    Pin,
    /// Переставить закреплённое приложение на столько строк.
    Reorder(isize),
    /// Переключить плитки и список.
    ToggleView,
    /// Закрыть окно.
    Close,
    /// Стереть последний символ запроса.
    Erase,
    /// Очистить запрос целиком.
    ClearQuery,
    /// Дописать символ к запросу.
    Type(char),
}

/// Переводит клавишу в команду.
///
/// `query` нужен для двух решений, которые иначе пришлось бы принимать в окне:
/// `Esc` сначала чистит непустой запрос, а `Backspace` на пустом запросе не
/// «назад», потому что возвращаться некуда — окно открыто сразу на весь список.
pub fn translate(event: KeyEvent, query: &str) -> Input {
    // Super занят биндами niri, поэтому ввод с ним не доходит.
    if event.logo {
        return Input::Ignore;
    }
    if event.ctrl {
        return match event.key {
            // Закрепление: `Ctrl+F` — мышечная память от прежнего окна rofi.
            Key::Char('f') | Key::Char('F') => Input::Pin,
            // Ctrl+↑↓ — порядок закреплённых; Ctrl+K/J/N остаются навигацией.
            Key::Up => Input::Reorder(-1),
            Key::Down => Input::Reorder(1),
            Key::Char('k') | Key::Char('K') | Key::Char('n') | Key::Char('N') => Input::Move(-1),
            Key::Char('j') | Key::Char('J') => Input::Move(1),
            Key::PageUp => Input::Page(-1),
            Key::PageDown => Input::Page(1),
            Key::Home => Input::Jump(false),
            Key::End => Input::Jump(true),
            Key::Char('u') | Key::Char('U') => Input::ClearQuery,
            Key::Escape => Input::Close,
            _ => Input::Ignore,
        };
    }
    match event.key {
        Key::Up => Input::Move(-1),
        Key::Down => Input::Move(1),
        Key::Left => Input::MoveCol(-1),
        Key::Right => Input::MoveCol(1),
        Key::Tab => Input::ToggleView,
        Key::PageUp => Input::Page(-1),
        Key::PageDown => Input::Page(1),
        Key::Home => Input::Jump(false),
        Key::End => Input::Jump(true),
        Key::Enter => Input::Activate,
        Key::Escape => {
            if query.is_empty() {
                Input::Close
            } else {
                Input::ClearQuery
            }
        }
        Key::Backspace => {
            if query.is_empty() {
                Input::Ignore
            } else {
                Input::Erase
            }
        }
        Key::Char(ch) => Input::Type(ch),
        Key::Other => Input::Ignore,
    }
}

/// Держать ли клавишу для автоповтора. Повтор нужен только навигации:
/// удержанный `Tab` переключал бы вид пачкой, а `Enter` запускал бы приложение
/// много раз.
pub fn repeats(key: Key) -> bool {
    matches!(
        key,
        Key::Up | Key::Down | Key::Left | Key::Right | Key::PageUp | Key::PageDown
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            ..KeyEvent::default()
        }
    }

    fn ctrl(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            ctrl: true,
            ..KeyEvent::default()
        }
    }

    #[test]
    fn typing_goes_into_the_query() {
        assert_eq!(translate(press(Key::Char('ф')), ""), Input::Type('ф'));
        assert_eq!(translate(press(Key::Char('7')), "ф"), Input::Type('7'));
    }

    #[test]
    fn arrows_and_vim_keys_move_the_selection() {
        assert_eq!(translate(press(Key::Up), ""), Input::Move(-1));
        assert_eq!(translate(press(Key::Down), ""), Input::Move(1));
        assert_eq!(translate(ctrl(Key::Char('j')), ""), Input::Move(1));
        assert_eq!(translate(ctrl(Key::Char('k')), ""), Input::Move(-1));
    }

    /// `Ctrl+F` в лаунчере — это закрепление; `Ctrl+P` больше не дублирует
    /// его, а остаётся свободным.
    #[test]
    fn ctrl_f_pins_and_ctrl_p_is_free() {
        assert_eq!(translate(ctrl(Key::Char('f')), ""), Input::Pin);
        assert_eq!(translate(ctrl(Key::Char('F')), ""), Input::Pin);
        assert_eq!(translate(ctrl(Key::Char('p')), ""), Input::Ignore);
        assert_eq!(translate(ctrl(Key::Char('P')), ""), Input::Ignore);
        // Стрелки с Ctrl двигают закреплённое, а не выбор.
        assert_eq!(translate(ctrl(Key::Up), ""), Input::Reorder(-1));
        assert_eq!(translate(ctrl(Key::Down), ""), Input::Reorder(1));
        // Навигация осталась на Ctrl+K/J.
        assert_eq!(translate(ctrl(Key::Char('k')), ""), Input::Move(-1));
        assert_eq!(translate(ctrl(Key::Char('j')), ""), Input::Move(1));
    }

    #[test]
    fn enter_launches() {
        assert_eq!(translate(press(Key::Enter), ""), Input::Activate);
    }

    #[test]
    fn escape_clears_the_query_before_closing() {
        assert_eq!(translate(press(Key::Escape), "фай"), Input::ClearQuery);
        assert_eq!(translate(press(Key::Escape), ""), Input::Close);
        assert_eq!(translate(ctrl(Key::Escape), "фай"), Input::Close);
    }

    /// На пустом запросе стирать нечего, и «назад» возвращать некуда: в меню
    /// Backspace на корне закрывает окно, здесь список один.
    #[test]
    fn backspace_erases_only_a_symbol() {
        assert_eq!(translate(press(Key::Backspace), "фай"), Input::Erase);
        assert_eq!(translate(press(Key::Backspace), ""), Input::Ignore);
    }

    #[test]
    fn ctrl_u_clears_the_query() {
        assert_eq!(translate(ctrl(Key::Char('u')), "фай"), Input::ClearQuery);
    }

    #[test]
    fn pages_and_ends_jump_further() {
        assert_eq!(translate(press(Key::PageDown), ""), Input::Page(1));
        assert_eq!(translate(press(Key::PageUp), ""), Input::Page(-1));
        assert_eq!(translate(press(Key::Home), ""), Input::Jump(false));
        assert_eq!(translate(press(Key::End), ""), Input::Jump(true));
    }

    /// Стрелки вбок двигают выбор по ряду сетки; в списочном виде модель
    /// просто ничего не сделает. `Enter` уже запускает.
    #[test]
    fn side_arrows_move_within_a_grid_row() {
        assert_eq!(translate(press(Key::Left), ""), Input::MoveCol(-1));
        assert_eq!(translate(press(Key::Right), ""), Input::MoveCol(1));
        assert_eq!(translate(ctrl(Key::Left), ""), Input::Ignore);
    }

    /// `Tab` переключает плитки и список; с `Ctrl` комбинация не наша.
    #[test]
    fn tab_toggles_the_view() {
        assert_eq!(translate(press(Key::Tab), ""), Input::ToggleView);
        assert_eq!(translate(ctrl(Key::Tab), ""), Input::Ignore);
    }

    #[test]
    fn super_never_reaches_the_window() {
        let event = KeyEvent {
            key: Key::Char('a'),
            logo: true,
            ..KeyEvent::default()
        };
        assert_eq!(translate(event, ""), Input::Ignore);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(translate(press(Key::Other), ""), Input::Ignore);
        assert_eq!(translate(ctrl(Key::Other), ""), Input::Ignore);
    }

    /// Автоповтор только у навигации: иначе удержание `Tab` или `Enter`
    /// срабатывало бы пачкой.
    #[test]
    fn only_navigation_keys_repeat() {
        for key in [
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::PageUp,
            Key::PageDown,
        ] {
            assert!(repeats(key), "{key:?} должна повторяться");
        }
        for key in [
            Key::Tab,
            Key::Enter,
            Key::Escape,
            Key::Backspace,
            Key::Home,
            Key::End,
            Key::Char('a'),
            Key::Other,
        ] {
            assert!(!repeats(key), "{key:?} повторяться не должна");
        }
    }
}

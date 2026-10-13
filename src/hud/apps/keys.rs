//! Ввод окна приложений: клавиша превращается в команду модели.
//!
//! Таблица своя, а не общая с меню, по одной причине: в меню `Ctrl+P` — это
//! «на строку вверх» (там так работает `vim`-раскладка), а в лаунчере та же
//! клавиша закрепляет приложение. Разбирать клавиши здесь, а не в окне, —
//! чтобы правила проверялись тестами без композитора.

use super::super::menu::input::{Key, KeyEvent};

/// Команда модели лаунчера.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// Ничего не делать.
    Ignore,
    /// Сдвинуть выбор на столько строк.
    Move(isize),
    /// Прокрутить список на страницу.
    Page(isize),
    /// В начало или в конец.
    Jump(bool),
    /// Запустить выбранное приложение.
    Activate,
    /// Закрепить или открепить выбранное приложение.
    Pin,
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
            // Закрепление: `Ctrl+P` — по первой букве слова, `Ctrl+F` оставлен
            // для мышечной памяти от прежнего окна rofi, где закрепляли им же.
            Key::Char('p') | Key::Char('P') | Key::Char('f') | Key::Char('F') => Input::Pin,
            Key::Up | Key::Char('k') | Key::Char('K') | Key::Char('n') | Key::Char('N') => {
                Input::Move(-1)
            }
            Key::Down | Key::Char('j') | Key::Char('J') => Input::Move(1),
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
        Key::Other | Key::Left | Key::Right => Input::Ignore,
    }
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

    /// `Ctrl+P` в лаунчере — это закрепление, а не «на строку вверх», как в
    /// меню. Расхождение намеренное: ради него у окна своя таблица.
    #[test]
    fn ctrl_p_pins_instead_of_moving_up() {
        assert_eq!(translate(ctrl(Key::Char('p')), ""), Input::Pin);
        assert_eq!(translate(ctrl(Key::Char('P')), ""), Input::Pin);
        // Мышечная память rofi: там закрепляли `Ctrl+F`.
        assert_eq!(translate(ctrl(Key::Char('f')), ""), Input::Pin);
        // Стрелка вверх осталась навигацией.
        assert_eq!(translate(ctrl(Key::Up), ""), Input::Move(-1));
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

    /// Стрелки вбок в лаунчере ничего не делают: единственный уровень списка
    /// уходить некуда, а `Enter` уже запускает.
    #[test]
    fn side_arrows_do_nothing() {
        assert_eq!(translate(press(Key::Left), ""), Input::Ignore);
        assert_eq!(translate(press(Key::Right), ""), Input::Ignore);
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
}

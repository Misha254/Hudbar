//! Ввод меню: клавиша и состояние запроса превращаются в команду модели.
//!
//! Модуль ничего не знает про Wayland: на входе ключ в виде [`KeyEvent`] и
//! текущий запрос, на выходе — [`MenuInput`]. Всё это можно разобрать в
//! тестах, не поднимая композитор, и именно поэтому ввод вынесен из окна.
//!
//! Раскладка. Правила те же, что в окнах биндов: `Ctrl` и `Super`
//! служебные, а `AltGr` на русской раскладке даёт ввод символов — поэтому
//! модификаторы проверяются по отдельности, и `Alt` не мешает печатать.
//! Транслитерацию запроса делает сам список (`Item::matches`), здесь только
//! доходит символ.

/// Клавиша в понятных терминах. Номера ключей приходят из XKB, но хранить их
/// здесь нельзя: тесты должны проверять смысл, а не коды.
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
    Enter,
    Backspace,
    Escape,
    /// Печатный символ: уже с учётом раскладки и AltGr.
    Char(char),
    /// Клавиша без команды: буквы в комбинациях, служебные клавиши.
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
pub enum MenuInput {
    /// Ничего не делать.
    Ignore,
    /// Сдвинуть выбор на столько строк.
    Move(isize),
    /// `Shift+↑`/`Shift+↓` над действием «порядок модулей».
    Nudge(isize),
    /// Прокрутить список на страницу.
    Page(isize),
    /// В начало или в конец.
    Jump(bool),
    /// Войти в подменю или применить значение.
    Activate,
    /// Назад на уровень.
    Back,
    /// Закрыть меню.
    Close,
    /// Стереть последний символ запроса.
    Erase,
    /// Очистить запрос целиком.
    ClearQuery,
    /// Дописать символ к запросу.
    Type(char),
}

/// Коды XKB для клавиш, у которых нет однозначного печатного символа.
/// Значения фиксированы стандартом X11 и не меняются: это единственное, что
/// позволяет разбирать ввод без `wayland-client`.
pub mod xkb {
    pub const ESCAPE: u32 = 0xff1b;
    pub const BACKSPACE: u32 = 0xff08;
    pub const RETURN: u32 = 0xff0d;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const LEFT: u32 = 0xff51;
    pub const UP: u32 = 0xff52;
    pub const RIGHT: u32 = 0xff53;
    pub const DOWN: u32 = 0xff54;
    pub const PAGE_UP: u32 = 0xff55;
    pub const PAGE_DOWN: u32 = 0xff56;
    pub const HOME: u32 = 0xff50;
    pub const END: u32 = 0xff57;

    pub const J: u32 = 0x006a;
    pub const K: u32 = 0x006b;
    pub const N: u32 = 0x006e;
    pub const P: u32 = 0x0070;
    pub const U: u32 = 0x0075;
}

/// Превращает XKB-код и текст клавиши в [`KeyEvent`].
///
/// `utf8` приходит от композитора и уже собран с учётом раскладки: на
/// русской раскладке это буквы кириллицы, а с AltGr — знаки вроде `<`. Если
/// текста нет, но код — буква, берём сам код: иначе `Ctrl+U` превратился бы в
/// печатную букву.
pub fn event(keysym: u32, utf8: Option<&str>, ctrl: bool, shift: bool, logo: bool) -> KeyEvent {
    let key = match keysym {
        xkb::ESCAPE => Key::Escape,
        xkb::BACKSPACE => Key::Backspace,
        xkb::RETURN | xkb::KP_ENTER => Key::Enter,
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

/// Переводит клавишу в команду модели.
///
/// `query` нужен для двух решений, которые иначе пришлось бы принимать в
/// окне: `Esc` сначала чистит непустой запрос, а `Backspace` на пустом
/// запросе означает «назад», а не «ничего».
pub fn translate(event: KeyEvent, query: &str) -> MenuInput {
    // Super занят биндами niri, поэтому ввод с ним не проходит дальше.
    if event.logo {
        return MenuInput::Ignore;
    }
    if event.shift && !event.ctrl && matches!(event.key, Key::Up | Key::Down) {
        return MenuInput::Nudge(match event.key {
            Key::Up => -1,
            _ => 1,
        });
    }
    if event.ctrl {
        return match event.key {
            Key::Up | Key::Char('k') | Key::Char('K') | Key::Char('p') | Key::Char('P') => {
                MenuInput::Move(-1)
            }
            Key::Down | Key::Char('j') | Key::Char('J') | Key::Char('n') | Key::Char('N') => {
                MenuInput::Move(1)
            }
            Key::PageUp => MenuInput::Page(-1),
            Key::PageDown => MenuInput::Page(1),
            Key::Home => MenuInput::Jump(false),
            Key::End => MenuInput::Jump(true),
            Key::Char('u') | Key::Char('U') => MenuInput::ClearQuery,
            Key::Escape => MenuInput::Close,
            _ => MenuInput::Ignore,
        };
    }
    match event.key {
        Key::Up => MenuInput::Move(-1),
        Key::Down => MenuInput::Move(1),
        Key::PageUp => MenuInput::Page(-1),
        Key::PageDown => MenuInput::Page(1),
        Key::Home => MenuInput::Jump(false),
        Key::End => MenuInput::Jump(true),
        Key::Enter | Key::Right => MenuInput::Activate,
        Key::Left => MenuInput::Back,
        Key::Escape => {
            if query.is_empty() {
                MenuInput::Back
            } else {
                MenuInput::ClearQuery
            }
        }
        Key::Backspace => {
            if query.is_empty() {
                MenuInput::Back
            } else {
                MenuInput::Erase
            }
        }
        Key::Char(ch) => MenuInput::Type(ch),
        Key::Other => MenuInput::Ignore,
    }
}

/// Переводит клавишу в команду модели для режима ввода секрета.
///
/// Отдельная таблица, а не флаг у [`translate`], потому что правила другие
/// и противоречат обычным: `Esc` отменяет ввод, а не возвращает на уровень
/// (и тем более не закрывает меню), `Backspace` стирает символ даже при пустом
/// поле, стрелки и `Ctrl+J/K` не двигают выбранную сеть, а `Ctrl+U` чистит
/// пароль. `Super` по-прежнему не доходит: это бинды niri.
pub fn translate_secret(event: KeyEvent) -> MenuInput {
    if event.logo {
        return MenuInput::Ignore;
    }
    if event.ctrl {
        return match event.key {
            Key::Char('u') | Key::Char('U') => MenuInput::ClearQuery,
            Key::Escape => MenuInput::Close,
            _ => MenuInput::Ignore,
        };
    }
    match event.key {
        Key::Char(ch) => MenuInput::Type(ch),
        Key::Backspace => MenuInput::Erase,
        Key::Enter | Key::Right => MenuInput::Activate,
        Key::Escape => MenuInput::Close,
        _ => MenuInput::Ignore,
    }
}

/// Что означает нажатие при прямом входе в раздел: `Esc` с этого уровня
/// закрывает меню, а не возвращает на корень. Функция отдельная, потому что
/// решается не по клавише, а по тому, как открыли меню.
pub fn escape_closes_at_root(direct: bool, depth: usize) -> bool {
    direct && depth == 1
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
    fn arrows_and_vim_keys_move_the_selection() {
        assert_eq!(translate(press(Key::Up), ""), MenuInput::Move(-1));
        assert_eq!(translate(press(Key::Down), ""), MenuInput::Move(1));
        assert_eq!(translate(ctrl(Key::Char('k')), ""), MenuInput::Move(-1));
        assert_eq!(translate(ctrl(Key::Char('j')), ""), MenuInput::Move(1));
        assert_eq!(translate(ctrl(Key::Char('p')), ""), MenuInput::Move(-1));
        assert_eq!(translate(ctrl(Key::Char('n')), ""), MenuInput::Move(1));
    }

    #[test]
    fn pages_and_homes_jump_further() {
        assert_eq!(translate(press(Key::PageUp), ""), MenuInput::Page(-1));
        assert_eq!(translate(press(Key::PageDown), ""), MenuInput::Page(1));
        assert_eq!(translate(press(Key::Home), ""), MenuInput::Jump(false));
        assert_eq!(translate(press(Key::End), ""), MenuInput::Jump(true));
        assert_eq!(translate(ctrl(Key::PageUp), ""), MenuInput::Page(-1));
        assert_eq!(translate(ctrl(Key::Home), ""), MenuInput::Jump(false));
    }

    #[test]
    fn enter_and_right_activate_left_goes_back() {
        assert_eq!(translate(press(Key::Enter), ""), MenuInput::Activate);
        assert_eq!(translate(press(Key::Right), ""), MenuInput::Activate);
        assert_eq!(translate(press(Key::Left), ""), MenuInput::Back);
    }

    #[test]
    fn escape_clears_the_query_before_anything_else() {
        assert_eq!(translate(press(Key::Escape), "dnd"), MenuInput::ClearQuery);
        assert_eq!(translate(press(Key::Escape), ""), MenuInput::Back);
    }

    #[test]
    fn backspace_erases_or_goes_back() {
        assert_eq!(translate(press(Key::Backspace), "dnd"), MenuInput::Erase);
        assert_eq!(translate(press(Key::Backspace), ""), MenuInput::Back);
    }

    #[test]
    fn ctrl_u_clears_the_query() {
        assert_eq!(
            translate(ctrl(Key::Char('u')), "dnd"),
            MenuInput::ClearQuery
        );
        assert_eq!(
            translate(ctrl(Key::Char('U')), "dnd"),
            MenuInput::ClearQuery
        );
    }

    #[test]
    fn printable_characters_go_into_the_query() {
        assert_eq!(translate(press(Key::Char('щ')), ""), MenuInput::Type('щ'));
        assert_eq!(translate(press(Key::Char('7')), ""), MenuInput::Type('7'));
        assert_eq!(translate(press(Key::Char(' ')), "d"), MenuInput::Type(' '));
    }

    /// `Super` у niri занят биндами: если событие всё же дошло до меню, его
    /// надо проигнорировать, иначе меню перехватывало бы чужие комбинации.
    #[test]
    fn super_never_reaches_the_menu() {
        assert_eq!(
            translate(press(Key::Char('a')), ""),
            MenuInput::Type('a'),
            "без Super буква идёт в поиск"
        );
        let event = KeyEvent {
            key: Key::Char('a'),
            logo: true,
            ..KeyEvent::default()
        };
        assert_eq!(translate(event, ""), MenuInput::Ignore);
        let arrows = KeyEvent {
            key: Key::Down,
            logo: true,
            ..KeyEvent::default()
        };
        assert_eq!(translate(arrows, ""), MenuInput::Ignore);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(translate(press(Key::Other), ""), MenuInput::Ignore);
        assert_eq!(translate(ctrl(Key::Other), ""), MenuInput::Ignore);
    }

    /// Клавиши без текста читаются по коду: иначе `Ctrl+U` превратился бы в
    /// печатную букву, а `U` сработала бы как ввод.
    #[test]
    fn keys_without_text_read_their_codes() {
        let escape = event(xkb::ESCAPE, None, false, false, false);
        assert_eq!(escape.key, Key::Escape);
        let enter = event(xkb::RETURN, None, false, false, false);
        assert_eq!(enter.key, Key::Enter);
        let keypad = event(xkb::KP_ENTER, None, false, false, false);
        assert_eq!(keypad.key, Key::Enter);
        for (code, key) in [
            (xkb::UP, Key::Up),
            (xkb::DOWN, Key::Down),
            (xkb::LEFT, Key::Left),
            (xkb::RIGHT, Key::Right),
            (xkb::PAGE_UP, Key::PageUp),
            (xkb::PAGE_DOWN, Key::PageDown),
            (xkb::HOME, Key::Home),
            (xkb::END, Key::End),
            (xkb::BACKSPACE, Key::Backspace),
        ] {
            assert_eq!(event(code, None, false, false, false).key, key);
        }
    }

    /// Кириллица с русской раскладки приходит как `utf8` и должна печататься.
    #[test]
    fn cyrillic_from_a_russian_layout_is_typed() {
        let key = event(0x0063, Some("с"), false, false, false);
        assert_eq!(key.key, Key::Char('с'));
        assert_eq!(translate(key, ""), MenuInput::Type('с'));
    }

    /// AltGr на русской раскладке даёт знаки вроде `<` — они печатаются, а не
    /// считаются служебной комбинацией.
    #[test]
    fn altgr_symbols_are_typed_not_ignored() {
        let key = event(0x006c, Some("<"), false, false, false);
        assert_eq!(translate(key, ""), MenuInput::Type('<'));
    }

    #[test]
    fn multiple_characters_from_one_key_are_ignored() {
        let key = event(0x0063, Some("аб"), false, false, false);
        assert_eq!(key.key, Key::Other);
    }

    #[test]
    fn control_characters_never_enter_the_query() {
        let key = event(0x0063, Some("a\nb"), false, false, false);
        assert_eq!(key.key, Key::Other, "в тексте есть перевод строки");
    }

    #[test]
    fn direct_entry_closes_from_its_own_level() {
        assert!(escape_closes_at_root(true, 1), "прямой вход: Esc закрывает");
        assert!(
            !escape_closes_at_root(true, 2),
            "ниже первого уровня — назад"
        );
        assert!(!escape_closes_at_root(false, 1), "обычный вход: Esc назад");
        assert!(!escape_closes_at_root(false, 0));
    }

    /// В режиме пароля правила свои: `Esc` отменяет ввод, `Backspace` стирает
    /// символ даже при пустом поле, `Ctrl+U` чистит буфер.
    #[test]
    fn secret_mode_maps_keys_to_the_password_buffer() {
        assert_eq!(
            translate_secret(press(Key::Char('щ'))),
            MenuInput::Type('щ')
        );
        assert_eq!(
            translate_secret(press(Key::Char(' '))),
            MenuInput::Type(' ')
        );
        assert_eq!(
            translate_secret(press(Key::Char('7'))),
            MenuInput::Type('7')
        );
        assert_eq!(translate_secret(press(Key::Backspace)), MenuInput::Erase);
        assert_eq!(translate_secret(press(Key::Enter)), MenuInput::Activate);
        assert_eq!(translate_secret(press(Key::Escape)), MenuInput::Close);
        assert_eq!(
            translate_secret(ctrl(Key::Char('u'))),
            MenuInput::ClearQuery
        );
        assert_eq!(
            translate_secret(ctrl(Key::Char('U'))),
            MenuInput::ClearQuery
        );
    }

    /// Выбор сети и прокрутка во время ввода пароля не двигаются: список не
    /// должен ездить под руками, пока печатается секрет.
    #[test]
    fn secret_mode_ignores_navigation() {
        for key in [
            Key::Up,
            Key::Down,
            Key::PageUp,
            Key::PageDown,
            Key::Home,
            Key::End,
        ] {
            assert_eq!(translate_secret(press(key)), MenuInput::Ignore, "{key:?}");
        }
        for key in [
            Key::Char('j'),
            Key::Char('k'),
            Key::Char('n'),
            Key::Char('p'),
        ] {
            assert_eq!(
                translate_secret(ctrl(key)),
                MenuInput::Ignore,
                "Ctrl+{key:?} не двигает выбор"
            );
        }
        assert_eq!(
            translate_secret(KeyEvent {
                key: Key::Down,
                shift: true,
                ..Default::default()
            }),
            MenuInput::Ignore,
            "Shift+стрелка тоже не трогает порядок модулей"
        );
    }

    /// `Super` в режиме пароля по-прежнему не доходит до меню.
    #[test]
    fn secret_mode_still_ignores_super() {
        let event = KeyEvent {
            key: Key::Char('a'),
            logo: true,
            ..KeyEvent::default()
        };
        assert_eq!(translate_secret(event), MenuInput::Ignore);
    }

    #[test]
    fn shift_arrows_nudge_the_module_order() {
        assert_eq!(
            translate(
                KeyEvent {
                    key: Key::Up,
                    shift: true,
                    ..Default::default()
                },
                ""
            ),
            MenuInput::Nudge(-1)
        );
        assert_eq!(
            translate(
                KeyEvent {
                    key: Key::Down,
                    shift: true,
                    ..Default::default()
                },
                ""
            ),
            MenuInput::Nudge(1)
        );
        assert_eq!(
            translate(
                KeyEvent {
                    key: Key::Up,
                    shift: false,
                    ..Default::default()
                },
                ""
            ),
            MenuInput::Move(-1)
        );
    }
}

//! Подписи экрана блокировки: те же языки, что у панели и гритера.

use crate::settings::Language;

pub struct Strings {
    pub title: &'static str,
    pub user: &'static str,
    pub password: &'static str,
    pub unlock: &'static str,
    pub checking: &'static str,
    pub wrong: &'static str,
    pub caps: &'static str,
    pub hints: &'static str,
}

pub const RU: Strings = Strings {
    title: "HUDBAR",
    user: "Пользователь",
    password: "Пароль",
    unlock: "РАЗБЛОКИРОВАТЬ",
    checking: "ПРОВЕРКА…",
    wrong: "Неверный пароль",
    caps: "CAPS LOCK",
    hints: "ENTER РАЗБЛОКИРОВАТЬ · BACKSPACE СТЕРЕТЬ",
};

pub const EN: Strings = Strings {
    title: "HUDBAR",
    user: "User",
    password: "Password",
    unlock: "UNLOCK",
    checking: "CHECKING…",
    wrong: "Wrong password",
    caps: "CAPS LOCK",
    hints: "ENTER UNLOCK · BACKSPACE DELETE",
};

pub fn text(lang: Language) -> &'static Strings {
    match lang {
        Language::Ru => &RU,
        Language::En => &EN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Английский экран не должен содержать кириллицы ни в одной подписи.
    #[test]
    fn the_english_screen_has_no_cyrillic() {
        let text = text(Language::En);
        for value in [
            text.user,
            text.password,
            text.unlock,
            text.checking,
            text.wrong,
            text.hints,
        ] {
            assert!(
                !value.chars().any(|c| matches!(c, 'А'..='я' | 'Ё' | 'ё')),
                "{value}"
            );
        }
    }
}

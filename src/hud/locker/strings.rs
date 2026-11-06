//! Подписи экрана блокировки: те же языки, что у панели и гритера.

use crate::settings::Language;

pub struct Strings {
    /// Заглушка пустого поля пароля.
    pub password: &'static str,
    pub checking: &'static str,
    pub wrong: &'static str,
    pub caps: &'static str,
}

pub const RU: Strings = Strings {
    password: "Пароль",
    checking: "ПРОВЕРКА…",
    wrong: "Неверный пароль",
    caps: "CAPS LOCK",
};

pub const EN: Strings = Strings {
    password: "Password",
    checking: "CHECKING…",
    wrong: "Wrong password",
    caps: "CAPS LOCK",
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
        for value in [text.password, text.checking, text.wrong, text.caps] {
            assert!(
                !value.chars().any(|c| matches!(c, 'А'..='я' | 'Ё' | 'ё')),
                "{value}"
            );
        }
    }
}

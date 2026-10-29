//! Подписи гритера на двух языках.
//!
//! Устроено как `app/strings.rs` панели: одна структура на язык и `text(lang)`
//! вместо `match` на месте. В гритере это важнее, чем в панели: он рисует весь
//! экран сразу, и любая строка, забытая в ветке `match`, видна на входе.
//!
//! Данные пользователя сюда не попадают и не переводятся: имя сеанса из
//! `.desktop`, имя пользователя и текст от PAM показываются как есть.

use super::settings::Language;

/// Все подписи экрана входа.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Text {
    /// Мелкий заголовок карточки.
    pub title: &'static str,
    /// Поле имени.
    pub user: &'static str,
    /// Поле пароля.
    pub password: &'static str,
    /// Акцентная кнопка.
    pub login: &'static str,
    /// Подписи полей для подвала.
    pub hint_login: &'static str,
    pub hint_field: &'static str,
    pub hint_session: &'static str,
    pub hint_power: &'static str,
    /// Режим питания в подвале.
    pub power_off: &'static str,
    pub power_reboot: &'static str,
    pub power_sleep: &'static str,
    pub power_cancel: &'static str,
    /// Метка включённого Caps Lock у поля пароля.
    pub caps: &'static str,
    /// Пока идёт проверка пароля.
    pub authenticating: &'static str,
    /// Запуск сеанса после успешного входа.
    pub starting: &'static str,
    /// Вход отклонён, а PAM не сказал почему.
    pub auth_failed: &'static str,
    /// Нет сокета greetd.
    pub no_greetd: &'static str,
    /// `start_session` не сработал.
    pub session_failed: &'static str,
    /// На машине нет ни одного `.desktop`-сеанса.
    pub no_sessions: &'static str,
    /// Заглушка имени сеанса, когда списка нет.
    pub session_unknown: &'static str,
    /// Подпись «сеанс» в подвале, когда режим питания выключен.
    pub session_prefix: &'static str,
}

impl Text {
    /// Все строки — для теста «в английском нет кириллицы» и для проверки,
    /// что ни одна подпись не осталась пустой.
    pub fn all(&self) -> [&'static str; 21] {
        [
            self.title,
            self.user,
            self.password,
            self.login,
            self.hint_login,
            self.hint_field,
            self.hint_session,
            self.hint_power,
            self.power_off,
            self.power_reboot,
            self.power_sleep,
            self.power_cancel,
            self.caps,
            self.authenticating,
            self.starting,
            self.auth_failed,
            self.no_greetd,
            self.session_failed,
            self.no_sessions,
            self.session_unknown,
            self.session_prefix,
        ]
    }
}

pub const RU: Text = Text {
    title: "HUDBAR",
    user: "Пользователь",
    password: "Пароль",
    login: "ВОЙТИ",
    hint_login: "ENTER ВОЙТИ",
    hint_field: "TAB ПОЛЕ",
    hint_session: "F2 СЕАНС",
    hint_power: "F12 ПИТАНИЕ",
    power_off: "1 ВЫКЛЮЧИТЬ",
    power_reboot: "2 ПЕРЕЗАГРУЗКА",
    power_sleep: "3 СОН",
    power_cancel: "ESC ОТМЕНА",
    caps: "CAPS LOCK",
    authenticating: "Проверяю…",
    starting: "Запускаю сеанс…",
    auth_failed: "Не удалось войти",
    no_greetd: "Нет связи с greetd",
    session_failed: "Сеанс не запустился",
    no_sessions: "Сеансы не найдены",
    session_unknown: "не найден",
    session_prefix: "СЕАНС",
};

pub const EN: Text = Text {
    title: "HUDBAR",
    user: "User",
    password: "Password",
    login: "LOG IN",
    hint_login: "ENTER LOG IN",
    hint_field: "TAB FIELD",
    hint_session: "F2 SESSION",
    hint_power: "F12 POWER",
    power_off: "1 POWER OFF",
    power_reboot: "2 REBOOT",
    power_sleep: "3 SLEEP",
    power_cancel: "ESC CANCEL",
    caps: "CAPS LOCK",
    authenticating: "Checking…",
    starting: "Starting session…",
    auth_failed: "Login failed",
    no_greetd: "No connection to greetd",
    session_failed: "Session did not start",
    no_sessions: "No sessions found",
    session_unknown: "not found",
    session_prefix: "SESSION",
};

/// Подписи на языке окна.
pub fn text(lang: Language) -> &'static Text {
    match lang {
        Language::Ru => &RU,
        Language::En => &EN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_cyrillic(text: &str) -> bool {
        text.chars().any(|c| matches!(c, 'А'..='я' | 'Ё' | 'ё'))
    }

    /// То же правило, что у меню (`menu/real.rs`, `english_tree_has_no_russian_labels`),
    /// но для экрана входа: английский набор не должен содержать кириллицы.
    /// Исключений вроде «Русский» здесь нет — все подписи служебные.
    #[test]
    fn english_text_has_no_russian_labels() {
        let bad: Vec<&str> = EN
            .all()
            .into_iter()
            .filter(|line| is_cyrillic(line))
            .collect();
        assert!(bad.is_empty(), "русские подписи в EN-гритере: {bad:?}");
    }

    /// Русский набор действительно русский: тест выше не должен проходить
    /// на пустом словаре.
    #[test]
    fn russian_text_is_actually_russian() {
        let ru = RU
            .all()
            .into_iter()
            .filter(|line| is_cyrillic(line))
            .count();
        assert!(ru >= 8, "кириллических строк в RU: {ru}");
    }

    /// Ни одна подпись не пустая: пустая строка на экране входа читается как
    /// сломанный интерфейс, а не как «подсказки нет».
    #[test]
    fn no_label_is_empty() {
        for lang in [Language::Ru, Language::En] {
            for line in text(lang).all() {
                assert!(!line.trim().is_empty(), "{lang:?}: пустая подпись");
            }
        }
    }
}

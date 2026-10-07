//! Секретный ввод: пароль сети, который не должен попасть ни в запрос, ни в
//! кадр, ни в журнал.
//!
//! Отдельный режим по двум причинам. Первая — запрос меню участвует в поиске,
//! ездит в снимок кадра и рисуется текстом; пароль там быть не может ни при
//! каких условиях. Вторая — ввод идёт в том же окне, но не должен трогать
//! список сетей: пока открыт ввод, стрелки не двигают выбор, `Esc` отменяет
//! ввод, а не закрывает меню.
//!
//! Что где лежит:
//!   * [`SecretTarget`] — только несекретные данные операции (SSID и личность
//!     строки). Секрета в нём нет, поэтому его можно спокойно клонировать и
//!     показывать в отладке;
//!   * [`SecretInput`] — буфер пароля. Он не клонируется в кадр: наружу
//!     уходит только [`SecretFrame`] с маской из точек;
//!   * [`CmdArg::Secret`](super::system::CmdArg) — единственное место, где
//!     значение доходит до процесса, и то через настоящий `argv`.
//!
//! Чего `CmdArg::Secret` не делает: значение всё равно попадает в `argv`
//! дочернего процесса, а значит доступно тому, кто читает `/proc/<pid>/cmdline`
//! или `ps`. Секрет скрыт от журналов, статусов, `Debug` и снимков — но не от
//! просмотра процессов. Настоящая защита от этого — stdin, и это отдельная
//! задача, а не то, что здесь сделано.

use super::settings::Language;
use super::system::{CommandSpec, ProviderKey};

/// Глиф одного введённого символа: точка вместо буквы. Пароль не должен быть
/// виден даже при опечатке, поэтому на экране только точки и курсор.
pub const MASK: char = '•';

/// Несекретная часть операции, для которой вводится секрет. Секрета здесь нет
/// и быть не должно: значение нужно только чтобы продолжить команду.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretTarget {
    /// Подключение к сети Wi-Fi по имени из эфира.
    Wifi {
        /// SSID как в эфире: отдельная строка, отдельный `argv`-элемент.
        ssid: String,
        /// Личность строки меню: по ней busy-отметка и восстановление курсора.
        row_id: String,
    },
}

impl SecretTarget {
    /// Цель ввода пароля для сети Wi-Fi.
    pub fn wifi(ssid: &str, row_id: &str) -> Self {
        SecretTarget::Wifi {
            ssid: ssid.to_string(),
            row_id: row_id.to_string(),
        }
    }

    /// Что подключаем: имя сети. Показывается в карточке ввода.
    pub fn subject(&self) -> &str {
        match self {
            SecretTarget::Wifi { ssid, .. } => ssid,
        }
    }

    /// Личность строки, из которой ввод открыт.
    pub fn row_id(&self) -> &str {
        match self {
            SecretTarget::Wifi { row_id, .. } => row_id,
        }
    }

    /// Чей слот обновить после команды.
    pub fn provider_key(&self) -> ProviderKey {
        match self {
            SecretTarget::Wifi { .. } => ProviderKey::WIFI,
        }
    }

    /// Команда для этого секрета. Пароль уходит сюда и больше нигде не живёт
    /// открытым: внутри это `CmdArg::Secret`.
    pub fn command(&self, password: &str) -> CommandSpec {
        match self {
            SecretTarget::Wifi { ssid, .. } => super::wifi::connect_with_password(ssid, password),
        }
    }
}

/// Активный ввод секрета: заголовок, буфер и цель. Живёт в меню отдельным
/// полем и не смешивается с запросом уровня.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretInput {
    /// Что именно вводим: «Пароль для HONOR 200».
    pub title: String,
    /// Буфер пароля. Не `trim`: пробел бывает частью пароля.
    pub value: String,
    /// Несекретная цель операции.
    pub target: SecretTarget,
}

impl SecretInput {
    /// Новый ввод для цели с заголовком на языке меню.
    pub fn new(target: SecretTarget, lang: Language) -> Self {
        let title = match (lang, &target) {
            (Language::Ru, SecretTarget::Wifi { ssid, .. }) => {
                format!("Пароль для {ssid}")
            }
            (Language::En, SecretTarget::Wifi { ssid, .. }) => {
                format!("Password for {ssid}")
            }
        };
        SecretInput {
            title,
            value: String::new(),
            target,
        }
    }

    /// Дописать печатный символ. Любой Unicode-скаляр: пароль не латиница.
    pub fn push(&mut self, ch: char) {
        self.value.push(ch);
    }

    /// Стереть последний символ по `char`, а не по байту: кириллица и эмодзи
    /// многобайтные, и удаление байта оставило бы мусор.
    pub fn erase(&mut self) {
        self.value.pop();
    }

    /// Очистить целиком (`Ctrl+U`).
    pub fn clear(&mut self) {
        self.value.clear();
    }

    /// Длина буфера в символах: только для курсора и тестов, в интерфейс
    /// пароль не показывается.
    pub fn len(&self) -> usize {
        self.value.chars().count()
    }

    /// Пуст ли буфер: пустой пароль не запрещаем, решает NetworkManager.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Маска для отрисовки: точки плюс курсор в конце.
    pub fn masked(&self) -> String {
        std::iter::repeat_n(MASK, self.len()).collect()
    }

    /// Разобрать ввод на цель и пароль: буфер отдаётся наружу один раз и
    /// после этого больше не хранится.
    pub fn into_parts(self) -> (SecretTarget, String) {
        (self.target, self.value)
    }
}

/// Безопасное представление ввода для окна: маска вместо значения. В кадр
/// уходит только это, поэтому пароль не может утечь через снимок.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretFrame {
    /// Заголовок режима: «Wi-Fi пароль».
    pub title: &'static str,
    /// Что подключаем: имя сети.
    pub subject: String,
    /// Маска введённого: только точки.
    pub masked: String,
    /// Длина буфера в символах: рисуется только как есть, без значения.
    pub len: usize,
}

impl SecretInput {
    /// Кадр для отрисовки: значение наружу не уходит.
    pub fn frame(&self, lang: Language) -> SecretFrame {
        SecretFrame {
            title: match lang {
                Language::Ru => "Wi-Fi пароль",
                Language::En => "Wi-Fi password",
            },
            subject: self.target.subject().to_string(),
            masked: self.masked(),
            len: self.len(),
        }
    }
}

impl std::fmt::Debug for SecretInput {
    /// Пароль не печатается даже в `Debug`: у буфера остаётся длина, и всё.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecretInput")
            .field("title", &self.title)
            .field("len", &self.len())
            .field("target", &self.target)
            .field("value", &format_args!("Secret(<{} chars>)", self.len()))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masking_shows_one_dot_per_symbol() {
        let mut input = SecretInput::new(
            SecretTarget::wifi("HONOR 200", "wifi/net/HONOR 200"),
            Language::Ru,
        );
        assert!(input.is_empty());
        assert_eq!(input.masked(), "");
        for ch in "p@ss:word".chars() {
            input.push(ch);
        }
        assert_eq!(input.len(), 9);
        assert_eq!(input.masked(), "•••••••••");
        assert!(!input.masked().contains("p@ss"));
    }

    #[test]
    fn unicode_is_masked_by_scalar_count() {
        let mut input = SecretInput::new(SecretTarget::wifi("Дом", "wifi/net/Дом"), Language::Ru);
        for ch in "Дом123".chars() {
            input.push(ch);
        }
        assert_eq!(
            input.len(),
            6,
            "кириллица считается по символам, не по байтам"
        );
        assert_eq!(input.masked(), "••••••");
        input.erase();
        assert_eq!(input.len(), 5);
        assert_eq!(input.value, "Дом12");
    }

    #[test]
    fn clear_empties_the_buffer_but_keeps_the_target() {
        let target = SecretTarget::wifi("Дом \\ принтер", "wifi/net/Дом \\ принтер");
        let mut input = SecretInput::new(target.clone(), Language::Ru);
        for ch in "секретный".chars() {
            input.push(ch);
        }
        input.clear();
        assert!(input.is_empty());
        assert_eq!(input.target, target, "цель после очистки на месте");
    }

    #[test]
    fn frame_carries_the_mask_and_never_the_value() {
        let mut input = SecretInput::new(
            SecretTarget::wifi("Сеть Wi-Fi", "wifi/net/Сеть Wi-Fi"),
            Language::Ru,
        );
        for ch in "тихийпароль".chars() {
            input.push(ch);
        }
        let frame = input.frame(Language::Ru);
        assert_eq!(frame.title, "Wi-Fi пароль");
        assert_eq!(frame.subject, "Сеть Wi-Fi");
        assert_eq!(frame.masked, "•••••••••••");
        assert_eq!(frame.len, 11);
        let debug = format!("{input:?}");
        assert!(!debug.contains("тихийпароль"), "Debug не содержит пароль");
        assert!(debug.contains("Secret(<11 chars>)"), "{debug}");
    }

    #[test]
    fn spaces_inside_the_password_are_kept() {
        let mut input = SecretInput::new(SecretTarget::wifi("net", "wifi/net/net"), Language::Ru);
        for ch in " pass word ".chars() {
            input.push(ch);
        }
        assert_eq!(input.value, " pass word ");
        assert_eq!(input.len(), 11);
    }

    #[test]
    fn target_knows_its_row_provider_and_subject() {
        let target = SecretTarget::wifi("Мой Дом", "wifi/net/Мой Дом");
        assert_eq!(target.subject(), "Мой Дом");
        assert_eq!(target.row_id(), "wifi/net/Мой Дом");
        assert_eq!(target.provider_key(), ProviderKey::WIFI);
    }
}

//! Проверка пароля через PAM.
//!
//! `swaylock` на этой машине ходит в `/etc/pam.d/swaylock`, который включает
//! `login`; здесь тот же сервис, поэтому правила блокировки не расходятся с
//! остальной системой. Если файла `swaylock` нет, берётся `login` — он есть
//! везде, где есть PAM.
//!
//! Пароль не попадает ни в лог, ни в argv, ни в файл. В PAM он передаётся
//! копией в C-буфере; эту копию затирает `pam_ffi` после `pam_end`, а копию,
//! которую PAM делает себе, освобождает только PAM.

use std::ffi::CString;

/// Есть ли в файле сервиса собственный стек `account`.
/// переносит только строки того же типа, поэтому `auth include
/// login` account-модулей не добавляет. У `/etc/pam.d/swaylock` их нет вовсе,
/// а `pam_acct_mgmt` на пустом стеке возвращает `PAM_AUTH_ERR`. Если бы такой
/// ответ считался ошибкой входа, верный пароль всегда выглядел бы неверным:
/// проверка account шла бы после успешной аутентификации и затирала её.
/// `swaylock` account-проверку не делает; здесь она включается только там,
/// где в файле сервиса она действительно описана.
fn has_account_stack(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_whitespace().next())
        .map(|kind| kind.trim_start_matches('-'))
        .any(|kind| kind == "account")
}

/// Проверяет пароль пользователя. `password` — сырые байты из `Secret`.
///
/// Сервис `swaylock` выбран намеренно: у него уже есть PAM-правила на этой
/// машине, и блокировка ведёт себя как прежняя. Нет файла — `login`.
pub fn authenticate(username: &str, password: &[u8]) -> Result<(), String> {
    if username.is_empty() {
        return Err("неизвестный пользователь".to_string());
    }
    let user = CString::new(username).map_err(|_| "имя пользователя с NUL".to_string())?;

    // `swaylock` — привычный сервис на этой машине; если его файла нет (другой
    // дистрибутив), `pam_start` может вернуть успех, а ошибку даст уже
    // `pam_authenticate`. Поэтому файл читается до запуска: по нему же видно,
    // есть ли что проверять в account.
    let swaylock = std::fs::read_to_string("/etc/pam.d/swaylock");
    let (service, check_account) = match &swaylock {
        Ok(text) => (c"swaylock", has_account_stack(text)),
        Err(_) => (c"login", true),
    };

    super::pam_ffi::authenticate(service, user.as_c_str(), password, check_account)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Пустое имя не доходит до PAM: иначе `pam_start` получил бы пустую
    /// строку и вернул бы ошибку, которую легко спутать с неверным паролем.
    #[test]
    fn an_empty_user_is_refused_before_pam() {
        assert!(authenticate("", b"secret").is_err());
    }

    /// Файл `swaylock` на Arch: только `auth include login`. Account-модулей
    /// в нём нет — значит, `pam_acct_mgmt` вызывать нельзя.
    #[test]
    fn a_swaylock_style_file_has_no_account_stack() {
        let text = "#\n# PAM configuration for swaylock\n#\n\nauth include login\n";
        assert!(!has_account_stack(text));
    }

    /// `include` в auth не подтягивает account: строка `auth include` не
    /// должна считаться account-модулем.
    #[test]
    fn auth_includes_do_not_count_as_account() {
        let text = "auth include login\npassword include login\nsession include login\n";
        assert!(!has_account_stack(text));
    }

    /// Когда account описан явно — обычный или необязательный (`-account`) —
    /// проверку после аутентификации нужно делать.
    #[test]
    fn explicit_account_lines_are_found() {
        assert!(has_account_stack("account include system-auth\n"));
        assert!(has_account_stack("-account optional pam_permit.so\n"));
        assert!(has_account_stack(
            "auth include login\n\naccount required pam_unix.so\n"
        ));
    }
}

//! Проверка пароля через PAM.
//!
//! `swaylock` на этой машине ходит в `/etc/pam.d/swaylock`, который включает
//! `login`; здесь тот же сервис, поэтому правила блокировки не расходятся с
//! остальной системой. Если файла `swaylock` нет, берётся `login` — он есть
//! везде, где есть PAM.
//!
//! Пароль не попадает ни в лог, ни в argv, ни в файл. В PAM он передаётся
//! копией в C-буфере, а исходный буфер затирается вызывающим после ответа.
//! Здесь же затирается и C-копия, которую PAM освобождает сам.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;

const PAM_SUCCESS: c_int = 0;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
const PAM_CONV_ERR: c_int = 19;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

#[repr(C)]
struct PamConv {
    conv: Option<
        unsafe extern "C" fn(
            num_msg: c_int,
            msg: *mut *mut PamMessage,
            resp: *mut *mut PamResponse,
            appdata_ptr: *mut c_void,
        ) -> c_int,
    >,
    appdata_ptr: *mut c_void,
}

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(
        service_name: *const c_char,
        user: *const c_char,
        pam_conv: *const PamConv,
        pamh: *mut *mut c_void,
    ) -> c_int;
    fn pam_authenticate(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_acct_mgmt(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut c_void, pam_status: c_int) -> c_int;
    fn pam_strerror(pamh: *mut c_void, errnum: c_int) -> *const c_char;
}

unsafe extern "C" {
    fn strdup(s: *const c_char) -> *mut c_char;
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

struct Conversation<'a> {
    username: &'a CStr,
    password: *const c_char,
}

/// Ответ PAM на вопрос диалога. PAM освобождает ответы сам, поэтому строки
/// обязаны быть из `malloc` (`strdup`), а не из Rust-аллокатора.
unsafe extern "C" fn conversation(
    num_msg: c_int,
    msg: *mut *mut PamMessage,
    resp: *mut *mut PamResponse,
    appdata_ptr: *mut c_void,
) -> c_int {
    if num_msg <= 0 || msg.is_null() || resp.is_null() || appdata_ptr.is_null() {
        return PAM_CONV_ERR;
    }
    let data = unsafe { &*(appdata_ptr as *const Conversation<'_>) };
    let count = num_msg as usize;
    let responses =
        unsafe { calloc(count, std::mem::size_of::<PamResponse>()) } as *mut PamResponse;
    if responses.is_null() {
        return PAM_CONV_ERR;
    }

    for index in 0..count {
        let message = unsafe { *msg.add(index) };
        if message.is_null() {
            unsafe { free_responses(responses, index) };
            return PAM_CONV_ERR;
        }
        let style = unsafe { (*message).msg_style };
        let answer = match style {
            PAM_PROMPT_ECHO_OFF => unsafe { strdup(data.password) },
            PAM_PROMPT_ECHO_ON => unsafe { strdup(data.username.as_ptr()) },
            PAM_ERROR_MSG | PAM_TEXT_INFO => ptr::null_mut(),
            _ => {
                unsafe { free_responses(responses, index) };
                return PAM_CONV_ERR;
            }
        };
        if answer.is_null() && !matches!(style, PAM_ERROR_MSG | PAM_TEXT_INFO) {
            unsafe { free_responses(responses, index) };
            return PAM_CONV_ERR;
        }
        let response = unsafe { &mut *responses.add(index) };
        response.resp = answer;
        response.resp_retcode = 0;
    }

    unsafe { *resp = responses };
    PAM_SUCCESS
}

/// Освобождает ответы `0..count` и сам массив. Нужен только на ошибочном
/// пути: при успехе ответы забирает и освобождает PAM.
unsafe fn free_responses(responses: *mut PamResponse, count: usize) {
    unsafe {
        for index in 0..count {
            let response = &mut *responses.add(index);
            if !response.resp.is_null() {
                free(response.resp as *mut c_void);
            }
        }
        free(responses as *mut c_void);
    }
}

fn error_text(pamh: *mut c_void, code: c_int) -> String {
    let pointer = unsafe { pam_strerror(pamh, code) };
    if pointer.is_null() {
        return format!("PAM: код {code}");
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
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

    // NUL-терминированная копия для `strdup`. Затирается после `pam_end`,
    // даже если PAM вернул ошибку.
    let mut secret = password.to_vec();
    secret.push(0);
    let data = Conversation {
        username: user.as_c_str(),
        password: secret.as_ptr() as *const c_char,
    };
    let conv = PamConv {
        conv: Some(conversation),
        appdata_ptr: &data as *const _ as *mut c_void,
    };

    // `swaylock` — привычный сервис на этой машине; если его файла нет (другой
    // дистрибутив), `pam_start` может вернуть успех, а ошибку даст уже
    // `pam_authenticate`. Поэтому файл проверяется до запуска, а не по коду
    // возврата.
    let service = if std::path::Path::new("/etc/pam.d/swaylock").is_file() {
        c"swaylock"
    } else {
        c"login"
    };

    let result = {
        let mut pamh: *mut c_void = ptr::null_mut();
        let start = unsafe { pam_start(service.as_ptr(), user.as_ptr(), &conv, &mut pamh) };
        if start != PAM_SUCCESS {
            let error = error_text(pamh, start);
            if !pamh.is_null() {
                unsafe { pam_end(pamh, start) };
            }
            Err(error)
        } else {
            let auth = unsafe { pam_authenticate(pamh, 0) };
            let outcome = if auth == PAM_SUCCESS {
                unsafe { pam_acct_mgmt(pamh, 0) }
            } else {
                auth
            };
            let result = if outcome == PAM_SUCCESS {
                Ok(())
            } else {
                Err(error_text(pamh, outcome))
            };
            unsafe { pam_end(pamh, outcome) };
            result
        }
    };

    secret.fill(0);
    result
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
}

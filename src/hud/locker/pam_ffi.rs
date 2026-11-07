//! FFI к PAM: единственное место с `unsafe` в модуле блокировки.
//!
//! Наружу торчит одна безопасная функция [`authenticate`]: она принимает
//! обычные Rust-строки и байты пароля, а весь разговор с C — структуры,
//! указатели, выделение и освобождение ответов — остаётся здесь.
//!
//! Правила, на которых держится безопасность:
//!
//! * `Conversation` живёт на стеке `authenticate` всё время вызова
//!   `pam_start`/`pam_authenticate`/`pam_acct_mgmt`; PAM получает только
//!   указатель на неё и не сохраняет его после возврата.
//! * Ответы диалога выделяются `calloc` и строками из `strdup`: PAM освобождает
//!   их сам после разговора. На ошибочном пути их освобождает
//!   [`free_responses`].
//! * Копия пароля для `strdup` живёт в `Vec<u8>` и затирается после `pam_end`
//!   при любом исходе. Копию, которую PAM делает себе, затереть нельзя — её
//!   освобождает и переиспользует только PAM.

use std::ffi::{CStr, c_char, c_int, c_void};
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

/// Что осталось от вызова: имя пользователя и NUL-терминированный пароль.
/// Живёт только на стеке [`authenticate`].
struct Conversation<'a> {
    username: &'a CStr,
    password: *const c_char,
}

/// Проверяет пароль через PAM. `service` — имя файла из `/etc/pam.d`,
/// `username` — имя пользователя, `password` — сырые байты (без NUL).
///
/// `check_account` включает `pam_acct_mgmt` после успешной аутентификации:
/// на пустом account-стеке он вернул бы ошибку и перечеркнул верный пароль.
pub fn authenticate(
    service: &CStr,
    username: &CStr,
    password: &[u8],
    check_account: bool,
) -> Result<(), String> {
    // NUL-терминированная копия для `strdup`: PAM ждёт C-строку. Копия
    // затирается после `pam_end` при любом исходе.
    let mut secret = password.to_vec();
    secret.push(0);
    let data = Conversation {
        username,
        password: secret.as_ptr().cast(),
    };
    let conv = PamConv {
        conv: Some(conversation),
        appdata_ptr: (&data as *const Conversation<'_>).cast_mut().cast(),
    };

    let result = authenticate_with(service, username, &conv, check_account);
    secret.fill(0);
    result
}

fn authenticate_with(
    service: &CStr,
    username: &CStr,
    conv: &PamConv,
    check_account: bool,
) -> Result<(), String> {
    let mut pamh: *mut c_void = ptr::null_mut();
    // SAFETY: service и username — живые NUL-строки, conv живёт до конца
    // функции, pamh — наш out-параметр. PAM не сохраняет их после возврата.
    let start = unsafe { pam_start(service.as_ptr(), username.as_ptr(), conv, &mut pamh) };
    if start != PAM_SUCCESS {
        let error = error_text(pamh, start);
        if !pamh.is_null() {
            // SAFETY: pamh вернул pam_start; это единственное его использование.
            unsafe { pam_end(pamh, start) };
        }
        return Err(error);
    }

    // SAFETY: pamh — живой handle после успешного pam_start.
    let auth = unsafe { pam_authenticate(pamh, 0) };
    // Account проверяется только после успешной аутентификации и только если
    // в файле сервиса для неё есть модули: иначе пустой стек отвечает
    // `PAM_AUTH_ERR` и перечёркивает верный пароль.
    let outcome = if auth == PAM_SUCCESS && check_account {
        // SAFETY: тот же живой handle.
        unsafe { pam_acct_mgmt(pamh, 0) }
    } else {
        auth
    };
    let result = if outcome == PAM_SUCCESS {
        Ok(())
    } else {
        Err(error_text(pamh, outcome))
    };
    // SAFETY: pamh всё ещё жив; это последний вызов с ним.
    unsafe { pam_end(pamh, outcome) };
    result
}

/// Ответ PAM на вопрос диалога. Строки обязаны быть из `malloc` (`strdup`):
/// PAM освобождает ответы сам.
unsafe extern "C" fn conversation(
    num_msg: c_int,
    msg: *mut *mut PamMessage,
    resp: *mut *mut PamResponse,
    appdata_ptr: *mut c_void,
) -> c_int {
    if num_msg <= 0 || msg.is_null() || resp.is_null() || appdata_ptr.is_null() {
        return PAM_CONV_ERR;
    }
    // SAFETY: appdata_ptr — тот же указатель, что положили в PamConv выше;
    // он указывает на `Conversation`, живую на всё время разговора, и PAM не
    // меняет её содержимое.
    let data = unsafe { &*(appdata_ptr as *const Conversation<'_>) };
    let count = num_msg as usize;
    // SAFETY: calloc выделяет count элементов и обнуляет их; PAM освобождает
    // массив сам после успешного разговора.
    let responses =
        unsafe { calloc(count, std::mem::size_of::<PamResponse>()) } as *mut PamResponse;
    if responses.is_null() {
        return PAM_CONV_ERR;
    }

    for index in 0..count {
        // SAFETY: PAM гарантирует массив из num_msg указателей; index < count.
        let message = unsafe { *msg.add(index) };
        if message.is_null() {
            // SAFETY: responses выделен на count элементов; освобождаем
            // заполненные 0..index.
            unsafe { free_responses(responses, index) };
            return PAM_CONV_ERR;
        }
        // SAFETY: message проверен на null и принадлежит PAM.
        let style = unsafe { (*message).msg_style };
        let answer = match style {
            // SAFETY: password — NUL-терминированный буфер, живой до конца
            // вызова; strdup копирует его в malloc-память.
            PAM_PROMPT_ECHO_OFF => unsafe { strdup(data.password) },
            // SAFETY: username — живая NUL-строка на всё время разговора.
            PAM_PROMPT_ECHO_ON => unsafe { strdup(data.username.as_ptr()) },
            PAM_ERROR_MSG | PAM_TEXT_INFO => ptr::null_mut(),
            _ => {
                // SAFETY: освобождаем уже заполненные ответы и массив.
                unsafe { free_responses(responses, index) };
                return PAM_CONV_ERR;
            }
        };
        if answer.is_null() && !matches!(style, PAM_ERROR_MSG | PAM_TEXT_INFO) {
            // SAFETY: strdup вернул null — освобождаем то, что успели.
            unsafe { free_responses(responses, index) };
            return PAM_CONV_ERR;
        }
        // SAFETY: responses выделен на count элементов, index < count.
        let response = unsafe { &mut *responses.add(index) };
        response.resp = answer;
        response.resp_retcode = 0;
    }

    // SAFETY: resp — out-параметр PAM, проверен на null; отдаём массив,
    // который дальше освобождает PAM.
    unsafe { *resp = responses };
    PAM_SUCCESS
}

/// Освобождает ответы `0..count` и сам массив. Нужен только на ошибочном
/// пути: при успехе ответы забирает и освобождает PAM.
///
/// # Safety
///
/// `responses` должен быть указателем от `calloc(count, ...)`, а все `0..count`
/// либо уже заполнены, либо остаются нулевыми (`calloc` это гарантирует).
unsafe fn free_responses(responses: *mut PamResponse, count: usize) {
    // SAFETY: инвариант функции: массив выделен на count элементов.
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
    // SAFETY: pam_strerror принимает любой handle (включая null) и код
    // статуса; возвращённый указатель валиден до следующего вызова PAM.
    let pointer = unsafe { pam_strerror(pamh, code) };
    if pointer.is_null() {
        return format!("PAM: код {code}");
    }
    // SAFETY: ненулевой указатель от pam_strerror — NUL-терминированная
    // строка, принадлежащая PAM.
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

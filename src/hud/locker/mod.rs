//! Экран блокировки Hudbar на ext-session-lock.
//!
//! Модуль подключается бинарником `hud-lock-rs` через `#[path]`, как гритер.
//! Общие примитивы (`canvas`, `text`, `ui_tokens`) лежат у корня бинарника;
//! поле и часы рисуются ими же, но раскладка у замка своя — без карточки.
//!
//! Границы:
//!
//! * `auth` — PAM без крейтов: проверка пароля и затирание буфера;
//! * `background` — обои и PNG-кэш для фона;
//! * `layout` — геометрия часов и поля пароля;
//! * `render` — кадр целиком, без Wayland;
//! * `strings` — подписи ru/en.

pub mod auth;
pub mod background;
pub mod layout;
pub mod render;
pub mod strings;

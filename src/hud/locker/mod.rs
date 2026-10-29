//! Экран блокировки Hudbar на ext-session-lock.
//!
//! Модуль подключается бинарником `hud-lock-rs` через `#[path]`, как гритер.
//! Общие примитивы (`canvas`, `card_frame`, `text`, `ui_tokens`) лежат у корня
//! бинарника; раскладка и отрисовка карточки переиспользуются из
//! `greeter::layout` и `greeter::render`.
//!
//! Границы:
//!
//! * `auth` — PAM без крейтов: проверка пароля и затирание буфера;
//! * `render` — кадр целиком, без Wayland;
//! * `strings` — подписи ru/en.

pub mod auth;
pub mod render;
pub mod strings;

//! Экран входа Hudbar: протокол greetd, логика входа, раскладка и отрисовка.
//!
//! Модуль подключается бинарником `hud-greeter-rs` через `#[path]`, поэтому
//! общие файлы (`canvas`, `text`, `ui_tokens`, `palette`, `layer`) лежат у
//! корня этого бинарника, а не в `hud/`: у крейта нет lib-цели, и каждый
//! бинарь собирает свои модули сам. Ровно так же устроены окно обоев и схем.
//!
//! Границы внутри:
//!
//! * `ipc` — сокет greetd и разбор его ответов, без UI;
//! * `flow` — конечный автомат входа, без Wayland и без greetd;
//! * `sessions` — список `.desktop`-сеансов и argv без shell;
//! * `state` — последний пользователь и сеанс;
//! * `config` — каталог данных, палитра, фон, `greeter.json`;
//! * `layout` — геометрия карточки, проверяется тестами;
//! * `render` — кадр целиком, включая снимок без Wayland;
//! * `image` — фон (PNG) и уменьшение обоев для `--sync`;
//! * `sync` — подкоманда `--sync` для обычного пользователя;
//! * `panel` — верхняя полоса: часы, батарея, Caps Lock.

pub mod config;
pub mod flow;
pub mod image;
pub mod ipc;
pub mod layout;
pub mod panel;
pub mod render;
pub mod sessions;
pub mod state;
pub mod strings;
pub mod sync;

// Общие модули крейта. Реэкспорт повторяет приём `wallpaper.rs`: подмодули
// гритера обращаются к ним через `super::`, и это работает и в бинарнике, и в
// тестах модуля.
#[allow(unused_imports)]
pub(crate) use super::{
    anim, canvas, card_frame, geom, i18n, log, palette, settings, settings_icons, text, ui_tokens,
};

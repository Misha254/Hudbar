//! HUDbar — верхняя панель и HUD-окна для niri на Wayland.
//!
//! Общая библиотека панели и окон: графика tiny-skia, текст cosmic-text,
//! слой wlr-layer-shell, темы, настройки и системные данные.

pub mod hud;

pub use hud::*;

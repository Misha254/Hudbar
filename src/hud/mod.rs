pub mod actions;
#[allow(dead_code)]
pub mod bind_data;
#[allow(dead_code)]
pub mod binds_layout;
// Модель настроек с зонами; окно подключит её вместе с записью на диск.
#[allow(dead_code)]
pub mod config;
#[allow(dead_code)]
pub mod config_io;
pub mod data;
// Точечная замена font/line_height/origin в dunst; подключит окно.
#[allow(dead_code)]
pub mod dunst;
pub mod lock;
pub mod log;
// OSD громкости и микрофона: детектор изменений, окно рисует App.
#[allow(dead_code)]
pub mod osd;
// Меню HUDbar: модель (M1) и отрисовка (M2). Окно появится в M4.
#[allow(dead_code)]
pub mod menu;
pub mod palette;
pub mod settings;
// Снимки окна настроек в PNG без Wayland.
#[allow(dead_code)]
pub mod settings_snapshot;
// Отрисовка окна: трейты View/DrawExt, общие для живого окна и снапшота.
#[allow(dead_code)]
pub mod settings_ui;
#[allow(dead_code)]
pub mod settings_view;
// Текст окна настроек: измерение глифов и центрирование подписей.
#[allow(dead_code)]
pub mod text;
pub mod tray;
// Слой раскладки: вертикальный стек полос, без отрисовки.
// Глифы интерфейса: Nerd Font, всегда JetBrainsMono.
#[allow(dead_code)]
pub mod settings_icons;
// Виджеты окна настроек.
#[allow(dead_code)]
pub mod settings_widgets;
#[allow(dead_code)]
pub mod ui_layout;
#[allow(dead_code)]
pub mod ui_tokens;
#[allow(dead_code)]
pub mod wallpaper;

pub mod app;

pub use app::App;
pub use app::ui;

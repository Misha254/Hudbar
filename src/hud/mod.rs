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
pub mod log;
pub mod palette;
pub mod settings;
// Рекурсивный список файлов обоев из ~/wallpapers; окно подключит его.
pub mod tray;
#[allow(dead_code)]
pub mod wallpaper;

pub mod app;

pub use app::App;
pub use app::ui;

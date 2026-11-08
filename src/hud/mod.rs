pub mod actions;
pub mod anim;
pub mod app;
#[allow(dead_code)]
pub mod apps;
#[allow(dead_code)]
pub mod bind_data;
#[allow(dead_code)]
pub mod bind_window;
#[allow(dead_code)]
pub mod binds_layout;
// Примитивы рисования по пиксельной карте: заливки, рамки, контекст виджета.
#[allow(dead_code)]
pub mod canvas;
// Общий каркас карточки (фон, шапка, подвал, скроллбар, подсветка строк) для
// окон биндов; геометрия и цвета — как у карточки меню.
#[allow(dead_code)]
pub mod card_frame;
// Модель настроек с зонами; окно подключит её вместе с записью на диск.
#[allow(dead_code)]
pub mod config;
#[allow(dead_code)]
pub mod config_io;
pub mod data;
#[allow(dead_code)]
pub mod exec_util;
// Геометрия окна: прямоугольник и проверки попадания.
#[allow(dead_code)]
pub mod geom;
#[allow(dead_code)]
pub mod greeter;
pub mod i18n;
#[allow(dead_code)]
pub mod key_display;
#[allow(dead_code)]
pub mod layer;
pub mod lock;
#[allow(dead_code)]
pub mod locker;
pub mod log;
// Меню HUDbar: модель (M1) и отрисовка (M2). Окно появится в M4.
#[allow(dead_code)]
pub mod menu;
// Эффекты окон niri по профилю питания: размытие и прозрачность.
pub mod niri_effects;
// OSD громкости и микрофона: детектор изменений, окно рисует App.
#[allow(dead_code)]
pub mod osd;
pub mod palette;
#[allow(dead_code)]
pub mod power_profile;
// Не каждый бинарь использует автоповтор: панели список не листать.
#[allow(dead_code)]
pub mod repeat;
// Окно схем отдельное; панели и прочим бинарям модуль не нужен.
#[allow(dead_code)]
pub mod schemes;
pub mod settings;
// Глифы интерфейса: Nerd Font, всегда JetBrainsMono.
#[allow(dead_code)]
pub mod settings_icons;
#[allow(dead_code)]
pub mod shell;
// Текст интерфейса: измерение глифов и центрирование подписей.
#[allow(dead_code)]
pub mod text;
pub mod tray;
#[allow(dead_code)]
pub mod ui_tokens;
#[allow(dead_code)]
pub mod wallpaper;

pub use app::App;
pub use app::ui;

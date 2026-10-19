pub mod actions;
#[allow(dead_code)]
pub mod bind_data;
#[allow(dead_code)]
pub mod binds_layout;
// Общий каркас карточки (фон, шапка, подвал, скроллбар, подсветка строк) для
// окон биндов; геометрия и цвета — как у карточки меню.
#[allow(dead_code)]
pub mod card_frame;
// Примитивы рисования по пиксельной карте: заливки, рамки, контекст виджета.
#[allow(dead_code)]
pub mod canvas;
// Модель настроек с зонами; окно подключит её вместе с записью на диск.
#[allow(dead_code)]
pub mod config;
#[allow(dead_code)]
pub mod config_io;
// Геометрия окна: прямоугольник и проверки попадания.
#[allow(dead_code)]
pub mod geom;
// Общая основа анимаций: кривые, твин, набор значений по ключу. Панель
// берёт отсюда и подсветку чипов, и выезд попапа.
pub mod anim;
pub mod data;
pub mod lock;
pub mod log;
// OSD громкости и микрофона: детектор изменений, окно рисует App.
#[allow(dead_code)]
pub mod osd;
// Меню HUDbar: модель (M1) и отрисовка (M2). Окно появится в M4.
#[allow(dead_code)]
pub mod menu;
pub mod palette;
// Не каждый бинарь использует автоповтор: панели список не листать.
#[allow(dead_code)]
pub mod repeat;
// Окно схем отдельное; панели и прочим бинарям модуль не нужен.
#[allow(dead_code)]
pub mod schemes;
pub mod settings;
// Текст интерфейса: измерение глифов и центрирование подписей.
#[allow(dead_code)]
pub mod text;
pub mod tray;
// Глифы интерфейса: Nerd Font, всегда JetBrainsMono.
#[allow(dead_code)]
pub mod settings_icons;
#[allow(dead_code)]
pub mod ui_tokens;
#[allow(dead_code)]
pub mod wallpaper;

pub mod app;

pub use app::App;
pub use app::ui;

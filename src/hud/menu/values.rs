//! Значения настоящего меню: чтение из `settings.json` и разовые опросы.
//!
//! Каждая строка дерева получает значение вызовом `get` в момент сборки
//! списка, а изменение — вызовом `set`. Здесь обе стороны: `get` читает
//! `settings::load()` или одноразовый опрос из `data::oneshot`, `set`
//! применяет точечный `Patch` через `config_io` или быструю системную
//! команду через `log::status`.
//!
//! Правила, которые здесь соблюдаются:
//!
//! * `get` никогда не пишет: сборка списка может вызывать его десятки раз;
//! * `set` для настроек идёт только через `Patch`, чужие ключи файла целы;
//! * долгие скрипты (`wall.sh`) из `set` не ждут: схема и обои применяются
//!   откреплённо через общий враппер `wallpaper`, тот же, что у окна
//!   настроек. Ошибки уходят в журнал, меню не виснет на пересчёте matugen.

use super::super::config::Config;
use super::super::config_io::{self, Patch};
use super::super::data::oneshot;
use super::super::log;
use super::super::schemes;
use super::super::settings::{
    self, FONT_SIZE_MAX, FONT_SIZE_MIN, HEIGHT_MAX, HEIGHT_MIN, IDLE_SCREEN_OFF_MAX_SECONDS,
    IDLE_SCREEN_OFF_MIN_SECONDS, IDLE_SUSPEND_MAX_SECONDS, IDLE_SUSPEND_MIN_SECONDS, Language,
    Module, NotificationPosition, OSD_DURATION_MAX_MS, OSD_DURATION_MIN_MS, clamp,
};
use super::super::wallpaper;

/// Переключатель модуля панели: чтение флага из файла, запись точечным
/// патчем. Макрос нужен, потому что `get`/`set` дерева — обычные функции, а
/// не замыкания: десять модулей без макроса превратились бы в двадцать
/// одинаковых функций.
macro_rules! module_switch {
    ($get:ident, $set:ident, $field:ident, $module:expr) => {
        fn $get() -> bool {
            settings::load().$field
        }
        fn $set(on: bool) {
            if let Err(error) = config_io::apply_patch(&Patch::ModuleVisible {
                module: $module,
                on,
            }) {
                log::warn(format!("модуль не переключился: {error}"));
            }
        }
    };
}

module_switch!(tray_get, tray_set, tray, Module::Tray);
module_switch!(weather_get, weather_set, weather, Module::Weather);
module_switch!(webcam_get, webcam_set, webcam, Module::Webcam);
module_switch!(clock_get, clock_set, clock, Module::Clock);
module_switch!(recorder_get, recorder_set, recorder, Module::Recorder);
module_switch!(battery_get, battery_set, battery, Module::Battery);
module_switch!(system_get, system_set, system, Module::System);
module_switch!(audio_get, audio_set, audio, Module::Audio);
module_switch!(network_get, network_set, network, Module::Network);
module_switch!(dnd_module_get, dnd_module_set, dnd, Module::Dnd);

/// Пара `get`/`set` для тумблера модуля панели.
#[allow(clippy::type_complexity)]
pub fn module_toggle(module: Module) -> (fn() -> bool, fn(bool)) {
    match module {
        Module::Tray => (tray_get, tray_set),
        Module::Weather => (weather_get, weather_set),
        Module::Webcam => (webcam_get, webcam_set),
        Module::Clock => (clock_get, clock_set),
        Module::Recorder => (recorder_get, recorder_set),
        Module::Battery => (battery_get, battery_set),
        Module::System => (system_get, system_set),
        Module::Audio => (audio_get, audio_set),
        Module::Network => (network_get, network_set),
        Module::Dnd => (dnd_module_get, dnd_module_set),
    }
}

/// Высота панели в пикселях.
pub fn height() -> f32 {
    settings::load().height as f32
}

/// Приведение высоты к поддерживаемым границам. Отдельной функцией, чтобы
/// границы проверялись тестом, а не окном.
pub fn clamp_height(value: f32) -> u32 {
    value.round().clamp(HEIGHT_MIN as f32, HEIGHT_MAX as f32) as u32
}

/// Запись высоты точечным патчем.
pub fn set_height(value: f32) {
    if let Err(error) = config_io::apply_patch(&Patch::Height(clamp_height(value))) {
        log::warn(format!("высота не записалась: {error}"));
    }
}

/// Анимации включены. Панель подхватывает файл сразу, окна — при следующем
/// запуске: они читают настройки один раз на старте.
pub fn animations() -> bool {
    settings::load().animations
}

/// Включение и выключение анимаций.
pub fn set_animations(on: bool) {
    if let Err(error) = config_io::apply_patch(&Patch::Animations {
        enabled: Some(on),
        speed: None,
    }) {
        log::warn(format!("анимации не переключились: {error}"));
    }
}

/// Множитель длительности анимаций: 2.0 — вдвое быстрее.
pub fn animation_speed() -> f32 {
    settings::load().animation_speed
}

/// Запись скорости анимаций. Значение зажато к границам из `settings.rs`.
pub fn set_animation_speed(value: f32) {
    let speed = settings::clamp_f(
        value as f64,
        settings::ANIMATION_SPEED_MIN,
        settings::ANIMATION_SPEED_MAX,
    );
    if let Err(error) = config_io::apply_patch(&Patch::Animations {
        enabled: None,
        speed: Some(speed),
    }) {
        log::warn(format!("скорость анимаций не записалась: {error}"));
    }
}

/// Простой до гашения экрана в минутах: в файле лежат секунды, а меню
/// показывает привычные минуты.
pub fn idle_screen_off() -> f32 {
    settings::load().idle_screen_off_seconds as f32 / 60.0
}

/// Запись простоя до гашения экрана. `sleep.sh` перезапускается, иначе
/// `swayidle` продолжил бы ждать по старым таймаутам.
pub fn set_idle_screen_off(value: f32) {
    let seconds = clamp(
        (value.max(1.0) * 60.0).round() as i64,
        IDLE_SCREEN_OFF_MIN_SECONDS,
        IDLE_SCREEN_OFF_MAX_SECONDS,
    );
    match config_io::apply_patch(&Patch::Idle {
        screen_off_seconds: Some(seconds),
        suspend_seconds: None,
    }) {
        Ok(true) => restart_idle(),
        Ok(false) => {}
        Err(error) => log::warn(format!("простой экрана не записался: {error}")),
    }
}

/// Простой до сна ноутбука в минутах.
pub fn idle_suspend() -> f32 {
    settings::load().idle_suspend_seconds as f32 / 60.0
}

/// Запись простоя до сна. После изменения `swayidle` перезапускается.
pub fn set_idle_suspend(value: f32) {
    let seconds = clamp(
        (value.max(1.0) * 60.0).round() as i64,
        IDLE_SUSPEND_MIN_SECONDS,
        IDLE_SUSPEND_MAX_SECONDS,
    );
    match config_io::apply_patch(&Patch::Idle {
        screen_off_seconds: None,
        suspend_seconds: Some(seconds),
    }) {
        Ok(true) => restart_idle(),
        Ok(false) => {}
        Err(error) => log::warn(format!("простой до сна не записался: {error}")),
    }
}

/// Своё окно громкости/микрофона вместо уведомления dunst.
pub fn osd() -> bool {
    settings::load().osd
}

/// Включение и выключение OSD.
pub fn set_osd(on: bool) {
    if let Err(error) = config_io::apply_patch(&Patch::Osd {
        enabled: Some(on),
        duration_ms: None,
    }) {
        log::warn(format!("OSD не переключился: {error}"));
    }
}

/// Длительность окна OSD в миллисекундах: ползунок пишет целое число.
pub fn osd_duration() -> f32 {
    settings::load().osd_duration().as_millis() as f32
}

/// Запись длительности окна OSD.
pub fn set_osd_duration(value: f32) {
    let duration = clamp(
        value.round() as i64,
        OSD_DURATION_MIN_MS,
        OSD_DURATION_MAX_MS,
    );
    if let Err(error) = config_io::apply_patch(&Patch::Osd {
        enabled: None,
        duration_ms: Some(duration),
    }) {
        log::warn(format!("срок OSD не записался: {error}"));
    }
}

/// Индекс языка: 0 — русский, 1 — английский.
pub fn language() -> usize {
    match settings::load().language {
        Language::Ru => 0,
        Language::En => 1,
    }
}

/// Смена языка точечным патчем. Подписи меню перестраиваются окном после
/// изменения: дерево хранит заголовки, а не пересчитывает их.
pub fn set_language(index: usize) {
    let language = if index == 1 {
        Language::En
    } else {
        Language::Ru
    };
    if let Err(error) = config_io::apply_patch(&Patch::Language(language)) {
        log::warn(format!("язык не записался: {error}"));
    }
}

/// Размер шрифта уведомлений.
pub fn font_size() -> f32 {
    settings::load().font_size as f32
}

/// Запись размера шрифта с границами окна настроек.
pub fn set_font_size(value: f32) {
    let size = value
        .round()
        .clamp(FONT_SIZE_MIN as f32, FONT_SIZE_MAX as f32) as u32;
    set_notifications(Some(size), None, None);
}

/// Высота строки уведомлений.
pub fn line_height() -> f32 {
    settings::load().line_height as f32
}

/// Запись высоты строки с границами окна настроек.
pub fn set_line_height(value: f32) {
    let line = value.round().clamp(
        settings::LINE_HEIGHT_MIN as f32,
        settings::LINE_HEIGHT_MAX as f32,
    ) as u32;
    set_notifications(None, Some(line), None);
}

fn set_notifications(
    font_size: Option<u32>,
    line_height: Option<u32>,
    position: Option<NotificationPosition>,
) {
    if let Err(error) = config_io::apply_patch(&Patch::Notifications {
        font_size,
        line_height,
        position,
    }) {
        log::warn(format!("уведомления не записались: {error}"));
    }
}

/// Индекс позиции уведомлений в порядке `NotificationPosition::ALL`.
pub fn notification_position() -> usize {
    NotificationPosition::ALL
        .into_iter()
        .position(|position| position == settings::load().position)
        .unwrap_or(1)
}

/// Смена позиции уведомлений точечным патчем.
pub fn set_notification_position(index: usize) {
    let position = NotificationPosition::ALL
        .get(index)
        .copied()
        .unwrap_or(NotificationPosition::TopRight);
    set_notifications(None, None, Some(position));
}

/// Системный «не беспокоить»: спрашиваем у dunst, а не у файла. Файл знает
/// только видимость модуля на панели.
pub fn dnd() -> bool {
    oneshot::dnd_paused()
}

/// Переключение «не беспокоить» прямой командой: быстро и без rofi, поэтому
/// меню остаётся открытым и показывает новое значение сразу.
pub fn set_dnd(on: bool) {
    let target = if on { "true" } else { "false" };
    if !log::status("dunstctl", &["set-paused", target]) {
        log::warn("dunstctl set-paused не выполнен");
    }
}

/// Wi-Fi включён: есть активное соединение.
pub fn wifi() -> bool {
    oneshot::network_name() != "Нет сети"
}

/// Переключение радиомодуля Wi-Fi прямой командой.
pub fn set_wifi(on: bool) {
    let target = if on { "on" } else { "off" };
    if !log::status("nmcli", &["radio", "wifi", target]) {
        log::warn("nmcli radio wifi не выполнен");
    }
}

/// Кофе-мод: экран не гаснет, пока существует `hud-coffee`.
pub fn coffee() -> bool {
    oneshot::coffee_on()
}

/// Переключение кофе-мода. Цель игнорируется намеренно: `enter` всегда зовёт
/// `set` с противоположным, а состояние переключается здесь же — по файлу в
/// рантайме, как это делал `coffee-toggle.sh`.
///
/// Кофе-режим — это не только галочка: он держит `swayidle` убитым, чтобы
/// экран не гас и компьютер не засыпал. Поэтому выключение возвращает
/// `sleep.sh`, а включение его глушит. Логика ровно та же, что была в скрипте,
/// только без оболочки.
pub fn set_coffee(_on: bool) {
    let dir = oneshot::runtime_dir();
    let state = dir.join("hud-coffee");
    let pid_file = dir.join("hud-idle.pid");
    let sleep = idle_sleep_running(&pid_file);
    if state.is_file() {
        let _ = std::fs::remove_file(&state);
        if !sleep {
            spawn_sleep();
        }
        dunstify("Caffeine OFF", "Idle management enabled");
    } else {
        if let Err(error) = std::fs::write(&state, "") {
            log::warn(format!("состояние кофе-мода не записать: {error}").as_str());
            return;
        }
        if sleep {
            stop_idle(&pid_file);
        }
        dunstify("☕ Caffeine ON", "Idle, dim and suspend disabled");
    }
}

/// Перезапустить `sleep.sh` после смены таймаутов. В кофе-моде он не должен
/// подниматься: там `swayidle` намеренно убит до выключения режима.
fn restart_idle() {
    let dir = oneshot::runtime_dir();
    if dir.join("hud-coffee").is_file() {
        return;
    }
    let pid_file = dir.join("hud-idle.pid");
    if idle_sleep_running(&pid_file) {
        stop_idle(&pid_file);
    }
    spawn_sleep();
}

/// Жив ли `swayidle` из `sleep.sh`: pid-файл может быть протухшим, поэтому
/// проверяем имя процесса, а не только наличие файла.
fn idle_sleep_running(pid_file: &std::path::Path) -> bool {
    let Ok(pid) = std::fs::read_to_string(pid_file) else {
        return false;
    };
    let pid = pid.trim();
    if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|comm| comm.trim() == "swayidle")
}

/// Снять ожидание сна: убить `swayidle` и убрать pid-файл, чтобы `sleep.sh`
/// не поднял второй экземпляр.
fn stop_idle(pid_file: &std::path::Path) {
    if let Ok(pid) = std::fs::read_to_string(pid_file) {
        let _ = std::fs::remove_file(pid_file);
        let pid: i32 = pid.trim().parse().unwrap_or(0);
        if pid > 0 {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGTERM,
            );
        }
    }
}

/// Вернуть управление сном: `sleep.sh` сам пишет pid-файл, поэтому просто
/// запускаем его отдельной сессией — как это делал скрипт через `setsid -f`.
fn spawn_sleep() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let script = std::path::PathBuf::from(home).join(".local/bin/sleep.sh");
    let _ = std::process::Command::new("setsid")
        .arg("-f")
        .arg(&script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Уведомление о смене кофе-режима: те же слова, что слал скрипт.
fn dunstify(summary: &str, body: &str) {
    log::status("dunstify", &["-a", "caffeine", summary, body]);
}

/// Идёт ли запись экрана.
pub fn recording() -> bool {
    oneshot::record_on()
}

/// Новый порядок модулей после сдвига на `delta`. Читает файл, но не пишет:
/// запись делает `Action::MoveModule` через `Runner`, поэтому тест маппинга
/// не трогает диск. Перенос через границу зоны запрещён: на краю порядок
/// остаётся прежним, а не «заворачивается» в начало зоны.
pub fn moved_order(module: Module, delta: i32) -> Vec<String> {
    let mut config = Config::from(&settings::load());
    let _ = config.move_within_zone(module, delta);
    config.flatten()
}

/// Текущая схема matugen. Совпадает с той, что читает панель.
pub fn current_scheme() -> String {
    oneshot::scheme().unwrap_or_else(|| "scheme-tonal-spot".to_string())
}

/// Индекс текущей схемы в списке окна настроек.
pub fn scheme_index() -> usize {
    scheme_index_of(current_scheme().as_str())
}

/// Индекс схемы по имени. Чистая функция ради теста.
pub fn scheme_index_of(name: &str) -> usize {
    schemes::SCHEMES
        .iter()
        .position(|scheme| *scheme == name)
        .unwrap_or(0)
}

/// Смена схемы: применяется к текущим обоям откреплённо через общий
/// враппер. Меню не ждёт пересчёт matugen.
pub fn set_scheme(index: usize) {
    let scheme = schemes::SCHEMES
        .get(index)
        .copied()
        .unwrap_or("scheme-tonal-spot");
    let Some(file) = current_wallpaper() else {
        log::warn("нет файла обоев для смены схемы");
        return;
    };
    wallpaper::apply_detached(&file, scheme);
}

/// Текущие обои: файл, который поставил `wall.sh`, или первый из списка.
pub fn current_wallpaper() -> Option<String> {
    let path = config_io::home().join(".config/wallpaper");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let file = text.trim().to_string();
        if !file.is_empty() {
            return Some(file);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_is_clamped_to_the_supported_range() {
        assert_eq!(clamp_height(10.0), HEIGHT_MIN);
        assert_eq!(clamp_height(24.0), 24);
        assert_eq!(clamp_height(27.6), 28);
        assert_eq!(clamp_height(48.0), 48);
        assert_eq!(clamp_height(99.0), HEIGHT_MAX);
    }

    #[test]
    fn scheme_index_matches_the_scheme_list() {
        assert_eq!(scheme_index_of("scheme-tonal-spot"), 0);
        assert!(scheme_index_of("scheme-smart") > 0);
        assert_eq!(scheme_index_of("nope"), 0);
        assert_eq!(schemes::SCHEMES.len(), 10);
    }

    #[test]
    fn module_toggles_cover_every_module() {
        for module in Module::ALL {
            let (get, _) = module_toggle(module);
            let _ = get();
        }
    }

    #[test]
    fn moved_order_keeps_every_module_exactly_once() {
        for module in Module::ALL {
            let order = moved_order(module, 1);
            assert_eq!(order.len(), settings::MODULE_KEYS.len());
            for key in settings::MODULE_KEYS {
                assert_eq!(order.iter().filter(|item| *item == key).count(), 1);
            }
        }
    }

    #[test]
    fn moved_order_keeps_zone_boundaries_instead_of_wrapping() {
        let before = settings::load().module_order;
        // Weather первым в центральной зоне: вверх — no-op, а не переброс
        // в конец зоны.
        assert_eq!(
            moved_order(Module::Weather, -1),
            before,
            "первый модуль зоны не поднимается выше"
        );
        // Clock последним в центре: вниз — no-op, а не в начало зоны.
        assert_eq!(
            moved_order(Module::Clock, 1),
            before,
            "последний модуль зоны не опускается ниже"
        );
    }

    #[test]
    fn moved_order_moves_the_module_down() {
        // Правая зона всегда из шести модулей, поэтому сдвиг audio вниз
        // обязан сменить его позицию при любом порядке в файле.
        let before = settings::load()
            .module_order
            .iter()
            .position(|key| key == "audio")
            .expect("audio в порядке");
        let order = moved_order(Module::Audio, 1);
        let after = order
            .iter()
            .position(|key| key == "audio")
            .expect("audio в порядке");
        assert_ne!(before, after, "audio не сдвинулся: {order:?}");
    }
}

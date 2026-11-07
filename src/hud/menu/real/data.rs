use super::*;

/// Есть ли программа: `PATH` или `~/.local/bin`.
pub(crate) fn has(program: &str) -> bool {
    oneshot::program_exists(program)
}

pub(crate) fn has_flatpak() -> bool {
    has("flatpak")
}
pub(crate) fn has_kitty() -> bool {
    has("kitty")
}
pub(crate) fn has_kitty_yazi() -> bool {
    has("kitty") && has("yazi")
}
pub(crate) fn has_kitty_opencode() -> bool {
    has("kitty") && has("opencode")
}
pub(crate) fn has_paru_or_yay() -> bool {
    has("paru") || has("yay")
}
pub(crate) fn has_steam() -> bool {
    has("steam")
}

/// Программы обоев на машине: окно выбора бинарником и запасной `wall.sh`.
/// Собирается снаружи — тест видит обе ветки, не имея ни одной программы.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WallpaperTools {
    pub(crate) picker: bool,
    pub(crate) script: bool,
}

impl WallpaperTools {
    /// Что стоит на машине.
    pub(crate) fn detect() -> Self {
        Self {
            picker: has("hud-wallpaper-rs"),
            script: has("wall.sh"),
        }
    }

    /// Виден ли пункт выбора обоев: окно установлено или есть запасной скрипт.
    pub(crate) fn chooses(&self) -> bool {
        self.picker || self.script
    }
}

/// Базовое имя файла текущих обоев из `~/.config/wallpaper`: показывается
/// справа от «Выбрать обои…». Чтение маленького файла, без процессов.
pub(crate) fn current_wallpaper_label() -> String {
    values::current_wallpaper()
        .as_deref()
        .and_then(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
        })
        .unwrap_or("")
        .to_string()
}
pub(crate) fn has_script(name: &str) -> bool {
    has(name)
}
pub(crate) fn has_calcurse() -> bool {
    oneshot::program_for_spawn_sh("calcurse")
}
pub(crate) fn has_note() -> bool {
    has_script("note.sh")
}
pub(crate) fn has_calc() -> bool {
    has_script("calc.sh")
}
pub(crate) fn has_emoji() -> bool {
    has_script("emoji.sh")
}
pub(crate) fn has_clip() -> bool {
    has_script("clip.sh")
}
pub(crate) fn has_tl() -> bool {
    has_script("tl.sh")
}
pub(crate) fn has_media() -> bool {
    has_script("playerctl.sh")
}
pub(crate) fn has_capture() -> bool {
    has_script("ss.sh")
}
pub(crate) fn has_record() -> bool {
    has_script("record.sh")
}
/// Профиль питания есть не везде: без `/sys`-файла и без сервиса
/// PowerProfiles пункт скрывается, а не падает по `Enter`.
pub(crate) fn has_power_profile() -> bool {
    crate::power_profile::supported()
}

/// Правое значение строки профиля: текущий, а не название действия.
pub(crate) fn power_profile_value() -> String {
    let ru = super::super::settings::load().language == Language::Ru;
    crate::power_profile::current_label(ru).to_string()
}
/// Замок: `dynalock.sh` умеет гасить экран и запускать блокировку. Без него
/// пункт скрыт, а не показывает ошибку по нажатию.
/// Гибернация есть не на каждой машине: без `disk` в `/sys/power/state`
/// ядро не умеет писать образ в swap, и строка была бы обещанием, которое
/// не выполнится.
pub(crate) fn has_hibernate() -> bool {
    std::fs::read_to_string("/sys/power/state")
        .is_ok_and(|states| states.split_whitespace().any(|state| state == "disk"))
}

pub(crate) fn has_dynalock() -> bool {
    has_script("dynalock.sh")
}
pub(crate) fn has_keybinds() -> bool {
    has("hud-keybinds-rs")
}
pub(crate) fn has_yazibinds() -> bool {
    has("hud-yazibinds-rs")
}
/// Подпись модуля панели на языке окна.
pub(crate) fn module_title(module: Module, lang: Language) -> &'static str {
    match lang {
        Language::Ru => match module {
            Module::Tray => "Трей",
            Module::Weather => "Погода",
            Module::Webcam => "Камера",
            Module::Clock => "Часы",
            Module::Recorder => "Запись",
            Module::Battery => "Батарея",
            Module::System => "Система",
            Module::Audio => "Звук",
            Module::Network => "Сеть",
            Module::Dnd => "DND",
        },
        Language::En => match module {
            Module::Tray => "Tray",
            Module::Weather => "Weather",
            Module::Webcam => "Webcam",
            Module::Clock => "Clock",
            Module::Recorder => "Recorder",
            Module::Battery => "Battery",
            Module::System => "System",
            Module::Audio => "Audio",
            Module::Network => "Network",
            Module::Dnd => "DND",
        },
    }
}

/// Синонимы модуля для поиска: английское и русское имя сразу.
pub(crate) fn module_keywords(module: Module) -> &'static [&'static str] {
    match module {
        Module::Tray => &["tray", "трей"],
        Module::Weather => &["weather", "погода"],
        Module::Webcam => &["webcam", "камера"],
        Module::Clock => &["clock", "часы"],
        Module::Recorder => &["recorder", "запись"],
        Module::Battery => &["battery", "батарея"],
        Module::System => &["system", "система"],
        Module::Audio => &["audio", "звук"],
        Module::Network => &["network", "сеть"],
        Module::Dnd => &["dnd", "днд", "do not disturb"],
    }
}

/// Иконка модуля панели.
pub(crate) fn module_icon(module: Module) -> &'static str {
    match module {
        Module::Tray => settings_icons::TRAY,
        Module::Weather => settings_icons::WEATHER,
        Module::Webcam => settings_icons::WEBCAM,
        Module::Clock => settings_icons::CLOCK,
        Module::Recorder => settings_icons::RECORDING,
        Module::Battery => settings_icons::BATTERY,
        Module::System => settings_icons::SYSTEM,
        Module::Audio => settings_icons::SOUND,
        Module::Network => settings_icons::NETWORK,
        Module::Dnd => settings_icons::DND,
    }
}

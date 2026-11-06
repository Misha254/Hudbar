//! Эффекты окон по профилю питания: в «Энергосбережении» niri перестаёт
//! размывать фон и делать окна прозрачными.
//!
//! niri не умеет менять эффекты по профилю питания, зато правила применяются
//! по порядку, и последнее перекрывает предыдущие. Поэтому панель держит
//! отдельный include-файл рядом с конфигом niri: в обычном профиле он пуст, в
//! энергосбережении гасит `blur` у всех окон и слоёв и ставит `opacity 1.0`.
//! Include должен быть подключён в `config.kdl` последним — см.
//! `docs/niri-menu.md`.
//!
//! Пишет файл только панель: она живёт всё время сеанса и замечает смену
//! профиля. Ошибка записи или перечитывания конфига не фатальна — окна просто
//! останутся с прежними эффектами, и в журнале будет строка.

use std::path::PathBuf;

/// Имя include-файла рядом с конфигом niri.
pub const FILE: &str = "hudbar-power.kdl";

/// Энергосбережение: размытие выключено у всех окон и слоёв, окна непрозрачны.
const SAVER: &str = "\
// Сгенерировано HUDbar: профиль питания «Энергосбережение».
// Файл подключён последним, поэтому перекрывает размытие и прозрачность
// остальных правил niri.
window-rule {
    opacity 1.0
    background-effect {
        blur false
    }
}

layer-rule {
    background-effect {
        blur false
    }
}
";

/// Обычный профиль: правила ничего не перекрывают.
const NORMAL: &str = "\
// Сгенерировано HUDbar: профиль питания обычный.
// Пустой include: размытие и прозрачность берутся из остальных файлов niri.
";

/// Содержимое include-файла для профиля. Чистая функция — проверяется тестом.
pub fn config(saver: bool) -> &'static str {
    if saver { SAVER } else { NORMAL }
}

/// Путь include-файла: рядом с конфигом niri.
fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("niri").join(FILE))
}

/// Приводит include к текущему профилю и просит niri перечитать конфиг.
///
/// Файл переписывается только при изменении: niri перечитывает конфиг целиком,
/// и лишний reload на каждом старте панели был бы заметен миганием эффектов.
pub fn apply(saver: bool) -> Result<(), String> {
    let path = path().ok_or_else(|| "не найден каталог конфига".to_string())?;
    let wanted = config(saver);
    let current = std::fs::read_to_string(&path).ok();
    if current.as_deref() == Some(wanted) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("каталог {}: {error}", dir.display()))?;
    }
    // Через временный файл: niri может прочитать include в момент записи.
    let temp = path.with_extension("kdl.tmp");
    std::fs::write(&temp, wanted).map_err(|error| format!("{}: {error}", temp.display()))?;
    std::fs::rename(&temp, &path).map_err(|error| format!("{}: {error}", path.display()))?;
    reload()
}

/// Перечитывание конфига niri. Отдельный процесс — так эффект подхватывается
/// тем же путём, что и после ручной правки правил.
fn reload() -> Result<(), String> {
    let status = std::process::Command::new("niri")
        .args(["msg", "action", "load-config-file"])
        .status()
        .map_err(|error| format!("niri: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("niri load-config-file: {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::config;

    #[test]
    fn saver_turns_off_blur_and_transparency() {
        let text = config(true);
        assert!(text.contains("blur false"), "размытие выключено:\n{text}");
        assert!(text.contains("opacity 1.0"), "окна непрозрачны:\n{text}");
        assert_eq!(
            text.matches("background-effect").count(),
            2,
            "и окна, и слои: панель и меню — слои"
        );
    }

    #[test]
    fn the_normal_profile_overrides_nothing() {
        let text = config(false);
        assert!(!text.contains("window-rule"), "правил нет:\n{text}");
        assert!(!text.contains("layer-rule"), "правил нет:\n{text}");
    }
}

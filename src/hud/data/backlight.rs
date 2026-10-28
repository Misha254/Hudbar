//! Яркость экрана: sysfs вместо запуска процесса.
//!
//! `brightnessctl` и `light` меняют `/sys/class/backlight/<устройство>/brightness`,
//! и события там нет: клавиша пишет файл, а узнать об этом можно только
//! прочитав его. Поэтому значение сверяется чтением одного маленького файла
//! раз в сто миллисекунд. Это дешевле, чем гонять `brightnessctl` (громкость
//! именно поэтому перешла на `pactl subscribe`), и не зависит от того, чем
//! крутят яркость: клавишей, ползунком или руками из терминала.
//!
//! Внешние мониторы через `ddcutil` сюда не входят осознанно: яркость там
//! читается по I2C сотни миллисекунд на запрос, и опрос шины в темпе OSD
//! стоил бы дороже самого оверлея. Появится такой монитор — источником
//! события должен стать тот, кто меняет яркость, а не опрос шины (ROADMAP, F).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::{Shared, Sys};

/// Как часто сверяется значение.
///
/// Сто миллисекунд — предел, за которым нажатие клавиши уже читается как
/// «опоздало»: оверлей звука появляется мгновенно по `pactl subscribe`, и
/// яркость не должна отставать заметно. Чтение sysfs — это одна страница
/// кэша, а не процесс, поэтому такой темп ничего не стоит.
const POLL: Duration = Duration::from_millis(100);

/// Пауза, когда устройства нет вовсе (десктоп без подсветки): искать его
/// десять раз в секунду незачем, хватит раза в пять секунд.
const MISSING_RETRY: Duration = Duration::from_secs(5);

const ROOT: &str = "/sys/class/backlight";

/// Процент от максимума.
///
/// Округление к ближайшему: иначе 119999 из 120000 даёт 99%, и полоса не
/// доходит до края, хотя яркость уже максимальная. `None`, когда максимум
/// нулевой: делить не на что.
fn percent(value: u64, max: u64) -> Option<u8> {
    if max == 0 {
        return None;
    }
    Some((value.saturating_mul(100).saturating_add(max / 2) / max).min(100) as u8)
}

/// Порядок, в котором пробуются устройства.
///
/// `acpi_video*` — firmware-зеркало: оно повторяет значение драйвера урезанно,
/// и «100%» там может не значить максимум панели. Настоящий драйвер
/// (`intel_backlight`, `amdgpu_bl0`) идёт первым. Внутри группы — по имени,
/// чтобы выбор не прыгал от кадра к кадру.
fn ordered<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut all: Vec<&str> = names.into_iter().collect();
    all.sort_unstable();
    all.sort_by_key(|name| name.starts_with("acpi_video"));
    all
}

fn read_u64(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Яркость одного устройства. `None`, если файлы не читаются: это не ноль, а
/// «неизвестно».
fn read_device(dir: &Path) -> Option<u8> {
    let value = read_u64(&dir.join("brightness"))?;
    let max = read_u64(&dir.join("max_brightness"))?;
    percent(value, max)
}

/// Яркость в процентах по указанному каталогу классов.
///
/// Каталог параметром, а не константой, чтобы проверяться на временном дереве
/// без настоящего sysfs.
fn read_root(root: &Path) -> Option<u8> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        // Записи `/sys/class/backlight` — симлинки на устройства, поэтому
        // `file_type` здесь не годится: он видит ссылку, а не каталог.
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    ordered(names.iter().map(String::as_str))
        .into_iter()
        .find_map(|name| read_device(&root.join(name)))
}

pub fn read() -> Option<u8> {
    read_root(Path::new(ROOT))
}

/// Переносит прочитанное значение в состояние.
///
/// `None` значит «устройства нет или файл не прочитался»: поле не трогаем,
/// иначе перезагрузка драйвера гасила бы яркость на панели и в оверлее.
///
/// Возвращает `true`, если состояние изменилось и пора перерисовать панель.
pub(super) fn apply(sys: &mut Sys, value: Option<u8>) -> bool {
    match value {
        Some(value) if sys.brightness != Some(value) => {
            sys.brightness = Some(value);
            true
        }
        _ => false,
    }
}

fn refresh(shared: &Shared, value: Option<u8>) {
    let changed = {
        let mut g = crate::hud::lock::mutex(&shared.sys);
        apply(&mut g, value)
    };
    if changed {
        shared.mark();
    }
}

pub fn spawn(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        loop {
            let value = read();
            // Нет устройства — не долбим каталог каждый кадр: на десктопе его
            // не будет никогда, а через пять секунд появится не позже.
            let pause = if value.is_some() { POLL } else { MISSING_RETRY };
            refresh(&shared, value);
            std::thread::sleep(pause);
        }
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn tree(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("hud-backlight-test-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn device(root: &Path, name: &str, value: u64, max: u64) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("brightness"), format!("{value}\n")).unwrap();
        std::fs::write(dir.join("max_brightness"), format!("{max}\n")).unwrap();
    }

    #[test]
    fn percent_rounds_to_the_nearest_value() {
        assert_eq!(percent(0, 120_000), Some(0));
        assert_eq!(percent(120_000, 120_000), Some(100));
        assert_eq!(percent(119_999, 120_000), Some(100));
        assert_eq!(percent(60_000, 120_000), Some(50));
        assert_eq!(percent(1, 3), Some(33), "две трети — это 67, треть — 33");
        assert_eq!(percent(2, 3), Some(67));
    }

    #[test]
    fn a_zero_maximum_has_no_percentage() {
        assert_eq!(percent(10, 0), None);
    }

    #[test]
    fn values_above_the_maximum_stop_at_one_hundred() {
        assert_eq!(percent(240_000, 120_000), Some(100));
    }

    #[test]
    fn the_real_driver_is_tried_before_the_firmware_mirror() {
        assert_eq!(
            ordered(["acpi_video0", "intel_backlight"]),
            ["intel_backlight", "acpi_video0"]
        );
        assert_eq!(
            ordered(["acpi_video1", "acpi_video0"]),
            ["acpi_video0", "acpi_video1"]
        );
        assert!(ordered([] as [&str; 0]).is_empty());
    }

    #[test]
    fn the_order_does_not_depend_on_directory_listing() {
        let names = ["amdgpu_bl0", "acpi_video0", "intel_backlight"];
        let expected = ordered(names);
        let mut reversed = names;
        reversed.reverse();

        assert_eq!(ordered(reversed), expected);
        assert_eq!(expected, ["amdgpu_bl0", "intel_backlight", "acpi_video0"]);
    }

    #[test]
    fn reads_the_percentage_of_the_preferred_device() {
        let root = tree("preferred");
        device(&root, "acpi_video0", 10, 10);
        device(&root, "intel_backlight", 60_000, 120_000);

        assert_eq!(read_root(&root), Some(50));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn class_entries_are_symlinks_to_devices() {
        // В настоящем sysfs `/sys/class/backlight/intel_backlight` — ссылка на
        // устройство, а не каталог. Проверка `file_type().is_dir()` такую
        // запись отбрасывала, и яркость не читалась вовсе.
        let root = tree("symlinked");
        let devices = root.join("devices");
        device(&devices, "intel_backlight", 120_000, 120_000);
        std::os::unix::fs::symlink(
            devices.join("intel_backlight"),
            root.join("intel_backlight"),
        )
        .unwrap();

        assert_eq!(read_root(&root), Some(100));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unreadable_preferred_device_falls_back_to_the_next() {
        let root = tree("fallback");
        std::fs::create_dir_all(root.join("intel_backlight")).unwrap();
        device(&root, "acpi_video0", 5, 10);

        assert_eq!(
            read_root(&root),
            Some(50),
            "нерабочий драйвер не должен скрывать зеркало"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_missing_value_is_not_zero() {
        let root = tree("broken");
        std::fs::create_dir_all(root.join("intel_backlight")).unwrap();

        assert_eq!(
            read_root(&root),
            None,
            "нечитаемый файл не должен выглядеть как нулевая яркость"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_empty_tree_reads_as_unknown() {
        let root = tree("empty");

        assert_eq!(read_root(&root), None);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_lost_device_does_not_reset_the_state() {
        let mut sys = Sys::default();
        assert!(apply(&mut sys, Some(40)));
        assert!(!apply(&mut sys, None), "пропажа устройства — не событие");
        assert_eq!(sys.brightness, Some(40));
    }

    #[test]
    fn the_same_value_does_not_redraw() {
        let mut sys = Sys::default();
        apply(&mut sys, Some(40));

        assert!(!apply(&mut sys, Some(40)));
        assert_eq!(sys.brightness, Some(40));
    }
}

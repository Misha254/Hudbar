//! Раздел «О системе»: паспорт машины строками-справками.
//!
//! Источники — только файлы: `/etc/os-release`, `/proc/sys/kernel/osrelease`,
//! `/proc/cpuinfo`, `/proc/meminfo`, `/proc/uptime`, `/sys/class/dmi/id`,
//! sysfs батареи и карточек drm. Ни одной внешней команды: дерево меню
//! строится на входе, и блокировать его спавнами или разбирать выводы
//! программ под таймаутом смысла нет. Чего нет в файлах или чего нельзя
//! показать универсально, та строка и не появляется — раздел честно
//! короче, а не рисует заглушки «Неизвестно».

use super::i18n;
use super::settings::Language;
use super::settings_icons;
use super::tree::Node;

const DMI: &str = "/sys/class/dmi/id";
const POWER_SUPPLY: &str = "/sys/class/power_supply";
const DRM: &str = "/sys/class/drm";

/// Раздел целиком. Поисковые слова живут здесь же: снаружи раздел
/// выглядел бы как загадочный чёрный ящик.
pub fn section(lang: Language) -> Node {
    let (title, kw) = (
        match lang {
            Language::Ru => "О системе",
            Language::En => "System info",
        },
        &[
            "specs",
            "характеристики",
            "железо",
            "hardware",
            "cpu",
            "ram",
            "ядро",
            "kernel",
        ] as &[&str],
    );
    Node::submenu(settings_icons::ABOUT, title, rows(lang))
        .with_id("system/info")
        .search_as(kw)
}

/// Строки-справки. Каждая добавляется, только если файлы дали данные.
fn rows(lang: Language) -> Vec<Node> {
    let mut nodes = Vec::new();

    if let Some(text) = os_row(lang) {
        nodes.push(info(lang, "ОС", "OS", &text, "system/os"));
    }
    if let Some(text) = machine_text() {
        nodes.push(info(lang, "Устройство", "Machine", &text, "system/machine"));
    }
    if let Some(text) = cpu_text() {
        nodes.push(info(lang, "Процессор", "CPU", &text, "system/cpu"));
    }
    if let Some(text) = gpu_text() {
        nodes.push(info(lang, "Видеокарта", "GPU", &text, "system/gpu"));
    }
    if let Some(text) = ram_text(lang) {
        nodes.push(info(lang, "ОЗУ", "RAM", &text, "system/ram"));
    }
    let uptime = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|content| content.split_whitespace().next()?.parse().ok());
    if let Some(text) = i18n::uptime(lang, uptime) {
        nodes.push(info(lang, "Аптайм", "Uptime", &text, "system/uptime"));
    }
    if let Some(text) = battery_text(lang) {
        nodes.push(info(
            lang,
            "Батарея",
            "Battery",
            &text,
            "system/battery-health",
        ));
    }

    nodes
}

/// Строка-справка с отдельными подписью и значением.
fn info(lang: Language, ru_label: &str, en_label: &str, value: &str, id: &str) -> Node {
    let label = match lang {
        Language::Ru => ru_label,
        Language::En => en_label,
    };
    Node::info_value(label, value).with_id(id)
}

fn os_row(lang: Language) -> Option<String> {
    let distro = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|content| distro_name(&content).map(str::to_string));
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok()?;
    match (distro, kernel.is_empty()) {
        (Some(distro), false) => {
            let word = match lang {
                Language::Ru => "ядро",
                Language::En => "kernel",
            };
            Some(format!("{distro} · {word} {}", kernel.trim()))
        }
        (Some(distro), true) => Some(distro),
        (None, false) => Some(kernel.trim().to_string()),
        (None, true) => None,
    }
}

fn distro_name(content: &str) -> Option<&str> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim().trim_matches('"'))
        .filter(|value| !value.is_empty())
}

/// Производитель + модель: из dmi, без фантазий. Заглушки dmidecode
/// («To Be Filled», «System Product Name») остаются файловыми байтом —
/// строка их не показывает.
fn machine_text() -> Option<String> {
    let vendor = std::fs::read_to_string(format!("{DMI}/sys_vendor")).ok()?;
    let product = std::fs::read_to_string(format!("{DMI}/product_name")).ok()?;
    let vendor = vendor.trim().trim_end_matches(" Inc.").trim();
    let product = product.trim();
    if vendor.is_empty() || product.is_empty() || !meaningful(product) {
        return None;
    }
    Some(format!("{vendor} {product}"))
}

/// Не заглушка ли это dmi: «To Be Filled By O.E.M.» и прочий шаблонный текст.
fn meaningful(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    !(lower.contains("to be filled") || lower.contains("system product") || lower == "none")
}

fn cpu_text() -> Option<String> {
    let content = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    let model = cpu_model(&content)?;
    let threads = cpu_threads(&content);
    Some(format!("{model} ×{threads}"))
}

/// `model name` из cpuinfo без скобок «(R)»/«(TM)» и двойных пробелов:
/// `Intel Core i5-9300H CPU @ 2.40GHz` читается лучше, чем исходник.
fn cpu_model(content: &str) -> Option<String> {
    let raw = content
        .lines()
        .find(|line| line.starts_with("model name"))
        .and_then(|line| line.split_once(':'))
        .map(|(_, value)| value.trim().replace("(R)", "").replace("(TM)", ""))?;
    let words = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.is_empty() { None } else { Some(words) }
}

/// Логические ядра: сколько строк `processor` — столько и видно системе.
fn cpu_threads(content: &str) -> usize {
    content
        .lines()
        .filter(|line| line.starts_with("processor"))
        .count()
}

/// Встроенные карты по vendor-кодам drm. NVIDIA код 0x10de пропускается:
/// имя дискретки читается из Model в /proc/driver/nvidia, оно честнее.
fn video_vendor(code: u32) -> Option<&'static str> {
    match code {
        0x8086 => Some("Intel"),
        0x1002 | 0x1022 => Some("AMD"),
        0x10de => None, // NVIDIA рисуется по Model из /proc/driver/nvidia
        _ => None,
    }
}

fn gpu_text() -> Option<String> {
    let model = nvidia_information().and_then(|content| nvidia_model(&content));
    let mut vendors = Vec::new();
    if let Ok(entries) = std::fs::read_dir(DRM) {
        for entry in entries.flatten() {
            // Только card0/card1/... — коннекторы вроде card0-DP-1 пропускаем.
            let named = entry.file_name();
            let Some(name) = named.to_str() else { continue };
            let Some(digits) = name.strip_prefix("card") else {
                continue;
            };
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let vendor = std::fs::read_to_string(entry.path().join("device/vendor"))
                .ok()
                .and_then(|code| u32::from_str_radix(code.trim().trim_start_matches("0x"), 16).ok())
                .and_then(video_vendor);
            match vendor {
                Some(name) if !vendors.contains(&name) => vendors.push(name),
                _ => {}
            }
        }
    }
    let mut parts: Vec<String> = Vec::new();
    if let Some(model) = model {
        parts.push(model);
    }
    for vendor in vendors {
        parts.push(vendor.to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

/// `/proc/driver/nvidia/gpus/<pci>/information` — единственный файл с
/// маркетинговым именем дискретки («GeForce GTX 1660 Ti») без вызова lspci.
fn nvidia_information() -> Option<String> {
    let pci = std::fs::read_dir("/proc/driver/nvidia/gpus").ok()?.next()?;
    let content = std::fs::read_to_string(pci.ok()?.path().join("information")).ok()?;
    Some(content)
}

fn nvidia_model(content: &str) -> Option<String> {
    content
        .lines()
        .find(|line| line.starts_with("Model:"))
        .map(|line| {
            let value = line.strip_prefix("Model:").unwrap_or(line).trim();
            value.to_string()
        })
        .filter(|value| !value.is_empty())
}

fn ram_text(lang: Language) -> Option<String> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    let gib = mem_total_gib(&content)?;
    Some(i18n::gib(lang, gib))
}

/// `MemTotal` в meminfo даёт килобайты: 16384000 kB → 15.6 ГиБ.
fn mem_total_gib(content: &str) -> Option<f64> {
    let kilobytes: f64 = content
        .lines()
        .find(|line| line.starts_with("MemTotal"))
        .and_then(|line| line.split_whitespace().nth(1))?
        .parse()
        .ok()?;
    Some(kilobytes / 1024.0 / 1024.0)
}

/// Здоровье батареи: полный заряд против проектного из которого машина
/// делает сам вывод; лимит заряда — если под вечером есть файл порога.
fn battery_text(lang: Language) -> Option<String> {
    let dir = first_battery_dir()?;
    let full = read_uint(join(&dir, "charge_full"))?;
    let design = read_uint(join(&dir, "charge_full_design")).filter(|design| *design > 0)?;
    let limit = std::fs::read_to_string(join(&dir, "charge_control_end_threshold"))
        .ok()
        .and_then(|content| parse_uint(&content));
    Some(battery_health_text(lang, full, design, limit))
}

/// Первый `BAT*` в power_supply. Остальные источники — mains/usb — не
/// хранят ёмкость.
fn first_battery_dir() -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(POWER_SUPPLY).ok()?;
    let mut paths: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("BAT"))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();
    paths.into_iter().next()
}

fn join(dir: &std::path::Path, file: &str) -> std::path::PathBuf {
    dir.join(file)
}

fn read_uint(path: std::path::PathBuf) -> Option<u64> {
    parse_uint(&std::fs::read_to_string(path).ok()?)
}

fn parse_uint(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// Расчёт с любыми входными числами — для тестов: «104% от проектной · лимит 80%».
fn battery_health_text(lang: Language, full: u64, design: u64, limit: Option<u64>) -> String {
    let percent = ((full as f64 / design as f64) * 100.0).round() as u64;
    match (lang, limit) {
        (Language::Ru, Some(limit)) => format!("{percent}% от проектной · лимит {limit}%"),
        (Language::Ru, None) => format!("{percent}% от проектной"),
        (Language::En, Some(limit)) => format!("{percent}% of design · limit {limit}%"),
        (Language::En, None) => format!("{percent}% of design"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::state::{Menu, Outcome};
    use super::*;

    /// Раздел из одних Info открывается без курсора: первая строка не
    /// выбирается, и Enter при открытии ничего не запускает.
    #[test]
    fn sysinfo_level_has_no_selection() {
        let mut menu = Menu::new(vec![section(Language::Ru)]);
        assert_eq!(menu.enter(), Outcome::Pushed);
        assert_eq!(menu.current().list.selected(), None);
        for item in menu.current().list.items() {
            assert!(
                !item.kind.is_selectable(),
                "в разделе только справки: {}",
                item.title
            );
        }
    }

    /// Поисковые слова живут на родительском разделе и покрывают железо и
    /// ядро; сами строки-справки в поиск не попадают.
    #[test]
    fn sysinfo_search_words_live_on_the_section() {
        let root = vec![section(Language::Ru)];
        let words = &root[0].keywords;
        for word in ["характеристики", "железо", "specs", "cpu", "ram", "ядро"]
        {
            assert!(words.contains(&word), "нет слова поиска {word}");
        }
        for (word, expected) in [
            ("cpu", "О системе"),
            ("ядро", "О системе"),
            ("kernel", "О системе"),
            ("ram", "О системе"),
        ] {
            let mut menu = Menu::new(root.clone());
            for ch in word.chars() {
                menu.type_char(ch);
            }
            let titles: Vec<&str> = menu
                .current()
                .list
                .items()
                .iter()
                .map(|item| item.title.as_str())
                .collect();
            assert_eq!(titles, vec![expected], "запрос {word}");
        }
    }

    #[test]
    fn distro_name_strips_quotes() {
        let content = "NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nID=arch\n";
        assert_eq!(distro_name(content), Some("Arch Linux"));
        assert_eq!(distro_name(""), None);
        assert_eq!(distro_name("PRETTY_NAME=\"\"\n"), None);
    }

    #[test]
    fn cpu_model_strips_markers_and_collapses_spaces() {
        let content = "processor\t: 0\n\
             model name\t: Intel(R) Core(TM) i5-9300H CPU @ 2.40GHz\n";
        assert_eq!(
            cpu_model(content).as_deref(),
            Some("Intel Core i5-9300H CPU @ 2.40GHz")
        );
        assert_eq!(cpu_model("processor : 0"), None);
    }

    #[test]
    fn cpu_threads_counts_processor_lines() {
        let content = "processor\t: 0\nprocessor\t: 1\nflags\t: sse\nprocessor\t: 2\n";
        assert_eq!(cpu_threads(content), 3);
    }

    #[test]
    fn mem_total_translates_kilobytes() {
        // 15.5 ГиБ: kib → GiB это /1024/1024.
        let content = "MemTotal:       16384000 kB\nMemFree:         1000000 kB\n";
        let gib = mem_total_gib(content).expect("оговорено");
        assert!((gib - 15.625).abs() < 0.001, "получилось {gib:.3}");
        assert_eq!(mem_total_gib("SwapCached: 0"), None);
    }

    #[test]
    fn video_vendor_knows_the_two_common_integrators() {
        assert_eq!(video_vendor(0x8086), Some("Intel"));
        assert_eq!(video_vendor(0x1002), Some("AMD"));
        assert_eq!(video_vendor(0x1234), None);
        assert_eq!(video_vendor(0x10de), None, "NVIDIA читается по Model");
    }

    #[test]
    fn nvidia_model_reads_the_information_file_shape() {
        let content = "Model:            GeForce GTX 1660 Ti\n\
             Driver Version:   580.0\n";
        assert_eq!(
            nvidia_model(content).as_deref(),
            Some("GeForce GTX 1660 Ti")
        );
        assert_eq!(nvidia_model("Driver Version: 1"), None);
    }

    #[test]
    fn battery_health_rounds_and_honours_the_limit() {
        assert_eq!(
            battery_health_text(Language::Ru, 3_805_000, 3_650_000, Some(80)),
            "104% от проектной · лимит 80%"
        );
        assert_eq!(
            battery_health_text(Language::Ru, 3_650_000, 3_650_000, None),
            "100% от проектной"
        );
        assert_eq!(
            battery_health_text(Language::En, 3_650_000, 3_650_000, None),
            "100% of design"
        );
    }

    #[test]
    fn meaningful_skips_the_dmi_placeholders() {
        assert!(meaningful("G3 3590"));
        assert!(!meaningful("To Be Filled By O.E.M."));
        assert!(!meaningful("System Product Name"));
        assert!(!meaningful("None"));
    }
}

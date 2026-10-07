//! Динамический раздел «Устройства»: накопители и безопасное извлечение.
//!
//! W5.5 — сознательно узкий кусок «управления устройствами»: список дисков,
//! размонтирование и извлечение. Ни отключения питания дисков, ни polkit, ни
//! монтирования здесь нет — на ноутбуке это всё либо требует пароля, либо
//! рискует данными, а раздел должен оставаться безопасным.
//!
//! Чтение — один вызов `lsblk --json`: он отдаёт дерево дисков с разделами,
//! точками монтирования, типом накопителя и признаком «съёмный». Действия —
//! `udisksctl unmount` и `udisksctl eject`, потому что udisks знает, что
//! занято, и отвечает об этом человеческим текстом.
//!
//! Почему не `udisksctl dump`: на этой машине он не умеет JSON и отдаёт
//! текстовое дерево с отступами, которое пришлось бы разбирать построчно.
//! `lsblk` даёт тот же список структурно.
//!
//! Безопасность. Кнопка «Извлечь» есть только у съёмных накопителей: у
//! внутреннего SATA или NVMe её не бывает в принципе. Кнопка
//! «Размонтировать» не появляется для системных точек — `/`, `/home`,
//! `/boot`, `/efi`: снять их с места можно, но система на этом не
//! заработает, а нажатие на разделе в меню выглядело бы как ошибка человека.
//! Всё остальное — пользовательские монтирования вроде `/mnt/data` — можно.

use super::action::Action;
use super::bluetooth::Block;
use super::settings::Language;
use super::settings_icons;
use super::system::{
    CommandError, CommandRunner, CommandSpec, ProviderError, ProviderKey, SystemCommandRunner,
    SystemProvider,
};
use super::tree::Node;

/// Псевдоточка монтирования: lsblk так помечает своп, и размонтировать его
/// нельзя — это не файловая система.
pub fn is_pseudo_mount(mount: &str) -> bool {
    mount.starts_with('[') && mount.ends_with(']')
}

/// Имена виртуальных блочных устройств: они есть в `lsblk`, но это не
/// накопители, и в списке дисков им не место.
pub fn is_virtual_disk(path: &str) -> bool {
    ["/dev/zram", "/dev/loop", "/dev/ram", "/dev/fd"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// Точки монтирования, которые нельзя предлагать снимать: система на них не
/// поднимется. Список закрытый и намеренно короткий.
pub const SYSTEM_MOUNTS: [&str; 5] = ["/", "/home", "/boot", "/boot/efi", "/efi"];

/// Раздел носителя.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Volume {
    /// Путь устройства: `/dev/sda1`.
    pub path: String,
    /// Размер в байтах.
    pub size: u64,
    /// Файловая система, если смонтирована или распознана.
    pub fs: Option<String>,
    /// Точка монтирования, если раздел смонтирован.
    pub mount: Option<String>,
}

impl Volume {
    /// Можно ли предлагать размонтирование: раздел действительно смонтирован
    /// и это не системная точка.
    ///
    /// Своп lsblk помечает псевдопочкой `[SWAP]` — это не монтирование, и
    /// `udisksctl unmount` на таком разделе всё равно откажет. Поэтому
    /// квадратные скобки в точке монтирования отсеиваются вместе с системными.
    pub fn can_unmount(&self) -> bool {
        match &self.mount {
            Some(mount) => !SYSTEM_MOUNTS.contains(&mount.as_str()) && !is_pseudo_mount(mount),
            None => false,
        }
    }

    /// Личность строки: путь устройства без ведущего слэша — он же аргумент
    /// `-b` для udisksctl, но в личности двойной слэш читался бы как опечатка.
    pub fn row_id(&self) -> String {
        format!("storage/volume/{}", self.path.trim_start_matches('/'))
    }
}

/// Накопитель целиком.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Disk {
    /// Путь устройства: `/dev/sda`.
    pub path: String,
    /// Модель из lsblk, может быть пустым.
    pub model: Option<String>,
    /// Размер в байтах.
    pub size: u64,
    /// Транспорт: `sata`, `nvme`, `usb`.
    pub transport: Option<String>,
    /// Вращается ли: у SSD `false`, и это полезно видеть в списке.
    pub rotational: bool,
    /// Съёмный накопитель: только у USB-флешек и карт.
    pub removable: bool,
    /// Разделы накопителя.
    pub volumes: Vec<Volume>,
}

impl Disk {
    /// Подпись в списке: модель, размер и транспорт.
    pub fn title(&self) -> String {
        let mut parts = Vec::new();
        parts.push(match &self.model {
            Some(model) if !model.trim().is_empty() => model.trim().to_string(),
            _ => self.path.clone(),
        });
        parts.push(human_size(self.size));
        if let Some(transport) = self
            .transport
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            parts.push(transport.to_string());
        }
        if self.rotational {
            parts.push("вращающийся".to_string());
        }
        parts.join(" · ")
    }

    /// Личность строки: путь устройства без ведущего слэша.
    pub fn row_id(&self) -> String {
        format!("storage/disk/{}", self.path.trim_start_matches('/'))
    }

    /// Можно ли предлагать извлечение: только съёмный накопитель.
    pub fn can_eject(&self) -> bool {
        self.removable
    }
}

/// Типизированный снимок раздела «Устройства».
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageSnapshot {
    /// Накопители: `Missing` — нет `lsblk`, `Failed` — команда ответила мусором.
    pub disks: Block<Vec<Disk>>,
}

/// Провайдер раздела «Устройства».
pub struct StorageProvider;

impl SystemProvider for StorageProvider {
    type Snapshot = StorageSnapshot;

    fn key(&self) -> ProviderKey {
        ProviderKey::STORAGE
    }

    fn fetch(&self, runner: &dyn CommandRunner) -> Result<Self::Snapshot, ProviderError> {
        // Один вызов на всё дерево: список дисков, разделов и точек
        // монтирования приходит разом, и N+1 на каждый диск не нужен.
        let disks = disks(runner);
        if matches!(disks, Block::Missing) {
            return Err(ProviderError::Unavailable("нет lsblk".to_string()));
        }
        Ok(StorageSnapshot { disks })
    }

    fn nodes(&self, snapshot: &Self::Snapshot, lang: Language) -> Vec<Node> {
        build_nodes(snapshot, lang)
    }
}

/// Fetch + nodes одним вызовом: так провайдера зовёт цикл окна.
pub fn fetch_nodes(lang: Language) -> Result<Vec<Node>, String> {
    let provider = StorageProvider;
    let snapshot = provider
        .fetch(&SystemCommandRunner::default())
        .map_err(|error| error.message())?;
    Ok(provider.nodes(&snapshot, lang))
}

/// Список накопителей одним вызовом `lsblk --json`.
fn disks(runner: &dyn CommandRunner) -> Block<Vec<Disk>> {
    let spec = CommandSpec::new("lsblk")
        .arg("--json")
        .arg("--bytes")
        .arg("--paths")
        .arg("--output")
        .arg("NAME,MODEL,SIZE,TRAN,ROTA,TYPE,FSTYPE,MOUNTPOINT,RM,HOTPLUG");
    match runner.run(&spec) {
        Err(CommandError::Spawn(_)) => Block::Missing,
        Err(error) => Block::Failed(error.to_string()),
        Ok(output) if output.success() => match parse_disks(&output.stdout) {
            Ok(list) => Block::Ready(list),
            Err(reason) => Block::Failed(reason),
        },
        Ok(output) => Block::Failed(failure(&output.stderr)),
    }
}

/// Причина сбоя: первая строка stderr, а если её нет — код возврата.
fn failure(stderr: &str) -> String {
    let line = stderr.lines().map(str::trim).find(|line| !line.is_empty());
    match line {
        Some(text) => text.to_string(),
        None => "lsblk: команда не сработала".to_string(),
    }
}

/// Разбор `lsblk --json`: дерево `blockdevices`, где у диска лежат
/// `children` — разделы. Вложенностью дисков больше одного уровня на ноутбуке
/// не бывает, но рекурсия тут дешевле, чем предположение.
pub fn parse_disks(text: &str) -> Result<Vec<Disk>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let entries = value
        .get("blockdevices")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "ожидался список blockdevices".to_string())?;
    let mut disks = Vec::with_capacity(entries.len());
    for entry in entries {
        // Раздел верхнего уровня без типа `disk` — это не накопитель.
        if entry.get("type").and_then(serde_json::Value::as_str) != Some("disk") {
            continue;
        }
        let path = entry
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if path.is_empty() || is_virtual_disk(&path) {
            continue;
        }
        disks.push(Disk {
            path: path.clone(),
            model: text_field(entry, "model"),
            size: number_field(entry, "size"),
            transport: text_field(entry, "tran"),
            rotational: entry
                .get("rota")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            removable: entry
                .get("rm")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            volumes: parse_volumes(entry, &path),
        });
    }
    Ok(disks)
}

/// Разделы накопителя: `children` с типом `part`.
fn parse_volumes(entry: &serde_json::Value, disk: &str) -> Vec<Volume> {
    entry
        .get("children")
        .and_then(serde_json::Value::as_array)
        .map(|children| {
            children
                .iter()
                .filter_map(|child| {
                    let path = child
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .trim();
                    if path.is_empty() {
                        return None;
                    }
                    Some(Volume {
                        path: path.to_string(),
                        size: number_field(child, "size"),
                        fs: text_field(child, "fstype"),
                        mount: child
                            .get("mountpoint")
                            .and_then(serde_json::Value::as_str)
                            .map(str::trim)
                            .filter(|mount| !mount.is_empty())
                            .map(str::to_string),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|volume| volume.path != disk)
        .collect()
}

/// Текстовое поле: пустая строка и `null` — это «не знаю».
fn text_field(entry: &serde_json::Value, path: &str) -> Option<String> {
    entry
        .get(path)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Числовое поле: `lsblk --bytes` отдаёт размер целым числом.
fn number_field(entry: &serde_json::Value, path: &str) -> u64 {
    entry
        .get(path)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
}

/// Размер по-человечески: 447.1 ГиБ, а не 480103981056.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [(&str, u64); 3] = [("ГиБ", 1 << 30), ("МиБ", 1 << 20), ("КиБ", 1 << 10)];
    for (unit, step) in UNITS {
        if bytes >= step {
            return format!("{:.1} {unit}", bytes as f64 / step as f64);
        }
    }
    format!("{bytes} Б")
}

/// Команда отмонтирования: `udisksctl unmount -b /dev/sda1`.
pub fn unmount_action(path: &str) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("udisksctl")
            .arg("unmount")
            .arg("-b")
            .arg(path),
        refresh: ProviderKey::STORAGE,
    }
}

/// Команда извлечения: `udisksctl eject -b /dev/sda`. Предлагается только
/// съёмным накопителям.
pub fn eject_action(path: &str) -> Action {
    Action::RefreshAndRun {
        command: CommandSpec::new("udisksctl")
            .arg("eject")
            .arg("-b")
            .arg(path),
        refresh: ProviderKey::STORAGE,
    }
}

fn ru(lang: Language) -> bool {
    lang == Language::Ru
}

/// Статусная строка раздела.
fn status_row(lang: Language, snapshot: &StorageSnapshot) -> Node {
    let id = "storage/status";
    let text = match &snapshot.disks {
        Block::Missing => {
            return Node::info(
                settings_icons::DOT,
                if ru(lang) {
                    "lsblk нет"
                } else {
                    "lsblk is missing"
                },
            )
            .with_id(id);
        }
        Block::Failed(reason) => {
            return Node::info(
                settings_icons::DOT,
                &if ru(lang) {
                    format!("Диски: {reason}")
                } else {
                    format!("Disks: {reason}")
                },
            )
            .with_id(id);
        }
        Block::Ready(list) if list.is_empty() => {
            return Node::info(
                settings_icons::DOT,
                if ru(lang) {
                    "Нет накопителей"
                } else {
                    "No disks"
                },
            )
            .with_id(id);
        }
        Block::Ready(list) => {
            let word = if ru(lang) { "дисков" } else { "disks" };
            format!("{word}: {}", list.len())
        }
    };
    Node::info(settings_icons::BATTERY, &text).with_id(id)
}

/// Подменю одного накопителя: разделы, их точки монтирования и кнопки.
fn disk_submenu(lang: Language, disk: &Disk) -> Node {
    let mut children = vec![
        Node::info(
            settings_icons::DOT,
            &if ru(lang) {
                format!("Диск: {}", disk.path)
            } else {
                format!("Disk: {}", disk.path)
            },
        )
        .with_id(&format!("{}/path", disk.row_id())),
    ];

    for volume in &disk.volumes {
        let mut title = format!("{} · {}", volume.path, human_size(volume.size));
        if let Some(fs) = &volume.fs {
            title.push_str(&format!(" · {fs}"));
        }
        if let Some(mount) = &volume.mount {
            title.push_str(&format!(" · {mount}"));
        }
        children.push(Node::info(settings_icons::DOT, &title).with_id(&volume.row_id()));
        if volume.can_unmount() {
            children.push(
                Node::action(
                    settings_icons::MINUS,
                    &format!("Размонтировать {}", volume.path),
                    unmount_action(&volume.path),
                )
                .with_id(&format!("{}/unmount", volume.row_id())),
            );
        }
    }

    if disk.can_eject() {
        children.push(
            Node::action(
                settings_icons::MINUS,
                if ru(lang) {
                    "Безопасно извлечь"
                } else {
                    "Safely eject"
                },
                eject_action(&disk.path),
            )
            .with_id(&format!("{}/eject", disk.row_id())),
        );
    }

    Node::submenu(settings_icons::BATTERY, &disk.title(), children).with_id(&disk.row_id())
}

/// Строки раздела «Устройства» из готового снимка.
pub fn build_nodes(snapshot: &StorageSnapshot, lang: Language) -> Vec<Node> {
    let mut rows = vec![status_row(lang, snapshot)];
    if let Block::Ready(list) = &snapshot.disks {
        rows.extend(list.iter().map(|disk| disk_submenu(lang, disk)));
    }
    rows.push(
        Node::action(
            settings_icons::UP,
            if ru(lang) {
                "Обновить"
            } else {
                "Refresh"
            },
            Action::RefreshDynamic(ProviderKey::STORAGE),
        )
        .with_id("storage/refresh"),
    );
    rows
}

#[cfg(test)]
mod tests {
    use super::super::system::{CommandError, CommandOutput, ScriptedRunner};
    use super::*;

    const LSBLK: &str = r#"{
        "blockdevices": [
            {"name": "/dev/sda", "model": "KINGSTON SA400S37480G", "size": 480103981056, "tran": "sata", "rota": false, "type": "disk", "fstype": null, "mountpoint": null, "rm": false, "hotplug": false,
             "children": [
                {"name": "/dev/sda1", "model": null, "size": 536870912, "tran": null, "rota": false, "type": "part", "fstype": "vfat", "mountpoint": "/boot", "rm": false, "hotplug": false},
                {"name": "/dev/sda2", "model": null, "size": 350717025792, "tran": null, "rota": false, "type": "part", "fstype": "f2fs", "mountpoint": "/mnt/data", "rm": false, "hotplug": false}
             ]},
            {"name": "/dev/nvme0n1", "model": "IM2P33F3 NVMe ADATA 512GB", "size": 512110190592, "tran": "nvme", "rota": false, "type": "disk", "fstype": null, "mountpoint": null, "rm": false, "hotplug": false,
             "children": [
                {"name": "/dev/nvme0n1p2", "model": null, "size": 510795972608, "tran": null, "rota": false, "type": "part", "fstype": "ext4", "mountpoint": "/", "rm": false, "hotplug": false},
                {"name": "/dev/nvme0n1p3", "model": null, "size": 1292894208, "tran": null, "rota": false, "type": "part", "fstype": "ext4", "mountpoint": "/home", "rm": false, "hotplug": false}
             ]}
        ]
    }"#;

    const USB: &str = r#"{
        "blockdevices": [
            {"name": "/dev/sdb", "model": "DataTraveler 3.0", "size": 32015167488, "tran": "usb", "rota": true, "type": "disk", "fstype": null, "mountpoint": null, "rm": true, "hotplug": true,
             "children": [
                {"name": "/dev/sdb1", "model": null, "size": 32014880768, "tran": null, "rota": false, "type": "part", "fstype": "exfat", "mountpoint": "/run/media/mihail/DATA", "rm": false, "hotplug": true}
             ]}
        ]
    }"#;

    fn ok(stdout: &str) -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            status: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    fn missing() -> Result<CommandOutput, CommandError> {
        Err(CommandError::Spawn("нет lsblk".to_string()))
    }

    #[test]
    fn parses_disks_with_their_partitions() {
        let disks = parse_disks(LSBLK).expect("список разобран");
        assert_eq!(disks.len(), 2);
        let sata = &disks[0];
        assert_eq!(sata.path, "/dev/sda");
        assert_eq!(sata.model.as_deref(), Some("KINGSTON SA400S37480G"));
        assert_eq!(sata.transport.as_deref(), Some("sata"));
        assert!(!sata.rotational, "SSD не вертится");
        assert!(!sata.removable);
        assert_eq!(sata.volumes.len(), 2);
        assert_eq!(sata.volumes[1].mount.as_deref(), Some("/mnt/data"));
        assert_eq!(sata.volumes[0].fs.as_deref(), Some("vfat"));
    }

    #[test]
    fn system_mounts_are_never_offered_for_unmount() {
        let disks = parse_disks(LSBLK).expect("список разобран");
        let nvme = &disks[1];
        let root = nvme
            .volumes
            .iter()
            .find(|volume| volume.mount.as_deref() == Some("/"))
            .expect("корень есть");
        assert!(!root.can_unmount(), "корень снимать нельзя");
        let home = nvme
            .volumes
            .iter()
            .find(|volume| volume.mount.as_deref() == Some("/home"))
            .expect("home есть");
        assert!(!home.can_unmount(), "/home тоже");
        let boot = &disks[0].volumes[0];
        assert!(!boot.can_unmount(), "/boot тоже");
        let data = &disks[0].volumes[1];
        assert!(data.can_unmount(), "а /mnt/data пользовательское");
    }

    #[test]
    fn an_unmounted_volume_has_nothing_to_unmount() {
        let volume = Volume {
            path: "/dev/sdb1".to_string(),
            size: 1024,
            fs: Some("exfat".to_string()),
            mount: None,
        };
        assert!(!volume.can_unmount());
    }

    #[test]
    fn eject_exists_only_for_removable_disks() {
        let disks = parse_disks(LSBLK).expect("список разобран");
        assert!(!disks[0].can_eject(), "внутренний SATA не извлекается");
        let usb = parse_disks(USB).expect("список разобран");
        assert!(usb[0].can_eject(), "флешка съёмная");
    }

    /// Своп помечается lsblk как `[SWAP]`, и это не монтирование: кнопки на
    /// нём быть не должно, как и у виртуальных zram-устройств, которым не
    /// место в списке накопителей.
    #[test]
    fn swap_and_virtual_devices_are_not_storage() {
        assert!(is_pseudo_mount("[SWAP]"));
        assert!(!is_pseudo_mount("/mnt/data"));
        assert!(is_virtual_disk("/dev/zram0"));
        assert!(is_virtual_disk("/dev/loop3"));
        assert!(!is_virtual_disk("/dev/sda"));
        let swap = Volume {
            path: "/dev/sda3".to_string(),
            size: 8589934592,
            fs: Some("swap".to_string()),
            mount: Some("[SWAP]".to_string()),
        };
        assert!(!swap.can_unmount(), "своп не размонтируется");

        let with_zram = LSBLK.replace(
            r#""blockdevices": ["#,
            r#""blockdevices": [{"name": "/dev/zram0", "model": null, "size": 8205631488, "tran": null, "rota": false, "type": "disk", "fstype": null, "mountpoint": null, "rm": false, "hotplug": false}, "#,
        );
        let disks = parse_disks(&with_zram).expect("список разобран");
        assert!(
            disks.iter().all(|disk| disk.path != "/dev/zram0"),
            "zram не накопитель: {:?}",
            disks.iter().map(|d| &d.path).collect::<Vec<_>>()
        );
        assert_eq!(disks.len(), 2, "остались только настоящие накопители");
    }

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(human_size(480103981056), "447.1 ГиБ");
        assert_eq!(human_size(536870912), "512.0 МиБ");
        assert_eq!(human_size(1024), "1.0 КиБ");
        assert_eq!(human_size(512), "512 Б");
    }

    #[test]
    fn commands_are_udisksctl_with_the_device_path() {
        let argv = |action: Action| match action {
            Action::RefreshAndRun { command, .. } => command.argv().1,
            _ => panic!("ожидалась команда"),
        };
        assert_eq!(
            argv(unmount_action("/dev/sda2")),
            ["unmount", "-b", "/dev/sda2"]
        );
        assert_eq!(argv(eject_action("/dev/sdb")), ["eject", "-b", "/dev/sdb"]);
    }

    #[test]
    fn fetch_asks_lsblk_exactly_once() {
        let runner = ScriptedRunner::new(vec![ok(LSBLK)]);
        let snapshot = StorageProvider.fetch(&runner).expect("снимок");
        assert_eq!(
            runner.calls(),
            [
                "lsblk --json --bytes --paths --output NAME,MODEL,SIZE,TRAN,ROTA,TYPE,FSTYPE,MOUNTPOINT,RM,HOTPLUG"
            ]
        );
        assert_eq!(snapshot.disks.ready().map(Vec::len), Some(2));
    }

    #[test]
    fn missing_lsblk_makes_the_section_unavailable() {
        let runner = ScriptedRunner::new(vec![missing()]);
        let error = StorageProvider
            .fetch(&runner)
            .expect_err("без lsblk раздел недоступен");
        assert!(matches!(error, ProviderError::Unavailable(_)), "{error:?}");
        assert_eq!(error.message(), "нет lsblk");
    }

    #[test]
    fn broken_json_is_a_failed_block_not_an_error() {
        let runner = ScriptedRunner::new(vec![ok("{\"blockdevices\":")]);
        let snapshot = StorageProvider.fetch(&runner).expect("снимок");
        assert!(snapshot.disks.failure().is_some());
    }

    #[test]
    fn nodes_offer_unmount_for_user_mounts_and_eject_only_for_usb() {
        let snapshot = StorageSnapshot {
            disks: Block::Ready(parse_disks(LSBLK).expect("список разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        assert_eq!(nodes[0].title, "дисков: 2");
        let by_id = |id: &str| {
            nodes
                .iter()
                .find(|node| node.identity() == id)
                .unwrap_or_else(|| panic!("нет строки {id}"))
        };
        let sata = by_id("storage/disk/dev/sda");
        assert_eq!(sata.title, "KINGSTON SA400S37480G · 447.1 ГиБ · sata");
        let children: Vec<&str> = sata
            .children()
            .expect("дети есть")
            .iter()
            .map(|c| c.identity())
            .collect();
        assert!(children.contains(&"storage/volume/dev/sda2"));
        assert!(
            children.contains(&"storage/volume/dev/sda2/unmount"),
            "пользовательский раздел можно снять: {children:?}"
        );
        assert!(
            !children.contains(&"storage/volume/dev/sda1/unmount"),
            "/boot не трогаем: {children:?}"
        );
        assert!(
            !children.contains(&"storage/disk/dev/sda/eject"),
            "внутренний диск не извлекается: {children:?}"
        );
    }

    #[test]
    fn a_usb_stick_gets_eject_and_unmount() {
        let snapshot = StorageSnapshot {
            disks: Block::Ready(parse_disks(USB).expect("список разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::En);
        let disk = nodes
            .iter()
            .find(|node| node.identity() == "storage/disk/dev/sdb")
            .expect("флешка есть");
        let children = disk.children().expect("дети есть");
        assert!(
            children
                .iter()
                .any(|row| row.identity() == "storage/disk/dev/sdb/eject"),
            "у съёмного накопителя есть «Извлечь»"
        );
        assert!(
            children
                .iter()
                .any(|row| row.identity() == "storage/volume/dev/sdb1/unmount"),
            "и размонтировать смонтированный раздел"
        );
        assert_eq!(
            disk.title,
            "DataTraveler 3.0 · 29.8 ГиБ · usb · вращающийся"
        );
    }

    #[test]
    fn every_row_has_its_own_identity() {
        for lang in [Language::Ru, Language::En] {
            let snapshot = StorageSnapshot {
                disks: Block::Ready(parse_disks(USB).expect("список разобран")),
            };
            let nodes = build_nodes(&snapshot, lang);
            let mut seen: Vec<String> = Vec::new();
            for node in &nodes {
                seen.push(node.identity().to_string());
                if let Some(children) = node.children() {
                    seen.extend(children.iter().map(|child| child.identity().to_string()));
                }
            }
            let before = seen.len();
            seen.sort();
            seen.dedup();
            assert_eq!(before, seen.len(), "повторяющиеся личности: {seen:?}");
        }
    }

    #[test]
    fn every_action_refreshes_the_section() {
        let snapshot = StorageSnapshot {
            disks: Block::Ready(parse_disks(USB).expect("список разобран")),
        };
        let nodes = build_nodes(&snapshot, Language::Ru);
        let mut actions = 0;
        for node in &nodes {
            let children = node.children().unwrap_or(&[]);
            for row in std::iter::once(node).chain(children.iter()) {
                if let super::super::tree::NodeKind::Action(Action::RefreshAndRun {
                    refresh, ..
                }) = &row.kind
                {
                    assert_eq!(*refresh, ProviderKey::STORAGE);
                    actions += 1;
                }
            }
        }
        assert_eq!(actions, 2, "размонтирование и извлечение");
    }

    /// Живой опрос хоста: только read-only команда `lsblk --json`.
    /// Запускается вручную (`cargo test --bin hud-menu-rs -- --ignored live`).
    #[test]
    #[ignore = "обращается к lsblk хоста"]
    fn live_reads_the_host() {
        let snapshot = StorageProvider
            .fetch(&SystemCommandRunner::default())
            .expect("на хосте есть lsblk");
        let disks = snapshot.disks.ready().expect("накопители есть");
        for disk in disks {
            println!("{disk:?}");
        }
    }
}

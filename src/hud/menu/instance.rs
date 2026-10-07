//! Один экземпляр меню на пользователя: pid-файл вместо поиска окна.
//!
//! Слои `zwlr_layer_shell` не видны в `niri msg windows`, поэтому переключатель
//! из скрипта настроек здесь не работает: он ищет `app_id`, которого у слоя
//! нет. Значит единственный способ узнать, открыто ли меню, — pid-файл.
//!
//! Файл может остаться протухшим: процесс убили сигналом, который не ловится,
//! или машина перезагрузилась. Поэтому проверка трёхчастная: сам pid, затем
//! `/proc/<pid>/exe` и — главное — совпадение этого `exe` с нашим бинарником.
//! Одной проверки «процесс жив» мало: pid после перезагрузки или долгого
//! простоя достаётся уже чужому процессу, и меню убило бы его вместо того,
//! чтобы просто открыться. Совпадение `exe` означает «закрыть и выйти»,
//! несовпадение — «занять место и работать».

use std::path::{Path, PathBuf};

/// Имя pid-файла в каталоге сессии.
pub const PID_FILE: &str = "hudbar-menu.pid";

/// Путь к pid-файлу: `$XDG_RUNTIME_DIR`, иначе `/tmp`.
pub fn pid_path() -> PathBuf {
    pid_path_for(PID_FILE)
}

/// Путь к произвольному pid-файлу. Имя бинарника задаётся вызывающим:
/// общий `instance.rs` обслуживает и меню, и wallpaper-picker, и у каждого
/// свой файл, чтобы они не затирали друг друга.
pub fn pid_path_for(name: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    dir.join(name)
}

/// Что делать при старте, увидев pid-файл.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Startup {
    /// Меню уже открыто: надо послать SIGTERM этому pid и выйти.
    CloseRunning(i32),
    /// Pid-файл протух: пишем свой pid и работаем.
    TakeOver,
    /// Файла нет: пишем свой pid и работаем.
    Fresh,
}

/// Решение по содержимому файла и проверке процесса из `/proc`.
pub fn decide(stored: Option<i32>, proc_root: &Path, our_exe: &Path) -> Startup {
    let Some(pid) = stored else {
        return Startup::Fresh;
    };
    if pid > 0 && is_same_exe(pid, proc_root, our_exe) {
        return Startup::CloseRunning(pid);
    }
    // Файл есть, но это не наше меню: значит pid достался чужому процессу.
    // Место занимаем, ничего не убивая.
    Startup::TakeOver
}

/// Читает pid из файла. Мусор в файле считается отсутствием: запись делает
/// только этот процесс, а битый файл — не повод молча вести себя так, будто
/// меню уже открыто.
pub fn read_pid(path: &Path) -> Option<i32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Жив ли процесс: `/proc/<pid>/exe` существует. Проверка существования, а не
/// принадлежности: кто именно запущен, решает [`is_same_exe`].
pub fn alive(pid: i32, proc_root: &Path) -> bool {
    pid > 0 && proc_root.join(pid.to_string()).join("exe").exists()
}

/// Указывает ли `/proc/<pid>/exe` на тот же файл, что и наш.
///
/// Ключевая проверка перед `SIGTERM`: без неё переиспользованный pid приводил
/// к тому, что меню убивало посторонний процесс и само выходило — снаружи это
/// выглядело как «`KP_4` не работает».
pub fn is_same_exe(pid: i32, proc_root: &Path, our_exe: &Path) -> bool {
    if pid <= 0 {
        return false;
    }
    let Ok(target) = std::fs::read_link(proc_root.join(pid.to_string()).join("exe")) else {
        return false;
    };
    same_file(&target, our_exe)
}

/// Сравнение путей с учётом двух особенностей `/proc`:
/// * ссылка бывает «путь (deleted)», если бинарник заменили, пока он работал;
/// * путь может быть относительным — приводим к абсолютному через `canonicalize`.
fn same_file(target: &Path, our_exe: &Path) -> bool {
    let target = target.to_string_lossy();
    let target = target.strip_suffix(" (deleted)").unwrap_or(&target);
    if Path::new(target) == our_exe {
        return true;
    }
    match (
        std::fs::canonicalize(target),
        std::fs::canonicalize(our_exe),
    ) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// Жив ли процесс в системном `/proc`.
pub fn alive_here(pid: i32) -> bool {
    alive(pid, Path::new("/proc"))
}

/// Полный сценарий старта: прочитать файл, проверить процесс, решить.
pub fn startup(proc_root: &Path) -> Startup {
    startup_for(PID_FILE, proc_root)
}

/// Сценарий старта для произвольного имени pid-файла: как `startup`, но
/// ориентируется на переданный вместо `PID_FILE`.
pub fn startup_for(name: &str, proc_root: &Path) -> Startup {
    let our_exe = std::env::current_exe().unwrap_or_default();
    decide(read_pid(&pid_path_for(name)), proc_root, &our_exe)
}

/// Записывает свой pid. Запись атомарная: временный файл рядом и переименование,
/// иначе второй запуск мог бы прочитать половину строки.
pub fn store(path: &Path, pid: i32) -> std::io::Result<()> {
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, format!("{pid}\n"))?;
    std::fs::rename(&temp, path)
}

/// Убирает pid-файл, но только если в нём нашёлся наш pid: иначе успеем
/// снести чужой файл, который успел записать следующий запуск.
pub fn clear(path: &Path, pid: i32) {
    if read_pid(path) == Some(pid) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Имена каталогов в тестах должны быть уникальными: тесты одного крейта
    /// идут параллельно иначе поделают один и тот же файл.
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "hud-menu-test-{tag}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Поддельный `/proc`: у каждого pid настоящая ссылка `exe`, потому что
    /// проверка принадлежности читает именно её, а не наличие каталога.
    /// Возвращается вместе с хранителем: если отдать только путь, временный
    /// каталог удалится до первой же проверки.
    fn fake_proc(root: &TempDir, our_exe: &Path, alive: &[i32], foreign: &[i32]) {
        std::fs::create_dir_all(&root.0).expect("proc root");
        for pid in alive {
            let dir = root.0.join(pid.to_string());
            std::fs::create_dir_all(&dir).expect("proc pid");
            std::os::unix::fs::symlink(our_exe, dir.join("exe")).expect("link to our exe");
        }
        for pid in foreign {
            let dir = root.0.join(pid.to_string());
            std::fs::create_dir_all(&dir).expect("proc pid");
            std::os::unix::fs::symlink("/usr/bin/kitty", dir.join("exe")).expect("foreign link");
        }
    }

    /// Наш бинарник: настоящий файл, на который будут указывать ссылки.
    fn our_binary(dir: &TempDir) -> PathBuf {
        let path = dir.join("hud-menu-rs");
        std::fs::write(&path, b"binary").expect("binary");
        std::fs::canonicalize(&path).expect("canonical binary")
    }

    #[test]
    fn fresh_start_needs_no_file() {
        let proc = TempDir::new("proc-fresh");
        let our = our_binary(&proc);
        fake_proc(&proc, &our, &[111], &[]);
        assert_eq!(decide(None, &proc.0, &our), Startup::Fresh);
    }

    #[test]
    fn running_menu_means_close_and_exit() {
        let proc = TempDir::new("proc-running");
        let our = our_binary(&proc);
        fake_proc(&proc, &our, &[4242], &[]);
        assert_eq!(
            decide(Some(4242), &proc.0, &our),
            Startup::CloseRunning(4242)
        );
    }

    /// Регрессия: pid достался чужому процессу. Меню обязано занять место, а
    /// не послать `SIGTERM` постороннему и выйти — снаружи это выглядело бы
    /// как «бинд не работает».
    #[test]
    fn recycled_pid_is_taken_over_instead_of_killed() {
        let proc = TempDir::new("proc-recycled");
        let our = our_binary(&proc);
        fake_proc(&proc, &our, &[], &[5150]);
        assert!(alive(5150, &proc.0), "процесс жив, но это не наше меню");
        assert_eq!(
            decide(Some(5150), &proc.0, &our),
            Startup::TakeOver,
            "чужой процесс нельзя убивать"
        );
        assert!(
            !is_same_exe(5150, &proc.0, &our),
            "чужой exe не должен считаться нашим"
        );
    }

    /// Протухший pid: файл лежит, процесса нет. Меню занимает место.
    #[test]
    fn stale_pid_is_taken_over() {
        let proc = TempDir::new("proc-stale");
        let our = our_binary(&proc);
        fake_proc(&proc, &our, &[4242], &[]);
        assert_eq!(decide(Some(99), &proc.0, &our), Startup::TakeOver);
        assert_eq!(
            decide(Some(4242), &proc.0, &our),
            Startup::CloseRunning(4242),
            "тот же pid и тот же бинарник — работаем как переключатель"
        );
        assert_eq!(decide(Some(0), &proc.0, &our), Startup::TakeOver);
        assert_eq!(decide(Some(-7), &proc.0, &our), Startup::TakeOver);
    }

    #[test]
    fn alive_looks_at_proc_not_at_the_number() {
        let proc = TempDir::new("proc-alive");
        let our = our_binary(&proc);
        fake_proc(&proc, &our, &[7], &[]);
        assert!(alive(7, &proc.0));
        assert!(!alive(8, &proc.0));
        assert!(!alive(0, &proc.0), "нулевой pid не бывает живым");
        assert!(!alive(-1, &proc.0), "отрицательный pid тоже");
    }

    /// Бинарник заменили, пока меню работает: `/proc` показывает путь с
    /// «(deleted)», и это всё ещё наш процесс.
    #[test]
    fn replaced_binary_still_counts_as_ours() {
        let proc = TempDir::new("proc-deleted");
        let our = our_binary(&proc);
        let dir = proc.0.join("77");
        std::fs::create_dir_all(&dir).expect("proc pid");
        let deleted = format!("{} (deleted)", our.display());
        std::os::unix::fs::symlink(&deleted, dir.join("exe")).expect("deleted link");
        assert!(is_same_exe(77, &proc.0, &our));
    }

    #[test]
    fn startup_decides_from_the_real_file() {
        let dir = TempDir::new("startup");
        let file = dir.join(PID_FILE);
        let proc = TempDir::new("proc-startup");
        let our = our_binary(&proc);
        fake_proc(&proc, &our, &[31337], &[777]);
        assert_eq!(startup_from(&file, &proc.0, &our), Startup::Fresh);

        store(&file, 31337).expect("store");
        assert_eq!(
            startup_from(&file, &proc.0, &our),
            Startup::CloseRunning(31337)
        );

        store(&file, 999).expect("store");
        assert_eq!(
            startup_from(&file, &proc.0, &our),
            Startup::TakeOver,
            "протухший pid не мешает старту: файл перезаписывается"
        );

        store(&file, 777).expect("store");
        assert_eq!(
            startup_from(&file, &proc.0, &our),
            Startup::TakeOver,
            "переиспользованный pid: занимаем место, чужой процесс цел"
        );
    }

    fn startup_from(file: &Path, proc: &Path, our: &Path) -> Startup {
        decide(read_pid(file), proc, our)
    }

    #[test]
    fn garbage_in_the_file_is_treated_as_no_file() {
        let dir = TempDir::new("garbage");
        let file = dir.join(PID_FILE);
        for text in ["", "не число", "12 34", "\n\n"] {
            std::fs::write(&file, text).expect("write");
            assert_eq!(read_pid(&file), None, "текст {text:?} не pid");
        }
    }

    #[test]
    fn store_and_clear_round_trip() {
        let dir = TempDir::new("store");
        let file = dir.join(PID_FILE);
        store(&file, 5150).expect("store");
        assert_eq!(read_pid(&file), Some(5150));
        clear(&file, 5150);
        assert_eq!(read_pid(&file), None);
    }

    /// Чужой pid-файл не трогаем: следующий запуск мог успеть записать свой.
    #[test]
    fn clear_ignores_a_foreign_pid() {
        let dir = TempDir::new("foreign");
        let file = dir.join(PID_FILE);
        store(&file, 2).expect("store");
        clear(&file, 1);
        assert_eq!(read_pid(&file), Some(2), "чужой файл должен уцелеть");
    }

    #[test]
    fn clearing_a_missing_file_is_not_an_error() {
        let dir = TempDir::new("missing");
        clear(&dir.join(PID_FILE), 1);
    }

    #[test]
    fn store_leaves_no_temporary_file() {
        let dir = TempDir::new("temp");
        let file = dir.join(PID_FILE);
        store(&file, 7).expect("store");
        let leftovers: Vec<_> = std::fs::read_dir(&dir.0)
            .expect("read")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "остался временный файл: {leftovers:?}"
        );
    }

    #[test]
    fn pid_path_lives_in_the_session_directory() {
        let path = pid_path();
        let name = path.file_name().expect("имя файла");
        assert_eq!(name, PID_FILE);
        assert!(
            path.parent().is_some_and(|dir| dir.is_absolute()),
            "путь должен быть абсолютным: {path:?}"
        );
    }
}

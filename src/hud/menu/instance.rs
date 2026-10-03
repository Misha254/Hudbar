//! Один экземпляр меню на пользователя: pid-файл вместо поиска окна.
//!
//! Слои `zwlr_layer_shell` не видны в `niri msg windows`, поэтому переключатель
//! из скрипта настроек здесь не работает: он ищет `app_id`, которого у слоя
//! нет. Значит единственный способ узнать, открыто ли меню, — pid-файл.
//!
//! Файл может остаться протухшим: процесс убили сигналом, который не ловится,
//! или машина перезагрузилась. Поэтому проверка двухчастная: сначала сам pid,
//! потом `/proc/<pid>/exe`. Файл с живым pid означает «закрыть и выйти»,
//! протухший — «перезаписать и работать».

use std::path::{Path, PathBuf};

/// Имя pid-файла в каталоге сессии.
pub const PID_FILE: &str = "hudbar-menu.pid";

/// Путь к pid-файлу: `$XDG_RUNTIME_DIR`, иначе `/tmp`.
pub fn pid_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    dir.join(PID_FILE)
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

/// Решение по содержимому файла и живости процесса из `/proc`.
pub fn decide(stored: Option<i32>, proc_root: &Path) -> Startup {
    match stored {
        None => Startup::Fresh,
        Some(pid) if alive(pid, proc_root) => Startup::CloseRunning(pid),
        Some(_) => Startup::TakeOver,
    }
}

/// Читает pid из файла. Мусор в файле считается отсутствием: запись делает
/// только этот процесс, а битый файл — не повод молча вести себя так, будто
/// меню уже открыто.
pub fn read_pid(path: &Path) -> Option<i32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Жив ли процесс. Проверяется не только цифра в файле, но и то, что
/// `/proc/<pid>/exe` действительно существует: pid после перезагрузки
/// принадлежит уже другому процессу.
pub fn alive(pid: i32, proc_root: &Path) -> bool {
    pid > 0 && proc_root.join(pid.to_string()).join("exe").exists()
}

/// Жив ли процесс в системном `/proc`.
pub fn alive_here(pid: i32) -> bool {
    alive(pid, Path::new("/proc"))
}

/// Полный сценарий старта: прочитать файл, проверить процесс, решить.
pub fn startup(proc_root: &Path) -> Startup {
    decide(read_pid(&pid_path()), proc_root)
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

    /// Поддельный `/proc`: каталог `<pid>/exe` существует только для живых.
    /// Возвращается вместе с хранителем: если отдать только путь, временный
    /// каталог удалится до первой же проверки.
    fn fake_proc(alive: &[i32]) -> TempDir {
        let root = TempDir::new("proc");
        for pid in alive {
            std::fs::create_dir_all(root.0.join(pid.to_string()).join("exe")).expect("proc pid");
        }
        std::fs::write(root.0.join("self"), "x").expect("marker");
        root
    }

    #[test]
    fn fresh_start_needs_no_file() {
        let proc = fake_proc(&[111]);
        assert_eq!(decide(None, &proc.0), Startup::Fresh);
    }

    #[test]
    fn running_menu_means_close_and_exit() {
        let proc = fake_proc(&[4242]);
        assert_eq!(decide(Some(4242), &proc.0), Startup::CloseRunning(4242));
    }

    /// Протухший pid: файл лежит, процесса нет. Меню должно занять место, а
    /// не закрыться и не остаться вакантным.
    #[test]
    fn stale_pid_is_taken_over() {
        let proc = fake_proc(&[4242]);
        assert_eq!(decide(Some(99), &proc.0), Startup::TakeOver);
        assert_eq!(
            decide(Some(4242), &proc.0),
            Startup::CloseRunning(4242),
            "тот же pid, но файл протух — работаем сами"
        );
        assert_eq!(decide(Some(0), &proc.0), Startup::TakeOver);
    }

    #[test]
    fn alive_looks_at_proc_not_at_the_number() {
        let proc = fake_proc(&[7]);
        assert!(alive(7, &proc.0));
        assert!(!alive(8, &proc.0));
        assert!(!alive(0, &proc.0), "нулевой pid не бывает живым");
        assert!(!alive(-1, &proc.0), "отрицательный pid тоже");
    }

    #[test]
    fn startup_decides_from_the_real_file() {
        let dir = TempDir::new("startup");
        let file = dir.join(PID_FILE);
        let proc = fake_proc(&[31337]);
        assert_eq!(startup_from(&file, &proc.0), Startup::Fresh);

        store(&file, 31337).expect("store");
        assert_eq!(
            startup_from(&file, &proc.0),
            Startup::CloseRunning(31337)
        );

        store(&file, 999).expect("store");
        assert_eq!(
            startup_from(&file, &proc.0),
            Startup::TakeOver,
            "протухший pid не мешает старту: файл перезаписывается"
        );
    }

    fn startup_from(file: &Path, proc: &Path) -> Startup {
        decide(read_pid(file), proc)
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
        assert!(leftovers.is_empty(), "остался временный файл: {leftovers:?}");
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

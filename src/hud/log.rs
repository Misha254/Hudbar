use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::Local;

use super::data::oneshot;

/// Сколько ждём внешнюю команду, прочитанную через `output`/`status`.
///
/// Предел обязателен: `log::output` зовётся с главного потока панели, и
/// зависший демон (`niri`, `nmcli`, `bluetoothctl`) иначе встал бы колом —
/// ни перерисовки, ни ввода, до самого SIGTERM.
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// Бюджет кадра: столько занимает кадр на 60 Гц и столько же длится тик
/// панели, пока идёт анимация.
///
/// Перерисовка дольше бюджета означает, что кадр опаздывает к своему тику:
/// панель успевает только к следующему, и движение идёт ступенями.
pub const FRAME_BUDGET: Duration = Duration::from_millis(16);

/// Окно наблюдения за кадрами: строка в журнал уходит одна на окно, а не на
/// каждый медленный кадр.
const FRAME_WINDOW: Duration = Duration::from_secs(1);

/// Предел размера журнала до ротации.
///
/// Журнал читают, чтобы понять, что было перед сбоем, а не как хронику за
/// месяцы: без предела он рос бы вместе с потоком «команда не ответила» и
/// пережил бы все полезные строки.
const MAX_BYTES: u64 = 256 * 1024;

/// Каталог журнала: `XDG_STATE_HOME`, а без него — `~/.local/state`.
#[cfg(not(test))]
fn path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        });
    base.join("hudbar.log")
}

/// Каталог журнала в тестовой сборке: свой у каждого процесса прогона.
#[cfg(test)]
fn journal_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("hudbar-test-log-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir
    })
}

/// Путь журнала в тестах — не настоящий.
///
/// Строки «команда не ответила» из прогона принадлежат прогону: настоящий
/// журнал читают, чтобы понять, что было перед сбоем панели, и ложный след там
/// дороже пустого места. Заодно прогон перестал требовать прав на
/// `~/.local/state` и не зависит от `$XDG_STATE_HOME` — в CI его ради этого и
/// подменяли.
#[cfg(test)]
fn path() -> PathBuf {
    journal_dir().join("hudbar.log")
}

/// Файл прошлого журнала: `hudbar.log` → `hudbar.log.1`. Хранится один:
/// второй файл того же возраста не расскажет ничего нового.
fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".1");
    PathBuf::from(name)
}

/// Ротация перед записью: файл дорос до предела — прошлый уходит в `.1`, где
/// его перекрывает следующий.
fn rotate(path: &Path) {
    if fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) < MAX_BYTES {
        return;
    }
    let backup = backup_path(path);
    let _ = fs::remove_file(&backup);
    let _ = fs::rename(path, backup);
}

/// Строка в журнал. Дверь одна на все уровни: замок общий, иначе строки
/// разных уровней перемешались бы посреди строки.
fn write_line(level: &str, message: &str) {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock().ok();
    let path = path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    rotate(&path);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            file,
            "{} {level} {message}",
            Local::now().format("%Y-%m-%d %H:%M:%S")
        );
    }
}

pub fn warn(message: impl AsRef<str>) {
    write_line("WARN", message.as_ref());
}

/// Трассировка жизненного цикла: слои, выводы, `configure`. Включается
/// `HUDBAR_TRACE=1`; пустое значение и `0` — выключено.
///
/// Отдельной зависимости ради логов не заводим: панель запускает композитор,
/// stderr ей никто не читает, и единственное место, где её строки видны, —
/// этот журнал.
pub fn trace(message: impl AsRef<str>) {
    if !trace_enabled() {
        return;
    }
    write_line("TRACE", message.as_ref());
}

/// Включена ли трассировка. Окружение за жизнь процесса не меняется, но
/// кэшировать его незачем: вызовов мало, и переменная читается на месте.
pub fn trace_enabled() -> bool {
    trace_wanted(std::env::var_os("HUDBAR_TRACE").as_deref())
}

/// Правило включения — отдельно от чтения окружения: тест проверяет его, не
/// трогая переменные процесса, которые общие на всю программу.
fn trace_wanted(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

/// Копилка медленных кадров: за окно запоминается самый долгий, и строка о
/// нарушении бюджета уходит одна на окно.
///
/// Без накопления поток медленных кадров при 60 Гц вытеснил бы из журнала
/// причину, ради которой журнал читают. Отдельная структура, а не статика с
/// замком: правило «что и когда писать» проверяется тестом без ожидания
/// секунды.
struct SlowFrames {
    since: Instant,
    worst: Duration,
}

impl SlowFrames {
    fn new(now: Instant) -> Self {
        Self {
            since: now,
            worst: Duration::ZERO,
        }
    }

    /// Отдаёт худший кадр окна, когда окно закрылось и бюджет нарушен.
    fn took(&mut self, took: Duration, now: Instant) -> Option<Duration> {
        self.worst = self.worst.max(took);
        if now.duration_since(self.since) < FRAME_WINDOW {
            return None;
        }
        let worst = std::mem::take(&mut self.worst);
        self.since = now;
        (worst > FRAME_BUDGET).then_some(worst)
    }
}

/// Кадр длиннее бюджета: перерисовка не успевает к своему тику. Длительность
/// считает вызывающий: журнал не знает, что там рисовалось.
///
/// `what` попадает в строку, потому что в одном журнале соседствуют панель и
/// окна — это разные процессы, и по одной длительности их не различить.
pub fn frame_took(what: &str, took: Duration) {
    static SLOW: OnceLock<Mutex<SlowFrames>> = OnceLock::new();
    let now = Instant::now();
    let Ok(mut slow) = SLOW.get_or_init(|| Mutex::new(SlowFrames::new(now))).lock() else {
        return;
    };
    let worst = slow.took(took, now);
    drop(slow);
    if let Some(worst) = worst {
        warn(format!(
            "{what}: самый долгий кадр за секунду — {} мс при бюджете {} мс",
            worst.as_millis(),
            FRAME_BUDGET.as_millis()
        ));
    }
}

/// Замер кадра: `let _frame = Frame::start("меню");` первой строкой отрисовки —
/// на выходе длительность уходит в журнал, если кадр не уложился в бюджет.
///
/// Замер на выходе из области, а не вызовом в её конце: у отрисовки есть
/// ранние выходы, и забытый вызов в одном из них молча выключил бы наблюдение.
#[must_use = "замер работает, пока его держат"]
pub struct Frame(Instant, &'static str);

impl Frame {
    pub fn start(what: &'static str) -> Self {
        Self(Instant::now(), what)
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        frame_took(self.1, self.0.elapsed());
    }
}

/// Строка команды для журнала: программа и аргументы, аргументы с пробелами —
/// в кавычках.
///
/// Секретов здесь нет и быть не должно: сюда попадают только команды чтения, а
/// всё, что несёт секрет, собирается как `CommandSpec` — там секрет помечен
/// типом (`SecretData`, `CmdArg::Secret`) и в строку журнала не попадает.
/// Угадывать секрет по соседнему слову (`password`) нельзя: ключ у команды
/// может быть любой, и правило молча ломается.
fn command_line(program: &str, args: &[&str]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(program.to_string());
    for part in args {
        parts.push(if part.contains(char::is_whitespace) {
            format!("{part:?}")
        } else {
            part.to_string()
        });
    }
    parts.join(" ")
}

pub fn output(program: &str, args: &[&str]) -> Option<Output> {
    let command = command_line(program, args);
    match oneshot::output_with_timeout(program, args, TIMEOUT) {
        Ok(output) if output.status.success() => Some(output),
        Ok(output) => {
            warn(format!(
                "command failed ({}) exit={:?} stderr={}",
                command,
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            None
        }
        // Сюда попадают и «не запустился», и «не ответил за 2000 мс»: оба
        // случая для панели выглядят одинаково — значения просто нет.
        Err(error) => {
            warn(format!("command failed ({command}): {error}"));
            None
        }
    }
}

pub fn status(program: &str, args: &[&str]) -> bool {
    output(program, args).is_some()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use super::{
        FRAME_BUDGET, MAX_BYTES, SlowFrames, TIMEOUT, backup_path, command_line, journal_dir,
        output, rotate, status, trace_wanted, warn,
    };

    /// Аргументы с пробелами берутся в кавычки: строка журнала должна читаться
    /// так же однозначно, как `argv`.
    #[test]
    fn whitespace_arguments_are_quoted() {
        let line = command_line("nmcli", &["dev", "wifi", "connect", "Дом принтер"]);

        assert_eq!(line, "nmcli dev wifi connect \"Дом принтер\"");
    }

    /// Главный поток панели ждёт внешнюю команду синхронно: без предела
    /// зависший `niri` или `nmcli` встал бы колом до самого SIGTERM.
    #[test]
    fn a_sleeping_command_is_cut_off_instead_of_being_waited_out() {
        let started = Instant::now();
        let result = output("sh", &["-c", "sleep 30"]);
        let elapsed = started.elapsed();

        assert!(result.is_none(), "зависшая команда — это None, а не вывод");
        assert!(
            elapsed < TIMEOUT + std::time::Duration::from_secs(2),
            "ждали {elapsed:?} при пределе {TIMEOUT:?}: команда отпущена не была"
        );
    }

    #[test]
    fn status_is_false_when_the_command_outlives_the_timeout() {
        assert!(!status("sh", &["-c", "sleep 30"]));
    }

    #[test]
    fn a_normal_command_still_returns_its_output() {
        let result = output("printf", &["ok"]).expect("printf есть");

        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "ok");
        assert!(result.status.success());
    }

    #[test]
    fn a_missing_program_is_none_not_a_panic() {
        assert!(output("hud-bar-nonexistent", &[]).is_none());
        assert!(!status("hud-bar-nonexistent", &[]));
    }

    #[test]
    fn the_backup_sits_next_to_the_log() {
        assert_eq!(
            backup_path(Path::new("/tmp/hudbar.log")),
            PathBuf::from("/tmp/hudbar.log.1")
        );
    }

    /// Прогон пишет в свой журнал, а не в настоящий: ложное «команда не
    /// ответила» в журнале панели читается как след сбоя.
    #[test]
    fn a_test_run_writes_to_its_own_journal() {
        let path = super::path();

        assert!(
            journal_dir()
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("hudbar-test-log-")),
            "каталог тестового журнала: {:?}",
            journal_dir()
        );
        assert!(
            path.starts_with(std::env::temp_dir()),
            "журнал теста: {path:?}"
        );

        let _ = std::fs::remove_file(&path);
        warn("строка из теста");
        let text = std::fs::read_to_string(&path).expect("журнал теста");

        assert!(text.contains("строка из теста"), "журнал: {text}");
    }

    /// Каталог под файл журнала: имя своё у каждого теста, иначе тесты,
    /// идущие врозь, писали бы в один файл.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("каталог теста");
        dir
    }

    /// Доросший журнал уходит в `.1` целиком, а не обрезается: обрезанный
    /// файл начинался бы с середины строки.
    #[test]
    fn a_full_log_moves_aside_before_the_next_line() {
        let dir = test_dir("hudbar-log-rotate-full");
        let path = dir.join("hudbar.log");
        let file = std::fs::File::create(&path).expect("файл журнала");
        file.set_len(MAX_BYTES).expect("набить журнал");
        drop(file);

        rotate(&path);

        assert!(!path.exists(), "полный журнал должен был уехать в .1");
        assert_eq!(
            std::fs::metadata(backup_path(&path))
                .map(|meta| meta.len())
                .unwrap_or(0),
            MAX_BYTES
        );
    }

    /// Прошлый `.1` перекрывается: двух подряд ротаций журнал не переживёт,
    /// а место на диске ограничено.
    #[test]
    fn rotation_overwrites_the_previous_backup() {
        let dir = test_dir("hudbar-log-rotate-overwrite");
        let path = dir.join("hudbar.log");
        std::fs::write(backup_path(&path), "прошлая ротация").expect("старый .1");
        let file = std::fs::File::create(&path).expect("файл журнала");
        file.set_len(MAX_BYTES + 1).expect("набить журнал");
        drop(file);

        rotate(&path);

        let content = std::fs::read(backup_path(&path)).expect(".1 после ротации");
        assert_eq!(content.len(), (MAX_BYTES + 1) as usize);
        assert!(
            !String::from_utf8_lossy(&content).contains("прошлая ротация"),
            "старый `.1` должен был быть перекрыт"
        );
    }

    /// Мелкий журнал ротация не трогает: иначе строки терялись бы на каждом
    /// запуске панели.
    #[test]
    fn a_small_log_stays_where_it_is() {
        let dir = test_dir("hudbar-log-rotate-small");
        let path = dir.join("hudbar.log");
        std::fs::write(&path, "строка журнала").expect("файл журнала");

        rotate(&path);

        assert!(path.exists(), "мелкий журнал должен остаться на месте");
        assert!(!backup_path(&path).exists(), "ротации быть не должно");
    }

    /// Строка о медленных кадрах одна на окно и по самому долгому кадру:
    /// иначе поток строк вытеснил бы из журнала причину, ради которой его
    /// читают.
    #[test]
    fn the_worst_slow_frame_is_reported_once_per_window() {
        let start = Instant::now();
        let mut slow = SlowFrames::new(start);

        assert_eq!(
            slow.took(Duration::from_millis(30), start),
            None,
            "окно только открылось"
        );
        assert_eq!(
            slow.took(FRAME_BUDGET, start + Duration::from_millis(500)),
            None,
            "окно ещё не закрылось"
        );
        assert_eq!(
            slow.took(FRAME_BUDGET, start + Duration::from_millis(1000)),
            Some(Duration::from_millis(30))
        );
        assert_eq!(
            slow.took(Duration::from_millis(1), start + Duration::from_secs(2)),
            None,
            "в новом окне медленных кадров не было"
        );
    }

    /// Кадр ровно в бюджет — не нарушение: сравнивать надо строго.
    #[test]
    fn a_frame_within_the_budget_is_not_reported() {
        let start = Instant::now();
        let mut slow = SlowFrames::new(start);

        assert_eq!(
            slow.took(FRAME_BUDGET, start + Duration::from_secs(1)),
            None
        );
    }

    /// Трассировка включается переменной, а `0` значит «выключено»: иначе
    /// `HUDBAR_TRACE=0` в конфиге композитора включал бы её.
    #[test]
    fn trace_turns_on_unless_the_variable_says_zero() {
        assert!(!trace_wanted(None));
        assert!(!trace_wanted(Some(OsStr::new(""))));
        assert!(!trace_wanted(Some(OsStr::new("0"))));
        assert!(trace_wanted(Some(OsStr::new("1"))));
        assert!(trace_wanted(Some(OsStr::new("true"))));
    }
}

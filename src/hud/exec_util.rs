//! Единая обёртка запуска внешних утилит.
//!
//! Раньше каждый вызов `Command` сам решал, что делать с таймаутом, кодом
//! выхода и ошибкой запуска. Здесь это в одном месте: зависший процесс
//! убивается вместе с группой, отсутствие утилиты отличается от её
//! ненулевого кода, а вывод всегда собирается целиком.
//!
//! Модуль объявлен у корня бинарника (`#[path = ".../exec_util.rs"]`), как
//! `power_profile`: его подключают и панель, и окна через `crate::exec_util`.
//!
//! Долгоживущие процессы — `pactl subscribe`, `setsid -f ...`, сканирование
//! Bluetooth — сюда не переводятся: у них нет конечного результата, и
//! таймаут означал бы не защиту, а обрыв по расписанию.

use std::fmt;
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

/// Почему команда не дала результата.
#[derive(Debug)]
pub enum ExecError {
    /// Утилиты нет в `PATH` — это не «код выхода», а другая ошибка.
    NotFound { program: String },
    /// Процесс не уложился в лимит и был убит вместе с группой.
    Timeout { program: String, timeout: Duration },
    /// Процесс завершился с ненулевым кодом.
    Exited {
        program: String,
        status: ExitStatus,
        stderr: String,
    },
    /// Запуск не удался по другой причине (права, EMFILE, ...).
    Spawn {
        program: String,
        error: std::io::Error,
    },
    /// Ошибка ожидания уже запущенного процесса.
    Wait {
        program: String,
        error: std::io::Error,
    },
}

impl fmt::Display for ExecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecError::NotFound { program } => {
                write!(formatter, "{program} не найден в PATH")
            }
            ExecError::Timeout { program, timeout } => write!(
                formatter,
                "{program} не ответил за {} мс",
                timeout.as_millis()
            ),
            ExecError::Exited {
                program,
                status,
                stderr,
            } => {
                let reason = first_line(stderr).unwrap_or("без описания");
                write!(
                    formatter,
                    "{program} завершился с кодом {}: {reason}",
                    status.code().unwrap_or(-1)
                )
            }
            ExecError::Spawn { program, error } => {
                write!(formatter, "{program} не запустился: {error}")
            }
            ExecError::Wait { program, error } => {
                write!(formatter, "ошибка ожидания {program}: {error}")
            }
        }
    }
}

impl std::error::Error for ExecError {}

fn first_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

/// Запуск команды с таймаутом. Ненулевой код — ошибка [`ExecError::Exited`]:
/// вызывающему нужен результат, а не «что-то ответило».
///
/// При превышении времени процесс убивается вместе с группой, иначе его дети
/// могли бы удерживать stdout/stderr открытыми и чтение не закончилось бы.
pub fn run_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, ExecError> {
    run_with_timeout_stdin(program, args, None, timeout)
}

/// Как [`run_with_timeout`], но с байтами на stdin. Нужно там, где секрет не
/// должен попадать в `argv`: pipe не виден в `ps` и `/proc/<pid>/cmdline`.
pub fn run_with_timeout_stdin(
    program: &str,
    args: &[&str],
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<Output, ExecError> {
    let output = capture_with_timeout(program, args, stdin, timeout)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(ExecError::Exited {
            program: program.to_string(),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// То же, но ненулевой код остаётся в [`Output`]: некоторым вызывающим нужен
/// именно код (`pgrep` так отвечает «не найден»), а не ошибка.
pub fn capture_with_timeout(
    program: &str,
    args: &[&str],
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<Output, ExecError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ExecError::NotFound {
                program: program.to_string(),
            }
        } else {
            ExecError::Spawn {
                program: program.to_string(),
                error,
            }
        }
    })?;
    // Секрет уходит в отдельном потоке: если команда не читает stdin, запись
    // не должна вешать ожидание; pipe закроется вместе с потоком.
    if let Some(input) = stdin.map(|input| input.to_vec())
        && let Some(mut stdin) = child.stdin.take()
    {
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(&input);
        });
    }
    // Вывод читаем в потоках: если не читать, а процесс много пишет, он
    // заблокируется на записи и не завершится до таймаута.
    let mut out = child.stdout.take().expect("stdout");
    let mut err = child.stderr.take().expect("stderr");
    let out_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = out.read_to_string(&mut buf);
        buf
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = err.read_to_string(&mut buf);
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(5).min(timeout - started.elapsed()));
            }
            Ok(None) => {
                stop_child(&mut child);
                let _ = child.wait();
                let _ = out_handle.join();
                let _ = err_handle.join();
                return Err(ExecError::Timeout {
                    program: program.to_string(),
                    timeout,
                });
            }
            Err(error) => {
                stop_child(&mut child);
                let _ = child.wait();
                let _ = out_handle.join();
                let _ = err_handle.join();
                return Err(ExecError::Wait {
                    program: program.to_string(),
                    error,
                });
            }
        }
    };
    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    Ok(Output {
        status,
        stdout: stdout.into_bytes(),
        stderr: stderr.into_bytes(),
    })
}

fn stop_child(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = -(child.id() as i32);
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(process_group),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    let _ = child.kill();
}

/// Запуск без ожидания и без вывода: приложение, скрипт, новая сессия.
/// Ошибка только от `spawn`: живёт ли процесс дальше — уже не наша забота.
pub fn spawn_detached(program: &str, args: &[&str]) -> Result<(), ExecError> {
    let child = detached_child(program, args)?;
    // Родитель может жить долго (меню, лаунчер): без ожидания завершившийся
    // ребёнок остался бы зомби. Приложение от этого не умирает — ждёт лишь
    // поток, который снимет его код выхода.
    std::thread::spawn(move || reap(child));
    Ok(())
}

fn detached_child(program: &str, args: &[&str]) -> Result<Child, ExecError> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ExecError::NotFound {
                    program: program.to_string(),
                }
            } else {
                ExecError::Spawn {
                    program: program.to_string(),
                    error,
                }
            }
        })
}

/// Снять завершившегося ребёнка: без `wait` запись в `/proc` живёт зомби.
fn reap(mut child: Child) {
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_TIMEOUT: Duration = Duration::from_secs(2);

    #[test]
    fn a_missing_utility_is_not_reported_as_a_failed_run() {
        let error = run_with_timeout("hud-bar-nonexistent", &[], TEST_TIMEOUT)
            .expect_err("утилиты нет — это ошибка");
        assert!(
            matches!(error, ExecError::NotFound { .. }),
            "не та ошибка: {error:?}"
        );
        assert!(error.to_string().contains("не найден"), "{error}");
    }

    #[test]
    fn a_non_zero_code_names_the_program_and_the_code() {
        let error = run_with_timeout("false", &[], TEST_TIMEOUT).expect_err("код 1 — ошибка");
        assert!(matches!(error, ExecError::Exited { .. }), "{error:?}");
        let text = error.to_string();
        assert!(text.contains("false"), "{text}");
        assert!(text.contains("кодом 1"), "{text}");
    }

    #[test]
    fn a_hung_process_is_killed_at_the_limit() {
        let started = Instant::now();
        let error = run_with_timeout("sh", &["-c", "sleep 5"], Duration::from_millis(50))
            .expect_err("sleep должен быть прерван");
        assert!(matches!(error, ExecError::Timeout { .. }), "{error:?}");
        assert!(error.to_string().contains("50 мс"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "процесс не завершён вовремя"
        );
    }

    #[test]
    fn a_successful_command_returns_its_output() {
        let output = run_with_timeout("printf", &["ok"], TEST_TIMEOUT).expect("printf есть");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "ok");
    }

    #[test]
    fn stdin_reaches_the_command_without_a_shell() {
        let output = run_with_timeout_stdin("cat", &[], Some(b"quiet secret"), TEST_TIMEOUT)
            .expect("cat есть");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "quiet secret");
    }

    /// Зомби — уже не работа: запись в `/proc` живёт, пока процесс не пожнут.
    #[test]
    fn a_detached_program_is_reaped_instead_of_staying_a_zombie() {
        let child = detached_child("true", &[]).expect("true есть");
        let pid = child.id();
        std::thread::spawn(move || reap(child));
        let deadline = Instant::now() + TEST_TIMEOUT;
        // Пока ребёнок не пожат, `/proc/<pid>` существует — даже мёртвым.
        while std::path::Path::new(&format!("/proc/{pid}")).exists() {
            assert!(Instant::now() < deadline, "true остался зомби");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

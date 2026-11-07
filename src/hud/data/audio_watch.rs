//! События звука: `pactl subscribe` вместо ожидания секундного опроса.
//!
//! Общий опрос состояния читает громкость раз в секунду, поэтому оверлей
//! появлялся бы на секунду позже нажатия и шёл ступеньками. `pactl subscribe`
//! присылает событие сразу, а значение добирается тем же `wpctl get-volume`,
//! которым пользуется опрос, — цифры не расходятся.
//!
//! Поток переподключается сам: демон PipeWire умеет перезапускаться, и после
//! обрыва чтения поток просто начинает слушать заново.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::oneshot::wpctl_volume;
use super::{Shared, Sys};

/// Событие, после которого громкость имеет смысл перечитать.
///
/// Строки про клиентов и карты игнорируются: они не меняют уровень, а на каждое
/// такое событие платили бы двумя запусками `wpctl`. Появление и исчезновение
/// узлов учитываются — смена устройства по умолчанию меняет и громкость.
fn is_audio_event(line: &str) -> bool {
    if !(line.contains(" on sink #") || line.contains(" on source #")) {
        return false;
    }
    line.contains("'change'") || line.contains("'new'") || line.contains("'remove'")
}

/// Переносит прочитанные значения в состояние. `None` означает, что узел не
/// ответил: тогда поле не трогаем, иначе неудачный запуск `wpctl` обнулил бы
/// громкость на панели и в оверлее.
///
/// Возвращает `true`, если состояние изменилось и пора перерисовать панель.
pub(super) fn apply(sys: &mut Sys, sink: Option<(u8, bool)>, source: Option<(u8, bool)>) -> bool {
    let mut changed = false;
    if let Some((value, muted)) = sink {
        sys.vol_known = true;
        if sys.vol != value || sys.vol_muted != muted {
            sys.vol = value;
            sys.vol_muted = muted;
            changed = true;
        }
    }
    if let Some((value, muted)) = source {
        sys.mic_known = true;
        if sys.mic != value || sys.mic_muted != muted {
            sys.mic = value;
            sys.mic_muted = muted;
            changed = true;
        }
    }
    changed
}

fn refresh(shared: &Shared) {
    let sink = wpctl_volume("@DEFAULT_AUDIO_SINK@");
    let source = wpctl_volume("@DEFAULT_AUDIO_SOURCE@");
    let changed = {
        let mut g = crate::hud::lock::mutex(&shared.sys);
        apply(&mut g, sink, source)
    };
    if changed {
        shared.mark();
    }
}

pub fn spawn(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let mut retry = 1u64;
        loop {
            let started = Instant::now();
            listen(&shared);
            let (wait, next) = retry_step(started.elapsed(), retry);
            retry = next;
            std::thread::sleep(Duration::from_secs(wait));
        }
    });
}

/// Стрим, проживший дольше этого, считается рабочим: после него переподключение
/// идёт сразу, без растущей паузы.
const STABLE: Duration = Duration::from_secs(30);
/// Предел паузы между переподключениями.
const RETRY_MAX_SECS: u64 = 30;

/// Пауза перед переподключением и следующий шаг.
///
/// `pactl subscribe` обычно живёт годами. Но если `pactl` есть, а PipeWire не
/// поднят, процесс умирает сразу — без нарастающей паузы поток запускал бы его
/// каждую секунду, то есть десятки тысяч процессов в сутки на пустом месте.
fn retry_step(lived: Duration, current_secs: u64) -> (u64, u64) {
    if lived >= STABLE {
        (1, 1)
    } else {
        let wait = current_secs.min(RETRY_MAX_SECS);
        (wait, (wait * 2).min(RETRY_MAX_SECS))
    }
}

/// Слушает поток событий до обрыва.
fn listen(shared: &Shared) {
    // Пока потока нет, панель опрашивает `wpctl` запасным путём.
    shared.audio_watch_live.store(false, Ordering::Relaxed);
    let Ok(mut child) = Command::new("pactl")
        .arg("subscribe")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    };
    shared.audio_watch_live.store(true, Ordering::Relaxed);
    // События приходят только на изменения. Начальное значение читаем один
    // раз, иначе до первого события панель показывала бы нулевую громкость.
    refresh(shared);
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if is_audio_event(&line) {
            refresh(shared);
        }
    }
    shared.audio_watch_live.store(false, Ordering::Relaxed);
    // `kill` без `wait` оставил бы зомби в долгоживущей панели.
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_sink_and_source_changes() {
        assert!(is_audio_event("Event 'change' on sink #12"));
        assert!(is_audio_event("Event 'change' on source #13"));
    }

    #[test]
    fn keeps_node_appearance_and_removal() {
        assert!(is_audio_event("Event 'new' on sink #14"));
        assert!(is_audio_event("Event 'remove' on source #15"));
    }

    #[test]
    fn ignores_client_and_card_events() {
        assert!(!is_audio_event("Event 'change' on client #20878"));
        assert!(!is_audio_event("Event 'new' on card #1"));
        assert!(!is_audio_event("Event 'change' on client #1"));
    }

    #[test]
    fn ignores_noise_and_empty_lines() {
        assert!(!is_audio_event(""));
        assert!(!is_audio_event("Server default sink: alsa_output.pci"));
    }

    #[test]
    fn reconnect_backs_off_while_pactl_keeps_failing() {
        let quick = Duration::from_millis(100);
        let mut retry = 1;
        let mut waits = Vec::new();
        for _ in 0..7 {
            let (wait, next) = retry_step(quick, retry);
            waits.push(wait);
            retry = next;
        }
        assert_eq!(waits, vec![1, 2, 4, 8, 16, 30, 30]);
    }

    #[test]
    fn a_stable_stream_reconnects_without_a_long_pause() {
        assert_eq!(retry_step(STABLE, RETRY_MAX_SECS), (1, 1));
        assert_eq!(retry_step(STABLE + Duration::from_secs(60), 16), (1, 1));
    }

    fn sys() -> Sys {
        Sys {
            vol: 40,
            vol_muted: false,
            mic: 70,
            mic_muted: false,
            ..Sys::default()
        }
    }

    #[test]
    fn writes_new_values_and_reports_the_change() {
        let mut state = sys();

        assert!(apply(&mut state, Some((65, false)), Some((70, false))));
        assert_eq!(state.vol, 65);
    }

    #[test]
    fn a_mute_toggle_counts_as_a_change() {
        let mut state = sys();

        assert!(apply(&mut state, Some((40, true)), Some((70, false))));
        assert!(state.vol_muted);
    }

    #[test]
    fn identical_values_report_no_change() {
        let mut state = sys();

        assert!(!apply(&mut state, Some((40, false)), Some((70, false))));
    }

    #[test]
    fn a_missing_node_leaves_the_previous_value_alone() {
        let mut state = sys();

        assert!(!apply(&mut state, None, None));
        assert_eq!(state.vol, 40);
        assert_eq!(state.mic, 70);
    }

    #[test]
    fn sink_and_source_move_independently() {
        let mut state = sys();

        assert!(apply(&mut state, Some((40, false)), Some((12, true))));
        assert_eq!(state.vol, 40);
        assert_eq!(state.mic, 12);
        assert!(state.mic_muted);
    }
}

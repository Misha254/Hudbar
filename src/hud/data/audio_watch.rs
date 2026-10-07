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
use std::time::Duration;

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
fn apply(sys: &mut Sys, sink: Option<(u8, bool)>, source: Option<(u8, bool)>) -> bool {
    let mut changed = false;
    if let Some((value, muted)) = sink
        && (sys.vol != value || sys.vol_muted != muted)
    {
        sys.vol = value;
        sys.vol_muted = muted;
        changed = true;
    }
    if let Some((value, muted)) = source
        && (sys.mic != value || sys.mic_muted != muted)
    {
        sys.mic = value;
        sys.mic_muted = muted;
        changed = true;
    }
    changed
}

fn refresh(shared: &Shared) {
    let sink = wpctl_volume("@DEFAULT_AUDIO_SINK@");
    let source = wpctl_volume("@DEFAULT_AUDIO_SOURCE@");
    let changed = {
        let mut g = shared.sys.lock().unwrap();
        apply(&mut g, sink, source)
    };
    if changed {
        shared.mark();
    }
}

pub fn spawn(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        while listen(&shared) {
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Слушает поток событий до обрыва. `true` — пора переподключиться.
fn listen(shared: &Shared) -> bool {
    let Ok(mut child) = Command::new("pactl")
        .arg("subscribe")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return true;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        return true;
    };
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if is_audio_event(&line) {
            refresh(shared);
        }
    }
    let _ = child.kill();
    true
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

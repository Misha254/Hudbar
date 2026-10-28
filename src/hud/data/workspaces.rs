use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Shared, Ws};

#[derive(Default)]
struct RefreshState {
    running: bool,
    pending: bool,
}

/// Один фоновый запрос списка столов на всех: пока он идёт, события только
/// взводят `pending`, а не запускают новый `niri msg`.
static REFRESH: Mutex<RefreshState> = Mutex::new(RefreshState {
    running: false,
    pending: false,
});
/// Поколение состояния столов: событие и ответ CLI применяются только к тому
/// поколению, которое было до запроса. Иначе ответ, пришедший после
/// `WorkspaceActivated`, перезаписал бы более свежие флаги старыми.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn note_change() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

pub(super) fn parse_ws(w: &serde_json::Value) -> Option<Ws> {
    Some(Ws {
        id: w.get("id")?.as_u64()?,
        idx: w.get("idx").and_then(|i| i.as_i64()).unwrap_or(0),
        name: w.get("name").and_then(|n| n.as_str()).map(str::to_string),
        output: w.get("output").and_then(|o| o.as_str()).map(str::to_string),
        active: w
            .get("is_active")
            .and_then(|b| b.as_bool())
            .unwrap_or(false),
        focused: w
            .get("is_focused")
            .and_then(|b| b.as_bool())
            .unwrap_or(false),
        urgent: w
            .get("is_urgent")
            .and_then(|b| b.as_bool())
            .unwrap_or(false),
        has_windows: w.get("active_window_id").and_then(|v| v.as_u64()).is_some(),
    })
}

/// Полная синхронизация столов через CLI: niri не шлёт WorkspacesChanged
/// при открытии/закрытии окон, только WindowOpenedOrChanged/WindowClosed.
///
/// Пока запрос в полёте, новые события не плодят процессы: они только
/// взводят `pending`, и после ответа цикл делает ещё один запрос. Ответ
/// применяется, только если за время запроса состояние не обновило событие.
pub fn refresh_workspaces(shared: &Arc<Shared>) {
    {
        let mut state = crate::hud::lock::mutex(&REFRESH);
        state.pending = true;
        if state.running {
            return;
        }
        state.running = true;
    }
    let s = shared.clone();
    std::thread::spawn(move || {
        loop {
            {
                let mut state = crate::hud::lock::mutex(&REFRESH);
                state.pending = false;
            }
            let generation = GENERATION.load(Ordering::SeqCst);
            let Some(out) = crate::hud::log::output("niri", &["msg", "-j", "workspaces"]) else {
                let again = {
                    let mut state = crate::hud::lock::mutex(&REFRESH);
                    if state.pending {
                        true
                    } else {
                        state.running = false;
                        false
                    }
                };
                if !again {
                    break;
                }
                continue;
            };
            let list: Vec<serde_json::Value> =
                serde_json::from_slice(&out.stdout).unwrap_or_default();
            let mut ws: Vec<Ws> = list.iter().filter_map(parse_ws).collect();
            ws.sort_by_key(|w| w.idx);
            let changed = {
                let mut g = crate::hud::lock::mutex(&s.sys);
                // Сверка поколения под тем же замком, что и события: иначе
                // `WorkspaceActivated` мог бы применить свежие флаги между
                // проверкой и записью ответа.
                if GENERATION.load(Ordering::SeqCst) != generation {
                    None
                } else {
                    let changed = g.workspaces != ws;
                    if changed {
                        g.workspaces = ws;
                    }
                    note_change();
                    Some(changed)
                }
            };
            if changed == Some(true) {
                s.mark();
            }
            let again = {
                let mut state = crate::hud::lock::mutex(&REFRESH);
                if state.pending {
                    true
                } else {
                    state.running = false;
                    false
                }
            };
            if !again {
                break;
            }
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Activated {
    Changed,
    Unchanged,
    Missing,
}

/// Применяет `WorkspaceActivated`, у которого есть только `id` и `focused`.
///
/// `focused` — глобальный флаг: он снимается у всех остальных столов. `active`
/// принадлежит выходу: при активации гасятся только столы того же `output`,
/// поэтому стол на втором мониторе не теряет `active`, когда фокус уходит на
/// первый. Если стола ещё нет в списке, флаги не выдумываются — нужен полный
/// запрос.
fn apply_activated(list: &mut [Ws], id: u64, focused: bool) -> Activated {
    let Some(target) = list.iter().position(|w| w.id == id) else {
        return Activated::Missing;
    };
    let output = list[target].output.clone();
    let mut changed = false;
    for (index, w) in list.iter_mut().enumerate() {
        if index == target {
            if !w.active || (focused && !w.focused) {
                w.active = true;
                if focused {
                    w.focused = true;
                }
                changed = true;
            }
        } else {
            if focused && w.focused {
                w.focused = false;
                changed = true;
            }
            if w.output == output && w.active {
                w.active = false;
                changed = true;
            }
        }
    }
    if changed {
        Activated::Changed
    } else {
        Activated::Unchanged
    }
}

fn handle_niri(shared: &Arc<Shared>, v: &serde_json::Value) {
    if let Some(list) = v
        .get("WorkspacesChanged")
        .and_then(|w| w.get("workspaces"))
        .and_then(|w| w.as_array())
    {
        let mut ws: Vec<Ws> = list.iter().filter_map(parse_ws).collect();
        ws.sort_by_key(|w| w.idx);
        let mut g = crate::hud::lock::mutex(&shared.sys);
        if g.workspaces != ws {
            g.workspaces = ws;
            note_change();
            drop(g);
            shared.mark();
        }
        return;
    }
    // Открытие/закрытие окон WorkspacesChanged не триггерит — дотягиваем полный список.
    if v.get("WindowOpenedOrChanged").is_some()
        || v.get("WindowClosed").is_some()
        || v.get("WorkspaceActiveWindowChanged").is_some()
    {
        refresh_workspaces(shared);
        return;
    }
    if let Some(ev) = v.get("WorkspaceActivated") {
        let Some(id) = ev.get("id").and_then(|i| i.as_u64()) else {
            return;
        };
        let focused = ev.get("focused").and_then(|f| f.as_bool()).unwrap_or(false);
        let activated = {
            let mut g = crate::hud::lock::mutex(&shared.sys);
            let activated = apply_activated(&mut g.workspaces, id, focused);
            if activated == Activated::Changed {
                note_change();
            }
            activated
        };
        match activated {
            Activated::Missing => refresh_workspaces(shared),
            Activated::Changed => shared.mark(),
            Activated::Unchanged => {}
        }
        return;
    }
    if let Some(ev) = v.get("WorkspaceUrgencyChanged") {
        let Some(id) = ev.get("id").and_then(|i| i.as_u64()) else {
            return;
        };
        let urgent = ev.get("urgent").and_then(|u| u.as_bool()).unwrap_or(false);
        let mut g = crate::hud::lock::mutex(&shared.sys);
        let mut changed = false;
        for w in g.workspaces.iter_mut() {
            if w.id == id && w.urgent != urgent {
                w.urgent = urgent;
                changed = true;
            }
        }
        if changed {
            note_change();
        }
        drop(g);
        if changed {
            shared.mark();
        }
    }
}

pub(super) fn niri_loop(shared: Arc<Shared>) {
    loop {
        if let Ok(path) = std::env::var("NIRI_SOCKET")
            && let Ok(mut stream) = UnixStream::connect(&path)
            && stream.write_all(b"\"EventStream\"\n").is_ok()
        {
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                let Ok(line) = line else {
                    break;
                };
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    handle_niri(&shared, &v);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: u64, output: &str, active: bool, focused: bool) -> Ws {
        Ws {
            id,
            idx: id as i64,
            name: None,
            output: Some(output.to_string()),
            active,
            focused,
            urgent: false,
            has_windows: false,
        }
    }

    #[test]
    fn a_workspace_on_another_output_becomes_active_without_focus() {
        let mut list = vec![ws(1, "eDP-1", true, true), ws(2, "HDMI-A-1", false, false)];

        assert_eq!(apply_activated(&mut list, 2, false), Activated::Changed);
        assert!(list[1].active, "стол второго выхода стал активным");
        assert!(!list[1].focused, "без фокуса флаг не выдумывается");
        assert!(list[0].active, "стол первого выхода остался активным");
        assert!(list[0].focused, "чужой фокус не снят");
    }

    #[test]
    fn moving_focus_to_the_first_output_keeps_the_second_active() {
        let mut list = vec![ws(1, "eDP-1", true, false), ws(2, "HDMI-A-1", true, false)];

        assert_eq!(apply_activated(&mut list, 1, true), Activated::Changed);
        assert!(list[0].active && list[0].focused, "фокус на первом выходе");
        assert!(list[1].active, "active второго выхода не гасится");
        assert!(!list[1].focused, "фокус уникален");
    }

    #[test]
    fn an_unknown_workspace_asks_for_a_full_refresh_instead_of_flags() {
        let mut list = vec![ws(1, "eDP-1", true, true)];

        assert_eq!(apply_activated(&mut list, 9, true), Activated::Missing);
        assert!(list[0].active && list[0].focused, "чужие флаги не тронуты");
    }

    #[test]
    fn parsing_keeps_the_output_name() {
        let value = serde_json::json!({
            "id": 4,
            "idx": 2,
            "name": null,
            "output": "HDMI-A-1",
            "is_active": true,
            "is_focused": false,
        });

        assert_eq!(
            parse_ws(&value).and_then(|w| w.output).as_deref(),
            Some("HDMI-A-1")
        );
    }
}

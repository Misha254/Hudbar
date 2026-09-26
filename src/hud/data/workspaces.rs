use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::Duration;

use super::{Shared, Ws};

pub(super) fn parse_ws(w: &serde_json::Value) -> Option<Ws> {
    Some(Ws {
        id: w.get("id")?.as_u64()?,
        idx: w.get("idx").and_then(|i| i.as_i64()).unwrap_or(0),
        name: w.get("name").and_then(|n| n.as_str()).map(str::to_string),
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
pub fn refresh_workspaces(shared: &Arc<Shared>) {
    let s = shared.clone();
    std::thread::spawn(move || {
        let out = std::process::Command::new("niri")
            .args(["msg", "-j", "workspaces"])
            .output();
        let text = out.map(|o| o.stdout).unwrap_or_default();
        let list: Vec<serde_json::Value> = serde_json::from_slice(&text).unwrap_or_default();
        let mut ws: Vec<Ws> = list.iter().filter_map(parse_ws).collect();
        ws.sort_by_key(|w| w.idx);
        let changed = {
            let mut g = s.sys.lock().unwrap();
            if g.workspaces != ws {
                g.workspaces = ws;
                true
            } else {
                false
            }
        };
        if changed {
            s.mark();
        }
    });
}

fn handle_niri(shared: &Arc<Shared>, v: &serde_json::Value) {
    if let Some(list) = v
        .get("WorkspacesChanged")
        .and_then(|w| w.get("workspaces"))
        .and_then(|w| w.as_array())
    {
        let mut ws: Vec<Ws> = list.iter().filter_map(parse_ws).collect();
        ws.sort_by_key(|w| w.idx);
        let mut g = shared.sys.lock().unwrap();
        if g.workspaces != ws {
            g.workspaces = ws;
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
        let mut g = shared.sys.lock().unwrap();
        let mut changed = false;
        for w in g.workspaces.iter_mut() {
            if w.id == id {
                if w.active != focused || w.focused != focused {
                    w.active = focused;
                    w.focused = focused;
                    changed = true;
                }
            } else if focused && (w.focused || w.active) {
                w.focused = false;
                w.active = false;
                changed = true;
            }
        }
        drop(g);
        if changed {
            shared.mark();
        }
        return;
    }
    if let Some(ev) = v.get("WorkspaceUrgencyChanged") {
        let Some(id) = ev.get("id").and_then(|i| i.as_u64()) else {
            return;
        };
        let urgent = ev.get("urgent").and_then(|u| u.as_bool()).unwrap_or(false);
        let mut g = shared.sys.lock().unwrap();
        let mut changed = false;
        for w in g.workspaces.iter_mut() {
            if w.id == id && w.urgent != urgent {
                w.urgent = urgent;
                changed = true;
            }
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

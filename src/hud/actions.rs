use std::sync::Arc;
use std::time::Duration;

use super::data::{self, Shared};

fn norm_app_key(s: &str) -> String {
    let mut s = s.to_lowercase().replace('_', "-");
    for p in ["org.", "com.", "io.", "net.", "app."] {
        if let Some(r) = s.strip_prefix(p) {
            s = r.to_string();
            break;
        }
    }
    for suf in [".desktop", "-desktop"] {
        if let Some(r) = s.strip_suffix(suf) {
            s = r.to_string();
            break;
        }
    }
    s
}

fn app_key_score(key: &str, app: &str) -> u8 {
    if key.len() < 2 || app.is_empty() || key == "desktop" || key == "app" {
        return 0;
    }
    if key == app {
        3
    } else if key.len() >= 3 && app.contains(key) {
        2
    } else if app.len() >= 3 && key.contains(app) {
        1
    } else {
        0
    }
}

/// Ищет окно приложения по ключам трея и фокусит его.
pub fn focus_tray_window(keys: &[String]) -> bool {
    let keys: Vec<String> = keys
        .iter()
        .map(|k| norm_app_key(k))
        .filter(|k| k.len() >= 2)
        .collect();
    if keys.is_empty() {
        return false;
    }
    let Some(out) = crate::hud::log::output("niri", &["msg", "-j", "windows"]) else {
        return false;
    };
    let wins: Vec<serde_json::Value> = match serde_json::from_slice(&out.stdout) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut best: Option<(u64, u8, u64)> = None;
    for w in &wins {
        let app = w.get("app_id").and_then(|v| v.as_str()).unwrap_or("");
        let na = norm_app_key(app);
        let score = keys
            .iter()
            .map(|k| app_key_score(k, &na))
            .max()
            .unwrap_or(0);
        if score == 0 {
            continue;
        }
        let Some(id) = w.get("id").and_then(|v| v.as_u64()) else {
            continue;
        };
        let ts = w
            .get("focus_timestamp")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let better = match best {
            None => true,
            Some((_, bs, bts)) => score > bs || (score == bs && ts > bts),
        };
        if better {
            best = Some((id, score, ts));
        }
    }
    let Some((id, _, _)) = best else {
        return false;
    };
    let id = id.to_string();
    crate::hud::log::status("niri", &["msg", "action", "focus-window", "--id", &id])
}

pub fn focus_workspace(reference: &str) {
    let reference = reference.to_string();
    std::thread::spawn(move || {
        let _ = crate::hud::log::status("niri", &["msg", "action", "focus-workspace", &reference]);
    });
}

pub fn vol_set(shared: &Arc<Shared>, pct: u8) {
    {
        let mut g = crate::hud::lock::mutex(&shared.sys);
        g.vol = pct.min(100);
        g.vol_muted = false;
    }
    shared.mark();
    let s = shared.clone();
    std::thread::spawn(move || {
        let volume = format!("{pct}%");
        let _ = crate::hud::log::status("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &volume]);
        std::thread::sleep(Duration::from_millis(150));
        if let Some((v, m)) = data::wpctl_volume("@DEFAULT_AUDIO_SINK@") {
            let mut g = crate::hud::lock::mutex(&s.sys);
            g.vol = v;
            g.vol_muted = m;
        }
        s.mark();
    });
}

pub fn vol_mute_toggle(shared: &Arc<Shared>) {
    let muted = !crate::hud::lock::mutex(&shared.sys).vol_muted;
    {
        let mut g = crate::hud::lock::mutex(&shared.sys);
        g.vol_muted = muted;
    }
    shared.mark();
    let s = shared.clone();
    std::thread::spawn(move || {
        let _ = crate::hud::log::status(
            "wpctl",
            &[
                "set-mute",
                "@DEFAULT_AUDIO_SINK@",
                if muted { "1" } else { "0" },
            ],
        );
        std::thread::sleep(Duration::from_millis(150));
        if let Some((v, m)) = data::wpctl_volume("@DEFAULT_AUDIO_SINK@") {
            let mut g = crate::hud::lock::mutex(&s.sys);
            g.vol = v;
            g.vol_muted = m;
        }
        s.mark();
    });
}

pub fn mic_set(shared: &Arc<Shared>, pct: u8) {
    {
        let mut g = crate::hud::lock::mutex(&shared.sys);
        g.mic = pct.min(100);
        g.mic_muted = false;
    }
    shared.mark();
    let s = shared.clone();
    std::thread::spawn(move || {
        let volume = format!("{pct}%");
        let _ =
            crate::hud::log::status("wpctl", &["set-volume", "@DEFAULT_AUDIO_SOURCE@", &volume]);
        std::thread::sleep(Duration::from_millis(150));
        if let Some((v, m)) = data::wpctl_volume("@DEFAULT_AUDIO_SOURCE@") {
            let mut g = crate::hud::lock::mutex(&s.sys);
            g.mic = v;
            g.mic_muted = m;
        }
        s.mark();
    });
}

pub fn mic_mute_toggle(shared: &Arc<Shared>) {
    let muted = !crate::hud::lock::mutex(&shared.sys).mic_muted;
    {
        let mut g = crate::hud::lock::mutex(&shared.sys);
        g.mic_muted = muted;
    }
    shared.mark();
    let s = shared.clone();
    std::thread::spawn(move || {
        let _ = crate::hud::log::status(
            "wpctl",
            &[
                "set-mute",
                "@DEFAULT_AUDIO_SOURCE@",
                if muted { "1" } else { "0" },
            ],
        );
        std::thread::sleep(Duration::from_millis(150));
        if let Some((v, m)) = data::wpctl_volume("@DEFAULT_AUDIO_SOURCE@") {
            let mut g = crate::hud::lock::mutex(&s.sys);
            g.mic = v;
            g.mic_muted = m;
        }
        s.mark();
    });
}

pub fn dnd_toggle() {
    std::thread::spawn(|| {
        let paused = crate::hud::log::output("dunstctl", &["is-paused"])
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .is_some_and(|value| value.trim() == "true");
        let target = if paused { "false" } else { "true" };
        let _ = crate::hud::log::status("dunstctl", &["set-paused", target]);
    });
}

pub fn spawn(command: &str) {
    let command = command.to_string();
    std::thread::spawn(move || {
        let _ = std::process::Command::new(command).spawn();
    });
}

pub fn bt_toggle(shared: &Arc<Shared>, mac: &str, connected: bool) {
    let s = shared.clone();
    let mac = mac.to_string();
    std::thread::spawn(move || {
        if connected {
            let _ = crate::hud::log::status("bluetoothctl", &["disconnect", &mac]);
        } else {
            let _ = crate::hud::log::status("bluetoothctl", &["pair", &mac]);
            let _ = crate::hud::log::status("bluetoothctl", &["connect", &mac]);
        }
        std::thread::sleep(Duration::from_millis(900));
        data::set_bt_list(&s, data::bt_now());
    });
}

#[cfg(test)]
mod tests {
    use super::{app_key_score, norm_app_key};

    #[test]
    fn normalizes_desktop_app_keys() {
        assert_eq!(norm_app_key("org.telegram.desktop"), "telegram");
        assert_eq!(norm_app_key("My_App-desktop"), "my-app");
    }

    #[test]
    fn prefers_exact_app_key_matches() {
        assert_eq!(app_key_score("telegram", "telegram"), 3);
        assert_eq!(app_key_score("tele", "telegram"), 2);
        assert_eq!(app_key_score("app", "telegram"), 0);
    }
}

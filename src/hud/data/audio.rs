use std::sync::Arc;
use std::time::Duration;

use super::{AudioDev, Shared};

fn short_audio_label(desc: &str) -> String {
    let s = desc.trim();
    if let Some(pos) = s.find("HD Audio ") {
        return s[pos + "HD Audio ".len()..].trim().to_string();
    }
    if let Some(pos) = s.find("High Definition Audio Controller ") {
        return s[pos + "High Definition Audio Controller ".len()..]
            .trim()
            .to_string();
    }
    if let Some(rest) = s.strip_prefix("Monitor of ") {
        return short_audio_label(rest);
    }
    s.to_string()
}
pub(crate) fn wpctl_volume(node: &str) -> Option<(u8, bool)> {
    let out = crate::hud::log::output("wpctl", &["get-volume", node])?;
    parse_wpctl_volume(&String::from_utf8_lossy(&out.stdout))
}

pub(super) fn parse_wpctl_volume(text: &str) -> Option<(u8, bool)> {
    let val = text
        .split_whitespace()
        .find_map(|part| part.parse::<f32>().ok())?;
    Some((
        (val * 100.0).round().clamp(0.0, 100.0) as u8,
        text.split_whitespace()
            .any(|part| part.eq_ignore_ascii_case("[MUTED]")),
    ))
}
fn pactl_json_list(kind: &str) -> Option<Vec<serde_json::Value>> {
    crate::hud::log::output("pactl", &["-f", "json", "list", kind])
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| v.as_array().cloned())
}

fn default_name(cmd: &str) -> String {
    crate::hud::log::output("pactl", &[cmd])
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EeState {
    pub running: bool,
    pub follow_out: bool,
    pub follow_in: bool,
    pub out_target: String,
    pub in_target: String,
}

fn ee_state_now(sinks: &[AudioDev]) -> EeState {
    let running = sinks.iter().any(|d| d.name.starts_with("easyeffects_"));
    let mut st = EeState {
        running,
        follow_out: true,
        follow_in: true,
        ..Default::default()
    };
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        return st;
    }
    let text = std::fs::read_to_string(format!("{home}/.config/easyeffects/db/easyeffectsrc"))
        .unwrap_or_default();
    if text.is_empty() {
        return st;
    }
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = match line {
                "[StreamOutputs]" => "out",
                "[StreamInputs]" => "in",
                _ => "",
            };
            continue;
        }
        let (k, v) = match line.split_once('=') {
            Some(p) => (p.0.trim(), p.1.trim()),
            None => continue,
        };
        match (section, k) {
            ("out", "outputDevice") => st.out_target = v.to_string(),
            ("out", "useDefaultOutputDevice") => st.follow_out = v != "false",
            ("in", "inputDevice") => st.in_target = v.to_string(),
            ("in", "useDefaultInputDevice") => st.follow_in = v != "false",
            _ => {}
        }
    }
    st
}

fn audio_lists_now() -> Result<(Vec<AudioDev>, Vec<AudioDev>), String> {
    let def_sink = default_name("get-default-sink");
    let def_source = default_name("get-default-source");
    let mut sinks = Vec::new();
    for s in pactl_json_list("sinks").ok_or_else(|| "pactl не вернул список выходов".to_string())?
    {
        let id = s.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let name = s
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let desc = s
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or(&name)
            .to_string();
        if name.is_empty() {
            continue;
        }
        sinks.push(AudioDev {
            id,
            label: short_audio_label(&desc),
            is_default: !def_sink.is_empty() && name == def_sink,
            name,
        });
    }
    sinks.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.label.cmp(&b.label)));
    let mut sources = Vec::new();
    for s in
        pactl_json_list("sources").ok_or_else(|| "pactl не вернул список входов".to_string())?
    {
        let class = s
            .get("properties")
            .and_then(|p| p.get("device.class"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if class == "monitor" {
            continue;
        }
        let name = s
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.ends_with(".monitor") || name.is_empty() {
            continue;
        }
        let id = s.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let desc = s
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or(&name)
            .to_string();
        sources.push(AudioDev {
            id,
            label: short_audio_label(&desc),
            is_default: !def_source.is_empty() && name == def_source,
            name,
        });
    }
    sources.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.label.cmp(&b.label)));
    Ok((sinks, sources))
}

fn set_audio_lists(shared: &Arc<Shared>, result: Result<(Vec<AudioDev>, Vec<AudioDev>), String>) {
    let (sinks, sources, error) = match result {
        Ok((sinks, sources)) => (sinks, sources, None),
        Err(error) => (Vec::new(), Vec::new(), Some(error)),
    };
    let ee = ee_state_now(&sinks);
    let changed = {
        let mut g = shared.sys.lock().unwrap();
        g.audio_loading = false;
        let mut c = false;
        if g.sinks != sinks {
            g.sinks = sinks;
            c = true;
        }
        if g.sources != sources {
            g.sources = sources;
            c = true;
        }
        if g.ee_running != ee.running {
            g.ee_running = ee.running;
            c = true;
        }
        if g.ee_follow_out != ee.follow_out {
            g.ee_follow_out = ee.follow_out;
            c = true;
        }
        if g.ee_follow_in != ee.follow_in {
            g.ee_follow_in = ee.follow_in;
            c = true;
        }
        if g.ee_out_target != ee.out_target {
            g.ee_out_target = ee.out_target;
            c = true;
        }
        if g.ee_in_target != ee.in_target {
            g.ee_in_target = ee.in_target;
            c = true;
        }
        if g.audio_error != error {
            g.audio_error = error;
            c = true;
        }
        c
    };
    if changed {
        shared.mark();
    }
}

pub fn refresh_audio(shared: &Arc<Shared>) {
    {
        let mut g = shared.sys.lock().unwrap();
        g.audio_loading = true;
    }
    shared.mark();
    let s = shared.clone();
    std::thread::spawn(move || {
        set_audio_lists(&s, audio_lists_now());
    });
}

pub fn audio_set_default(shared: &Arc<Shared>, node: &str, is_sink: bool) {
    {
        let mut g = shared.sys.lock().unwrap();
        g.audio_loading = true;
        let list = if is_sink {
            &mut g.sinks
        } else {
            &mut g.sources
        };
        for d in list.iter_mut() {
            d.is_default = d.name == node;
        }
    }
    shared.mark();
    let node = node.to_string();
    let s = shared.clone();
    std::thread::spawn(move || {
        // wpctl set-default ест только числовые PipeWire-ID, поэтому через pactl по имени.
        // EE в follow-режиме подхватывает новый дефолт сам, эквалайзер не рвётся.
        let cmd = if is_sink {
            "set-default-sink"
        } else {
            "set-default-source"
        };
        let _ = crate::hud::log::status("pactl", &[cmd, &node]);
        std::thread::sleep(Duration::from_millis(800));
        set_audio_lists(&s, audio_lists_now());
    });
}

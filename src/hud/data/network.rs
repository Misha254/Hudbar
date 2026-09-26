use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::system::{read_trim, rfkill_state};
use super::{BtDev, Shared, WifiNet};

pub(super) fn wifi() -> (bool, u8, Option<String>) {
    let mut iface = None;
    if let Ok(rd) = fs::read_dir("/sys/class/net") {
        for e in rd.flatten() {
            if e.path().join("wireless").exists() {
                iface = Some(e.file_name().to_string_lossy().to_string());
                break;
            }
        }
    }
    let Some(iface) = iface else {
        return (false, 0, None);
    };
    let up = read_trim(
        &PathBuf::from("/sys/class/net")
            .join(&iface)
            .join("operstate"),
    ) == "up";
    let mut signal = 0u8;
    if let Ok(s) = fs::read_to_string("/proc/net/wireless") {
        for line in s.lines().skip(2) {
            let mut parts = line.split(':');
            if let (Some(name), Some(rest)) = (parts.next(), parts.next())
                && name.trim() == iface
                && let Some(q) = rest.split_whitespace().nth(1)
            {
                let q: f32 = q.trim_end_matches('.').parse().unwrap_or(0.0);
                signal = ((q / 70.0) * 100.0).clamp(0.0, 100.0) as u8;
            }
        }
    }
    (up, signal, Some(iface))
}

pub(super) fn wifi_radio_on(iface: &Option<String>) -> bool {
    rfkill_state("wlan").unwrap_or_else(|| iface.is_some())
}
fn split_nmcli_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::with_capacity(4);
    let mut field = String::new();
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            field.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == ':' && fields.len() < 3 {
            fields.push(std::mem::take(&mut field));
        } else {
            field.push(ch);
        }
    }
    if escaped {
        field.push('\\');
    }
    fields.push(field);
    fields
}

pub(super) fn parse_wifi_list(text: &str) -> Vec<WifiNet> {
    let mut list = text
        .lines()
        .filter_map(|line| {
            let fields = split_nmcli_fields(line);
            if fields.len() < 4 || fields[1].is_empty() {
                return None;
            }
            Some(WifiNet {
                in_use: fields[0].trim() == "*",
                ssid: fields[1].to_string(),
                signal: fields[2].trim().parse::<u16>().unwrap_or(0).min(100) as u8,
                security: fields[3].trim().to_string(),
            })
        })
        .collect::<Vec<_>>();

    list.sort_by(|a, b| {
        b.in_use
            .cmp(&a.in_use)
            .then(b.signal.cmp(&a.signal))
            .then(a.ssid.cmp(&b.ssid))
    });
    let mut seen = HashSet::new();
    list.retain(|network| seen.insert(network.ssid.clone()));
    list
}

fn wifi_now() -> Result<Vec<WifiNet>, String> {
    let args = [
        "-t",
        "-f",
        "IN-USE,SSID,SIGNAL,SECURITY",
        "dev",
        "wifi",
        "list",
        "--rescan",
        "no",
    ];
    let text = crate::hud::log::output("nmcli", &args)
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .or_else(|| {
            crate::hud::log::output(
                "nmcli",
                &[
                    "-t",
                    "-f",
                    "IN-USE,SSID,SIGNAL,SECURITY",
                    "dev",
                    "wifi",
                    "list",
                ],
            )
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        })
        .ok_or_else(|| "nmcli недоступен".to_string())?;
    Ok(parse_wifi_list(&text))
}

fn set_wifi_list(shared: &Arc<Shared>, result: Result<Vec<WifiNet>, String>) {
    let (list, error) = match result {
        Ok(list) => (list, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    let changed = {
        let mut g = shared.sys.lock().unwrap();
        g.wifi_loading = false;
        let mut changed = false;
        if g.wifi_list != list {
            g.wifi_list = list;
            changed = true;
        }
        if g.wifi_error != error {
            g.wifi_error = error;
            changed = true;
        }
        changed
    };
    if changed {
        shared.mark();
    }
}

pub(super) fn parse_bt_devices(all: &str, connected: &str) -> Vec<BtDev> {
    let connected: HashSet<&str> = connected
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .collect();
    let mut seen = HashSet::new();
    let mut list = Vec::new();
    for line in all.lines() {
        let mut p = line.split_whitespace();
        if p.next() != Some("Device") {
            continue;
        }
        let Some(mac) = p.next() else { continue };
        if !seen.insert(mac.to_string()) {
            continue;
        }
        let name = p.collect::<Vec<_>>().join(" ");
        list.push(BtDev {
            mac: mac.to_string(),
            name,
            connected: connected.contains(mac),
        });
    }
    list.sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.cmp(&b.name)));
    list
}

pub(crate) fn bt_now() -> Result<Vec<BtDev>, String> {
    let all = crate::hud::log::output("bluetoothctl", &["devices"])
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .ok_or_else(|| "bluetoothctl недоступен".to_string())?;
    let connected = crate::hud::log::output("bluetoothctl", &["devices", "Connected"])
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .ok_or_else(|| "bluetoothctl не отвечает".to_string())?;
    Ok(parse_bt_devices(&all, &connected))
}

pub(crate) fn set_bt_list(shared: &Arc<Shared>, result: Result<Vec<BtDev>, String>) {
    let (list, error) = match result {
        Ok(list) => (list, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    let changed = {
        let mut g = shared.sys.lock().unwrap();
        g.bt_loading = false;
        let mut changed = false;
        if g.bt_list != list {
            g.bt_list = list;
            changed = true;
        }
        if g.bt_error != error {
            g.bt_error = error;
            changed = true;
        }
        changed
    };
    if changed {
        shared.mark();
    }
}

pub fn refresh_wifi(shared: &Arc<Shared>) {
    {
        let mut g = shared.sys.lock().unwrap();
        g.wifi_loading = true;
    }
    shared.mark();
    let s = shared.clone();
    std::thread::spawn(move || set_wifi_list(&s, wifi_now()));
}

pub fn refresh_bt(shared: &Arc<Shared>) {
    {
        let mut g = shared.sys.lock().unwrap();
        g.bt_loading = true;
    }
    shared.mark();
    let s = shared.clone();
    std::thread::spawn(move || set_bt_list(&s, bt_now()));
}
pub fn wifi_connect(shared: &Arc<Shared>, ssid: &str, secured: bool) {
    let s = shared.clone();
    let ssid = ssid.to_string();
    std::thread::spawn(move || {
        if secured {
            let out = std::process::Command::new("zenity")
                .args(["--password", "--title", &format!("Wi-Fi: {ssid}")])
                .output();
            let pass = match out {
                Ok(o) if o.status.success() => {
                    String::from_utf8_lossy(&o.stdout).trim().to_string()
                }
                _ => return,
            };
            let _ = crate::hud::log::status(
                "nmcli",
                &["dev", "wifi", "connect", &ssid, "password", &pass],
            );
        } else {
            let _ = crate::hud::log::status("nmcli", &["dev", "wifi", "connect", &ssid]);
        }
        std::thread::sleep(Duration::from_millis(900));
        set_wifi_list(&s, wifi_now());
    });
}

pub fn wifi_connect_pass(shared: &Arc<Shared>, ssid: &str, pass: &str) {
    let s = shared.clone();
    let ssid = ssid.to_string();
    let pass = pass.to_string();
    std::thread::spawn(move || {
        let _ = crate::hud::log::status(
            "nmcli",
            &["dev", "wifi", "connect", &ssid, "password", &pass],
        );
        std::thread::sleep(Duration::from_millis(900));
        set_wifi_list(&s, wifi_now());
    });
}

pub fn wifi_disconnect(shared: &Arc<Shared>) {
    let s = shared.clone();
    std::thread::spawn(move || {
        let iface = { s.sys.lock().unwrap().wifi_iface.clone() };
        if let Some(iface) = iface {
            let _ = crate::hud::log::status("nmcli", &["dev", "disconnect", &iface]);
        }
        std::thread::sleep(Duration::from_millis(700));
        set_wifi_list(&s, wifi_now());
    });
}

pub fn wifi_radio(shared: &Arc<Shared>, on: bool) {
    {
        let mut g = shared.sys.lock().unwrap();
        g.wifi_loading = true;
    }
    shared.mark();
    let s = shared.clone();
    std::thread::spawn(move || {
        let _ = crate::hud::log::status("nmcli", &["radio", "wifi", if on { "on" } else { "off" }]);
        std::thread::sleep(Duration::from_millis(600));
        set_wifi_list(&s, wifi_now());
    });
}

pub fn bt_power(shared: &Arc<Shared>, on: bool) {
    {
        let mut g = shared.sys.lock().unwrap();
        g.bt_loading = true;
    }
    shared.mark();
    let s = shared.clone();
    std::thread::spawn(move || {
        let _ = crate::hud::log::status("bluetoothctl", &["power", if on { "on" } else { "off" }]);
        std::thread::sleep(Duration::from_millis(600));
        set_bt_list(&s, bt_now());
    });
}

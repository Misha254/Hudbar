use std::fs;
use std::path::Path;
use std::time::Duration;

use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;

pub(super) fn read_trim(p: &Path) -> String {
    fs::read_to_string(p)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub(super) fn battery() -> (Option<u8>, bool, bool) {
    let mut pct = None;
    let mut charging = false;
    let mut ac = false;
    if let Ok(rd) = fs::read_dir("/sys/class/power_supply") {
        for e in rd.flatten() {
            let p = e.path();
            match read_trim(&p.join("type")).as_str() {
                "Battery" => {
                    if pct.is_none() {
                        pct = read_trim(&p.join("capacity")).parse().ok();
                    }
                    let st = read_trim(&p.join("status"));
                    if st == "Charging" || st == "Full" {
                        charging = true;
                    }
                }
                "Mains" => {
                    ac = read_trim(&p.join("online")) == "1";
                }
                _ => {}
            }
        }
    }
    (pct, charging, ac)
}

pub(super) fn power_state() -> Option<(bool, bool)> {
    let mut found = false;
    let mut charging = false;
    let mut ac = false;
    let Ok(rd) = fs::read_dir("/sys/class/power_supply") else {
        return None;
    };

    for e in rd.flatten() {
        let p = e.path();
        match read_trim(&p.join("type")).as_str() {
            "Battery" => {
                found = true;
                let status = read_trim(&p.join("status"));
                charging |= status == "Charging";
            }
            "Mains" => {
                found = true;
                ac |= read_trim(&p.join("online")) == "1";
            }
            _ => {}
        }
    }

    found.then_some((charging, ac))
}

pub(super) fn bat_time_now() -> Option<String> {
    let rd = fs::read_dir("/sys/class/power_supply").ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if read_trim(&p.join("type")) != "Battery" {
            continue;
        }
        let st = read_trim(&p.join("status"));
        let t = bat_time(&p, &st);
        if t.is_some() {
            return t;
        }
    }
    None
}

fn charge_limit_pct(p: &Path) -> f64 {
    read_trim(&p.join("charge_control_end_threshold"))
        .parse::<f64>()
        .map(|v| v.clamp(1.0, 100.0))
        .unwrap_or(100.0)
}

fn bat_time(p: &Path, status: &str) -> Option<String> {
    if status != "Discharging" && status != "Charging" {
        return None;
    }
    let parse = |f: &str| read_trim(&p.join(f)).parse::<f64>().ok();
    let (now, full, rate) = {
        match (
            parse("energy_now"),
            parse("energy_full"),
            parse("power_now"),
        ) {
            (Some(n), Some(f), Some(r)) if r > 0.0 => (n, f, r),
            _ => (
                parse("charge_now")?,
                parse("charge_full")?,
                parse("current_now")?,
            ),
        }
    };
    if rate <= 0.0 {
        return None;
    }
    let secs = if status == "Discharging" {
        now / rate * 3600.0
    } else {
        // С лимитом заряда (например 80%) считаем до порога, а не до 100%.
        let target = full * charge_limit_pct(p) / 100.0;
        if now >= target {
            return None;
        }
        (target - now).max(0.0) / rate * 3600.0
    };
    let total_min = ((secs + 30.0) / 60.0) as u64;
    let (h, m) = (total_min / 60, total_min % 60);
    Some(if h > 0 {
        format!("{h}ч{m:02}м")
    } else {
        format!("{m}м")
    })
}

fn proc_stat_cpu() -> Option<(u64, u64)> {
    let s = fs::read_to_string("/proc/stat").ok()?;
    let nums: Vec<u64> = s
        .lines()
        .next()?
        .split_whitespace()
        .skip(1)
        .take(8)
        .filter_map(|x| x.parse().ok())
        .collect();
    if nums.len() < 5 {
        return None;
    }
    Some((nums[3] + nums[4], nums.iter().sum()))
}

pub(super) fn cpu_pct(prev: &mut Option<(u64, u64)>) -> u8 {
    let Some((idle, total)) = proc_stat_cpu() else {
        return 0;
    };
    let pct = match prev {
        Some((pi, pt)) => {
            let di = idle.saturating_sub(*pi);
            let dt = total.saturating_sub(*pt);
            if dt == 0 {
                0
            } else {
                (100.0 * (dt - di) as f64 / dt as f64)
                    .round()
                    .clamp(0.0, 100.0) as u8
            }
        }
        None => 0,
    };
    *prev = Some((idle, total));
    pct
}

pub(super) fn cpu_temp() -> Option<i32> {
    let rd = fs::read_dir("/sys/class/hwmon").ok()?;
    for e in rd.flatten() {
        if read_trim(&e.path().join("name")) == "coretemp" {
            let t = read_trim(&e.path().join("temp1_input"));
            return t.parse::<i64>().ok().map(|v| (v / 1000) as i32);
        }
    }
    None
}

pub(super) fn mem_used_gib() -> Option<f32> {
    let s = fs::read_to_string("/proc/meminfo").ok()?;
    let (mut total, mut avail) = (0f32, 0f32);
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total = v.split_whitespace().next()?.parse().ok()?;
        } else if let Some(v) = line.strip_prefix("MemAvailable:") {
            avail = v.split_whitespace().next()?.parse().ok()?;
        }
    }
    Some((total - avail) / 1024.0 / 1024.0)
}

pub(super) fn mem_pct() -> u8 {
    let Ok(s) = fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    let (mut total, mut avail) = (0f32, 0f32);
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total = v
                .split_whitespace()
                .next()
                .and_then(|x| x.parse().ok())
                .unwrap_or(0.0);
        } else if let Some(v) = line.strip_prefix("MemAvailable:") {
            avail = v
                .split_whitespace()
                .next()
                .and_then(|x| x.parse().ok())
                .unwrap_or(0.0);
        }
    }
    if total <= 0.0 {
        return 0;
    }
    (100.0 * (total - avail) / total).round().clamp(0.0, 100.0) as u8
}
pub(super) fn rfkill_state(kind: &str) -> Option<bool> {
    let rd = fs::read_dir("/sys/class/rfkill").ok()?;
    let mut found = false;
    for e in rd.flatten() {
        let p = e.path();
        if read_trim(&p.join("type")) != kind {
            continue;
        }
        found = true;
        let soft = read_trim(&p.join("soft")) == "0";
        let hard = read_trim(&p.join("hard")) == "0";
        if !(soft && hard) {
            return Some(false);
        }
    }
    found.then_some(true)
}

pub(super) fn bt_on(conn: Option<&dbus::blocking::Connection>) -> bool {
    if let Some(conn) = conn {
        let proxy = dbus::blocking::Proxy::new(
            "org.bluez",
            "/org/bluez/hci0",
            Duration::from_millis(250),
            conn,
        );
        if let Ok(powered) = proxy.get::<bool>("org.bluez.Adapter1", "Powered") {
            return powered;
        }
    }
    rfkill_state("bluetooth").unwrap_or(false)
}

pub(super) fn recorder_on() -> bool {
    let Ok(rd) = fs::read_dir("/proc") else {
        return false;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if read_trim(&e.path().join("comm")) == "wf-recorder" {
            return true;
        }
    }
    false
}

pub(super) fn webcam_active() -> bool {
    let mut devices: Vec<String> = Vec::new();
    if let Ok(rd) = fs::read_dir("/dev") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("video") {
                devices.push(format!("/dev/{n}"));
            }
        }
    }
    if devices.is_empty() {
        return false;
    }
    let Ok(rd) = fs::read_dir("/proc") else {
        return false;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(fds) = fs::read_dir(e.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(t) = fs::read_link(fd.path()) {
                let t = t.to_string_lossy();
                if devices.iter().any(|d| t == d.as_str()) {
                    return true;
                }
            }
        }
    }
    false
}

pub(super) fn dnd_paused() -> bool {
    crate::hud::log::output("dunstctl", &["is-paused"])
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

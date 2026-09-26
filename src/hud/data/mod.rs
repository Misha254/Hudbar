use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nix::fcntl::OFlag;

mod audio;
mod network;
mod system;
mod weather;
mod workspaces;

pub(crate) use audio::wpctl_volume;
pub use audio::{audio_set_default, refresh_audio};
pub(crate) use network::{bt_now, set_bt_list};
pub use network::{
    bt_power, refresh_bt, refresh_wifi, wifi_connect, wifi_connect_pass, wifi_disconnect,
    wifi_radio,
};
pub use weather::refresh_weather;

use network::{wifi, wifi_radio_on};
use system::{
    bat_time_now, battery, bt_on, cpu_pct, cpu_temp, dnd_paused, mem_pct, mem_used_gib,
    recorder_on, webcam_active,
};
use weather::weather_loop;
use workspaces::niri_loop;

#[derive(Clone, Debug, PartialEq)]
pub struct Ws {
    pub id: u64,
    pub idx: i64,
    pub name: Option<String>,
    pub active: bool,
    pub focused: bool,
    pub urgent: bool,
    pub has_windows: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WifiNet {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
    pub in_use: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BtDev {
    pub mac: String,
    pub name: String,
    pub connected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioDev {
    pub id: u32,
    pub name: String,
    pub label: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct WeatherHour {
    pub t: u8,
    pub temp: i32,
    pub icon: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct WeatherDay {
    pub date: String,
    pub icon: String,
    pub label: String,
    pub max: i32,
    pub min: i32,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct WeatherData {
    pub temp: i32,
    pub feels: i32,
    pub humidity: u8,
    pub wind: f32,
    pub icon: String,
    pub label: String,
    pub updated: String,
    pub stale: bool,
    pub hourly: Vec<WeatherHour>,
    pub daily: Vec<WeatherDay>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sys {
    pub battery_pct: Option<u8>,
    pub charging: bool,
    pub ac_online: bool,
    pub bat_time: Option<String>,
    pub cpu_pct: u8,
    pub temp_c: Option<i32>,
    pub mem_used_gib: f32,
    pub mem_pct: u8,
    pub vol: u8,
    pub vol_muted: bool,
    pub mic: u8,
    pub mic_muted: bool,
    pub wifi_up: bool,
    pub wifi_signal: u8,
    pub wifi_iface: Option<String>,
    pub wifi_enabled: bool,
    pub bt_on: bool,
    pub wifi_list: Vec<WifiNet>,
    pub wifi_loading: bool,
    pub wifi_error: Option<String>,
    pub bt_list: Vec<BtDev>,
    pub bt_loading: bool,
    pub bt_error: Option<String>,
    pub sinks: Vec<AudioDev>,
    pub sources: Vec<AudioDev>,
    pub audio_loading: bool,
    pub audio_error: Option<String>,
    pub ee_running: bool,
    pub ee_follow_out: bool,
    pub ee_follow_in: bool,
    pub ee_out_target: String,
    pub ee_in_target: String,
    pub weather: Option<String>,
    pub weather_data: Option<WeatherData>,
    pub weather_loading: bool,
    pub weather_error: Option<String>,
    pub webcam_active: bool,
    pub recorder_on: bool,
    pub dnd: bool,
    pub workspaces: Vec<Ws>,
    pub tray: Vec<crate::hud::tray::TrayItem>,
    pub tray_menu: Vec<crate::hud::tray::TrayMenuEntry>,
    pub tray_menu_loading: bool,
    pub tray_menu_owner: (String, String),
}

pub struct Shared {
    pub sys: Mutex<Sys>,
    pub dirty: AtomicBool,
    pub wake_tx: OwnedFd,
}

impl Shared {
    pub fn mark(&self) {
        self.dirty.store(true, Ordering::Relaxed);
        let _ = nix::unistd::write(&self.wake_tx, &[1u8]);
    }
}
pub fn wake_pipe() -> (OwnedFd, OwnedFd) {
    nix::unistd::pipe2(OFlag::O_NONBLOCK | OFlag::O_CLOEXEC).expect("pipe2")
}

pub fn spawn(shared: Arc<Shared>) {
    let s = shared.clone();
    std::thread::spawn(move || sys_loop(s));
    let s = shared.clone();
    std::thread::spawn(move || niri_loop(s));
    let s = shared.clone();
    std::thread::spawn(move || weather_loop(s));
    crate::hud::tray::spawn(shared.clone());
}
pub fn refresh_tray_menu(shared: &Arc<Shared>, service: &str, menu_path: &str) {
    {
        let mut g = shared.sys.lock().unwrap();
        g.tray_menu_loading = true;
        g.tray_menu_owner = (service.to_string(), menu_path.to_string());
        g.tray_menu.clear();
    }
    shared.mark();
    let (svc, mp) = (service.to_string(), menu_path.to_string());
    let s = shared.clone();
    std::thread::spawn(move || {
        let entries = crate::hud::tray::fetch_menu_entries(&svc, &mp);
        let mine = {
            let mut g = s.sys.lock().unwrap();
            if g.tray_menu_owner != (svc.clone(), mp.clone()) {
                false
            } else {
                g.tray_menu_loading = false;
                g.tray_menu = entries;
                true
            }
        };
        if mine {
            s.mark();
        }
    });
}

pub fn send_tray_menu_click(shared: &Arc<Shared>, entry_id: i32) {
    let (svc, mp) = shared.sys.lock().unwrap().tray_menu_owner.clone();
    if svc.is_empty() || mp.is_empty() {
        return;
    }
    crate::hud::tray::send_menu_event(&svc, &mp, entry_id);
}
fn sys_loop(shared: Arc<Shared>) {
    let mut prev_cpu: Option<(u64, u64)> = None;
    let mut tick: u64 = 0;
    let bt_conn = dbus::blocking::Connection::new_system().ok();
    loop {
        tick += 1;
        // Батарея + время разряда: раз в 30 сек (tick % 30 == 1 — сразу на старте).
        let bat = if tick % 30 == 1 {
            Some((battery(), bat_time_now()))
        } else {
            None
        };
        let stats = if tick % 12 == 1 {
            Some((
                cpu_pct(&mut prev_cpu),
                cpu_temp(),
                mem_used_gib().unwrap_or(0.0),
                mem_pct(),
            ))
        } else {
            None
        };
        let (vol, vol_muted) = wpctl_volume("@DEFAULT_AUDIO_SINK@").unwrap_or((0, false));
        let (mic, mic_muted) = wpctl_volume("@DEFAULT_AUDIO_SOURCE@").unwrap_or((0, false));
        let (wifi_up, wifi_signal, wifi_iface) = wifi();
        let wifi_en = wifi_radio_on(&wifi_iface);
        let bt = bt_on(bt_conn.as_ref());
        let extras = if tick.is_multiple_of(2) {
            Some((recorder_on(), webcam_active(), dnd_paused()))
        } else {
            None
        };

        let mut changed = false;
        {
            let mut g = shared.sys.lock().unwrap();
            macro_rules! upd {
                ($f:ident, $v:expr) => {
                    let v = $v;
                    if g.$f != v {
                        g.$f = v;
                        changed = true;
                    }
                };
            }
            if let Some(((bat_pct, charging, ac), bat_time)) = bat {
                upd!(battery_pct, bat_pct);
                upd!(charging, charging);
                upd!(ac_online, ac);
                upd!(bat_time, bat_time);
            }
            if let Some((cpu, temp, mem, mem_pct)) = stats {
                upd!(cpu_pct, cpu);
                upd!(temp_c, temp);
                upd!(mem_used_gib, mem);
                upd!(mem_pct, mem_pct);
            }
            upd!(vol, vol);
            upd!(vol_muted, vol_muted);
            upd!(mic, mic);
            upd!(mic_muted, mic_muted);
            upd!(wifi_up, wifi_up);
            upd!(wifi_signal, wifi_signal);
            upd!(wifi_iface, wifi_iface);
            upd!(wifi_enabled, wifi_en);
            upd!(bt_on, bt);
            if let Some((rec, cam, dnd)) = extras {
                upd!(recorder_on, rec);
                upd!(webcam_active, cam);
                upd!(dnd, dnd);
            }
        }
        if changed {
            shared.mark();
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
}
#[cfg(test)]
mod tests {
    use super::audio::parse_wpctl_volume;
    use super::network::{parse_bt_devices, parse_wifi_list};
    use super::weather::parse_weather;
    use super::workspaces::parse_ws;
    use super::{BtDev, WifiNet};

    #[test]
    fn parses_wifi_escaped_separators_and_keeps_strongest_network() {
        let networks = parse_wifi_list(
            "*:Cafe\\:Guest:86:WPA2\n :Cafe\\:Guest:99:WPA2\n :Open:101:\n :broken:not-a-number:\n",
        );

        assert_eq!(
            networks,
            vec![
                WifiNet {
                    ssid: "Cafe:Guest".into(),
                    signal: 86,
                    security: "WPA2".into(),
                    in_use: true,
                },
                WifiNet {
                    ssid: "Open".into(),
                    signal: 100,
                    security: "".into(),
                    in_use: false,
                },
                WifiNet {
                    ssid: "broken".into(),
                    signal: 0,
                    security: "".into(),
                    in_use: false,
                },
            ]
        );
    }

    #[test]
    fn parses_bluetooth_with_variable_spacing_and_connected_state() {
        let devices = parse_bt_devices(
            "Device AA:BB:CC:DD:EE:FF  Headphones   Pro\nDevice 11:22:33:44:55:66 Keyboard\n",
            "Device AA:BB:CC:DD:EE:FF Headphones Pro\n",
        );

        assert_eq!(
            devices,
            vec![
                BtDev {
                    mac: "AA:BB:CC:DD:EE:FF".into(),
                    name: "Headphones Pro".into(),
                    connected: true,
                },
                BtDev {
                    mac: "11:22:33:44:55:66".into(),
                    name: "Keyboard".into(),
                    connected: false,
                },
            ]
        );
    }

    #[test]
    fn parses_wpctl_volume_and_muted_marker() {
        assert_eq!(
            parse_wpctl_volume("Volume: 0.50 [MUTED]\n"),
            Some((50, true))
        );
        assert_eq!(parse_wpctl_volume("Volume: 1.20\n"), Some((100, false)));
        assert_eq!(parse_wpctl_volume("no volume"), None);
    }

    #[test]
    fn parses_weather_and_clamps_untrusted_values() {
        let value = serde_json::json!({
            "text": "<b>+21</b> облачно",
            "data": {
                "temp": 21,
                "feels": 20,
                "humidity": 140,
                "wind": 2.5,
                "icon": "cloud",
                "label": "Облачно",
                "hourly": [{"t": 12, "temp": 22, "icon": "sun"}],
                "daily": [{"date": "2026-09-25", "icon": "cloud", "label": "Облачно", "max": 23, "min": 15}]
            }
        });
        let (text, data) = parse_weather(&value);
        let data = data.expect("weather data");

        assert_eq!(text.as_deref(), Some("+21 облачно"));
        assert_eq!(data.humidity, 100);
        assert_eq!(data.hourly.len(), 1);
        assert_eq!(data.daily.len(), 1);
    }

    #[test]
    fn rejects_workspace_without_id() {
        let value = serde_json::json!({"idx": 2, "is_active": true});
        assert!(parse_ws(&value).is_none());
    }
}

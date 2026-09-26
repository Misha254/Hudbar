use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::{Shared, WeatherData, WeatherDay, WeatherHour};

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

pub(super) fn parse_weather(v: &serde_json::Value) -> (Option<String>, Option<WeatherData>) {
    let text = v.get("text").and_then(|t| t.as_str()).map(strip_tags);
    let data = v.get("data").map(|d| {
        let gi = |k: &str| d.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
        let gs = |k: &str| d.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        WeatherData {
            temp: gi("temp") as i32,
            feels: gi("feels") as i32,
            humidity: gi("humidity").clamp(0, 100) as u8,
            wind: d.get("wind").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
            icon: gs("icon"),
            label: gs("label"),
            updated: gs("updated"),
            stale: d.get("stale").and_then(|x| x.as_bool()).unwrap_or(false),
            hourly: d
                .get("hourly")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|p| {
                            Some(WeatherHour {
                                t: p.get("t")?.as_u64()? as u8,
                                temp: p.get("temp")?.as_i64()? as i32,
                                icon: p.get("icon")?.as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            daily: d
                .get("daily")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|p| {
                            Some(WeatherDay {
                                date: p.get("date")?.as_str()?.to_string(),
                                icon: p.get("icon")?.as_str()?.to_string(),
                                label: p.get("label")?.as_str()?.to_string(),
                                max: p.get("max")?.as_i64()? as i32,
                                min: p.get("min")?.as_i64()? as i32,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    });
    (text, data)
}

fn weather_run(shared: &Arc<Shared>) -> bool {
    let script = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".local/bin/weather-fancy.py");
    let out = match script.to_str() {
        Some(script) => crate::hud::log::output(script, &[])
            .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok()),
        None => None,
    };
    let (text, data, error) = match &out {
        Some(v) => {
            let (text, data) = parse_weather(v);
            (text, data, None)
        }
        None => (
            None,
            None,
            Some("weather-fancy.py недоступен или вернул неверный JSON".to_string()),
        ),
    };
    let mut changed = false;
    {
        let mut g = shared.sys.lock().unwrap();
        if g.weather != text {
            g.weather = text.clone();
            changed = true;
        }
        if g.weather_data != data {
            g.weather_data = data;
            changed = true;
        }
        if g.weather_loading {
            g.weather_loading = false;
            changed = true;
        }
        if g.weather_error != error {
            g.weather_error = error;
            changed = true;
        }
    }
    if changed {
        shared.mark();
    }
    text.is_some()
}

pub(super) fn weather_loop(shared: Arc<Shared>) {
    loop {
        let ok = weather_run(&shared);
        let wait = if ok { 5400 } else { 60 };
        std::thread::sleep(Duration::from_secs(wait));
    }
}

pub fn refresh_weather(shared: &Arc<Shared>) {
    {
        let mut g = shared.sys.lock().unwrap();
        g.weather_loading = true;
    }
    shared.mark();
    let s = shared.clone();
    std::thread::spawn(move || {
        weather_run(&s);
    });
}

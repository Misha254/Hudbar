use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::blocking::{Connection, Proxy};
use dbus::channel::{MatchingReceiver, Sender};
use dbus::message::{MatchRule, Message, MessageType};

use crate::hud::data::Shared;

const WATCHER_BUS: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_IFACE: &str = "org.kde.StatusNotifierWatcher";
const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
const DBUS_IFACE: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DEFAULT_ITEM_PATH: &str = "/StatusNotifierItem";

#[derive(Clone, Debug, PartialEq)]
pub struct TrayIcon {
    pub w: i32,
    pub h: i32,
    /// BGRA bytes (native-endian ARGB32), premultiplied
    pub argb: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrayMenuEntry {
    pub id: i32,
    pub label: String,
    pub parent: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrayItem {
    pub service: String,
    pub path: String,
    pub sni_id: String,
    pub title: String,
    pub tooltip: String,
    pub status: String,
    pub icon: Option<TrayIcon>,
    pub icon_name: String,
    pub menu: String,
}

impl TrayItem {
    pub fn visible(&self) -> bool {
        self.status != "Passive" && self.icon.is_some()
    }

    pub fn display_name(&self) -> String {
        if !self.tooltip.is_empty() {
            self.tooltip.clone()
        } else if !self.title.is_empty() {
            self.title.clone()
        } else if !self.icon_name.is_empty() {
            self.icon_name.clone()
        } else {
            self.service.clone()
        }
    }
}

struct ItemState {
    sni_id: String,
    title: String,
    tooltip: String,
    status: String,
    icon: Option<TrayIcon>,
    icon_name: String,
    menu: String,
}

type State = HashMap<(String, String), ItemState>;
type Pending = Arc<Mutex<Vec<(String, String, u8)>>>;

pub fn spawn(shared: Arc<Shared>) {
    std::thread::spawn(move || run(shared));
}

fn run(shared: Arc<Shared>) {
    let conn = match Connection::new_session() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("tray: no session bus: {e}");
            return;
        }
    };
    if conn.request_name(WATCHER_BUS, false, true, false).is_err() {
        eprintln!("tray: cannot own {WATCHER_BUS}");
        return;
    }
    let state = Arc::new(Mutex::new(State::new()));
    let pending: Pending = Arc::new(Mutex::new(Vec::new()));

    {
        let st = state.clone();
        let sh = shared.clone();
        let pd = pending.clone();
        let rule = MatchRule::new_method_call().with_path(WATCHER_PATH);
        let _ = conn.start_receive(
            rule,
            Box::new(move |msg: Message, conn: &Connection| -> bool {
                handle_call(&msg, conn, &st, &sh, &pd);
                true
            }),
        );
    }
    {
        let st = state.clone();
        let sh = shared.clone();
        let rule = MatchRule::new()
            .with_type(MessageType::Signal)
            .with_interface(ITEM_IFACE);
        let _ = conn.add_match(
            rule,
            move |_args: VecDeque<Box<dyn RefArg>>, conn: &Connection, msg: &Message| -> bool {
                let sender = msg.sender().map(|s| s.to_string()).unwrap_or_default();
                let path = msg.path().map(|p| p.to_string()).unwrap_or_default();
                if let Some(
                    "NewIcon" | "NewAttentionIcon" | "NewOverlayIcon" | "NewTitle" | "NewStatus"
                    | "NewToolTip",
                ) = msg.member().map(|m| m.to_string()).as_deref()
                {
                    fetch_item(conn, &st, &sh, &sender, &path, Duration::from_secs(2));
                }
                true
            },
        );
    }
    {
        let st = state.clone();
        let sh = shared.clone();
        let rule = MatchRule::new()
            .with_type(MessageType::Signal)
            .with_interface(DBUS_IFACE)
            .with_member("NameOwnerChanged")
            .with_path(DBUS_PATH);
        let _ = conn.add_match(
            rule,
            move |_args: VecDeque<Box<dyn RefArg>>, conn: &Connection, msg: &Message| -> bool {
                let (name, _old, new): (Option<String>, Option<String>, Option<String>) =
                    msg.get3();
                if let (Some(name), Some(new)) = (name, new) {
                    if new.is_empty() {
                        let removed = {
                            let mut st = st.lock().unwrap();
                            let before = st.len();
                            st.retain(|(s, _), _| s != &name);
                            st.len() != before
                        };
                        if removed {
                            publish(&st, &sh);
                        }
                    } else {
                        probe_name(conn, &st, &sh, &name);
                    }
                }
                true
            },
        );
    }

    {
        let proxy = Proxy::new(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            Duration::from_secs(3),
            &conn,
        );
        let res: Result<(Vec<String>,), dbus::Error> =
            proxy.method_call("org.freedesktop.DBus", "ListNames", ());
        if let Ok((names,)) = res {
            for n in &names {
                probe_name(&conn, &state, &shared, n);
            }
        }
    }

    loop {
        let jobs: Vec<(String, String, u8)> = {
            let mut p = pending.lock().unwrap();
            std::mem::take(&mut *p)
        };
        for (bus, path, retries) in jobs {
            if !fetch_item(&conn, &state, &shared, &bus, &path, Duration::from_secs(2))
                && retries < 6
            {
                pending.lock().unwrap().push((bus, path, retries + 1));
            }
        }
        if conn.process(Duration::from_millis(500)).is_err() {
            break;
        }
    }
}

fn split_service(service: &str) -> (String, String) {
    match service.find('/') {
        Some(i) => (service[..i].to_string(), service[i..].to_string()),
        None => (service.to_string(), DEFAULT_ITEM_PATH.to_string()),
    }
}

fn publish(state: &Arc<Mutex<State>>, shared: &Arc<Shared>) {
    let items: Vec<TrayItem> = {
        let st = state.lock().unwrap();
        let mut v: Vec<TrayItem> = st
            .iter()
            .map(|((service, path), s)| TrayItem {
                service: service.clone(),
                path: path.clone(),
                sni_id: s.sni_id.clone(),
                title: s.title.clone(),
                tooltip: s.tooltip.clone(),
                status: s.status.clone(),
                icon: s.icon.clone(),
                icon_name: s.icon_name.clone(),
                menu: s.menu.clone(),
            })
            .collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
    };
    let mut g = shared.sys.lock().unwrap();
    if g.tray != items {
        g.tray = items;
        drop(g);
        shared.mark();
    }
}

fn get_props(conn: &Connection, service: &str, path: &str, timeout: Duration) -> Option<PropMap> {
    if service.is_empty() || path.is_empty() {
        return None;
    }
    let proxy = Proxy::new(service, path, timeout, conn);
    proxy.get_all(ITEM_IFACE).ok()
}

fn parse_tooltip(props: &PropMap) -> String {
    let Some(v) = props.get("ToolTip") else {
        return String::new();
    };
    let Some(mut it) = v.0.as_iter() else {
        return String::new();
    };
    // SNI ToolTip struct: (icon-name, pixmaps, title, text)
    let _icon = it.next().and_then(|x| x.as_str()).unwrap_or("");
    let _pixmaps = it.next();
    let title = it
        .next()
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let text = it
        .next()
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        text
    } else if text.is_empty() || text == title {
        title
    } else {
        format!("{title} — {text}")
    }
}

fn store_props(
    state: &Arc<Mutex<State>>,
    shared: &Arc<Shared>,
    service: &str,
    path: &str,
    props: &PropMap,
) {
    let get_str = |k: &str| -> String {
        props
            .get(k)
            .and_then(|v| v.0.as_str())
            .unwrap_or("")
            .to_string()
    };
    let title = get_str("Title");
    let status = get_str("Status");
    let icon_name = get_str("IconName");
    let tooltip = parse_tooltip(props);
    let menu = get_str("Menu");
    let sni_id = get_str("Id");

    let mut pixmaps: Vec<(i32, i32, Vec<u8>)> = Vec::new();
    if let Some(v) = props.get("IconPixmap")
        && let Some(outer) = v.0.as_iter()
    {
        for entry in outer {
            let mut it = match entry.as_iter() {
                Some(i) => i,
                None => continue,
            };
            let w = it.next().and_then(|x| x.as_i64()).unwrap_or(0) as i32;
            let h = it.next().and_then(|x| x.as_i64()).unwrap_or(0) as i32;
            let data: Vec<u8> = match it.next() {
                Some(d) => match d.as_iter() {
                    Some(bytes) => bytes.map(|b| b.as_i64().unwrap_or(0) as u8).collect(),
                    None => Vec::new(),
                },
                None => Vec::new(),
            };
            if w > 0 && h > 0 && data.len() == (w as usize) * (h as usize) * 4 {
                pixmaps.push((w, h, data));
            }
        }
    }
    pixmaps.sort_by_key(|(w, h, _)| (*w, *h));
    let icon = pixmaps
        .iter()
        .find(|(w, h, _)| *w >= 34 && *h >= 34)
        .or_else(|| pixmaps.last())
        .map(|(w, h, d)| TrayIcon {
            w: *w,
            h: *h,
            argb: d.clone(),
        })
        .or_else(|| icon_from_name(&get_str("IconThemePath"), &icon_name));

    {
        let mut st = state.lock().unwrap();
        st.insert(
            (service.to_string(), path.to_string()),
            ItemState {
                sni_id,
                title,
                tooltip,
                status,
                icon,
                icon_name,
                menu,
            },
        );
    }
    publish(state, shared);
}

fn icon_from_name(theme_path: &str, name: &str) -> Option<TrayIcon> {
    if name.is_empty() {
        return None;
    }
    let mut cands: Vec<PathBuf> = Vec::new();
    let direct = Path::new(name);
    if direct.is_absolute() {
        cands.push(direct.to_path_buf());
    } else {
        for dir in theme_path.split(':').filter(|d| !d.is_empty()) {
            cands.push(Path::new(dir).join(format!("{name}.png")));
        }
        cands.push(Path::new("/usr/share/pixmaps").join(format!("{name}.png")));
        for size in [16, 22, 24, 32, 48, 64, 128] {
            for cat in ["apps", "status", "devices", "panel"] {
                cands.push(PathBuf::from(format!(
                    "/usr/share/icons/hicolor/{size}x{size}/{cat}/{name}.png"
                )));
            }
        }
    }
    for c in cands {
        let Ok(bytes) = std::fs::read(&c) else {
            continue;
        };
        if let Some(icon) = png_to_icon(&bytes) {
            return Some(icon);
        }
    }
    None
}

pub(crate) fn png_to_icon(bytes: &[u8]) -> Option<TrayIcon> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as i32, info.height as i32);
    if w <= 0 || h <= 0 {
        return None;
    }
    let px = (w * h) as usize;
    let mut argb = vec![0u8; px * 4];
    let src = &buf[..info.buffer_size()];
    let get = |i: usize| -> (u8, u8, u8, u8) {
        match info.color_type {
            png::ColorType::Rgba => (src[i * 4], src[i * 4 + 1], src[i * 4 + 2], src[i * 4 + 3]),
            png::ColorType::Rgb => (src[i * 3], src[i * 3 + 1], src[i * 3 + 2], 255),
            png::ColorType::GrayscaleAlpha => {
                let v = src[i * 2];
                (v, v, v, src[i * 2 + 1])
            }
            png::ColorType::Grayscale => {
                let v = src[i];
                (v, v, v, 255)
            }
            _ => (255, 255, 255, 255),
        }
    };
    for i in 0..px {
        let (r, g, b, a) = get(i);
        argb[i * 4] = b;
        argb[i * 4 + 1] = g;
        argb[i * 4 + 2] = r;
        argb[i * 4 + 3] = a;
    }
    Some(TrayIcon { w, h, argb })
}

fn fetch_item(
    conn: &Connection,
    state: &Arc<Mutex<State>>,
    shared: &Arc<Shared>,
    service: &str,
    path: &str,
    timeout: Duration,
) -> bool {
    match get_props(conn, service, path, timeout) {
        Some(props) => {
            store_props(state, shared, service, path, &props);
            true
        }
        None => false,
    }
}

fn probe_name(conn: &Connection, state: &Arc<Mutex<State>>, shared: &Arc<Shared>, name: &str) {
    if name.is_empty() || name == WATCHER_BUS || name == "org.freedesktop.DBus" {
        return;
    }
    {
        let st = state.lock().unwrap();
        if st.keys().any(|(s, _)| s == name) {
            return;
        }
    }
    let Some(props) = get_props(conn, name, DEFAULT_ITEM_PATH, Duration::from_millis(300)) else {
        return;
    };
    if !props.contains_key("Title") && !props.contains_key("IconPixmap") {
        return;
    }
    store_props(state, shared, name, DEFAULT_ITEM_PATH, &props);
}

fn emit(conn: &Connection, member: &str, arg: &str) {
    let path = dbus::strings::Path::new(WATCHER_PATH).unwrap();
    let iface = dbus::strings::Interface::new(WATCHER_IFACE).unwrap();
    let mem = dbus::strings::Member::new(member).unwrap();
    let _ = conn.send(Message::signal(&path, &iface, &mem).append1(arg.to_string()));
}

fn send_error(msg: &Message, conn: &Connection, what: &str) {
    let name = dbus::strings::ErrorName::new(format!("org.freedesktop.DBus.Error.{what}")).unwrap();
    let text = std::ffi::CString::new(what).unwrap();
    let _ = conn.send(msg.error(&name, &text));
}

fn handle_props(msg: &Message, conn: &Connection, state: &Arc<Mutex<State>>) {
    let member = msg.member().map(|m| m.to_string()).unwrap_or_default();
    if member == "GetAll" {
        let iface: Option<String> = msg.get1();
        if iface.as_deref() != Some(WATCHER_IFACE) {
            send_error(msg, conn, "UnknownInterface");
            return;
        }
        let st = state.lock().unwrap();
        let mut items: Vec<String> = st.keys().map(|(s, p)| format!("{s}{p}")).collect();
        items.sort();
        let mut map = PropMap::new();
        map.insert(
            "RegisteredStatusNotifierItems".to_string(),
            Variant(Box::new(items) as Box<dyn RefArg>),
        );
        map.insert(
            "IsStatusNotifierHostRegistered".to_string(),
            Variant(Box::new(true)),
        );
        map.insert("ProtocolVersion".to_string(), Variant(Box::new(0i32)));
        let _ = conn.send(msg.method_return().append1(map));
    } else if member == "Get" {
        let (iface, prop): (Option<String>, Option<String>) = msg.get2();
        if iface.as_deref() != Some(WATCHER_IFACE) {
            send_error(msg, conn, "UnknownInterface");
            return;
        }
        let st = state.lock().unwrap();
        let v: Option<Variant<Box<dyn RefArg>>> = match prop.as_deref() {
            Some("RegisteredStatusNotifierItems") => {
                let mut items: Vec<String> = st.keys().map(|(s, p)| format!("{s}{p}")).collect();
                items.sort();
                Some(Variant(Box::new(items) as Box<dyn RefArg>))
            }
            Some("IsStatusNotifierHostRegistered") => Some(Variant(Box::new(true))),
            Some("ProtocolVersion") => Some(Variant(Box::new(0i32))),
            _ => None,
        };
        match v {
            Some(v) => {
                let _ = conn.send(msg.method_return().append1(v));
            }
            None => send_error(msg, conn, "UnknownProperty"),
        }
    } else {
        send_error(msg, conn, "UnknownMethod");
    }
}

fn handle_call(
    msg: &Message,
    conn: &Connection,
    state: &Arc<Mutex<State>>,
    shared: &Arc<Shared>,
    pending: &Pending,
) {
    let iface = msg.interface().map(|i| i.to_string()).unwrap_or_default();
    let member = msg.member().map(|m| m.to_string()).unwrap_or_default();
    if iface == PROPS_IFACE {
        handle_props(msg, conn, state);
        return;
    }
    if iface != WATCHER_IFACE {
        send_error(msg, conn, "UnknownInterface");
        return;
    }
    match member.as_str() {
        "RegisterStatusNotifierItem" => {
            let service: Option<String> = msg.get1();
            if let Some(service) = service {
                let (bus, path) = if service.starts_with('/') {
                    let sender = msg.sender().map(|s| s.to_string()).unwrap_or_default();
                    (sender, service.clone())
                } else {
                    split_service(&service)
                };
                if !bus.is_empty() {
                    pending.lock().unwrap().push((bus, path, 0));
                }
                emit(conn, "StatusNotifierItemRegistered", &service);
            }
            let _ = conn.send(msg.method_return());
        }
        "UnregisterStatusNotifierItem" => {
            let service: Option<String> = msg.get1();
            if let Some(service) = service {
                let (bus, path) = split_service(&service);
                let removed = {
                    let mut st = state.lock().unwrap();
                    st.remove(&(bus, path)).is_some()
                };
                if removed {
                    publish(state, shared);
                    emit(conn, "StatusNotifierItemUnregistered", &service);
                }
            }
            let _ = conn.send(msg.method_return());
        }
        "RegisterStatusNotifierHost" => {
            emit(conn, "StatusNotifierHostRegistered", "");
            let _ = conn.send(msg.method_return());
        }
        _ => send_error(msg, conn, "UnknownMethod"),
    }
}

const MENU_IFACE: &str = "com.canonical.dbusmenu";

fn clean_menu_label(s: &str) -> String {
    // Убираем мнемоники Qt (&X) и Gtk (_X), схлопываем пробелы.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            continue;
        }
        if c == '&' {
            let next_is_amp = chars.peek().is_some_and(|nc| *nc == '&');
            let next_is_alnum = chars.peek().is_some_and(|nc| nc.is_alphanumeric());
            if next_is_amp {
                out.push('&');
                chars.next();
            } else if next_is_alnum {
                if let Some(nc) = chars.next() {
                    out.push(nc);
                }
            } else {
                out.push('&');
            }
            continue;
        }
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn unvariant(item: dbus::arg::messageitem::MessageItem) -> dbus::arg::messageitem::MessageItem {
    match item {
        dbus::arg::messageitem::MessageItem::Variant(b) => *b,
        o => o,
    }
}

fn props_map(
    item: dbus::arg::messageitem::MessageItem,
) -> Vec<(String, dbus::arg::messageitem::MessageItem)> {
    use dbus::arg::messageitem::MessageItem as MI;
    // a{sv} в get_items приходит как Dict.
    let MI::Dict(d) = unvariant(item) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (k, v) in d.into_vec() {
        let MI::Str(ks) = unvariant(k) else {
            continue;
        };
        out.push((ks, unvariant(v)));
    }
    out
}

fn menu_prop_str(
    props: &[(String, dbus::arg::messageitem::MessageItem)],
    key: &str,
) -> Option<String> {
    use dbus::arg::messageitem::MessageItem as MI;
    props
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| match v {
            MI::Str(s) => Some(s.clone()),
            _ => None,
        })
}

fn menu_prop_bool(
    props: &[(String, dbus::arg::messageitem::MessageItem)],
    key: &str,
    default: bool,
) -> bool {
    use dbus::arg::messageitem::MessageItem as MI;
    props
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| match v {
            MI::Bool(b) => *b,
            _ => default,
        })
        .unwrap_or(default)
}

fn parse_menu_node(
    item: dbus::arg::messageitem::MessageItem,
    parent: &str,
    out: &mut Vec<TrayMenuEntry>,
) {
    use dbus::arg::messageitem::MessageItem as MI;
    // Нода: Struct(id: i32, props: a{sv}, children: av).
    let MI::Struct(mut f) = unvariant(item) else {
        return;
    };
    if f.len() != 3 {
        return;
    }
    let children = f.pop().unwrap();
    let props = f.pop().unwrap();
    let MI::Int32(id) = f.pop().unwrap() else {
        return;
    };
    let props = props_map(props);
    if menu_prop_str(&props, "type").as_deref() == Some("separator") {
        return;
    }
    if !menu_prop_bool(&props, "visible", true) {
        return;
    }
    let label = menu_prop_str(&props, "label")
        .map(|s| clean_menu_label(&s))
        .unwrap_or_default();
    let enabled = menu_prop_bool(&props, "enabled", true);
    let MI::Array(children) = unvariant(children) else {
        return;
    };
    let children = children.into_vec();
    let has_submenu = menu_prop_str(&props, "children-display").as_deref() == Some("submenu")
        || !children.is_empty();
    if label.is_empty() && !has_submenu {
        return;
    }
    if has_submenu && !children.is_empty() {
        // Подменю разворачиваем плоским списком, родитель — в правой колонке.
        let parent_label = if label.is_empty() {
            parent.to_string()
        } else {
            label.clone()
        };
        for c in children {
            parse_menu_node(c, &parent_label, out);
        }
        // Сам родитель тоже показываем: dbusmenu разрешает клик и по нему.
        if !label.is_empty() {
            out.push(TrayMenuEntry {
                id,
                label,
                parent: parent.to_string(),
                enabled,
            });
        }
        return;
    }
    if label.is_empty() {
        return;
    }
    out.push(TrayMenuEntry {
        id,
        label,
        parent: parent.to_string(),
        enabled,
    });
}

fn menu_call(
    conn: &Connection,
    service: &str,
    path: &str,
    member: &str,
    args: &[dbus::arg::messageitem::MessageItem],
) -> Option<Vec<dbus::arg::messageitem::MessageItem>> {
    use dbus::blocking::BlockingSender;
    let bus = dbus::strings::BusName::new(service).ok()?;
    let path = dbus::strings::Path::new(path).ok()?;
    let iface = dbus::strings::Interface::new(MENU_IFACE).ok()?;
    let member = dbus::strings::Member::new(member).ok()?;
    let mut msg = Message::method_call(&bus, &path, &iface, &member);
    msg.append_items(args);
    conn.send_with_reply_and_block(msg, Duration::from_secs(3))
        .ok()
        .map(|r| r.get_items())
}

pub fn fetch_menu_entries(service: &str, menu_path: &str) -> Vec<TrayMenuEntry> {
    use dbus::arg::messageitem::MessageItem as MI;
    let mut out = Vec::new();
    if service.is_empty() || menu_path.is_empty() {
        return out;
    }
    let Ok(conn) = Connection::new_session() else {
        return out;
    };
    // Меню может подгружаться лениво — AboutToShow(0) перед GetLayout.
    let _ = menu_call(&conn, service, menu_path, "AboutToShow", &[MI::Int32(0)]);
    let Ok(props_arg) = MI::new_array(
        ["label", "enabled", "visible", "children-display", "type"]
            .into_iter()
            .map(|s| MI::Str(s.into()))
            .collect(),
    ) else {
        return out;
    };
    let items = menu_call(
        &conn,
        service,
        menu_path,
        "GetLayout",
        &[MI::Int32(0), MI::Int32(2), props_arg],
    );
    let Some(mut items) = items else {
        return out;
    };
    // Ответ: (revision, layout: Struct).
    if items.len() != 2 {
        return out;
    }
    let layout = items.pop().unwrap();
    let MI::Struct(mut root) = unvariant(layout) else {
        return out;
    };
    if root.len() != 3 {
        return out;
    }
    let children = root.pop().unwrap();
    let MI::Array(children) = unvariant(children) else {
        return out;
    };
    for c in children.into_vec() {
        parse_menu_node(c, "", &mut out);
    }
    out
}

pub fn activate_item(service: String, path: String) {
    std::thread::spawn(move || {
        let Ok(conn) = Connection::new_session() else {
            return;
        };
        let proxy = Proxy::new(
            service.as_str(),
            path.as_str(),
            Duration::from_secs(3),
            &conn,
        );
        let _: Result<(), dbus::Error> = proxy.method_call(ITEM_IFACE, "Activate", (0i32, 0i32));
    });
}

pub fn send_menu_event(service: &str, menu_path: &str, id: i32) {
    use dbus::arg::messageitem::MessageItem as MI;
    let service = service.to_string();
    let menu_path = menu_path.to_string();
    std::thread::spawn(move || {
        let Ok(conn) = Connection::new_session() else {
            return;
        };
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);
        let _ = menu_call(
            &conn,
            &service,
            &menu_path,
            "Event",
            &[
                MI::Int32(id),
                MI::Str("clicked".into()),
                MI::Variant(Box::new(MI::Str(String::new()))),
                MI::UInt32(ts),
            ],
        );
    });
}

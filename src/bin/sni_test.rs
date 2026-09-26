use std::sync::{Arc, Mutex};
use std::time::Duration;

type Pixmap = (i32, i32, Vec<u8>);
type Tooltip = (String, Vec<Pixmap>, String, String);

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::{Connection, Proxy};
use dbus::channel::{MatchingReceiver, Sender};
use dbus::message::{MatchRule, Message};

const PATH: &str = "/StatusNotifierItem";
const IFACE: &str = "org.kde.StatusNotifierItem";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

fn pixmap(color: (u8, u8, u8)) -> Vec<(i32, i32, Vec<u8>)> {
    let (w, h) = (32, 32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let o = ((y * w + x) * 4) as usize;
            let border = x < 3 || y < 3 || x >= w - 3 || y >= h - 3;
            let (r, g, b) = if border { (255, 255, 255) } else { color };
            px[o] = b;
            px[o + 1] = g;
            px[o + 2] = r;
            px[o + 3] = 255;
        }
    }
    vec![(w, h, px)]
}

fn props(color: (u8, u8, u8)) -> PropMap {
    let mut m = PropMap::new();
    let s = |v: &str| Variant(Box::new(v.to_string()) as Box<dyn RefArg>);
    m.insert("Category".into(), s("ApplicationStatus"));
    m.insert("Id".into(), s("sni-test"));
    m.insert("Title".into(), s("SNI Test"));
    m.insert("Status".into(), s("Active"));
    m.insert(
        "WindowId".into(),
        Variant(Box::new(0i32) as Box<dyn RefArg>),
    );
    m.insert("IconName".into(), s(""));
    m.insert("OverlayIconName".into(), s(""));
    m.insert("AttentionIconName".into(), s(""));
    m.insert("AttentionMovieName".into(), s(""));
    m.insert(
        "Menu".into(),
        Variant(
            Box::new(dbus::strings::Path::new("/Menu").unwrap().into_static()) as Box<dyn RefArg>,
        ),
    );
    let pm = pixmap(color);
    m.insert(
        "IconPixmap".into(),
        Variant(Box::new(pm.clone()) as Box<dyn RefArg>),
    );
    m.insert(
        "OverlayIconPixmap".into(),
        Variant(Box::new(Vec::<(i32, i32, Vec<u8>)>::new()) as Box<dyn RefArg>),
    );
    m.insert(
        "AttentionIconPixmap".into(),
        Variant(Box::new(pm) as Box<dyn RefArg>),
    );
    let tooltip: Tooltip = ("".into(), vec![], "SNI Test".into(), "test tooltip".into());
    m.insert(
        "ToolTip".into(),
        Variant(Box::new(tooltip) as Box<dyn RefArg>),
    );
    m
}

const MENU_PATH: &str = "/Menu";
const MENU_IFACE: &str = "com.canonical.dbusmenu";

fn menu_prop(
    k: &str,
    v: dbus::arg::messageitem::MessageItem,
) -> (
    dbus::arg::messageitem::MessageItem,
    dbus::arg::messageitem::MessageItem,
) {
    use dbus::arg::messageitem::MessageItem as MI;
    (MI::Str(k.into()), MI::Variant(Box::new(v)))
}

fn menu_dict(
    pairs: Vec<(
        dbus::arg::messageitem::MessageItem,
        dbus::arg::messageitem::MessageItem,
    )>,
) -> dbus::arg::messageitem::MessageItem {
    use dbus::arg::messageitem::{MessageItem as MI, MessageItemDict};
    MI::Dict(
        MessageItemDict::new(
            pairs,
            dbus::Signature::new("s").unwrap(),
            dbus::Signature::new("v").unwrap(),
        )
        .unwrap(),
    )
}

fn menu_kids(
    kids: Vec<dbus::arg::messageitem::MessageItem>,
) -> dbus::arg::messageitem::MessageItem {
    use dbus::arg::messageitem::MessageItem as MI;
    if kids.is_empty() {
        MI::Array(
            dbus::arg::messageitem::MessageItemArray::new(
                vec![],
                dbus::Signature::new("av").unwrap(),
            )
            .unwrap(),
        )
    } else {
        // Дети по спеке — варианты, иначе вложенные ноды дают разную глубину типа.
        MI::new_array(kids.into_iter().map(|k| MI::Variant(Box::new(k))).collect()).unwrap()
    }
}

fn menu_node(
    id: i32,
    label: &str,
    enabled: bool,
    kids: Vec<dbus::arg::messageitem::MessageItem>,
) -> dbus::arg::messageitem::MessageItem {
    use dbus::arg::messageitem::MessageItem as MI;
    let mut props = vec![
        menu_prop("label", MI::Str(label.into())),
        menu_prop("enabled", MI::Bool(enabled)),
        menu_prop("visible", MI::Bool(true)),
    ];
    if !kids.is_empty() {
        props.push(menu_prop("children-display", MI::Str("submenu".into())));
    }
    MI::Struct(vec![MI::Int32(id), menu_dict(props), menu_kids(kids)])
}

fn menu_layout() -> dbus::arg::messageitem::MessageItem {
    use dbus::arg::messageitem::MessageItem as MI;
    let kids = vec![
        menu_node(1, "Open SNI Test", true, vec![]),
        MI::Struct(vec![
            MI::Int32(2),
            menu_dict(vec![menu_prop("type", MI::Str("separator".into()))]),
            menu_kids(vec![]),
        ]),
        menu_node(
            3,
            "Settings",
            true,
            vec![menu_node(4, "Sub item", true, vec![])],
        ),
        menu_node(5, "_Quit", true, vec![]),
        menu_node(6, "Disabled item", false, vec![]),
    ];
    MI::Struct(vec![MI::Int32(0), menu_dict(vec![]), menu_kids(kids)])
}

fn handle_menu(msg: Message, conn: &Connection) -> bool {
    use dbus::arg::messageitem::MessageItem as MI;
    let member = msg.member().map(|m| m.to_string()).unwrap_or_default();
    match member.as_str() {
        "AboutToShow" => {
            let _ = conn.send(msg.method_return().append1(true));
        }
        "GetLayout" => {
            let mut reply = msg.method_return();
            reply.append_items(&[MI::UInt32(7), menu_layout()]);
            let _ = conn.send(reply);
        }
        "Event" => {
            let items = msg.get_items();
            let line = format!("MENU-EVENT {items:?}\n");
            let _ = std::fs::write("/tmp/sni_menu_event.log", line);
            let _ = conn.send(msg.method_return());
        }
        _ => {
            let name =
                dbus::strings::ErrorName::new("org.freedesktop.DBus.Error.UnknownMethod").unwrap();
            let text = std::ffi::CString::new("nope").unwrap();
            let _ = conn.send(msg.error(&name, &text));
        }
    }
    true
}

fn main() {
    let conn = Connection::new_session().expect("session bus");
    let color = Arc::new(Mutex::new((0u8, 200u8, 0u8)));

    {
        let color = color.clone();
        let rule = MatchRule::new_method_call().with_path(PATH);
        let _tok = conn.start_receive(
            rule,
            Box::new(move |msg: Message, conn: &Connection| -> bool {
                let iface = msg.interface().map(|i| i.to_string()).unwrap_or_default();
                let member = msg.member().map(|m| m.to_string()).unwrap_or_default();
                if iface == PROPS_IFACE && member == "GetAll" {
                    let c = *color.lock().unwrap();
                    let _ = conn.send(msg.method_return().append1(props(c)));
                } else if iface == IFACE && member == "Activate" {
                    let line = format!("ACTIVATE {:?}\n", std::time::SystemTime::now());
                    let _ = std::fs::write("/tmp/opencode/sni_activate.log", line);
                    let _ = conn.send(msg.method_return());
                } else if iface == IFACE && member == "ContextMenu" {
                    let _ = std::fs::write("/tmp/opencode/sni_activate.log", "CONTEXTMENU\n");
                    let _ = conn.send(msg.method_return());
                } else {
                    let name =
                        dbus::strings::ErrorName::new("org.freedesktop.DBus.Error.UnknownMethod")
                            .unwrap();
                    let text = std::ffi::CString::new("nope").unwrap();
                    let _ = conn.send(msg.error(&name, &text));
                }
                true
            }),
        );
    }

    {
        let rule = MatchRule::new_method_call().with_path(MENU_PATH);
        let _tok = conn.start_receive(
            rule,
            Box::new(move |msg: Message, conn: &Connection| -> bool {
                let iface = msg.interface().map(|i| i.to_string()).unwrap_or_default();
                if iface == MENU_IFACE {
                    return handle_menu(msg, conn);
                }
                let name =
                    dbus::strings::ErrorName::new("org.freedesktop.DBus.Error.UnknownMethod")
                        .unwrap();
                let text = std::ffi::CString::new("nope").unwrap();
                let _ = conn.send(msg.error(&name, &text));
                true
            }),
        );
    }

    let me_name = conn.unique_name().to_string();
    let proxy = Proxy::new(
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        Duration::from_secs(2),
        &conn,
    );
    let _: Result<(), dbus::Error> = proxy.method_call(
        "org.kde.StatusNotifierWatcher",
        "RegisterStatusNotifierItem",
        (me_name,),
    );
    eprintln!("sni-test registered");

    let start = std::time::Instant::now();
    let mut switched = false;
    loop {
        let _ = conn.process(Duration::from_millis(200));
        if !switched && start.elapsed() > Duration::from_secs(4) {
            switched = true;
            *color.lock().unwrap() = (200, 0, 0);
            let p = dbus::strings::Path::new(PATH).unwrap();
            let i = dbus::strings::Interface::new(IFACE).unwrap();
            let m = dbus::strings::Member::new("NewIcon").unwrap();
            let _ = conn.send(Message::signal(&p, &i, &m));
            eprintln!("sni-test switched to red + NewIcon");
        }
    }
}

//! Minimal `org.kde.StatusNotifierWatcher` D-Bus server using `zbus`.
//!
//! Adapted from `cprussin/domicile` (`packages/domicile-host/src/tray.rs`),
//! MIT License — Copyright (c) Connor Prussin (https://github.com/cprussin/domicile).

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use gtk4::gdk;
use gtk4::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{IconLookupFlags, IconTheme, TextDirection};
use serde::Serialize;
use zbus::blocking::connection::Builder as ConnectionBuilder;
use zbus::blocking::Connection;
use zbus::interface;
use zbus::message::Header;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::dbus_bridge::{BridgeEvent, SendEvent};

#[derive(Debug, Clone, Serialize)]
pub struct TrayItem {
    pub bus: String,
    pub path: String,
    pub title: String,
    pub icon_name: String,
    pub icon_url: String,
    pub menu_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrayMenuItem {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub item_type: String,
    pub toggle_type: String,
    pub toggle_state: i32,
    pub children: Vec<TrayMenuItem>,
}

#[derive(Clone)]
struct TrayStore {
    items: Arc<Mutex<Vec<TrayItem>>>,
    send: SendEvent,
}

impl TrayStore {
    fn new(send: SendEvent) -> Self {
        Self {
            items: Arc::new(Mutex::new(Vec::new())),
            send,
        }
    }

    fn upsert(&self, item: TrayItem) {
        let mut guard = self.items.lock().unwrap();
        if let Some(existing) = guard
            .iter_mut()
            .find(|i| i.bus == item.bus && i.path == item.path)
        {
            *existing = item;
        } else {
            guard.push(item);
        }
        let snapshot = guard.clone();
        drop(guard);
        (self.send)(BridgeEvent::Tray { items: snapshot });
    }

    fn registered_items(&self) -> Vec<String> {
        self.items
            .lock()
            .unwrap()
            .iter()
            .map(|i| format!("{}{}", i.bus, i.path))
            .collect()
    }
}

struct StatusNotifierWatcher {
    store: TrayStore,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl StatusNotifierWatcher {
    fn register_status_notifier_item(&self, service: &str, #[zbus(header)] header: Header<'_>) {
        let (bus, path) = if service.starts_with('/') {
            let sender = header
                .sender()
                .map(|s| s.to_string())
                .unwrap_or_default();
            (sender, service.to_string())
        } else {
            (service.to_string(), "/StatusNotifierItem".to_string())
        };

        if bus.is_empty() {
            return;
        }

        let store = self.store.clone();
        thread::spawn(move || {
            let (title, icon_name, icon_url, menu_path) = fetch_item_properties(&bus, &path);
            store.upsert(TrayItem {
                bus,
                path,
                title,
                icon_name,
                icon_url,
                menu_path,
            });
        });
    }

    fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.store.registered_items()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

fn fetch_item_properties(bus: &str, path: &str) -> (String, String, String, String) {
    let conn = match Connection::session() {
        Ok(c) => c,
        Err(_) => return (String::new(), String::new(), String::new(), String::new()),
    };

    let reply = conn.call_method(
        Some(bus),
        path,
        Some("org.freedesktop.DBus.Properties"),
        "GetAll",
        &("org.kde.StatusNotifierItem",),
    );

    let Ok(msg) = reply else {
        return (String::new(), String::new(), String::new(), String::new());
    };

    let Ok(props) = msg.body().deserialize::<HashMap<String, OwnedValue>>() else {
        return (String::new(), String::new(), String::new(), String::new());
    };

    let id = props
        .get("Id")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    let mut title = props
        .get("Title")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    if title.is_empty() {
        title = id;
    }
    let icon_name = props
        .get("IconName")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    let icon_theme_path = props
        .get("IconThemePath")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    let pixmaps = props
        .get("IconPixmap")
        .and_then(|v| Vec::<(i32, i32, Vec<u8>)>::try_from(v.clone()).ok())
        .unwrap_or_default();
    let menu_path = props
        .get("Menu")
        .and_then(|v| OwnedObjectPath::try_from(v.clone()).ok())
        .map(|p| p.to_string())
        .unwrap_or_default();

    let icon_url = resolve_icon_url(&icon_name, &icon_theme_path, pixmaps);
    (title, icon_name, icon_url, menu_path)
}

fn resolve_icon_url(
    icon_name: &str,
    icon_theme_path: &str,
    pixmaps: Vec<(i32, i32, Vec<u8>)>,
) -> String {
    if !icon_name.is_empty() {
        if Path::new(icon_name).is_file() {
            if let Some(url) = file_to_data_url(Path::new(icon_name)) {
                return url;
            }
        }
        if let Some(url) = lookup_theme_icon_data_url(icon_name, icon_theme_path) {
            return url;
        }
    }

    if let Some((w, h, argb)) = select_best_pixmap(pixmaps) {
        if let Some(url) = argb_pixmap_to_png_data_url(w, h, &argb) {
            return url;
        }
    }

    String::new()
}

fn select_best_pixmap(pixmaps: Vec<(i32, i32, Vec<u8>)>) -> Option<(i32, i32, Vec<u8>)> {
    pixmaps
        .into_iter()
        .filter(|(w, h, bytes)| *w > 0 && *h > 0 && bytes.len() == (*w as usize) * (*h as usize) * 4)
        .max_by_key(|(w, h, _)| (*w).min(*h))
}

fn argb_pixmap_to_png_data_url(width: i32, height: i32, argb: &[u8]) -> Option<String> {
    let mut rgba = Vec::with_capacity(argb.len());
    for px in argb.chunks_exact(4) {
        rgba.push(px[1]);
        rgba.push(px[2]);
        rgba.push(px[3]);
        rgba.push(px[0]);
    }
    let pixbuf = Pixbuf::from_bytes(
        &glib::Bytes::from_owned(rgba),
        Colorspace::Rgb,
        true,
        8,
        width,
        height,
        width * 4,
    );
    let png = pixbuf.save_to_bufferv("png", &[]).ok()?;
    Some(format!("data:image/png;base64,{}", BASE64.encode(png)))
}

fn file_to_data_url(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let mime = match path.extension().and_then(|e| e.to_str()) {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("xpm") => "image/x-xpixmap",
        _ => "application/octet-stream",
    };
    Some(format!("data:{mime};base64,{}", BASE64.encode(bytes)))
}

fn lookup_theme_icon_data_url(icon_name: &str, icon_theme_path: &str) -> Option<String> {
    let (tx, rx) = mpsc::sync_channel(1);
    let name = icon_name.to_string();
    let theme_path = icon_theme_path.to_string();

    glib::idle_add_once(move || {
        let theme = gdk::Display::default()
            .map(|d| IconTheme::for_display(&d))
            .unwrap_or_else(IconTheme::new);
        if !theme_path.is_empty() && !theme.search_path().iter().any(|p| p == Path::new(&theme_path))
        {
            theme.add_search_path(&theme_path);
        }
        let paintable = theme.lookup_icon(
            &name,
            &[],
            24,
            1,
            TextDirection::None,
            IconLookupFlags::empty(),
        );
        let file_path = paintable.file().and_then(|f| f.path());
        let _ = tx.send(file_path.and_then(|p| file_to_data_url(&p)));
    });

    rx.recv().ok().flatten()
}

pub fn spawn_server(send: SendEvent) {
    thread::spawn(move || {
        let watcher = StatusNotifierWatcher {
            store: TrayStore::new(send),
        };
        match ConnectionBuilder::session()
            .and_then(|b| b.name("org.kde.StatusNotifierWatcher"))
            .and_then(|b| b.serve_at("/StatusNotifierWatcher", watcher))
            .and_then(|b| b.build())
        {
            Ok(_conn) => loop {
                thread::park();
            },
            Err(err) => {
                eprintln!(
                    "webbar: could not claim org.kde.StatusNotifierWatcher ({err}). \
                     Another StatusNotifierWatcher may already be running."
                );
            }
        }
    });
}

pub fn activate(bus: String, path: String) {
    thread::spawn(move || {
        let conn = match Connection::session() {
            Ok(c) => c,
            Err(err) => {
                eprintln!("webbar: failed to connect to session bus for tray activate: {err}");
                return;
            }
        };
        if let Err(err) = conn.call_method(
            Some(bus.as_str()),
            path.as_str(),
            Some("org.kde.StatusNotifierItem"),
            "Activate",
            &(0i32, 0i32),
        ) {
            eprintln!("webbar: failed to activate tray item {bus}{path}: {err}");
        }
    });
}

type RawMenuNode = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

fn strip_mnemonic(raw: &str) -> String {
    raw.replace("__", "\x00")
        .replace('_', "")
        .replace('\x00', "_")
}

fn parse_menu_node(node: RawMenuNode) -> TrayMenuItem {
    let (id, props, raw_children) = node;
    let label = props
        .get("label")
        .and_then(|v| String::try_from(v.clone()).ok())
        .map(|s| strip_mnemonic(&s))
        .unwrap_or_default();
    let enabled = props
        .get("enabled")
        .and_then(|v| bool::try_from(v.clone()).ok())
        .unwrap_or(true);
    let visible = props
        .get("visible")
        .and_then(|v| bool::try_from(v.clone()).ok())
        .unwrap_or(true);
    let item_type = props
        .get("type")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_else(|| "standard".to_string());
    let toggle_type = props
        .get("toggle-type")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    let toggle_state = props
        .get("toggle-state")
        .and_then(|v| i32::try_from(v.clone()).ok())
        .unwrap_or(-1);

    let children = raw_children
        .into_iter()
        .filter_map(|child_val| RawMenuNode::try_from(child_val).ok())
        .map(parse_menu_node)
        .filter(|item| item.visible)
        .collect();

    TrayMenuItem {
        id,
        label,
        enabled,
        visible,
        item_type,
        toggle_type,
        toggle_state,
        children,
    }
}

pub fn get_menu(id: u32, bus: String, menu_path: String, send: SendEvent) {
    thread::spawn(move || {
        let send_err = |msg: String| {
            send(BridgeEvent::TrayMenuReply {
                id,
                ok: false,
                items: Vec::new(),
                error: Some(msg),
            });
        };

        let conn = match Connection::session() {
            Ok(c) => c,
            Err(err) => return send_err(err.to_string()),
        };

        let _ = conn.call_method(
            Some(bus.as_str()),
            menu_path.as_str(),
            Some("com.canonical.dbusmenu"),
            "AboutToShow",
            &(0i32,),
        );

        let reply = conn.call_method(
            Some(bus.as_str()),
            menu_path.as_str(),
            Some("com.canonical.dbusmenu"),
            "GetLayout",
            &(0i32, -1i32, Vec::<&str>::new()),
        );

        let msg = match reply {
            Ok(m) => m,
            Err(err) => return send_err(err.to_string()),
        };

        let (_rev, root): (u32, RawMenuNode) = match msg.body().deserialize() {
            Ok(parsed) => parsed,
            Err(err) => return send_err(err.to_string()),
        };

        let parsed_root = parse_menu_node(root);
        send(BridgeEvent::TrayMenuReply {
            id,
            ok: true,
            items: parsed_root.children,
            error: None,
        });
    });
}

pub fn menu_event(bus: String, menu_path: String, item_id: i32) {
    thread::spawn(move || {
        let conn = match Connection::session() {
            Ok(c) => c,
            Err(err) => {
                eprintln!("webbar: failed to connect to session bus for tray menu event: {err}");
                return;
            }
        };
        if let Err(err) = conn.call_method(
            Some(bus.as_str()),
            menu_path.as_str(),
            Some("com.canonical.dbusmenu"),
            "Event",
            &(item_id, "clicked", Value::from(""), 0u32),
        ) {
            eprintln!("webbar: failed to send tray menu event {bus}{menu_path}#{item_id}: {err}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_dbusmenu_node_and_strips_mnemonics() {
        let mut child_props: HashMap<String, OwnedValue> = HashMap::new();
        child_props.insert(
            "label".to_string(),
            Value::from("Turn Bluetooth _Off").try_into().unwrap(),
        );
        child_props.insert("enabled".to_string(), OwnedValue::from(true));

        let child_tuple: RawMenuNode = (65536, child_props, Vec::new());
        let child_val: OwnedValue = Value::from(child_tuple).try_into().unwrap();

        let root_props: HashMap<String, OwnedValue> = HashMap::new();
        let parsed = parse_menu_node((0, root_props, vec![child_val]));
        assert_eq!(parsed.children.len(), 1);
        assert_eq!(parsed.children[0].id, 65536);
        assert_eq!(parsed.children[0].label, "Turn Bluetooth Off");
        assert!(parsed.children[0].enabled);
    }
}

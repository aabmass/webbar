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
use futures_util::StreamExt;
use zbus::blocking::connection::Builder as ConnectionBuilder;
use zbus::blocking::Connection;
use zbus::interface;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{MatchRule, MessageStream};

use crate::bridge::{BridgeEvent, SendEvent};

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct TrayItem {
    pub bus: String,
    pub path: String,
    pub title: String,
    pub icon_name: String,
    pub icon_url: String,
    pub menu_path: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
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
struct TrayEntry {
    item: TrayItem,
    unique_bus: String,
}

#[derive(Clone)]
struct TrayStore {
    items: Arc<Mutex<Vec<TrayEntry>>>,
    send: SendEvent,
}

impl TrayStore {
    fn new(send: SendEvent) -> Self {
        Self {
            items: Arc::new(Mutex::new(Vec::new())),
            send,
        }
    }

    fn snapshot_locked(entries: &[TrayEntry]) -> Vec<TrayItem> {
        entries.iter().map(|e| e.item.clone()).collect()
    }

    fn upsert(&self, item: TrayItem, unique_bus: String) {
        let mut guard = self.items.lock().unwrap();
        if let Some(existing) = guard
            .iter_mut()
            .find(|e| e.item.bus == item.bus && e.item.path == item.path)
        {
            existing.item = item;
            if !unique_bus.is_empty() {
                existing.unique_bus = unique_bus;
            }
        } else {
            guard.push(TrayEntry { item, unique_bus });
        }
        let snapshot = Self::snapshot_locked(&guard);
        drop(guard);
        (self.send)(BridgeEvent::Tray { items: snapshot });
    }

    fn update_if_present(
        &self,
        bus: &str,
        path: &str,
        title: String,
        icon_name: String,
        icon_url: String,
        menu_path: String,
    ) -> bool {
        let mut guard = self.items.lock().unwrap();
        let Some(existing) = guard
            .iter_mut()
            .find(|e| e.item.bus == bus && e.item.path == path)
        else {
            return false;
        };

        if existing.item.title == title
            && existing.item.icon_name == icon_name
            && existing.item.icon_url == icon_url
            && existing.item.menu_path == menu_path
        {
            return false;
        }

        existing.item.title = title;
        existing.item.icon_name = icon_name;
        existing.item.icon_url = icon_url;
        existing.item.menu_path = menu_path;

        let snapshot = Self::snapshot_locked(&guard);
        drop(guard);
        (self.send)(BridgeEvent::Tray { items: snapshot });
        true
    }

    fn find_target(&self, sender: &str, path: &str) -> Option<(String, String)> {
        self.items
            .lock()
            .unwrap()
            .iter()
            .find(|e| (e.item.bus == sender || e.unique_bus == sender) && e.item.path == path)
            .map(|e| (e.item.bus.clone(), e.item.path.clone()))
    }

    fn remove_by_bus(&self, bus: &str) -> Vec<String> {
        let mut guard = self.items.lock().unwrap();
        let mut removed = Vec::new();
        guard.retain(|entry| {
            if entry.item.bus == bus || entry.unique_bus == bus {
                removed.push(format!("{}{}", entry.item.bus, entry.item.path));
                false
            } else {
                true
            }
        });
        if removed.is_empty() {
            return removed;
        }
        let snapshot = Self::snapshot_locked(&guard);
        drop(guard);
        (self.send)(BridgeEvent::Tray { items: snapshot });
        removed
    }

    fn registered_items(&self) -> Vec<String> {
        self.items
            .lock()
            .unwrap()
            .iter()
            .map(|e| format!("{}{}", e.item.bus, e.item.path))
            .collect()
    }
}

struct StatusNotifierWatcher {
    store: TrayStore,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl StatusNotifierWatcher {
    fn register_status_notifier_item(&self, service: &str, #[zbus(header)] header: Header<'_>) {
        let sender = header
            .sender()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let (bus, path) = if service.starts_with('/') {
            (sender.clone(), service.to_string())
        } else {
            (service.to_string(), "/StatusNotifierItem".to_string())
        };

        if bus.is_empty() {
            return;
        }

        let unique_bus = if bus.starts_with(':') {
            bus.clone()
        } else {
            sender
        };

        let store = self.store.clone();
        thread::spawn(move || {
            let (title, icon_name, icon_url, menu_path) = fetch_item_properties(&bus, &path);
            store.upsert(
                TrayItem {
                    bus,
                    path,
                    title,
                    icon_name,
                    icon_url,
                    menu_path,
                },
                unique_bus,
            );
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

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
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
    for px in argb.as_chunks::<4>().0 {
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
            .unwrap_or_default();
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

fn handle_name_owner_changed(store: &TrayStore, name: &str, new_owner: &str) -> Vec<String> {
    if !new_owner.is_empty() {
        return Vec::new();
    }
    store.remove_by_bus(name)
}

fn should_refresh_sni_signal(msg: &zbus::Message) -> bool {
    let header = msg.header();
    let iface = header.interface().map(|i| i.as_str());
    let member = header.member().map(|m| m.as_str());

    match (iface, member) {
        (
            Some("org.kde.StatusNotifierItem"),
            Some("NewIcon" | "NewAttentionIcon" | "NewOverlayIcon" | "NewTitle" | "NewStatus"),
        ) => true,
        (Some("org.freedesktop.DBus.Properties"), Some("PropertiesChanged")) => {
            matches!(
                msg.body()
                    .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>(),
                Ok((changed_iface, _, _)) if changed_iface == "org.kde.StatusNotifierItem"
            )
        }
        _ => false,
    }
}

fn spawn_item_refresh(store: TrayStore, bus: String, path: String) {
    thread::spawn(move || {
        let (title, icon_name, icon_url, menu_path) = fetch_item_properties(&bus, &path);
        store.update_if_present(&bus, &path, title, icon_name, icon_url, menu_path);
    });
}

pub fn spawn_server(send: SendEvent) {
    thread::spawn(move || {
        let store = TrayStore::new(send);
        let watcher = StatusNotifierWatcher {
            store: store.clone(),
        };
        let conn = match ConnectionBuilder::session()
            .and_then(|b| b.name("org.kde.StatusNotifierWatcher"))
            .and_then(|b| b.serve_at("/StatusNotifierWatcher", watcher))
            .and_then(|b| b.build())
        {
            Ok(conn) => conn,
            Err(err) => {
                eprintln!(
                    "webbar: could not claim org.kde.StatusNotifierWatcher ({err}). \
                     Another StatusNotifierWatcher may already be running."
                );
                return;
            }
        };

        let Ok(name_owner_rule) = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|b| b.path("/org/freedesktop/DBus"))
            .and_then(|b| b.interface("org.freedesktop.DBus"))
            .and_then(|b| b.member("NameOwnerChanged"))
            .map(|b| b.build())
        else {
            loop {
                thread::park();
            }
        };

        let sni_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.kde.StatusNotifierItem")
            .map(|b| b.build())
            .ok();

        let props_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .and_then(|b| b.member("PropertiesChanged"))
            .and_then(|b| b.arg(0, "org.kde.StatusNotifierItem"))
            .map(|b| b.build())
            .ok();

        async_io::block_on(async {
            let mut streams = Vec::new();
            let Ok(name_stream) =
                MessageStream::for_match_rule(name_owner_rule, conn.inner(), Some(64)).await
            else {
                return;
            };
            streams.push(name_stream);

            if let Some(rule) = sni_rule {
                if let Ok(stream) =
                    MessageStream::for_match_rule(rule, conn.inner(), Some(64)).await
                {
                    streams.push(stream);
                }
            }

            if let Some(rule) = props_rule {
                if let Ok(stream) =
                    MessageStream::for_match_rule(rule, conn.inner(), Some(64)).await
                {
                    streams.push(stream);
                }
            }

            let mut combined = futures_util::stream::select_all(streams);

            while let Some(Ok(msg)) = combined.next().await {
                let header = msg.header();
                if header.interface().map(|i| i.as_str()) == Some("org.freedesktop.DBus")
                    && header.member().map(|m| m.as_str()) == Some("NameOwnerChanged")
                {
                    if let Ok((name, _old_owner, new_owner)) =
                        msg.body().deserialize::<(String, String, String)>()
                    {
                        let removed = handle_name_owner_changed(&store, &name, &new_owner);
                        if !removed.is_empty() {
                            if let Ok(iface) = conn
                                .object_server()
                                .interface::<_, StatusNotifierWatcher>("/StatusNotifierWatcher")
                            {
                                for service in removed {
                                    let _ =
                                        StatusNotifierWatcher::status_notifier_item_unregistered(
                                            iface.signal_emitter(),
                                            &service,
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                    continue;
                }

                if should_refresh_sni_signal(&msg) {
                    let Some(sender) = header.sender().map(|s| s.as_str()) else {
                        continue;
                    };
                    let Some(path) = header.path().map(|p| p.as_str()) else {
                        continue;
                    };
                    if let Some((bus, item_path)) = store.find_target(sender, path) {
                        spawn_item_refresh(store.clone(), bus, item_path);
                    }
                }
            }
        });

        loop {
            thread::park();
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
            crate::bridge::reply_err(&send, id, msg);
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
        crate::bridge::reply_data(&send, id, parsed_root.children);
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

    #[test]
    fn removes_tray_items_when_registered_bus_loses_owner() {
        let events: Arc<Mutex<Vec<BridgeEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let send: SendEvent = Arc::new(move |ev| {
            events_clone.lock().unwrap().push(ev);
        });
        let store = TrayStore::new(send);

        store.upsert(
            TrayItem {
                bus: ":1.42".to_string(),
                path: "/StatusNotifierItem".to_string(),
                title: "Stasis".to_string(),
                icon_name: "stasis".to_string(),
                icon_url: String::new(),
                menu_path: "/MenuBar".to_string(),
            },
            ":1.42".to_string(),
        );
        store.upsert(
            TrayItem {
                bus: ":1.99".to_string(),
                path: "/org/ayatana/NotificationItem/foo/Menu".to_string(),
                title: "Other".to_string(),
                icon_name: "other".to_string(),
                icon_url: String::new(),
                menu_path: String::new(),
            },
            ":1.99".to_string(),
        );
        assert_eq!(events.lock().unwrap().len(), 2);

        // Non-empty new_owner (ownership acquired or transferred) must not remove items.
        assert!(handle_name_owner_changed(&store, ":1.42", ":1.100").is_empty());
        assert_eq!(store.registered_items().len(), 2);
        assert_eq!(events.lock().unwrap().len(), 2);

        // Empty new_owner means the bus disconnected/exited.
        let removed = handle_name_owner_changed(&store, ":1.42", "");
        assert_eq!(removed, vec![":1.42/StatusNotifierItem".to_string()]);
        assert_eq!(
            store.registered_items(),
            vec![":1.99/org/ayatana/NotificationItem/foo/Menu".to_string()]
        );

        let guard = events.lock().unwrap();
        assert_eq!(guard.len(), 3);
        match guard.last().unwrap() {
            BridgeEvent::Tray { items } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].bus, ":1.99");
            }
            other => panic!("expected BridgeEvent::Tray, got {other:?}"),
        }
        drop(guard);

        // Repeating for an unknown/already-removed bus is a no-op with no extra event.
        assert!(handle_name_owner_changed(&store, ":1.42", "").is_empty());
        assert_eq!(events.lock().unwrap().len(), 3);
    }

    #[test]
    fn finds_and_updates_existing_tray_item_without_resurrecting_removed_ones() {
        let events: Arc<Mutex<Vec<BridgeEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let send: SendEvent = Arc::new(move |ev| {
            events_clone.lock().unwrap().push(ev);
        });
        let store = TrayStore::new(send);

        store.upsert(
            TrayItem {
                bus: "org.kde.StatusNotifierItem-100-1".to_string(),
                path: "/StatusNotifierItem".to_string(),
                title: "NetworkManager Applet".to_string(),
                icon_name: "nm-no-connection".to_string(),
                icon_url: "data:image/png;base64,AAA".to_string(),
                menu_path: "/org/ayatana/NotificationItem/nm_applet/Menu".to_string(),
            },
            ":1.55".to_string(),
        );
        assert_eq!(events.lock().unwrap().len(), 1);

        // Matches by either unique sender (:1.55) or registered bus name.
        assert_eq!(
            store.find_target(":1.55", "/StatusNotifierItem"),
            Some((
                "org.kde.StatusNotifierItem-100-1".to_string(),
                "/StatusNotifierItem".to_string()
            ))
        );
        assert_eq!(
            store.find_target("org.kde.StatusNotifierItem-100-1", "/StatusNotifierItem"),
            Some((
                "org.kde.StatusNotifierItem-100-1".to_string(),
                "/StatusNotifierItem".to_string()
            ))
        );
        assert_eq!(store.find_target(":1.55", "/OtherPath"), None);

        // No-op update does not emit a duplicate event.
        assert!(!store.update_if_present(
            "org.kde.StatusNotifierItem-100-1",
            "/StatusNotifierItem",
            "NetworkManager Applet".to_string(),
            "nm-no-connection".to_string(),
            "data:image/png;base64,AAA".to_string(),
            "/org/ayatana/NotificationItem/nm_applet/Menu".to_string(),
        ));
        assert_eq!(events.lock().unwrap().len(), 1);

        // Icon update emits a new snapshot with the updated icon.
        assert!(store.update_if_present(
            "org.kde.StatusNotifierItem-100-1",
            "/StatusNotifierItem",
            "NetworkManager Applet".to_string(),
            "nm-signal-75".to_string(),
            "data:image/png;base64,BBB".to_string(),
            "/org/ayatana/NotificationItem/nm_applet/Menu".to_string(),
        ));
        assert_eq!(events.lock().unwrap().len(), 2);
        match events.lock().unwrap().last().unwrap() {
            BridgeEvent::Tray { items } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].icon_name, "nm-signal-75");
                assert_eq!(items[0].icon_url, "data:image/png;base64,BBB");
            }
            other => panic!("expected BridgeEvent::Tray, got {other:?}"),
        }

        // Removing by unique_bus also removes well-known-name items, and
        // update_if_present must not resurrect removed items.
        assert_eq!(
            handle_name_owner_changed(&store, ":1.55", ""),
            vec!["org.kde.StatusNotifierItem-100-1/StatusNotifierItem".to_string()]
        );
        assert!(!store.update_if_present(
            "org.kde.StatusNotifierItem-100-1",
            "/StatusNotifierItem",
            "NetworkManager Applet".to_string(),
            "nm-signal-100".to_string(),
            "data:image/png;base64,CCC".to_string(),
            "/org/ayatana/NotificationItem/nm_applet/Menu".to_string(),
        ));
        assert!(store.registered_items().is_empty());
    }
}

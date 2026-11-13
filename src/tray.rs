//! Minimal `org.kde.StatusNotifierWatcher` D-Bus server using `zbus`.
//!
//! Adapted from `cprussin/domicile` (`packages/domicile-host/src/tray.rs`),
//! MIT License — Copyright (c) Connor Prussin (https://github.com/cprussin/domicile).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Serialize;
use zbus::blocking::connection::Builder as ConnectionBuilder;
use zbus::blocking::Connection;
use zbus::interface;
use zbus::message::Header;
use zbus::zvariant::OwnedValue;

use crate::dbus_bridge::{BridgeEvent, SendEvent};

#[derive(Debug, Clone, Serialize)]
pub struct TrayItem {
    pub bus: String,
    pub path: String,
    pub title: String,
    pub icon_name: String,
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
            let (title, icon_name) = fetch_item_properties(&bus, &path);
            store.upsert(TrayItem {
                bus,
                path,
                title,
                icon_name,
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

fn fetch_item_properties(bus: &str, path: &str) -> (String, String) {
    let conn = match Connection::session() {
        Ok(c) => c,
        Err(_) => return (String::new(), String::new()),
    };

    let reply = conn.call_method(
        Some(bus),
        path,
        Some("org.freedesktop.DBus.Properties"),
        "GetAll",
        &("org.kde.StatusNotifierItem",),
    );

    let Ok(msg) = reply else {
        return (String::new(), String::new());
    };

    let Ok(props) = msg.body().deserialize::<HashMap<String, OwnedValue>>() else {
        return (String::new(), String::new());
    };

    let title = props
        .get("Title")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    let icon_name = props
        .get("IconName")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();

    (title, icon_name)
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

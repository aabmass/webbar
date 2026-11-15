//! Minimal `org.freedesktop.Notifications` D-Bus server using `zbus`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use serde::Serialize;
use zbus::blocking::connection::Builder as ConnectionBuilder;
use zbus::blocking::Connection;
use zbus::interface;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};

use crate::bridge::{BridgeEvent, SendEvent};

const MAX_HISTORY: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
#[serde(rename_all = "lowercase")]
pub enum NotificationUrgency {
    Low,
    Normal,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct NotificationAction {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct NotificationItem {
    pub id: u32,
    pub app_name: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
    pub urgency: NotificationUrgency,
    pub actions: Vec<NotificationAction>,
}

#[derive(Clone)]
pub struct NotificationStore {
    next_id: Arc<AtomicU32>,
    items: Arc<Mutex<Vec<NotificationItem>>>,
    send: SendEvent,
    conn: Arc<OnceLock<Connection>>,
}

pub fn parse_urgency(hints: &HashMap<String, OwnedValue>) -> NotificationUrgency {
    let raw = hints.get("urgency").and_then(|v| match &**v {
        Value::U8(n) => Some(*n),
        Value::I32(n) => u8::try_from(*n).ok(),
        Value::U32(n) => u8::try_from(*n).ok(),
        _ => None,
    });
    match raw {
        Some(0) => NotificationUrgency::Low,
        Some(2) => NotificationUrgency::Critical,
        _ => NotificationUrgency::Normal,
    }
}

pub fn parse_actions(raw: &[String]) -> Vec<NotificationAction> {
    raw.chunks_exact(2)
        .filter(|pair| !pair[0].is_empty())
        .map(|pair| NotificationAction {
            key: pair[0].clone(),
            label: pair[1].clone(),
        })
        .collect()
}

impl NotificationStore {
    pub fn new(send: SendEvent) -> Self {
        Self {
            next_id: Arc::new(AtomicU32::new(1)),
            items: Arc::new(Mutex::new(Vec::new())),
            send,
            conn: Arc::new(OnceLock::new()),
        }
    }

    pub fn push(&self, replaces_id: u32, app_name: String, summary: String, body: String) -> u32 {
        self.push_full(
            replaces_id,
            app_name,
            String::new(),
            summary,
            body,
            NotificationUrgency::Normal,
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_full(
        &self,
        replaces_id: u32,
        app_name: String,
        icon: String,
        summary: String,
        body: String,
        urgency: NotificationUrgency,
        actions: Vec<NotificationAction>,
    ) -> u32 {
        let mut items = self.items.lock().unwrap();
        let id = if replaces_id != 0 {
            if let Some(existing) = items.iter_mut().find(|n| n.id == replaces_id) {
                existing.app_name = app_name;
                existing.icon = icon;
                existing.summary = summary;
                existing.body = body;
                existing.urgency = urgency;
                existing.actions = actions;
                let snapshot = items.clone();
                drop(items);
                (self.send)(BridgeEvent::Notifications { items: snapshot });
                return replaces_id;
            }
            replaces_id
        } else {
            self.next_id.fetch_add(1, Ordering::Relaxed)
        };

        items.push(NotificationItem {
            id,
            app_name,
            icon,
            summary,
            body,
            urgency,
            actions,
        });
        if items.len() > MAX_HISTORY {
            let drain = items.len() - MAX_HISTORY;
            items.drain(0..drain);
        }
        let snapshot = items.clone();
        drop(items);
        (self.send)(BridgeEvent::Notifications { items: snapshot });
        id
    }

    pub fn dismiss(&self, id: u32) -> bool {
        let mut items = self.items.lock().unwrap();
        let before = items.len();
        items.retain(|n| n.id != id);
        let removed = items.len() != before;
        if removed {
            let snapshot = items.clone();
            drop(items);
            (self.send)(BridgeEvent::Notifications { items: snapshot });
            self.emit_closed(id, 2);
        }
        removed
    }

    pub fn invoke_action(&self, id: u32, action_key: &str) {
        if let Some(conn) = self.conn.get() {
            if let Ok(iface) = conn
                .object_server()
                .interface::<_, NotificationsServer>("/org/freedesktop/Notifications")
            {
                let _ = async_io::block_on(NotificationsServer::action_invoked(
                    iface.signal_emitter(),
                    id,
                    action_key,
                ));
            }
        }
    }

    fn emit_closed(&self, id: u32, reason: u32) {
        if let Some(conn) = self.conn.get() {
            if let Ok(iface) = conn
                .object_server()
                .interface::<_, NotificationsServer>("/org/freedesktop/Notifications")
            {
                let _ = async_io::block_on(NotificationsServer::notification_closed(
                    iface.signal_emitter(),
                    id,
                    reason,
                ));
            }
        }
    }

    pub fn clear_all(&self) {
        let mut items = self.items.lock().unwrap();
        if !items.is_empty() {
            let ids: Vec<u32> = items.iter().map(|n| n.id).collect();
            items.clear();
            drop(items);
            (self.send)(BridgeEvent::Notifications { items: Vec::new() });
            for id in ids {
                self.emit_closed(id, 2);
            }
        }
    }
}

struct NotificationsServer {
    store: NotificationStore,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationsServer {
    fn get_capabilities(&self) -> Vec<String> {
        vec![
            "body".to_string(),
            "persistence".to_string(),
            "actions".to_string(),
        ]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        _expire_timeout: i32,
    ) -> u32 {
        self.store.push_full(
            replaces_id,
            app_name,
            app_icon,
            summary,
            body,
            parse_urgency(&hints),
            parse_actions(&actions),
        )
    }

    fn close_notification(&self, id: u32) {
        self.store.dismiss(id);
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "webbar".to_string(),
            "webbar".to_string(),
            "0.1.0".to_string(),
            "1.2".to_string(),
        )
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

pub fn spawn_server(store: NotificationStore) {
    thread::spawn(move || {
        let server = NotificationsServer {
            store: store.clone(),
        };
        match ConnectionBuilder::session()
            .and_then(|b| b.name("org.freedesktop.Notifications"))
            .and_then(|b| b.serve_at("/org/freedesktop/Notifications", server))
            .and_then(|b| b.build())
        {
            Ok(conn) => {
                let _ = store.conn.set(conn);
                // Keep the connection alive for the lifetime of the bar.
                loop {
                    thread::park();
                }
            }
            Err(err) => {
                eprintln!(
                    "webbar: could not claim org.freedesktop.Notifications ({err}). \
                     If another daemon (mako/dunst/swaync) is running, stop it first to see notifications here."
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture() -> (SendEvent, Arc<Mutex<Vec<Vec<NotificationItem>>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let log_clone = log.clone();
        let send: SendEvent = Arc::new(move |event| {
            if let BridgeEvent::Notifications { items } = event {
                log_clone.lock().unwrap().push(items);
            }
        });
        (send, log)
    }

    #[test]
    fn assigns_sequential_ids_and_emits_snapshots() {
        let (send, log) = capture();
        let store = NotificationStore::new(send);

        let id1 = store.push(0, "app1".into(), "sum1".into(), "body1".into());
        let id2 = store.push(0, "app2".into(), "sum2".into(), "body2".into());
        let id3 = store.push(0, "app3".into(), "sum3".into(), "body3".into());

        assert_eq!((id1, id2, id3), (1, 2, 3));

        let snapshots = log.lock().unwrap();
        assert_eq!(snapshots.len(), 3);
        let ids: Vec<u32> = snapshots[2].iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn replaces_id_updates_in_place_without_duplicate() {
        let (send, log) = capture();
        let store = NotificationStore::new(send);

        let id1 = store.push(0, "mail".into(), "New mail".into(), "From Alice".into());
        let id2 = store.push(0, "chat".into(), "Ping".into(), "Hey".into());
        let replaced = store.push(
            id1,
            "mail".into(),
            "2 new mails".into(),
            "From Alice & Bob".into(),
        );

        assert_eq!(replaced, id1);

        let snapshots = log.lock().unwrap();
        let latest = snapshots.last().unwrap();
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0].id, id1);
        assert_eq!(latest[0].summary, "2 new mails");
        assert_eq!(latest[0].body, "From Alice & Bob");
        assert_eq!(latest[1].id, id2);
    }

    #[test]
    fn dismiss_returns_whether_found_and_emits_only_when_removed() {
        let (send, log) = capture();
        let store = NotificationStore::new(send);

        assert!(!store.dismiss(42));
        assert_eq!(log.lock().unwrap().len(), 0);

        let id1 = store.push(0, "a".into(), "s1".into(), "b1".into());
        let id2 = store.push(0, "b".into(), "s2".into(), "b2".into());

        assert!(!store.dismiss(999));
        assert_eq!(log.lock().unwrap().len(), 2);

        assert!(store.dismiss(id1));
        assert!(!store.dismiss(id1));

        let snapshots = log.lock().unwrap();
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[2].len(), 1);
        assert_eq!(snapshots[2][0].id, id2);
    }

    #[test]
    fn clear_all_is_noop_when_empty_and_clears_when_populated() {
        let (send, log) = capture();
        let store = NotificationStore::new(send);

        store.clear_all();
        assert_eq!(log.lock().unwrap().len(), 0);

        store.push(0, "app".into(), "sum".into(), "body".into());
        store.clear_all();
        store.clear_all();

        let snapshots = log.lock().unwrap();
        assert_eq!(snapshots.len(), 2);
        assert!(snapshots[1].is_empty());
    }

    #[test]
    fn caps_history_at_max_history_dropping_oldest() {
        let (send, log) = capture();
        let store = NotificationStore::new(send);

        let total = MAX_HISTORY + 5;
        for i in 0..total {
            store.push(0, "app".into(), format!("sum {i}"), String::new());
        }

        let snapshots = log.lock().unwrap();
        let latest = snapshots.last().unwrap();
        assert_eq!(latest.len(), MAX_HISTORY);
        assert_eq!(latest.first().unwrap().id, 6);
        assert_eq!(latest.last().unwrap().id, total as u32);
    }

    #[test]
    fn parses_urgency_and_action_pairs() {
        let mut hints = HashMap::new();
        assert_eq!(parse_urgency(&hints), NotificationUrgency::Normal);

        hints.insert("urgency".into(), OwnedValue::from(0u8));
        assert_eq!(parse_urgency(&hints), NotificationUrgency::Low);

        hints.insert("urgency".into(), OwnedValue::from(2u8));
        assert_eq!(parse_urgency(&hints), NotificationUrgency::Critical);

        let actions = parse_actions(&[
            "default".into(),
            "Open".into(),
            "reply".into(),
            "Reply".into(),
            "dangling".into(),
        ]);
        assert_eq!(
            actions,
            vec![
                NotificationAction {
                    key: "default".into(),
                    label: "Open".into(),
                },
                NotificationAction {
                    key: "reply".into(),
                    label: "Reply".into(),
                },
            ]
        );
    }
}

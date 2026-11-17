//! Shared bridge protocol types and latest-snapshot cache for WebView replay.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bus {
    Session,
    System,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct InputRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct SwayState {
    pub workspaces: Vec<crate::sway_ipc::SwayWorkspace>,
    pub focused_title: String,
    pub mode: String,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct AudioState {
    pub available: bool,
    pub outputs: Vec<crate::audio::AudioDevice>,
    pub inputs: Vec<crate::audio::AudioDevice>,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct BluetoothState {
    pub available: bool,
    pub adapters: Vec<crate::bluetooth::BluetoothAdapter>,
    pub devices: Vec<crate::bluetooth::BluetoothDevice>,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct NetworkState {
    pub available: bool,
    pub connectivity: crate::network::Connectivity,
    pub link: crate::network::NetworkLink,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct BacklightState {
    pub available: bool,
    pub brightness: Option<u8>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct ExecResult {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeRequest {
    SwayCommand {
        command: String,
    },
    TrayActivate {
        bus: String,
        path: String,
    },
    TrayGetMenu {
        id: u32,
        bus: String,
        menu_path: String,
    },
    TrayMenuEvent {
        bus: String,
        menu_path: String,
        item_id: i32,
    },
    DismissNotification {
        id: u32,
    },
    InvokeNotificationAction {
        id: u32,
        action_key: String,
    },
    ClearNotifications,
    AudioSetVolume {
        id: u32,
        device_id: u32,
        volume: f64,
    },
    AudioSetMuted {
        id: u32,
        device_id: u32,
        muted: bool,
    },
    AudioSetDefault {
        id: u32,
        device_id: u32,
    },
    BluetoothSetPowered {
        id: u32,
        path: String,
        powered: bool,
    },
    BluetoothConnect {
        id: u32,
        path: String,
    },
    BluetoothDisconnect {
        id: u32,
        path: String,
    },
    BacklightSet {
        id: u32,
        percent: f64,
    },
    SetInputRegion {
        rects: Vec<InputRect>,
    },
    SetOverlay {
        active: bool,
    },
    // Escape-hatch primitives
    DbusCall {
        id: u32,
        bus: Bus,
        destination: String,
        path: String,
        interface: String,
        member: String,
        #[serde(default)]
        signature: String,
        #[serde(default = "empty_array_json")]
        body: serde_json::Value,
    },
    DbusMatch {
        id: u32,
        bus: Bus,
        sender: Option<String>,
        path: Option<String>,
        interface: Option<String>,
        member: Option<String>,
    },
    DbusUnwatch {
        id: u32,
    },
    ProcessSpawn {
        id: u32,
        argv: Vec<String>,
        cwd: Option<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    ProcessStreamSpawn {
        id: u32,
        argv: Vec<String>,
        cwd: Option<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    ProcessKill {
        id: u32,
        signal: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessOutputStream {
    Stdout,
    Stderr,
}

fn empty_array_json() -> serde_json::Value {
    serde_json::Value::Array(Vec::new())
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    RpcReply {
        id: u32,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    SwayState {
        workspaces: Vec<crate::sway_ipc::SwayWorkspace>,
        focused_title: String,
        mode: String,
    },
    Tray {
        items: Vec<crate::tray::TrayItem>,
    },
    Notifications {
        items: Vec<crate::notifications::NotificationItem>,
    },
    AudioState {
        available: bool,
        outputs: Vec<crate::audio::AudioDevice>,
        inputs: Vec<crate::audio::AudioDevice>,
    },
    BluetoothState {
        available: bool,
        adapters: Vec<crate::bluetooth::BluetoothAdapter>,
        devices: Vec<crate::bluetooth::BluetoothDevice>,
    },
    NetworkState {
        available: bool,
        connectivity: crate::network::Connectivity,
        link: crate::network::NetworkLink,
    },
    BacklightState {
        available: bool,
        brightness: Option<u8>,
        error: Option<String>,
    },
    // Escape-hatch events
    DbusMatchStarted {
        id: u32,
        ok: bool,
        error: Option<String>,
    },
    DbusSignal {
        id: u32,
        sender: String,
        path: String,
        interface: String,
        member: String,
        signature: String,
        body: serde_json::Value,
    },
    DbusMatchEnded {
        id: u32,
        error: Option<String>,
    },
    ProcessStreamStarted {
        id: u32,
        ok: bool,
        error: Option<String>,
    },
    ProcessStreamOutput {
        id: u32,
        stream: ProcessOutputStream,
        line: String,
    },
    ProcessStreamEnded {
        id: u32,
        code: Option<i32>,
        signal: Option<i32>,
    },
}

pub type SendEvent = Arc<dyn Fn(BridgeEvent) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapshotKey {
    Sway,
    Tray,
    Notifications,
    Audio,
    Bluetooth,
    Network,
    Backlight,
}

impl BridgeEvent {
    pub fn snapshot_key(&self) -> Option<SnapshotKey> {
        match self {
            BridgeEvent::SwayState { .. } => Some(SnapshotKey::Sway),
            BridgeEvent::Tray { .. } => Some(SnapshotKey::Tray),
            BridgeEvent::Notifications { .. } => Some(SnapshotKey::Notifications),
            BridgeEvent::AudioState { .. } => Some(SnapshotKey::Audio),
            BridgeEvent::BluetoothState { .. } => Some(SnapshotKey::Bluetooth),
            BridgeEvent::NetworkState { .. } => Some(SnapshotKey::Network),
            BridgeEvent::BacklightState { .. } => Some(SnapshotKey::Backlight),
            _ => None,
        }
    }
}

#[derive(Clone, Default)]
pub struct SnapshotCache {
    latest: Arc<Mutex<HashMap<SnapshotKey, BridgeEvent>>>,
}

impl SnapshotCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, event: &BridgeEvent) {
        if let Some(key) = event.snapshot_key() {
            if let Ok(mut guard) = self.latest.lock() {
                guard.insert(key, event.clone());
            }
        }
    }

    pub fn snapshot_all(&self) -> Vec<BridgeEvent> {
        let Ok(guard) = self.latest.lock() else {
            return Vec::new();
        };
        let order = [
            SnapshotKey::Sway,
            SnapshotKey::Tray,
            SnapshotKey::Notifications,
            SnapshotKey::Audio,
            SnapshotKey::Bluetooth,
            SnapshotKey::Network,
            SnapshotKey::Backlight,
        ];
        order
            .iter()
            .filter_map(|key| guard.get(key).cloned())
            .collect()
    }
}

pub fn reply_ok(send: &SendEvent, id: u32) {
    send(BridgeEvent::RpcReply {
        id,
        ok: true,
        data: None,
        error: None,
    });
}

pub fn reply_data(send: &SendEvent, id: u32, data: impl Serialize) {
    match serde_json::to_value(data) {
        Ok(val) => send(BridgeEvent::RpcReply {
            id,
            ok: true,
            data: Some(val),
            error: None,
        }),
        Err(err) => reply_err(send, id, err),
    }
}

pub fn reply_err(send: &SendEvent, id: u32, err: impl ToString) {
    send(BridgeEvent::RpcReply {
        id,
        ok: false,
        data: None,
        error: Some(err.to_string()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_bridge_request_and_event() {
        let raw = r#"{"type":"audio_set_volume","id":42,"device_id":7,"volume":0.75}"#;
        let req: BridgeRequest = serde_json::from_str(raw).unwrap();
        match req {
            BridgeRequest::AudioSetVolume {
                id,
                device_id,
                volume,
            } => {
                assert_eq!(id, 42);
                assert_eq!(device_id, 7);
                assert!((volume - 0.75).abs() < f64::EPSILON);
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        let ev = BridgeEvent::RpcReply {
            id: 42,
            ok: true,
            data: None,
            error: None,
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["type"], "rpc_reply");
        assert_eq!(json["id"], 42);
        assert_eq!(json["ok"], true);
    }

    #[test]
    fn snapshot_cache_keeps_latest_in_deterministic_order() {
        let cache = SnapshotCache::new();
        cache.record(&BridgeEvent::BacklightState {
            available: true,
            brightness: Some(50),
            error: None,
        });
        cache.record(&BridgeEvent::BacklightState {
            available: true,
            brightness: Some(80),
            error: None,
        });
        cache.record(&BridgeEvent::SwayState {
            workspaces: Vec::new(),
            focused_title: "term".into(),
            mode: "default".into(),
        });
        cache.record(&BridgeEvent::RpcReply {
            id: 1,
            ok: true,
            data: None,
            error: None,
        });

        let all = cache.snapshot_all();
        assert_eq!(all.len(), 2);
        assert!(matches!(all[0], BridgeEvent::SwayState { .. }));
        assert!(matches!(
            all[1],
            BridgeEvent::BacklightState {
                brightness: Some(80),
                ..
            }
        ));
    }
}

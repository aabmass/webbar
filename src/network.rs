//! NetworkManager state watcher via `zbus` on the system bus.

use std::collections::HashMap;
use std::thread;

use futures_util::stream::StreamExt;
use serde::Serialize;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream};

use crate::bridge::{BridgeEvent, SendEvent};

const NM_BUS: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_IFACE: &str = "org.freedesktop.NetworkManager";
const ACTIVE_CONN_IFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const AP_IFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
#[serde(rename_all = "lowercase")]
pub enum Connectivity {
    Unknown,
    None,
    Portal,
    Limited,
    Full,
}

#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NetworkLink {
    Wifi {
        name: String,
        ssid: String,
        strength: f64,
    },
    Wired {
        name: String,
    },
    Other {
        name: String,
    },
    None,
}

pub fn map_connectivity(raw: u32) -> Connectivity {
    match raw {
        1 => Connectivity::None,
        2 => Connectivity::Portal,
        3 => Connectivity::Limited,
        4 => Connectivity::Full,
        _ => Connectivity::Unknown,
    }
}

pub fn map_link(
    conn_type: Option<&str>,
    name: Option<String>,
    wifi_ssid: Option<String>,
    wifi_strength: Option<u8>,
) -> NetworkLink {
    let Some(kind) = conn_type else {
        return NetworkLink::None;
    };
    let name = name.unwrap_or_default();
    match kind {
        "802-11-wireless" => NetworkLink::Wifi {
            ssid: wifi_ssid.unwrap_or_else(|| name.clone()),
            name,
            strength: f64::from(wifi_strength.unwrap_or(0).min(100)) / 100.0,
        },
        "802-3-ethernet" => NetworkLink::Wired { name },
        _ => NetworkLink::Other { name },
    }
}

async fn get_all_props(
    conn: &Connection,
    path: &str,
    iface: &str,
) -> Option<HashMap<String, OwnedValue>> {
    let reply = conn
        .call_method(Some(NM_BUS), path, Some(PROPS_IFACE), "GetAll", &(iface,))
        .await
        .ok()?;
    reply.body().deserialize::<HashMap<String, OwnedValue>>().ok()
}

fn prop_str(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| match &**v {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    })
}

fn prop_u32(props: &HashMap<String, OwnedValue>, key: &str) -> Option<u32> {
    props.get(key).and_then(|v| match &**v {
        Value::U32(n) => Some(*n),
        _ => None,
    })
}

fn prop_u8(props: &HashMap<String, OwnedValue>, key: &str) -> Option<u8> {
    props.get(key).and_then(|v| match &**v {
        Value::U8(n) => Some(*n),
        _ => None,
    })
}

fn prop_path(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| match &**v {
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    })
}

fn prop_bytes(props: &HashMap<String, OwnedValue>, key: &str) -> Option<Vec<u8>> {
    props.get(key).and_then(|v| match &**v {
        Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr.iter() {
                if let Value::U8(b) = item {
                    out.push(*b);
                }
            }
            Some(out)
        }
        _ => None,
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TrackedPaths {
    primary_connection: Option<String>,
    access_point: Option<String>,
}

fn normalize_dbus_path(raw: Option<String>) -> Option<String> {
    raw.filter(|p| !p.is_empty() && p != "/")
}

fn is_relevant_nm_path(signal_path: Option<&str>, tracked: &TrackedPaths) -> bool {
    let Some(path) = signal_path else {
        return true;
    };
    path == NM_PATH
        || tracked.primary_connection.as_deref() == Some(path)
        || tracked.access_point.as_deref() == Some(path)
}

async fn fetch_network_state(
    conn: &Connection,
) -> Option<(Connectivity, NetworkLink, TrackedPaths)> {
    let nm_props = get_all_props(conn, NM_PATH, NM_IFACE).await?;
    let connectivity = map_connectivity(prop_u32(&nm_props, "Connectivity").unwrap_or(0));
    let primary_connection = normalize_dbus_path(prop_path(&nm_props, "PrimaryConnection"));

    let Some(ref primary) = primary_connection else {
        return Some((connectivity, NetworkLink::None, TrackedPaths::default()));
    };

    let Some(ac_props) = get_all_props(conn, primary, ACTIVE_CONN_IFACE).await else {
        return Some((
            connectivity,
            NetworkLink::None,
            TrackedPaths {
                primary_connection,
                access_point: None,
            },
        ));
    };

    let conn_type = prop_str(&ac_props, "Type");
    let conn_name = prop_str(&ac_props, "Id");

    let (ssid, strength, access_point) = if conn_type.as_deref() == Some("802-11-wireless") {
        let ap_path = normalize_dbus_path(prop_path(&ac_props, "SpecificObject"));
        if let Some(ref ap) = ap_path {
            if let Some(ap_props) = get_all_props(conn, ap, AP_IFACE).await {
                let ssid = prop_bytes(&ap_props, "Ssid")
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                let strength = prop_u8(&ap_props, "Strength");
                (ssid, strength, ap_path)
            } else {
                (None, None, ap_path)
            }
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    let link = map_link(conn_type.as_deref(), conn_name, ssid, strength);
    Some((
        connectivity,
        link,
        TrackedPaths {
            primary_connection,
            access_point,
        },
    ))
}

pub fn spawn(send: SendEvent) {
    thread::spawn(move || {
        async_io::block_on(async move {
            let conn = match Connection::system().await {
                Ok(c) => c,
                Err(err) => {
                    eprintln!("webbar: system D-Bus unavailable for network: {err}");
                    send(BridgeEvent::NetworkState {
                        available: false,
                        connectivity: Connectivity::Unknown,
                        link: NetworkLink::None,
                    });
                    return;
                }
            };

            let Ok(rule) = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender(NM_BUS)
                .and_then(|b| b.interface(PROPS_IFACE))
                .and_then(|b| b.member("PropertiesChanged"))
                .map(|b| b.build())
            else {
                return;
            };

            let mut stream = MessageStream::for_match_rule(rule, &conn, Some(64))
                .await
                .ok();

            let mut last: Option<(bool, Connectivity, NetworkLink)> = None;
            let mut emit = |available: bool, connectivity: Connectivity, link: NetworkLink| {
                if last
                    .as_ref()
                    .is_some_and(|(a, c, l)| *a == available && *c == connectivity && l == &link)
                {
                    return;
                }
                last = Some((available, connectivity, link.clone()));
                send(BridgeEvent::NetworkState {
                    available,
                    connectivity,
                    link,
                });
            };

            let mut tracked = if let Some((connectivity, link, next_tracked)) =
                fetch_network_state(&conn).await
            {
                emit(true, connectivity, link);
                next_tracked
            } else {
                emit(false, Connectivity::Unknown, NetworkLink::None);
                TrackedPaths::default()
            };

            let Some(ref mut stream) = stream else {
                return;
            };

            while let Some(Ok(msg)) = stream.next().await {
                let header = msg.header();
                let sig_path = header.path().map(|p| p.as_str());
                if !is_relevant_nm_path(sig_path, &tracked) {
                    continue;
                }
                if let Some((connectivity, link, next_tracked)) = fetch_network_state(&conn).await
                {
                    tracked = next_tracked;
                    emit(true, connectivity, link);
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_connectivity_values() {
        assert_eq!(map_connectivity(1), Connectivity::None);
        assert_eq!(map_connectivity(2), Connectivity::Portal);
        assert_eq!(map_connectivity(3), Connectivity::Limited);
        assert_eq!(map_connectivity(4), Connectivity::Full);
        assert_eq!(map_connectivity(99), Connectivity::Unknown);
    }

    #[test]
    fn maps_wifi_and_wired_links() {
        assert_eq!(
            map_link(
                Some("802-11-wireless"),
                Some("Home".into()),
                Some("HomeNet".into()),
                Some(75),
            ),
            NetworkLink::Wifi {
                name: "Home".into(),
                ssid: "HomeNet".into(),
                strength: 0.75,
            }
        );
        assert_eq!(
            map_link(Some("802-3-ethernet"), Some("eth0".into()), None, None),
            NetworkLink::Wired {
                name: "eth0".into(),
            }
        );
        assert_eq!(map_link(None, None, None, None), NetworkLink::None);
    }

    #[test]
    fn normalizes_empty_and_root_dbus_paths() {
        assert_eq!(normalize_dbus_path(None), None);
        assert_eq!(normalize_dbus_path(Some(String::new())), None);
        assert_eq!(normalize_dbus_path(Some("/".into())), None);
        assert_eq!(
            normalize_dbus_path(Some("/org/freedesktop/NetworkManager/ActiveConnection/3".into())),
            Some("/org/freedesktop/NetworkManager/ActiveConnection/3".into())
        );
    }

    #[test]
    fn filters_nm_signals_to_manager_primary_and_active_ap_paths() {
        let wifi_tracked = TrackedPaths {
            primary_connection: Some("/org/freedesktop/NetworkManager/ActiveConnection/3".into()),
            access_point: Some("/org/freedesktop/NetworkManager/AccessPoint/42".into()),
        };

        assert!(is_relevant_nm_path(Some(NM_PATH), &wifi_tracked));
        assert!(is_relevant_nm_path(
            Some("/org/freedesktop/NetworkManager/ActiveConnection/3"),
            &wifi_tracked
        ));
        assert!(is_relevant_nm_path(
            Some("/org/freedesktop/NetworkManager/AccessPoint/42"),
            &wifi_tracked
        ));
        assert!(is_relevant_nm_path(None, &wifi_tracked));

        // Neighboring AP scans, other active connections, and device traffic stats are ignored
        assert!(!is_relevant_nm_path(
            Some("/org/freedesktop/NetworkManager/AccessPoint/99"),
            &wifi_tracked
        ));
        assert!(!is_relevant_nm_path(
            Some("/org/freedesktop/NetworkManager/ActiveConnection/4"),
            &wifi_tracked
        ));
        assert!(!is_relevant_nm_path(
            Some("/org/freedesktop/NetworkManager/Devices/2"),
            &wifi_tracked
        ));

        let wired_tracked = TrackedPaths {
            primary_connection: Some("/org/freedesktop/NetworkManager/ActiveConnection/1".into()),
            access_point: None,
        };
        assert!(is_relevant_nm_path(Some(NM_PATH), &wired_tracked));
        assert!(is_relevant_nm_path(
            Some("/org/freedesktop/NetworkManager/ActiveConnection/1"),
            &wired_tracked
        ));
        assert!(!is_relevant_nm_path(
            Some("/org/freedesktop/NetworkManager/AccessPoint/42"),
            &wired_tracked
        ));
    }
}

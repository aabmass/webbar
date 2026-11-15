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

async fn fetch_network_state(conn: &Connection) -> Option<(Connectivity, NetworkLink)> {
    let nm_props = get_all_props(conn, NM_PATH, NM_IFACE).await?;
    let connectivity = map_connectivity(prop_u32(&nm_props, "Connectivity").unwrap_or(0));
    let primary = prop_path(&nm_props, "PrimaryConnection").unwrap_or_default();

    if primary.is_empty() || primary == "/" {
        return Some((connectivity, NetworkLink::None));
    }

    let Some(ac_props) = get_all_props(conn, &primary, ACTIVE_CONN_IFACE).await else {
        return Some((connectivity, NetworkLink::None));
    };

    let conn_type = prop_str(&ac_props, "Type");
    let conn_name = prop_str(&ac_props, "Id");

    let (ssid, strength) = if conn_type.as_deref() == Some("802-11-wireless") {
        let ap_path = prop_path(&ac_props, "SpecificObject").unwrap_or_default();
        if !ap_path.is_empty() && ap_path != "/" {
            if let Some(ap_props) = get_all_props(conn, &ap_path, AP_IFACE).await {
                let ssid = prop_bytes(&ap_props, "Ssid")
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                let strength = prop_u8(&ap_props, "Strength");
                (ssid, strength)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let link = map_link(conn_type.as_deref(), conn_name, ssid, strength);
    Some((connectivity, link))
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

            if let Some((connectivity, link)) = fetch_network_state(&conn).await {
                emit(true, connectivity, link);
            } else {
                emit(false, Connectivity::Unknown, NetworkLink::None);
            }

            let Some(ref mut stream) = stream else {
                return;
            };

            while let Some(Ok(_msg)) = stream.next().await {
                if let Some((connectivity, link)) = fetch_network_state(&conn).await {
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
}

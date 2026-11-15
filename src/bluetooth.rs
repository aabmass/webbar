//! BlueZ Bluetooth state & control via `zbus` on the system bus.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use futures_util::stream::StreamExt;
use serde::Serialize;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream};

use crate::bridge::{reply_err, reply_ok, BridgeEvent, SendEvent};

const BLUEZ_BUS: &str = "org.bluez";
const ADAPTER_IFACE: &str = "org.bluez.Adapter1";
const DEVICE_IFACE: &str = "org.bluez.Device1";
const BATTERY_IFACE: &str = "org.bluez.Battery1";
const OBJ_MANAGER_IFACE: &str = "org.freedesktop.DBus.ObjectManager";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

type ManagedObjects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;
type ObjectMap = HashMap<String, HashMap<String, HashMap<String, OwnedValue>>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct BluetoothAdapter {
    pub path: String,
    pub name: String,
    pub powered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct BluetoothDevice {
    pub path: String,
    pub name: String,
    pub address: String,
    pub paired: bool,
    pub connected: bool,
    pub battery: Option<u8>,
}

#[derive(Clone, Default)]
pub struct BluetoothController {
    _marker: (),
}

fn prop_str(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| match &**v {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    })
}

fn prop_bool(props: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    props.get(key).and_then(|v| match &**v {
        Value::Bool(b) => Some(*b),
        _ => None,
    })
}

fn prop_u8(props: &HashMap<String, OwnedValue>, key: &str) -> Option<u8> {
    props.get(key).and_then(|v| match &**v {
        Value::U8(n) => Some(*n),
        _ => None,
    })
}

pub fn build_state(objects: &ObjectMap) -> (Vec<BluetoothAdapter>, Vec<BluetoothDevice>) {
    let mut adapters = Vec::new();
    let mut devices = Vec::new();

    for (path, ifaces) in objects {
        if let Some(props) = ifaces.get(ADAPTER_IFACE) {
            adapters.push(BluetoothAdapter {
                path: path.clone(),
                name: prop_str(props, "Alias")
                    .or_else(|| prop_str(props, "Name"))
                    .unwrap_or_else(|| path.clone()),
                powered: prop_bool(props, "Powered").unwrap_or(false),
            });
        }
        if let Some(props) = ifaces.get(DEVICE_IFACE) {
            let battery = ifaces
                .get(BATTERY_IFACE)
                .and_then(|b| prop_u8(b, "Percentage"));
            let address = prop_str(props, "Address").unwrap_or_default();
            let name = prop_str(props, "Alias")
                .or_else(|| prop_str(props, "Name"))
                .unwrap_or_else(|| address.clone());
            devices.push(BluetoothDevice {
                path: path.clone(),
                name,
                address,
                paired: prop_bool(props, "Paired").unwrap_or(false),
                connected: prop_bool(props, "Connected").unwrap_or(false),
                battery,
            });
        }
    }

    adapters.sort_by(|a, b| a.path.cmp(&b.path));
    devices.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    (adapters, devices)
}

pub fn spawn(send: SendEvent) -> BluetoothController {
    let objects = Arc::new(Mutex::new(ObjectMap::new()));

    {
        let send = send.clone();
        let objects = objects.clone();
        thread::spawn(move || {
            async_io::block_on(async move {
                let conn = match Connection::system().await {
                    Ok(c) => c,
                    Err(err) => {
                        eprintln!("webbar: system D-Bus unavailable for bluetooth: {err}");
                        send(BridgeEvent::BluetoothState {
                            available: false,
                            adapters: Vec::new(),
                            devices: Vec::new(),
                        });
                        return;
                    }
                };

                // Subscribe before GetManagedObjects to avoid missing transitions
                let iface_added_rule = MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .sender(BLUEZ_BUS)
                    .and_then(|b| b.interface(OBJ_MANAGER_IFACE))
                    .and_then(|b| b.member("InterfacesAdded"))
                    .map(|b| b.build());
                let iface_removed_rule = MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .sender(BLUEZ_BUS)
                    .and_then(|b| b.interface(OBJ_MANAGER_IFACE))
                    .and_then(|b| b.member("InterfacesRemoved"))
                    .map(|b| b.build());
                let props_rule = MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .sender(BLUEZ_BUS)
                    .and_then(|b| b.interface(PROPS_IFACE))
                    .and_then(|b| b.member("PropertiesChanged"))
                    .map(|b| b.build());

                let mut added_stream = match iface_added_rule {
                    Ok(r) => MessageStream::for_match_rule(r, &conn, Some(64)).await.ok(),
                    Err(_) => None,
                };
                let mut removed_stream = match iface_removed_rule {
                    Ok(r) => MessageStream::for_match_rule(r, &conn, Some(64)).await.ok(),
                    Err(_) => None,
                };
                let mut props_stream = match props_rule {
                    Ok(r) => MessageStream::for_match_rule(r, &conn, Some(128)).await.ok(),
                    Err(_) => None,
                };

                // Initial fetch
                match conn
                    .call_method(Some(BLUEZ_BUS), "/", Some(OBJ_MANAGER_IFACE), "GetManagedObjects", &())
                    .await
                {
                    Ok(reply) => {
                        if let Ok(managed) = reply.body().deserialize::<ManagedObjects>() {
                            let mut guard = objects.lock().unwrap();
                            *guard = managed
                                .into_iter()
                                .map(|(p, ifaces)| (p.to_string(), ifaces))
                                .collect();
                            let (adapters, devices) = build_state(&guard);
                            send(BridgeEvent::BluetoothState {
                                available: true,
                                adapters,
                                devices,
                            });
                        }
                    }
                    Err(err) => {
                        eprintln!("webbar: BlueZ GetManagedObjects failed: {err}");
                        send(BridgeEvent::BluetoothState {
                            available: false,
                            adapters: Vec::new(),
                            devices: Vec::new(),
                        });
                    }
                }

                // Event loop across all three streams
                loop {
                    let changed = futures_util::select! {
                        msg = async {
                            match added_stream.as_mut() {
                                Some(s) => s.next().await,
                                None => futures_util::future::pending().await,
                            }
                        }.fuse() => {
                            let Some(Ok(msg)) = msg else { break; };
                            if let Ok((path, ifaces)) = msg.body().deserialize::<(OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>)>() {
                                let mut guard = objects.lock().unwrap();
                                guard.entry(path.to_string()).or_default().extend(ifaces);
                                true
                            } else {
                                false
                            }
                        }
                        msg = async {
                            match removed_stream.as_mut() {
                                Some(s) => s.next().await,
                                None => futures_util::future::pending().await,
                            }
                        }.fuse() => {
                            let Some(Ok(msg)) = msg else { break; };
                            if let Ok((path, ifaces)) = msg.body().deserialize::<(OwnedObjectPath, Vec<String>)>() {
                                let mut guard = objects.lock().unwrap();
                                let key = path.to_string();
                                if let Some(entry) = guard.get_mut(&key) {
                                    for iface in ifaces {
                                        entry.remove(&iface);
                                    }
                                    if entry.is_empty() {
                                        guard.remove(&key);
                                    }
                                }
                                true
                            } else {
                                false
                            }
                        }
                        msg = async {
                            match props_stream.as_mut() {
                                Some(s) => s.next().await,
                                None => futures_util::future::pending().await,
                            }
                        }.fuse() => {
                            let Some(Ok(msg)) = msg else { break; };
                            let path = msg.header().path().map(|p| p.to_string()).unwrap_or_default();
                            if let Ok((iface, changed_props, invalidated)) = msg.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>() {
                                if matches!(iface.as_str(), ADAPTER_IFACE | DEVICE_IFACE | BATTERY_IFACE) {
                                    let mut guard = objects.lock().unwrap();
                                    let entry = guard.entry(path).or_default().entry(iface).or_default();
                                    for (k, v) in changed_props {
                                        entry.insert(k, v);
                                    }
                                    for k in invalidated {
                                        entry.remove(&k);
                                    }
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                    };

                    if changed {
                        let guard = objects.lock().unwrap();
                        let (adapters, devices) = build_state(&guard);
                        send(BridgeEvent::BluetoothState {
                            available: true,
                            adapters,
                            devices,
                        });
                    }
                }
            });
        });
    }

    BluetoothController { _marker: () }
}

use futures_util::FutureExt;

impl BluetoothController {
    pub fn set_powered(&self, req_id: u32, path: String, powered: bool, send: SendEvent) {
        thread::spawn(move || {
            let res = (|| -> Result<(), String> {
                let conn = zbus::blocking::Connection::system().map_err(|e| e.to_string())?;
                conn.call_method(
                    Some(BLUEZ_BUS),
                    path.as_str(),
                    Some(PROPS_IFACE),
                    "Set",
                    &(ADAPTER_IFACE, "Powered", Value::Bool(powered)),
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            })();
            match res {
                Ok(()) => reply_ok(&send, req_id),
                Err(err) => reply_err(&send, req_id, err),
            }
        });
    }

    pub fn connect(&self, req_id: u32, path: String, send: SendEvent) {
        call_device_method(req_id, path, "Connect", send);
    }

    pub fn disconnect(&self, req_id: u32, path: String, send: SendEvent) {
        call_device_method(req_id, path, "Disconnect", send);
    }
}

fn call_device_method(req_id: u32, path: String, member: &'static str, send: SendEvent) {
    thread::spawn(move || {
        let res = (|| -> Result<(), String> {
            let conn = zbus::blocking::Connection::system().map_err(|e| e.to_string())?;
            conn.call_method(Some(BLUEZ_BUS), path.as_str(), Some(DEVICE_IFACE), member, &())
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        match res {
            Ok(()) => reply_ok(&send, req_id),
            Err(err) => reply_err(&send, req_id, err),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_adapters_and_devices_with_battery() {
        let mut objects = ObjectMap::new();

        let mut adapter_props = HashMap::new();
        adapter_props.insert("Alias".into(), OwnedValue::from(zbus::zvariant::Str::from("hci0")));
        adapter_props.insert("Powered".into(), OwnedValue::from(true));
        let mut adapter_ifaces = HashMap::new();
        adapter_ifaces.insert(ADAPTER_IFACE.into(), adapter_props);
        objects.insert("/org/bluez/hci0".into(), adapter_ifaces);

        let mut dev_props = HashMap::new();
        dev_props.insert("Alias".into(), OwnedValue::from(zbus::zvariant::Str::from("Headphones")));
        dev_props.insert("Address".into(), OwnedValue::from(zbus::zvariant::Str::from("AA:BB:CC:DD:EE:FF")));
        dev_props.insert("Paired".into(), OwnedValue::from(true));
        dev_props.insert("Connected".into(), OwnedValue::from(true));
        let mut bat_props = HashMap::new();
        bat_props.insert("Percentage".into(), OwnedValue::from(85u8));
        let mut dev_ifaces = HashMap::new();
        dev_ifaces.insert(DEVICE_IFACE.into(), dev_props);
        dev_ifaces.insert(BATTERY_IFACE.into(), bat_props);
        objects.insert("/org/bluez/hci0/dev_AA".into(), dev_ifaces);

        let (adapters, devices) = build_state(&objects);
        assert_eq!(adapters.len(), 1);
        assert!(adapters[0].powered);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "Headphones");
        assert_eq!(devices[0].battery, Some(85));
        assert!(devices[0].connected);
    }
}

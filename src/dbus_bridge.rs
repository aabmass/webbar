//! Generic D-Bus call and signal-matching escape hatch.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use futures_channel::oneshot;
use futures_util::future::FutureExt;
use futures_util::select;
use futures_util::stream::StreamExt;
use zbus::blocking::Connection as BlockingConnection;
use zbus::zvariant::{Signature, Structure, StructureBuilder};
use zbus::{Connection as AsyncConnection, MessageStream};

use crate::bridge::{reply_data, reply_err, BridgeEvent, Bus, SendEvent};
use crate::dbus_json;

pub struct DbusBridge {
    buses: Arc<Mutex<HashMap<Bus, BlockingConnection>>>,
    matches: Arc<Mutex<HashMap<u32, oneshot::Sender<()>>>>,
    send: SendEvent,
}

impl DbusBridge {
    pub fn new(send: SendEvent) -> Self {
        Self {
            buses: Arc::new(Mutex::new(HashMap::new())),
            matches: Arc::new(Mutex::new(HashMap::new())),
            send,
        }
    }

    pub fn call(
        &self,
        id: u32,
        bus: Bus,
        destination: String,
        path: String,
        interface: String,
        member: String,
        signature: String,
        body: serde_json::Value,
    ) {
        let buses = self.buses.clone();
        let send = self.send.clone();
        thread::spawn(move || {
            let result = (|| -> Result<(String, serde_json::Value), String> {
                let body_str = body.to_string();
                let values = dbus_json::read(&signature, &body_str).map_err(|e| e.to_string())?;
                let conn = shared_connection(&buses, bus).map_err(|e| e.to_string())?;
                let returned = if values.is_empty() {
                    conn.call_method(
                        Some(destination.as_str()),
                        path.as_str(),
                        Some(interface.as_str()),
                        member.as_str(),
                        &(),
                    )
                } else {
                    let args = values
                        .into_iter()
                        .fold(StructureBuilder::new(), StructureBuilder::append_field)
                        .build()
                        .map_err(|e| e.to_string())?;
                    conn.call_method(
                        Some(destination.as_str()),
                        path.as_str(),
                        Some(interface.as_str()),
                        member.as_str(),
                        &args,
                    )
                }
                .map_err(|e| e.to_string())?;
                body_of(&returned).map_err(|e| e.to_string())
            })();

            match result {
                Ok((sig, body_val)) => reply_data(
                    &send,
                    id,
                    serde_json::json!({ "signature": sig, "body": body_val }),
                ),
                Err(err) => reply_err(&send, id, err),
            }
        });
    }

    pub fn match_signals(
        &self,
        id: u32,
        bus: Bus,
        sender: Option<String>,
        path: Option<String>,
        interface: Option<String>,
        member: Option<String>,
    ) {
        let matches = self.matches.clone();
        let send = self.send.clone();
        thread::spawn(move || {
            let rule = [
                ("sender", sender),
                ("path", path),
                ("interface", interface),
                ("member", member),
            ]
            .into_iter()
            .filter_map(|(field, value)| value.map(|v| format!(",{field}='{v}'")))
            .fold("type='signal'".to_string(), |acc, field| acc + &field);

            async_io::block_on(async move {
                let setup = async {
                    let conn = connect_async(bus).await.map_err(|e| e.to_string())?;
                    let signals = MessageStream::for_match_rule(rule.as_str(), &conn, None)
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok::<_, String>((conn, signals))
                }
                .await;

                let (_conn, mut signals) = match setup {
                    Ok(ok) => ok,
                    Err(err) => {
                        send(BridgeEvent::DbusMatchStarted {
                            id,
                            ok: false,
                            error: Some(err),
                        });
                        return;
                    }
                };

                let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
                matches.lock().unwrap().insert(id, cancel_tx);
                send(BridgeEvent::DbusMatchStarted {
                    id,
                    ok: true,
                    error: None,
                });

                let mut cancel_rx = cancel_rx.fuse();
                loop {
                    select! {
                        _ = cancel_rx => {
                            return;
                        }
                        maybe_msg = signals.next().fuse() => {
                            match maybe_msg {
                                Some(Ok(msg)) => {
                                    let header = msg.header();
                                    match body_of(&msg) {
                                        Ok((sig, body_json)) => send(BridgeEvent::DbusSignal {
                                            id,
                                            sender: header
                                                .sender()
                                                .map(|s| s.to_string())
                                                .unwrap_or_default(),
                                            path: header.path().map(|p| p.to_string()).unwrap_or_default(),
                                            interface: header
                                                .interface()
                                                .map(|i| i.to_string())
                                                .unwrap_or_default(),
                                            member: header
                                                .member()
                                                .map(|m| m.to_string())
                                                .unwrap_or_default(),
                                            signature: sig,
                                            body: body_json,
                                        }),
                                        Err(err) => {
                                            if matches.lock().unwrap().remove(&id).is_some() {
                                                send(BridgeEvent::DbusMatchEnded {
                                                    id,
                                                    error: Some(err.to_string()),
                                                });
                                            }
                                            return;
                                        }
                                    }
                                }
                                Some(Err(err)) => {
                                    if matches.lock().unwrap().remove(&id).is_some() {
                                        send(BridgeEvent::DbusMatchEnded {
                                            id,
                                            error: Some(err.to_string()),
                                        });
                                    }
                                    return;
                                }
                                None => {
                                    if matches.lock().unwrap().remove(&id).is_some() {
                                        send(BridgeEvent::DbusMatchEnded { id, error: None });
                                    }
                                    return;
                                }
                            }
                        }
                    }
                }
            });
        });
    }

    pub fn unwatch(&self, id: u32) {
        if let Some(cancel_tx) = self.matches.lock().unwrap().remove(&id) {
            let _ = cancel_tx.send(());
            (self.send)(BridgeEvent::DbusMatchEnded { id, error: None });
        }
    }
}

fn connect(bus: Bus) -> zbus::Result<BlockingConnection> {
    match bus {
        Bus::Session => BlockingConnection::session(),
        Bus::System => BlockingConnection::system(),
    }
}

async fn connect_async(bus: Bus) -> zbus::Result<AsyncConnection> {
    match bus {
        Bus::Session => AsyncConnection::session().await,
        Bus::System => AsyncConnection::system().await,
    }
}

fn shared_connection(
    buses: &Mutex<HashMap<Bus, BlockingConnection>>,
    bus: Bus,
) -> zbus::Result<BlockingConnection> {
    let mut guard = buses.lock().unwrap();
    if let Some(conn) = guard.get(&bus) {
        return Ok(conn.clone());
    }
    let conn = connect(bus)?;
    guard.insert(bus, conn.clone());
    Ok(conn)
}

fn body_of(message: &zbus::Message) -> zbus::Result<(String, serde_json::Value)> {
    let body = message.body();
    let signature = body.signature();
    let values = match signature {
        Signature::Unit => Vec::new(),
        _ => body.deserialize::<Structure>()?.into_fields(),
    };
    Ok((signature.to_string_no_parens(), dbus_json::written_value(&values)))
}

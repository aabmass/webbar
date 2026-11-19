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
use zbus::{Connection as AsyncConnection, MatchRule, MessageStream};

use crate::bridge::{reply_data, reply_err, BridgeEvent, Bus, SendEvent};
use crate::dbus_json;

#[derive(Debug, Default, Clone)]
pub struct MatchCriteria {
    pub sender: Option<String>,
    pub path: Option<String>,
    pub interface: Option<String>,
    pub member: Option<String>,
}

/// Build a signal match rule with zbus's validating builder, so client-supplied
/// values cannot inject extra match terms.
pub fn build_match_rule(criteria: &MatchCriteria) -> Result<MatchRule<'static>, String> {
    let mut builder = MatchRule::builder().msg_type(zbus::message::Type::Signal);
    if let Some(sender) = criteria.sender.as_deref() {
        builder = builder.sender(sender).map_err(|e| e.to_string())?;
    }
    if let Some(path) = criteria.path.as_deref() {
        builder = builder.path(path).map_err(|e| e.to_string())?;
    }
    if let Some(iface) = criteria.interface.as_deref() {
        builder = builder.interface(iface).map_err(|e| e.to_string())?;
    }
    if let Some(member) = criteria.member.as_deref() {
        builder = builder.member(member).map_err(|e| e.to_string())?;
    }
    Ok(builder.build().into_owned())
}

#[derive(Default)]
struct MatchState {
    closed: bool,
    active: HashMap<u32, oneshot::Sender<()>>,
}

pub struct DbusBridge {
    buses: Arc<Mutex<HashMap<Bus, BlockingConnection>>>,
    matches: Arc<Mutex<MatchState>>,
    send: SendEvent,
}

impl DbusBridge {
    pub fn new(send: SendEvent) -> Self {
        Self {
            buses: Arc::new(Mutex::new(HashMap::new())),
            matches: Arc::new(Mutex::new(MatchState::default())),
            send,
        }
    }

    #[allow(clippy::too_many_arguments)]
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

    pub fn match_signals(&self, id: u32, bus: Bus, criteria: MatchCriteria) {
        if self.matches.lock().unwrap().closed {
            return;
        }
        let rule = match build_match_rule(&criteria) {
            Ok(r) => r,
            Err(err) => {
                (self.send)(BridgeEvent::DbusMatchStarted {
                    id,
                    ok: false,
                    error: Some(err),
                });
                return;
            }
        };
        let matches = self.matches.clone();
        let send = self.send.clone();
        thread::spawn(move || {
            async_io::block_on(async move {
                let setup = async {
                    let conn = connect_async(bus).await.map_err(|e| e.to_string())?;
                    let signals = MessageStream::for_match_rule(rule, &conn, None)
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
                {
                    let mut guard = matches.lock().unwrap();
                    if guard.closed {
                        return;
                    }
                    guard.active.insert(id, cancel_tx);
                }
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
                                            if matches.lock().unwrap().active.remove(&id).is_some() {
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
                                    if matches.lock().unwrap().active.remove(&id).is_some() {
                                        send(BridgeEvent::DbusMatchEnded {
                                            id,
                                            error: Some(err.to_string()),
                                        });
                                    }
                                    return;
                                }
                                None => {
                                    if matches.lock().unwrap().active.remove(&id).is_some() {
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
        if let Some(cancel_tx) = self.matches.lock().unwrap().active.remove(&id) {
            let _ = cancel_tx.send(());
            (self.send)(BridgeEvent::DbusMatchEnded { id, error: None });
        }
    }
}

impl Drop for DbusBridge {
    fn drop(&mut self) {
        let matches: Vec<_> = {
            let mut guard = self.matches.lock().unwrap();
            guard.closed = true;
            guard.active.drain().collect()
        };
        for (_, cancel_tx) in matches {
            let _ = cancel_tx.send(());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_match_rule_with_all_fields() {
        let rule = build_match_rule(&MatchCriteria {
            sender: Some("org.freedesktop.login1".into()),
            path: Some("/org/freedesktop/login1".into()),
            interface: Some("org.freedesktop.DBus.Properties".into()),
            member: Some("PropertiesChanged".into()),
        })
        .expect("valid match rule");

        let s = rule.to_string();
        assert!(s.contains("type='signal'"));
        assert!(s.contains("sender='org.freedesktop.login1'"));
        assert!(s.contains("path='/org/freedesktop/login1'"));
        assert!(s.contains("interface='org.freedesktop.DBus.Properties'"));
        assert!(s.contains("member='PropertiesChanged'"));
    }

    #[test]
    fn rejects_invalid_match_criteria_and_emits_error() {
        // Invalid object path
        assert!(build_match_rule(&MatchCriteria {
            path: Some("not_a_path".into()),
            ..Default::default()
        })
        .is_err());

        // Quote injection rejected
        assert!(build_match_rule(&MatchCriteria {
            sender: Some("org.foo',eavesdrop='true".into()),
            ..Default::default()
        })
        .is_err());
        assert!(build_match_rule(&MatchCriteria {
            member: Some("Sig',eavesdrop='true".into()),
            ..Default::default()
        })
        .is_err());

        // match_signals emits DbusMatchStarted(ok: false) immediately on invalid criteria
        let sent = Arc::new(Mutex::new(Vec::new()));
        let sink = sent.clone();
        let bridge = DbusBridge::new(Arc::new(move |ev| sink.lock().unwrap().push(ev)));
        bridge.match_signals(
            7,
            Bus::Session,
            MatchCriteria {
                path: Some("not_a_path".into()),
                ..Default::default()
            },
        );
        let events = sent.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            BridgeEvent::DbusMatchStarted {
                id: 7,
                ok: false,
                error: Some(_),
            }
        ));
    }
}

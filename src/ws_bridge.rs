//! Development-only local WebSocket bridge for driving `HostServices` from an
//! external browser (e.g. Vite dev server).

use std::collections::HashMap;
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::StatusCode;
use tungstenite::{accept_hdr, Error as WsError, Message};

use crate::bridge::{BridgeEvent, BridgeRequest, SendEvent, SnapshotCache};
use crate::dbus_bridge::DbusBridge;
use crate::process_bridge::ProcessBridge;

const CLIENT_CHANNEL_CAPACITY: usize = 256;

#[derive(Clone, Default)]
pub struct WsBroadcaster {
    next_id: Arc<AtomicU64>,
    clients: Arc<Mutex<HashMap<u64, mpsc::SyncSender<String>>>>,
}

impl WsBroadcaster {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn broadcast_json(&self, json: &str) {
        let mut clients = self.clients.lock().unwrap();
        clients.retain(|_, tx| tx.try_send(json.to_string()).is_ok());
    }

    fn insert(&self, tx: mpsc::SyncSender<String>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.clients.lock().unwrap().insert(id, tx);
        id
    }

    fn remove(&self, id: u64) {
        self.clients.lock().unwrap().remove(&id);
    }
}

pub fn generate_token() -> String {
    let mut buf = [0u8; 16];
    if let Ok(mut f) = File::open("/dev/urandom") {
        if f.read_exact(&mut buf).is_ok() {
            return buf.iter().map(|b| format!("{b:02x}")).collect();
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos:032x}")
}

pub fn spawn<F>(
    port: u16,
    token: String,
    broadcaster: WsBroadcaster,
    snapshots: SnapshotCache,
    on_request: F,
) -> std::io::Result<()>
where
    F: Fn(BridgeRequest, &DbusBridge, &ProcessBridge, &SendEvent) + Send + Sync + 'static,
{
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr)?;
    eprintln!("webbar: ws-bridge listening on ws://{addr}?token={token}");

    let on_request = Arc::new(on_request);
    let token = Arc::new(token);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let broadcaster = broadcaster.clone();
            let snapshots = snapshots.clone();
            let on_request = on_request.clone();
            let token = token.clone();
            thread::spawn(move || {
                handle_client(stream, &token, broadcaster, snapshots, &*on_request);
            });
        }
    });
    Ok(())
}

fn handle_client(
    stream: TcpStream,
    expected_token: &str,
    broadcaster: WsBroadcaster,
    snapshots: SnapshotCache,
    on_request: &dyn Fn(BridgeRequest, &DbusBridge, &ProcessBridge, &SendEvent),
) {
    let mut ws = match accept_hdr(stream, |req: &Request, resp: Response| {
        if let Some(origin) = req.headers().get("origin").and_then(|v| v.to_str().ok()) {
            if !is_allowed_origin(origin) {
                return Err(http_error(StatusCode::FORBIDDEN, "forbidden origin"));
            }
        }
        if !has_valid_token(req.uri().query(), expected_token) {
            return Err(http_error(StatusCode::UNAUTHORIZED, "invalid ws token"));
        }
        Ok(resp)
    }) {
        Ok(ws) => ws,
        Err(_) => return,
    };

    let _ = ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(25)));

    let (tx, rx) = mpsc::sync_channel::<String>(CLIENT_CHANNEL_CAPACITY);
    let client_id = broadcaster.insert(tx.clone());

    for state in snapshots.snapshot_all() {
        if let Ok(json) = serde_json::to_string(&state) {
            if ws.send(Message::Text(json)).is_err() {
                broadcaster.remove(client_id);
                return;
            }
        }
    }

    let client_send: SendEvent = Arc::new(move |event: BridgeEvent| {
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = tx.try_send(json);
        }
    });
    let client_dbus = DbusBridge::new(client_send.clone());
    let client_procs = ProcessBridge::new(client_send.clone());

    loop {
        while let Ok(json) = rx.try_recv() {
            if ws.send(Message::Text(json)).is_err() {
                client_dbus.shutdown();
                client_procs.shutdown();
                broadcaster.remove(client_id);
                return;
            }
        }

        match ws.read() {
            Ok(Message::Text(text)) => match serde_json::from_str::<BridgeRequest>(&text) {
                Ok(req) => on_request(req, &client_dbus, &client_procs, &client_send),
                Err(err) => eprintln!("webbar: ws-bridge invalid request: {err}"),
            },
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(WsError::Io(err))
                if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }

    client_dbus.shutdown();
    client_procs.shutdown();
    broadcaster.remove(client_id);
}

fn http_error(status: StatusCode, body: &str) -> ErrorResponse {
    let mut resp = ErrorResponse::new(Some(body.to_string()));
    *resp.status_mut() = status;
    resp
}

fn is_allowed_origin(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("http://") else {
        return false;
    };
    let host = if let Some(after_bracket) = rest.strip_prefix('[') {
        let Some((ip, tail)) = after_bracket.split_once(']') else {
            return false;
        };
        if !tail.is_empty() && !tail.starts_with(':') {
            return false;
        }
        ip
    } else {
        rest.split(':').next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn has_valid_token(query: Option<&str>, expected: &str) -> bool {
    let Some(query) = query else {
        return false;
    };
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .any(|(k, v)| k == "token" && percent_decode(v).as_deref() == Some(expected))
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = bytes.get(i + 1).copied()?;
            let lo = bytes.get(i + 2).copied()?;
            let hex = [hi, lo];
            let hex_str = std::str::from_utf8(&hex).ok()?;
            let byte = u8::from_str_radix(hex_str, 16).ok()?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_allows_only_loopback_http() {
        assert!(is_allowed_origin("http://localhost:5173"));
        assert!(is_allowed_origin("http://127.0.0.1:5173"));
        assert!(is_allowed_origin("http://[::1]:5173"));
        assert!(is_allowed_origin("http://localhost"));
        assert!(!is_allowed_origin("https://evil.com"));
        assert!(!is_allowed_origin("http://localhost.evil.com"));
        assert!(!is_allowed_origin("null"));
    }

    #[test]
    fn token_validation_requires_exact_query_match() {
        assert!(has_valid_token(Some("token=abc123"), "abc123"));
        assert!(has_valid_token(Some("foo=1&token=abc123&bar=2"), "abc123"));
        assert!(has_valid_token(Some("token=a%2Bb%2Fc%3D"), "a+b/c="));
        assert!(!has_valid_token(Some("token=%ZZ"), "abc123"));
        assert!(!has_valid_token(Some("token=wrong"), "abc123"));
        assert!(!has_valid_token(None, "abc123"));
    }
}

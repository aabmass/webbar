//! Local WebSocket IPC bridge for driving `HostServices` from embedded WebKit
//! views and external browsers (e.g. Vite dev server).

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::net::{TcpListener, TcpStream};
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
    clients: Arc<Mutex<Vec<mpsc::SyncSender<String>>>>,
}

impl WsBroadcaster {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn broadcast_json(&self, json: &str) {
        self.clients.lock().unwrap().retain(|tx| {
            !matches!(
                tx.try_send(json.to_string()),
                Err(mpsc::TrySendError::Disconnected(_))
            )
        });
    }

    fn push(&self, tx: mpsc::SyncSender<String>) {
        self.clients.lock().unwrap().push(tx);
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
    log_listening: bool,
    token: String,
    broadcaster: WsBroadcaster,
    snapshots: SnapshotCache,
    on_request: F,
) -> std::io::Result<u16>
where
    F: Fn(Option<&str>, BridgeRequest, &DbusBridge, &ProcessBridge, &SendEvent)
        + Send
        + Sync
        + 'static,
{
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let bound_port = listener.local_addr()?.port();
    if log_listening {
        eprintln!("webbar: ws-bridge listening on ws://127.0.0.1:{bound_port}?token={token}");
    }

    let on_request = Arc::new(on_request);
    let token = Arc::new(token);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let broadcaster = broadcaster.clone();
            let snapshots = snapshots.clone();
            let on_request = on_request.clone();
            let token = token.clone();
            thread::spawn(move || {
                handle_client(stream, &token, broadcaster, snapshots, &*on_request);
            });
        }
    });
    Ok(bound_port)
}

fn handle_client(
    stream: TcpStream,
    expected_token: &str,
    broadcaster: WsBroadcaster,
    snapshots: SnapshotCache,
    on_request: &dyn Fn(Option<&str>, BridgeRequest, &DbusBridge, &ProcessBridge, &SendEvent),
) {
    let mut client_output: Option<String> = None;
    let Ok(mut ws) = accept_hdr(stream, |req: &Request, resp: Response| {
        if let Some(origin) = req.headers().get("origin").and_then(|v| v.to_str().ok()) {
            if !is_allowed_origin(origin) {
                return Err(http_error(StatusCode::FORBIDDEN, "forbidden origin"));
            }
        }
        let query = req.uri().query();
        if !has_valid_token(query, expected_token) {
            return Err(http_error(StatusCode::UNAUTHORIZED, "invalid ws token"));
        }
        client_output = query_param(query, "output").filter(|s| !s.is_empty());
        Ok(resp)
    }) else {
        return;
    };

    let _ = ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(25)));

    let (tx, rx) = mpsc::sync_channel::<String>(CLIENT_CHANNEL_CAPACITY);
    let client_send: SendEvent = {
        let tx = tx.clone();
        Arc::new(move |event: BridgeEvent| {
            if let Ok(json) = serde_json::to_string(&event) {
                let _ = tx.try_send(json);
            }
        })
    };
    for state in snapshots.snapshot_all() {
        client_send(state);
    }
    broadcaster.push(tx);

    let client_dbus = DbusBridge::new(client_send.clone());
    let client_procs = ProcessBridge::new(client_send.clone());

    loop {
        while let Ok(json) = rx.try_recv() {
            if ws.send(Message::Text(json)).is_err() {
                return;
            }
        }

        match ws.read() {
            Ok(Message::Text(text)) => match serde_json::from_str::<BridgeRequest>(&text) {
                Ok(req) => on_request(
                    client_output.as_deref(),
                    req,
                    &client_dbus,
                    &client_procs,
                    &client_send,
                ),
                Err(err) => eprintln!("webbar: ws-bridge invalid request: {err}"),
            },
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(WsError::Io(err))
                if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }
}

fn http_error(status: StatusCode, body: &str) -> ErrorResponse {
    let mut resp = ErrorResponse::new(Some(body.to_string()));
    *resp.status_mut() = status;
    resp
}

fn is_allowed_origin(origin: &str) -> bool {
    if origin == "webbar://app" {
        return true;
    }
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

pub fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| percent_decode(v))
}

fn has_valid_token(query: Option<&str>, expected: &str) -> bool {
    query_param(query, "token").as_deref() == Some(expected)
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = [*bytes.get(i + 1)?, *bytes.get(i + 2)?];
            out.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
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
    fn origin_allows_only_loopback_http_and_webbar_scheme() {
        assert!(is_allowed_origin("http://localhost:5173"));
        assert!(is_allowed_origin("http://127.0.0.1:5173"));
        assert!(is_allowed_origin("http://[::1]:5173"));
        assert!(is_allowed_origin("http://localhost"));
        assert!(is_allowed_origin("webbar://app"));
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

//! Local WebSocket IPC bridge for driving `HostServices` from embedded WebKit
//! views and external browsers (e.g. Vite dev server).

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use async_io::Async;
use futures_channel::mpsc;
use futures_util::future::FutureExt;
use futures_util::select;
use futures_util::stream::StreamExt;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::StatusCode;
use tungstenite::{accept_hdr, Error as WsError, Message, WebSocket};

use crate::bridge::{BridgeEvent, BridgeRequest, SendEvent, SnapshotCache};
use crate::dbus_bridge::DbusBridge;
use crate::process_bridge::ProcessBridge;

const CLIENT_CHANNEL_CAPACITY: usize = 256;

type RequestHandler =
    dyn Fn(Option<&str>, BridgeRequest, &DbusBridge, &ProcessBridge, &SendEvent) + Send + Sync;

#[derive(Clone, Default)]
pub struct WsBroadcaster {
    clients: Arc<Mutex<Vec<mpsc::Sender<String>>>>,
}

impl WsBroadcaster {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn broadcast_json(&self, json: &str) {
        self.clients
            .lock()
            .unwrap()
            .retain_mut(|tx| match tx.try_send(json.to_string()) {
                Ok(()) => true,
                Err(err) => !err.is_disconnected(),
            });
    }

    fn push(&self, tx: mpsc::Sender<String>) {
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

#[allow(clippy::result_large_err)]
fn handle_client(
    stream: TcpStream,
    expected_token: &str,
    broadcaster: WsBroadcaster,
    snapshots: SnapshotCache,
    on_request: &RequestHandler,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

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

    let _ = ws.get_ref().set_read_timeout(None);
    let _ = ws.get_ref().set_write_timeout(None);
    // Async::new puts the cloned fd in non-blocking mode. The clone shares the
    // open file description with `ws`'s stream, so `ws` reads/writes now return
    // WouldBlock instead of blocking, which the helpers below rely on.
    let Ok(watcher) = ws.get_ref().try_clone().and_then(Async::new) else {
        return;
    };

    let (tx, mut rx) = mpsc::channel::<String>(CLIENT_CHANNEL_CAPACITY);
    let client_send: SendEvent = {
        let tx = Mutex::new(tx.clone());
        Arc::new(move |event: BridgeEvent| {
            if let Ok(json) = serde_json::to_string(&event) {
                let _ = tx.lock().unwrap().try_send(json);
            }
        })
    };
    for state in snapshots.snapshot_all() {
        client_send(state);
    }
    broadcaster.push(tx);

    let client_dbus = DbusBridge::new(client_send.clone());
    let client_procs = ProcessBridge::new(client_send.clone());

    async_io::block_on(async move {
        while let Ok(json) = rx.try_recv() {
            if !send_ws_text(&mut ws, &watcher, json).await {
                return;
            }
        }

        if !drain_ws_reads(
            &mut ws,
            &watcher,
            client_output.as_deref(),
            on_request,
            &client_dbus,
            &client_procs,
            &client_send,
        )
        .await
        {
            return;
        }

        loop {
            select! {
                maybe_json = rx.next().fuse() => {
                    let Some(json) = maybe_json else { break; };
                    if !send_ws_text(&mut ws, &watcher, json).await {
                        break;
                    }
                    while let Ok(more) = rx.try_recv() {
                        if !send_ws_text(&mut ws, &watcher, more).await {
                            return;
                        }
                    }
                }
                readable = watcher.readable().fuse() => {
                    if readable.is_err() {
                        break;
                    }
                    if !drain_ws_reads(
                        &mut ws,
                        &watcher,
                        client_output.as_deref(),
                        on_request,
                        &client_dbus,
                        &client_procs,
                        &client_send,
                    )
                    .await
                    {
                        break;
                    }
                }
            }
        }
    });
}

async fn flush_ws(ws: &mut WebSocket<TcpStream>, watcher: &Async<TcpStream>) -> bool {
    loop {
        match ws.flush() {
            Ok(()) => return true,
            Err(WsError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {
                if watcher.writable().await.is_err() {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
}

async fn send_ws_text(
    ws: &mut WebSocket<TcpStream>,
    watcher: &Async<TcpStream>,
    json: String,
) -> bool {
    match ws.write(Message::Text(json)) {
        Ok(()) => flush_ws(ws, watcher).await,
        Err(WsError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {
            flush_ws(ws, watcher).await
        }
        Err(_) => false,
    }
}

async fn drain_ws_reads(
    ws: &mut WebSocket<TcpStream>,
    watcher: &Async<TcpStream>,
    client_output: Option<&str>,
    on_request: &RequestHandler,
    client_dbus: &DbusBridge,
    client_procs: &ProcessBridge,
    client_send: &SendEvent,
) -> bool {
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => match serde_json::from_str::<BridgeRequest>(&text) {
                Ok(req) => on_request(client_output, req, client_dbus, client_procs, client_send),
                Err(err) => eprintln!("webbar: ws-bridge invalid request: {err}"),
            },
            Ok(Message::Close(_)) => {
                let _ = flush_ws(ws, watcher).await;
                return false;
            }
            Ok(_) => {}
            Err(WsError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {
                return flush_ws(ws, watcher).await;
            }
            Err(_) => return false,
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

    #[test]
    fn replays_snapshots_dispatches_requests_and_broadcasts_events() {
        let snapshots = SnapshotCache::new();
        snapshots.record(&BridgeEvent::BacklightState {
            available: true,
            brightness: Some(42),
            error: None,
            displays: Vec::new(),
        });

        let broadcaster = WsBroadcaster::new();
        let port = spawn(
            0,
            false,
            "secret-token".to_string(),
            broadcaster.clone(),
            snapshots,
            |output, req, _dbus, _procs, client_send| {
                assert_eq!(output, Some("DP-1"));
                match req {
                    BridgeRequest::BacklightSet {
                        id,
                        percent,
                        display_id,
                    } => {
                        assert_eq!(id, 9);
                        assert!((percent - 55.0).abs() < f64::EPSILON);
                        assert_eq!(display_id, None);
                        crate::bridge::reply_ok(client_send, id);
                    }
                    other => panic!("unexpected request: {other:?}"),
                }
            },
        )
        .expect("spawn ws_bridge");

        // Invalid token is rejected with 401 Unauthorized during handshake
        let bad_stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        bad_stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let bad_err = tungstenite::client(
            format!("ws://127.0.0.1:{port}/?token=wrong"),
            bad_stream,
        )
        .expect_err("invalid token must fail handshake");
        assert!(matches!(
            bad_err,
            tungstenite::HandshakeError::Failure(WsError::Http(resp))
                if resp.status() == StatusCode::UNAUTHORIZED
        ));

        // Valid connection receives snapshot, rpc reply, and broadcast push immediately
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (mut ws, _resp) = tungstenite::client(
            format!("ws://127.0.0.1:{port}/?token=secret-token&output=DP-1"),
            stream,
        )
        .expect("valid ws handshake");

        let snap_msg = ws.read().expect("initial snapshot");
        let snap: serde_json::Value = serde_json::from_str(snap_msg.to_text().unwrap()).unwrap();
        assert_eq!(snap["type"], "backlight_state");
        assert_eq!(snap["brightness"], 42);

        ws.send(Message::Text(
            r#"{"type":"backlight_set","id":9,"percent":55}"#.into(),
        ))
        .unwrap();
        let reply_msg = ws.read().expect("rpc reply");
        let reply: serde_json::Value = serde_json::from_str(reply_msg.to_text().unwrap()).unwrap();
        assert_eq!(reply["type"], "rpc_reply");
        assert_eq!(reply["id"], 9);
        assert_eq!(reply["ok"], true);

        broadcaster.broadcast_json(r#"{"type":"tray","items":[]}"#);
        let broadcast_msg = ws.read().expect("broadcast push");
        let broadcast: serde_json::Value =
            serde_json::from_str(broadcast_msg.to_text().unwrap()).unwrap();
        assert_eq!(broadcast["type"], "tray");
        assert_eq!(broadcast["items"], serde_json::json!([]));

        let _ = ws.close(None);
    }
}

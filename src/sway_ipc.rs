//! Sway / i3 IPC client for workspace, window focus, and binding mode state.

use std::env;
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dbus_bridge::{BridgeEvent, SendEvent};

const MAGIC: &[u8; 6] = b"i3-ipc";

pub const RUN_COMMAND: u32 = 0;
pub const GET_WORKSPACES: u32 = 1;
pub const SUBSCRIBE: u32 = 2;
pub const GET_TREE: u32 = 4;

const EVENT_WORKSPACE: u32 = 0x8000_0000;
const EVENT_MODE: u32 = 0x8000_0002;
const EVENT_WINDOW: u32 = 0x8000_0003;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwayWorkspace {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub num: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub urgent: bool,
    #[serde(default)]
    pub output: String,
}

pub fn encode_frame(msg_type: u32, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(14 + payload.len());
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    frame.extend_from_slice(&msg_type.to_ne_bytes());
    frame.extend_from_slice(payload);
    frame
}

pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<(u32, Vec<u8>)> {
    let mut header = [0u8; 14];
    reader.read_exact(&mut header)?;
    if &header[0..6] != MAGIC {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "invalid i3-ipc magic header",
        ));
    }
    let len = u32::from_ne_bytes(header[6..10].try_into().unwrap()) as usize;
    let msg_type = u32::from_ne_bytes(header[10..14].try_into().unwrap());
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    Ok((msg_type, payload))
}

pub fn extract_focused_title(tree: &Value) -> String {
    fn find_focused(node: &Value) -> Option<String> {
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = node.get(key).and_then(|v| v.as_array()) {
                for child in children {
                    if let Some(title) = find_focused(child) {
                        return Some(title);
                    }
                }
            }
        }
        if node.get("focused").and_then(|v| v.as_bool()) == Some(true) {
            return Some(
                node.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
            );
        }
        None
    }

    find_focused(tree).unwrap_or_default()
}

use std::sync::{Mutex, OnceLock};

static LATEST_STATE: OnceLock<Mutex<Option<BridgeEvent>>> = OnceLock::new();

fn store_and_send(send: &SendEvent, event: BridgeEvent) {
    let lock = LATEST_STATE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = lock.lock() {
        *guard = Some(event.clone());
    }
    send(event);
}

pub fn latest_state() -> Option<BridgeEvent> {
    LATEST_STATE
        .get()
        .and_then(|lock| lock.lock().ok())
        .and_then(|guard| guard.clone())
}

pub fn spawn(send: SendEvent) {
    thread::spawn(move || {
        let sock_path = match env::var("SWAYSOCK") {
            Ok(path) if !path.is_empty() => path,
            _ => {
                eprintln!("webbar: SWAYSOCK is not set; Sway IPC disabled");
                return;
            }
        };

        let mut query_stream = match UnixStream::connect(&sock_path) {
            Ok(s) => s,
            Err(err) => {
                eprintln!("webbar: failed to connect to SWAYSOCK ({sock_path}): {err}");
                return;
            }
        };

        let mut event_stream = match UnixStream::connect(&sock_path) {
            Ok(s) => s,
            Err(err) => {
                eprintln!("webbar: failed to open Sway event socket: {err}");
                return;
            }
        };

        let mut workspaces = fetch_workspaces(&mut query_stream).unwrap_or_default();
        let mut focused_title = fetch_focused_title(&mut query_stream).unwrap_or_default();
        let mut mode = "default".to_string();

        store_and_send(
            &send,
            BridgeEvent::SwayState {
                workspaces: workspaces.clone(),
                focused_title: focused_title.clone(),
                mode: mode.clone(),
            },
        );

        let sub_payload = br#"["workspace","window","mode"]"#;
        if let Err(err) = event_stream
            .write_all(&encode_frame(SUBSCRIBE, sub_payload))
            .and_then(|_| read_frame(&mut event_stream).map(|_| ()))
        {
            eprintln!("webbar: failed to subscribe to Sway events: {err}");
            return;
        }

        while let Ok((msg_type, payload)) = read_frame(&mut event_stream) {
            match msg_type {
                EVENT_WORKSPACE => {
                    if let Ok(ws) = fetch_workspaces(&mut query_stream) {
                        workspaces = ws;
                    }
                    if let Ok(title) = fetch_focused_title(&mut query_stream) {
                        focused_title = title;
                    }
                }
                EVENT_WINDOW => {
                    if let Ok(title) = fetch_focused_title(&mut query_stream) {
                        focused_title = title;
                    }
                }
                EVENT_MODE => {
                    if let Ok(val) = serde_json::from_slice::<Value>(&payload) {
                        if let Some(change) = val.get("change").and_then(|v| v.as_str()) {
                            mode = change.to_string();
                        }
                    }
                }
                _ => continue,
            }

            store_and_send(
                &send,
                BridgeEvent::SwayState {
                    workspaces: workspaces.clone(),
                    focused_title: focused_title.clone(),
                    mode: mode.clone(),
                },
            );
        }
    });
}

pub fn run_command(command: &str) -> Result<(), String> {
    let sock_path = env::var("SWAYSOCK").map_err(|_| "SWAYSOCK is not set".to_string())?;
    let mut stream = UnixStream::connect(&sock_path).map_err(|e| e.to_string())?;
    stream
        .write_all(&encode_frame(RUN_COMMAND, command.as_bytes()))
        .map_err(|e| e.to_string())?;
    let (_msg_type, payload) = read_frame(&mut stream).map_err(|e| e.to_string())?;
    if let Ok(results) = serde_json::from_slice::<Vec<Value>>(&payload) {
        for item in results {
            if item.get("success").and_then(|v| v.as_bool()) == Some(false) {
                let err = item
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("sway command failed");
                return Err(err.to_string());
            }
        }
    }
    Ok(())
}

fn query_json(stream: &mut UnixStream, msg_type: u32) -> io::Result<Value> {
    stream.write_all(&encode_frame(msg_type, &[]))?;
    let (_reply_type, payload) = read_frame(stream)?;
    serde_json::from_slice(&payload).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
}

fn fetch_workspaces(stream: &mut UnixStream) -> io::Result<Vec<SwayWorkspace>> {
    let val = query_json(stream, GET_WORKSPACES)?;
    serde_json::from_value(val).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
}

fn fetch_focused_title(stream: &mut UnixStream) -> io::Result<String> {
    let tree = query_json(stream, GET_TREE)?;
    Ok(extract_focused_title(&tree))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::json;

    use super::{encode_frame, extract_focused_title, read_frame, GET_WORKSPACES};

    #[test]
    fn encode_decode_frame_roundtrip() {
        let payload = br#"{"hello":"world"}"#;
        let frame = encode_frame(GET_WORKSPACES, payload);
        let mut cursor = Cursor::new(frame);
        let (msg_type, decoded) = read_frame(&mut cursor).unwrap();
        assert_eq!(msg_type, GET_WORKSPACES);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn decode_rejects_invalid_magic() {
        let mut bad = Vec::from(&b"not-i3"[..]);
        bad.extend_from_slice(&0u32.to_ne_bytes());
        bad.extend_from_slice(&0u32.to_ne_bytes());
        let mut cursor = Cursor::new(bad);
        assert!(read_frame(&mut cursor).is_err());
    }

    #[test]
    fn extract_focused_title_from_sample_tree() {
        let tree = json!({
            "name": "root",
            "focused": false,
            "nodes": [
                {
                    "name": "eDP-1",
                    "focused": false,
                    "nodes": [
                        {
                            "name": "1",
                            "focused": false,
                            "nodes": [
                                {
                                    "name": "Alacritty",
                                    "focused": false,
                                    "nodes": [],
                                    "floating_nodes": []
                                },
                                {
                                    "name": "Mozilla Firefox",
                                    "focused": true,
                                    "nodes": [],
                                    "floating_nodes": []
                                }
                            ],
                            "floating_nodes": []
                        }
                    ],
                    "floating_nodes": []
                }
            ],
            "floating_nodes": []
        });
        assert_eq!(extract_focused_title(&tree), "Mozilla Firefox");
    }

    #[test]
    fn extract_focused_title_from_floating_node() {
        let tree = json!({
            "name": "root",
            "focused": false,
            "nodes": [],
            "floating_nodes": [
                {
                    "name": "Picture-in-Picture",
                    "focused": true,
                    "nodes": [],
                    "floating_nodes": []
                }
            ]
        });
        assert_eq!(extract_focused_title(&tree), "Picture-in-Picture");
    }
}

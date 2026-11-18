//! Sway / i3 IPC client for workspace, window focus, and binding mode state.

use std::collections::HashMap;
use std::env;
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bridge::{BridgeEvent, SendEvent};

const MAGIC: &[u8; 6] = b"i3-ipc";

pub const RUN_COMMAND: u32 = 0;
pub const GET_WORKSPACES: u32 = 1;
pub const SUBSCRIBE: u32 = 2;
pub const GET_TREE: u32 = 4;

const EVENT_WORKSPACE: u32 = 0x8000_0000;
const EVENT_MODE: u32 = 0x8000_0002;
const EVENT_WINDOW: u32 = 0x8000_0003;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct SwayWorkspace {
    #[serde(default)]
    #[ts(type = "number")]
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
    /// Number of application windows (tiled + floating) on this workspace.
    /// Not part of Sway's GET_WORKSPACES reply; filled in from GET_TREE.
    #[serde(default)]
    pub window_count: u32,
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

/// Count application windows (leaf containers with a pid) per workspace name.
pub fn count_workspace_windows(tree: &Value) -> HashMap<String, u32> {
    fn count_windows(node: &Value) -> u32 {
        let mut children = 0usize;
        let mut total = 0u32;
        for key in ["nodes", "floating_nodes"] {
            if let Some(list) = node.get(key).and_then(|v| v.as_array()) {
                children += list.len();
                total += list.iter().map(count_windows).sum::<u32>();
            }
        }
        if children == 0 {
            let is_con = matches!(
                node.get("type").and_then(|v| v.as_str()),
                Some("con" | "floating_con")
            );
            let has_pid = node.get("pid").and_then(|v| v.as_i64()).is_some();
            return u32::from(is_con && has_pid);
        }
        total
    }

    fn walk(node: &Value, out: &mut HashMap<String, u32>) {
        if node.get("type").and_then(|v| v.as_str()) == Some("workspace") {
            if let Some(name) = node.get("name").and_then(|v| v.as_str()) {
                if name != "__i3_scratch" {
                    out.insert(name.to_string(), count_windows(node));
                }
            }
            return;
        }
        if let Some(list) = node.get("nodes").and_then(|v| v.as_array()) {
            for child in list {
                walk(child, out);
            }
        }
    }

    let mut out = HashMap::new();
    walk(tree, &mut out);
    out
}

fn apply_window_counts(workspaces: &mut [SwayWorkspace], counts: &HashMap<String, u32>) {
    for ws in workspaces {
        ws.window_count = counts.get(&ws.name).copied().unwrap_or(0);
    }
}

/// Refresh focused title and per-workspace window counts from one GET_TREE.
fn refresh_from_tree(
    stream: &mut UnixStream,
    workspaces: &mut [SwayWorkspace],
    focused_title: &mut String,
) {
    if let Ok(tree) = query_json(stream, GET_TREE) {
        *focused_title = extract_focused_title(&tree);
        apply_window_counts(workspaces, &count_workspace_windows(&tree));
    }
}

fn store_and_send(send: &SendEvent, event: BridgeEvent) {
    send(event);
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
        let mut focused_title = String::new();
        refresh_from_tree(&mut query_stream, &mut workspaces, &mut focused_title);
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
                    refresh_from_tree(&mut query_stream, &mut workspaces, &mut focused_title);
                }
                EVENT_WINDOW => {
                    refresh_from_tree(&mut query_stream, &mut workspaces, &mut focused_title);
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::json;

    use super::{
        count_workspace_windows, encode_frame, extract_focused_title, read_frame, GET_WORKSPACES,
    };

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

    #[test]
    fn count_workspace_windows_counts_tiled_and_floating_leaves() {
        let tree = json!({
            "type": "root",
            "nodes": [
                {
                    "type": "output",
                    "name": "__i3",
                    "nodes": [
                        { "type": "workspace", "name": "__i3_scratch", "nodes": [],
                          "floating_nodes": [{ "type": "floating_con", "pid": 9, "nodes": [], "floating_nodes": [] }] }
                    ]
                },
                {
                    "type": "output",
                    "name": "eDP-1",
                    "nodes": [
                        {
                            "type": "workspace",
                            "name": "1",
                            "nodes": [
                                { "type": "con", "pid": 1, "nodes": [], "floating_nodes": [] },
                                {
                                    "type": "con",
                                    "nodes": [
                                        { "type": "con", "pid": 2, "nodes": [], "floating_nodes": [] },
                                        { "type": "con", "pid": 3, "nodes": [], "floating_nodes": [] }
                                    ],
                                    "floating_nodes": []
                                }
                            ],
                            "floating_nodes": [
                                { "type": "floating_con", "pid": 4, "nodes": [], "floating_nodes": [] }
                            ]
                        },
                        { "type": "workspace", "name": "2", "nodes": [], "floating_nodes": [] }
                    ]
                }
            ]
        });
        let counts = count_workspace_windows(&tree);
        assert_eq!(counts.get("1"), Some(&4));
        assert_eq!(counts.get("2"), Some(&0));
        assert!(!counts.contains_key("__i3_scratch"));
    }
}

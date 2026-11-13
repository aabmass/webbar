//! Filesystem read, directory listing, and stat bridge.
//!
//! Adapted from `cprussin/domicile` (`packages/domicile-host/src/system.rs`),
//! MIT License — Copyright (c) Connor Prussin (https://github.com/cprussin/domicile).

use std::fs::{self, FileType};
use std::io::{self, ErrorKind};
use std::thread;
use std::time::UNIX_EPOCH;

use serde_json::{json, Value};

use crate::dbus_bridge::{BridgeEvent, SendEvent};

pub fn read_text(id: u32, path: String, send: SendEvent) {
    thread::spawn(move || match fs::read_to_string(&path) {
        Ok(text) => send_ok(id, Value::String(text), &send),
        Err(err) => send_err(id, &err, &send),
    });
}

pub fn read_dir(id: u32, path: String, send: SendEvent) {
    thread::spawn(move || match read_dir_entries(&path) {
        Ok(entries) => send_ok(id, Value::Array(entries), &send),
        Err(err) => send_err(id, &err, &send),
    });
}

pub fn stat(id: u32, path: String, send: SendEvent) {
    thread::spawn(move || match fs::symlink_metadata(&path) {
        Ok(meta) => {
            let file_type = file_type_code(&meta.file_type());
            let size = meta.len();
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let real_path = fs::canonicalize(&path)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(path);

            send_ok(
                id,
                json!({
                    "fileType": file_type,
                    "size": size,
                    "modifiedMs": modified_ms,
                    "realPath": real_path,
                }),
                &send,
            );
        }
        Err(err) => send_err(id, &err, &send),
    });
}

fn read_dir_entries(path: &str) -> io::Result<Vec<Value>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let file_type = match entry.file_type() {
            Ok(ft) => file_type_code(&ft),
            Err(_) => 3,
        };
        entries.push(json!({
            "name": name,
            "fileType": file_type,
        }));
    }
    Ok(entries)
}

fn file_type_code(ft: &FileType) -> u8 {
    if ft.is_file() {
        0
    } else if ft.is_dir() {
        1
    } else if ft.is_symlink() {
        2
    } else {
        3
    }
}

fn map_error_kind(err: &io::Error) -> &'static str {
    match err.kind() {
        ErrorKind::NotFound => "NotFound",
        ErrorKind::PermissionDenied => "PermissionDenied",
        ErrorKind::AlreadyExists => "AlreadyExists",
        _ => match err.raw_os_error() {
            Some(20) => "NotADirectory",
            Some(21) => "IsADirectory",
            _ => "Other",
        },
    }
}

fn send_ok(id: u32, data: Value, send: &SendEvent) {
    send(BridgeEvent::FileReply {
        id,
        ok: true,
        data: Some(data),
        error_kind: None,
        error: None,
    });
}

fn send_err(id: u32, err: &io::Error, send: &SendEvent) {
    send(BridgeEvent::FileReply {
        id,
        ok: false,
        data: None,
        error_kind: Some(map_error_kind(err).to_string()),
        error: Some(err.to_string()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "webbar-file-bridge-test-{}-{n}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn one_shot() -> (SendEvent, mpsc::Receiver<BridgeEvent>) {
        let (tx, rx) = mpsc::channel();
        let send: SendEvent = Arc::new(move |ev| {
            let _ = tx.send(ev);
        });
        (send, rx)
    }

    fn recv_reply(rx: &mpsc::Receiver<BridgeEvent>) -> (u32, bool, Option<Value>, Option<String>) {
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            BridgeEvent::FileReply {
                id,
                ok,
                data,
                error_kind,
                ..
            } => (id, ok, data, error_kind),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn read_text_returns_file_contents() {
        let dir = TempDir::new();
        let file = dir.0.join("hello.txt");
        fs::write(&file, "hello\nworld\n").unwrap();

        let (send, rx) = one_shot();
        read_text(1, file.to_string_lossy().into_owned(), send);

        let (id, ok, data, error_kind) = recv_reply(&rx);
        assert_eq!(id, 1);
        assert!(ok);
        assert_eq!(data, Some(Value::String("hello\nworld\n".to_string())));
        assert_eq!(error_kind, None);
    }

    #[test]
    fn read_dir_reports_file_directory_and_symlink_types() {
        let dir = TempDir::new();
        let file = dir.0.join("regular.txt");
        let subdir = dir.0.join("subdir");
        let link = dir.0.join("link");
        fs::write(&file, "abc").unwrap();
        fs::create_dir(&subdir).unwrap();
        symlink(&file, &link).unwrap();

        let (send, rx) = one_shot();
        read_dir(2, dir.0.to_string_lossy().into_owned(), send);

        let (id, ok, data, error_kind) = recv_reply(&rx);
        assert_eq!(id, 2);
        assert!(ok);
        assert_eq!(error_kind, None);

        let entries = data.unwrap();
        let arr = entries.as_array().unwrap();
        let by_name: HashMap<&str, u64> = arr
            .iter()
            .map(|e| {
                (
                    e["name"].as_str().unwrap(),
                    e["fileType"].as_u64().unwrap(),
                )
            })
            .collect();

        assert_eq!(by_name.get("regular.txt"), Some(&0));
        assert_eq!(by_name.get("subdir"), Some(&1));
        assert_eq!(by_name.get("link"), Some(&2));
        assert_eq!(by_name.len(), 3);
    }

    #[test]
    fn stat_returns_size_and_resolves_symlink_real_path() {
        let dir = TempDir::new();
        let target = dir.0.join("target.bin");
        let link = dir.0.join("alias");
        fs::write(&target, b"12345").unwrap();
        symlink(&target, &link).unwrap();

        let (send, rx) = one_shot();
        stat(3, target.to_string_lossy().into_owned(), send);
        let (id, ok, data, _) = recv_reply(&rx);
        assert_eq!(id, 3);
        assert!(ok);
        let meta = data.unwrap();
        assert_eq!(meta["fileType"], 0);
        assert_eq!(meta["size"], 5);
        assert_eq!(
            meta["realPath"].as_str().unwrap(),
            fs::canonicalize(&target).unwrap().to_string_lossy()
        );

        let (send, rx) = one_shot();
        stat(4, link.to_string_lossy().into_owned(), send);
        let (id, ok, data, _) = recv_reply(&rx);
        assert_eq!(id, 4);
        assert!(ok);
        let link_meta = data.unwrap();
        assert_eq!(link_meta["fileType"], 2);
        assert_eq!(
            link_meta["realPath"].as_str().unwrap(),
            fs::canonicalize(&target).unwrap().to_string_lossy()
        );
    }

    #[test]
    fn reports_not_found_is_a_directory_and_not_a_directory() {
        let dir = TempDir::new();
        let file = dir.0.join("file.txt");
        let subdir = dir.0.join("sub");
        let missing = dir.0.join("missing.txt");
        fs::write(&file, "x").unwrap();
        fs::create_dir(&subdir).unwrap();

        let (send, rx) = one_shot();
        read_text(10, missing.to_string_lossy().into_owned(), send);
        let (_, ok, _, kind) = recv_reply(&rx);
        assert!(!ok);
        assert_eq!(kind.as_deref(), Some("NotFound"));

        let (send, rx) = one_shot();
        read_text(11, subdir.to_string_lossy().into_owned(), send);
        let (_, ok, _, kind) = recv_reply(&rx);
        assert!(!ok);
        assert_eq!(kind.as_deref(), Some("IsADirectory"));

        let (send, rx) = one_shot();
        read_dir(12, file.to_string_lossy().into_owned(), send);
        let (_, ok, _, kind) = recv_reply(&rx);
        assert!(!ok);
        assert_eq!(kind.as_deref(), Some("NotADirectory"));
    }
}

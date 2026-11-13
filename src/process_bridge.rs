//! Child process bridge with base64-encoded stdio streaming.
//!
//! Adapted from `cprussin/domicile` (`packages/domicile-host/src/system.rs`),
//! MIT License — Copyright (c) Connor Prussin (https://github.com/cprussin/domicile).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use crate::dbus_bridge::{BridgeEvent, SendEvent};

struct ProcessHandle {
    pid: u32,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
}

pub struct ProcessBridge {
    processes: Arc<Mutex<HashMap<u32, ProcessHandle>>>,
    send: SendEvent,
}

impl ProcessBridge {
    pub fn new(send: SendEvent) -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            send,
        }
    }

    pub fn spawn(
        &self,
        id: u32,
        argv: Vec<String>,
        cwd: Option<String>,
        env: HashMap<String, String>,
        stdin: bool,
    ) {
        if argv.is_empty() {
            (self.send)(BridgeEvent::ProcessStarted {
                id,
                ok: false,
                error: Some("argv must not be empty".to_string()),
            });
            return;
        }

        let processes = self.processes.clone();
        let send = self.send.clone();
        thread::spawn(move || {
            let mut cmd = Command::new(&argv[0]);
            cmd.args(&argv[1..])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .stdin(if stdin {
                    Stdio::piped()
                } else {
                    Stdio::null()
                });

            if let Some(dir) = cwd {
                cmd.current_dir(dir);
            }
            if !env.is_empty() {
                cmd.envs(env);
            }

            let mut child = match cmd.spawn() {
                Ok(child) => child,
                Err(err) => {
                    send(BridgeEvent::ProcessStarted {
                        id,
                        ok: false,
                        error: Some(err.to_string()),
                    });
                    return;
                }
            };

            let pid = child.id();
            let child_stdin = child.stdin.take();
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();

            processes.lock().unwrap().insert(
                id,
                ProcessHandle {
                    pid,
                    stdin: Arc::new(Mutex::new(child_stdin)),
                },
            );

            send(BridgeEvent::ProcessStarted {
                id,
                ok: true,
                error: None,
            });

            let stdout_handle = stdout.map(|out| {
                let send = send.clone();
                thread::spawn(move || stream_output(id, "stdout", out, send))
            });
            let stderr_handle = stderr.map(|err| {
                let send = send.clone();
                thread::spawn(move || stream_output(id, "stderr", err, send))
            });

            if let Some(handle) = stdout_handle {
                let _ = handle.join();
            }
            if let Some(handle) = stderr_handle {
                let _ = handle.join();
            }

            let (code, signal) = match child.wait() {
                Ok(status) => (status.code(), status.signal()),
                Err(_) => (None, None),
            };

            processes.lock().unwrap().remove(&id);
            send(BridgeEvent::ProcessExited { id, code, signal });
        });
    }

    pub fn write_stdin(&self, id: u32, data_b64: String) {
        let bytes = match STANDARD.decode(&data_b64) {
            Ok(bytes) => bytes,
            Err(err) => {
                eprintln!("webbar: invalid base64 for process {id} stdin: {err}");
                return;
            }
        };

        let stdin_arc = {
            let guard = self.processes.lock().unwrap();
            guard.get(&id).map(|h| h.stdin.clone())
        };

        if let Some(stdin_arc) = stdin_arc {
            let mut guard = stdin_arc.lock().unwrap();
            if let Some(ref mut stdin) = *guard {
                let _ = stdin.write_all(&bytes);
                let _ = stdin.flush();
            }
        }
    }

    pub fn close_stdin(&self, id: u32) {
        let stdin_arc = {
            let guard = self.processes.lock().unwrap();
            guard.get(&id).map(|h| h.stdin.clone())
        };

        if let Some(stdin_arc) = stdin_arc {
            *stdin_arc.lock().unwrap() = None;
        }
    }

    pub fn kill(&self, id: u32, signal: i32) {
        let pid = {
            let guard = self.processes.lock().unwrap();
            guard.get(&id).map(|h| h.pid)
        };

        if let Some(pid) = pid {
            let _ = Command::new("kill")
                .arg(format!("-{signal}"))
                .arg(pid.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

fn stream_output<R: Read>(id: u32, stream: &str, mut reader: R, send: SendEvent) {
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let data_b64 = STANDARD.encode(&buf[..n]);
                send(BridgeEvent::ProcessOutput {
                    id,
                    stream: stream.to_string(),
                    data_b64,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn make_bridge() -> (ProcessBridge, mpsc::Receiver<BridgeEvent>) {
        let (tx, rx) = mpsc::channel();
        let send: SendEvent = Arc::new(move |ev| {
            let _ = tx.send(ev);
        });
        (ProcessBridge::new(send), rx)
    }

    fn collect_until_exit(
        rx: &mpsc::Receiver<BridgeEvent>,
        target_id: u32,
    ) -> Vec<BridgeEvent> {
        let mut events = Vec::new();
        loop {
            let ev = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let done = matches!(ev, BridgeEvent::ProcessExited { id, .. } if id == target_id);
            events.push(ev);
            if done {
                break;
            }
        }
        events
    }

    fn decoded_stream(events: &[BridgeEvent], stream_name: &str) -> String {
        let mut bytes = Vec::new();
        for ev in events {
            if let BridgeEvent::ProcessOutput {
                stream, data_b64, ..
            } = ev
            {
                if stream == stream_name {
                    bytes.extend_from_slice(&STANDARD.decode(data_b64).unwrap());
                }
            }
        }
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn spawns_process_and_streams_stdout_stderr_and_exit_code() {
        let (bridge, rx) = make_bridge();
        bridge.spawn(
            1,
            vec![
                "sh".into(),
                "-c".into(),
                "printf 'hi\\n'; printf 'oops\\n' >&2; exit 7".into(),
            ],
            None,
            HashMap::new(),
            false,
        );

        let events = collect_until_exit(&rx, 1);
        assert!(matches!(
            events.first(),
            Some(BridgeEvent::ProcessStarted {
                id: 1,
                ok: true,
                error: None
            })
        ));
        assert_eq!(decoded_stream(&events, "stdout"), "hi\n");
        assert_eq!(decoded_stream(&events, "stderr"), "oops\n");
        assert!(matches!(
            events.last(),
            Some(BridgeEvent::ProcessExited {
                id: 1,
                code: Some(7),
                signal: None
            })
        ));
    }

    #[test]
    fn writes_and_closes_stdin_for_cat() {
        let (bridge, rx) = make_bridge();
        bridge.spawn(2, vec!["cat".into()], None, HashMap::new(), true);

        let started = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            started,
            BridgeEvent::ProcessStarted {
                id: 2,
                ok: true,
                error: None
            }
        ));

        bridge.write_stdin(2, STANDARD.encode(b"hello stdin\n"));
        bridge.close_stdin(2);

        let events = collect_until_exit(&rx, 2);
        assert_eq!(decoded_stream(&events, "stdout"), "hello stdin\n");
        assert_eq!(decoded_stream(&events, "stderr"), "");
        assert!(matches!(
            events.last(),
            Some(BridgeEvent::ProcessExited {
                id: 2,
                code: Some(0),
                signal: None
            })
        ));
    }

    #[test]
    fn reports_failure_when_spawning_nonexistent_binary_or_empty_argv() {
        let (bridge, rx) = make_bridge();
        bridge.spawn(
            3,
            vec!["/nonexistent-webbar-test-binary-12345".into()],
            None,
            HashMap::new(),
            false,
        );

        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            BridgeEvent::ProcessStarted { id, ok, error } => {
                assert_eq!(id, 3);
                assert!(!ok);
                assert!(error.is_some());
            }
            other => panic!("unexpected event: {other:?}"),
        }

        bridge.spawn(4, Vec::new(), None, HashMap::new(), false);
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            BridgeEvent::ProcessStarted { id, ok, error } => {
                assert_eq!(id, 4);
                assert!(!ok);
                assert!(error.is_some());
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}

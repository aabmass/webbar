//! Child process bridge.
//!
//! NOTE: stdout/stderr are currently buffered in Rust until process exit because
//! `WebbarApi.exec` returns a single `Promise<ExecResult>`. If streaming output
//! is needed later, re-introduce chunked output events here.

use std::collections::HashMap;
use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::bridge::{reply_data, reply_err, ExecResult, SendEvent};

pub struct ProcessBridge {
    pids: Arc<Mutex<HashMap<u32, u32>>>,
    send: SendEvent,
}

impl ProcessBridge {
    pub fn new(send: SendEvent) -> Self {
        Self {
            pids: Arc::new(Mutex::new(HashMap::new())),
            send,
        }
    }

    pub fn spawn(
        &self,
        id: u32,
        argv: Vec<String>,
        cwd: Option<String>,
        env: HashMap<String, String>,
    ) {
        if argv.is_empty() {
            reply_err(&self.send, id, "argv must not be empty");
            return;
        }

        let pids = self.pids.clone();
        let send = self.send.clone();
        thread::spawn(move || {
            let mut cmd = Command::new(&argv[0]);
            cmd.args(&argv[1..])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .stdin(Stdio::null());

            if let Some(dir) = cwd {
                cmd.current_dir(dir);
            }
            if !env.is_empty() {
                cmd.envs(env);
            }

            let mut child = match cmd.spawn() {
                Ok(child) => child,
                Err(err) => {
                    reply_err(&send, id, err);
                    return;
                }
            };

            pids.lock().unwrap().insert(id, child.id());

            let stdout_handle = child.stdout.take().map(|out| thread::spawn(move || read_all(out)));
            let stderr_handle = child.stderr.take().map(|err| thread::spawn(move || read_all(err)));

            let stdout = stdout_handle
                .and_then(|h| h.join().ok())
                .unwrap_or_default();
            let stderr = stderr_handle
                .and_then(|h| h.join().ok())
                .unwrap_or_default();

            let (code, signal) = match child.wait() {
                Ok(status) => (status.code(), status.signal()),
                Err(_) => (None, None),
            };

            pids.lock().unwrap().remove(&id);
            reply_data(
                &send,
                id,
                ExecResult {
                    code,
                    signal,
                    stdout: String::from_utf8_lossy(&stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&stderr).into_owned(),
                },
            );
        });
    }

    pub fn kill(&self, id: u32, signal: i32) {
        let pid = self.pids.lock().unwrap().get(&id).copied();
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

fn read_all<R: Read>(mut reader: R) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::BridgeEvent;
    use std::sync::mpsc;
    use std::time::Duration;

    fn make_bridge() -> (ProcessBridge, mpsc::Receiver<BridgeEvent>) {
        let (tx, rx) = mpsc::channel();
        let send: SendEvent = Arc::new(move |ev| {
            let _ = tx.send(ev);
        });
        (ProcessBridge::new(send), rx)
    }

    #[test]
    fn spawns_process_and_returns_stdout_stderr_and_exit_code() {
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
        );

        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            BridgeEvent::RpcReply {
                id: 1,
                ok: true,
                data: Some(data),
                error: None,
            } => {
                assert_eq!(data["code"], 7);
                assert!(data.get("signal").is_none() || data["signal"].is_null());
                assert_eq!(data["stdout"], "hi\n");
                assert_eq!(data["stderr"], "oops\n");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn reports_failure_when_spawning_nonexistent_binary_or_empty_argv() {
        let (bridge, rx) = make_bridge();
        bridge.spawn(
            3,
            vec!["/nonexistent-webbar-test-binary-12345".into()],
            None,
            HashMap::new(),
        );

        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            BridgeEvent::RpcReply {
                id: 3,
                ok: false,
                error: Some(_),
                ..
            } => {}
            other => panic!("unexpected event: {other:?}"),
        }

        bridge.spawn(4, Vec::new(), None, HashMap::new());
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            BridgeEvent::RpcReply {
                id: 4,
                ok: false,
                error: Some(_),
                ..
            } => {}
            other => panic!("unexpected event: {other:?}"),
        }
    }
}

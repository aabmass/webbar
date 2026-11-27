//! `greetd` IPC client for building custom Wayland greeters.
//!
//! Mirrors Quickshell's `Quickshell.Services.Greetd` lifecycle:
//! `Inactive` -> `Authenticating` -> `ReadyToLaunch` -> `Launching` -> `Launched`.
//! Completely passive unless `GREETD_SOCK` is set.

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use freedesktop_desktop_entry::{get_languages_from_env, DesktopEntry};
use greetd_ipc::codec::SyncCodec;
use greetd_ipc::{AuthMessageType, Request, Response};
use serde::Serialize;
use ts_rs::TS;

use crate::bridge::{reply_data, reply_err, reply_ok, BridgeEvent, SendEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../preload/src/generated/")]
pub enum GreetdPhase {
    Inactive,
    Authenticating,
    ReadyToLaunch,
    Launching,
    Launched,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../preload/src/generated/")]
pub enum GreetdAuthMessageType {
    Visible,
    Secret,
    Info,
    Error,
}

impl From<AuthMessageType> for GreetdAuthMessageType {
    fn from(kind: AuthMessageType) -> Self {
        match kind {
            AuthMessageType::Visible => Self::Visible,
            AuthMessageType::Secret => Self::Secret,
            AuthMessageType::Info => Self::Info,
            AuthMessageType::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct GreetdAuthPrompt {
    pub kind: GreetdAuthMessageType,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../preload/src/generated/")]
pub enum GreetdSessionKind {
    Wayland,
    X11,
}

impl GreetdSessionKind {
    fn subdir(self) -> &'static str {
        match self {
            Self::Wayland => "wayland-sessions",
            Self::X11 => "xsessions",
        }
    }

    fn id_prefix(self) -> &'static str {
        match self {
            Self::Wayland => "wayland",
            Self::X11 => "x11",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct GreetdSession {
    pub id: String,
    pub name: String,
    pub command: Vec<String>,
    pub kind: GreetdSessionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct GreetdState {
    pub available: bool,
    pub phase: GreetdPhase,
    pub user: Option<String>,
    pub prompt: Option<GreetdAuthPrompt>,
    pub error: Option<String>,
    pub sessions: Vec<GreetdSession>,
}

impl GreetdState {
    fn unavailable() -> Self {
        Self {
            available: false,
            phase: GreetdPhase::Inactive,
            user: None,
            prompt: None,
            error: None,
            sessions: Vec::new(),
        }
    }

    fn inactive(sessions: Vec<GreetdSession>) -> Self {
        Self {
            available: true,
            phase: GreetdPhase::Inactive,
            user: None,
            prompt: None,
            error: None,
            sessions,
        }
    }
}

enum GreetdCommand {
    CreateSession {
        id: u32,
        user: String,
        reply: SendEvent,
    },
    Respond {
        id: u32,
        response: Option<String>,
        reply: SendEvent,
    },
    CancelSession {
        id: u32,
        reply: SendEvent,
    },
    Launch {
        id: u32,
        command: Vec<String>,
        env: Vec<String>,
        exit: bool,
        reply: SendEvent,
    },
}

#[derive(Clone)]
pub struct GreetdController {
    tx: Option<Sender<GreetdCommand>>,
}

impl GreetdController {
    fn send(&self, id: u32, reply: &SendEvent, cmd: GreetdCommand) {
        let Some(tx) = &self.tx else {
            reply_err(reply, id, "greetd socket not available");
            return;
        };
        if tx.send(cmd).is_err() {
            reply_err(reply, id, "greetd worker stopped");
        }
    }

    pub fn create_session(&self, id: u32, user: String, reply: SendEvent) {
        self.send(
            id,
            &reply,
            GreetdCommand::CreateSession {
                id,
                user,
                reply: reply.clone(),
            },
        );
    }

    pub fn respond(&self, id: u32, response: Option<String>, reply: SendEvent) {
        self.send(
            id,
            &reply,
            GreetdCommand::Respond {
                id,
                response,
                reply: reply.clone(),
            },
        );
    }

    pub fn cancel_session(&self, id: u32, reply: SendEvent) {
        self.send(
            id,
            &reply,
            GreetdCommand::CancelSession {
                id,
                reply: reply.clone(),
            },
        );
    }

    pub fn launch(
        &self,
        id: u32,
        command: Vec<String>,
        env: Vec<String>,
        exit: bool,
        reply: SendEvent,
    ) {
        self.send(
            id,
            &reply,
            GreetdCommand::Launch {
                id,
                command,
                env,
                exit,
                reply: reply.clone(),
            },
        );
    }
}

/// Returns the `greetd` IPC socket path from `GREETD_SOCK`, if set.
pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os("GREETD_SOCK")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

fn try_exec_available(program: &str) -> bool {
    let trimmed = program.trim();
    if trimmed.is_empty() {
        return true;
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return is_executable_file(path);
    }
    let Some(path_env) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_env).any(|dir| is_executable_file(&dir.join(trimmed)))
}

fn session_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    if let Some(raw) = std::env::var_os("XDG_DATA_DIRS") {
        for dir in std::env::split_paths(&raw) {
            if dir.is_absolute() && seen.insert(dir.clone()) {
                dirs.push(dir);
            }
        }
    }
    if dirs.is_empty() {
        dirs.push(PathBuf::from("/usr/local/share"));
        dirs.push(PathBuf::from("/usr/share"));
    }
    dirs
}

fn discover_sessions() -> Vec<GreetdSession> {
    let locales = get_languages_from_env();
    discover_sessions_in_dirs(&session_data_dirs(), &locales)
}

fn discover_sessions_in_dirs(data_dirs: &[PathBuf], locales: &[String]) -> Vec<GreetdSession> {
    let mut sessions = Vec::new();
    let mut seen_ids = HashSet::new();

    for kind in [GreetdSessionKind::Wayland, GreetdSessionKind::X11] {
        for root in data_dirs {
            let dir = root.join(kind.subdir());
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };

            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                let Some(stem) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty())
                else {
                    continue;
                };
                let id = format!("{}:{stem}", kind.id_prefix());
                if seen_ids.contains(&id) {
                    continue;
                }
                let Ok(desktop) = DesktopEntry::from_path(&path, Some(locales)) else {
                    continue;
                };
                seen_ids.insert(id.clone());
                if desktop.hidden() || desktop.no_display() {
                    continue;
                }
                if let Some(try_exec) = desktop.try_exec() {
                    if !try_exec_available(try_exec) {
                        continue;
                    }
                }
                let Ok(command) = desktop.parse_exec() else {
                    continue;
                };
                if command.is_empty() {
                    continue;
                }
                let name = desktop
                    .name(locales)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| stem.to_string());
                sessions.push(GreetdSession {
                    id,
                    name,
                    command,
                    kind,
                });
            }
        }
    }

    sessions.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    sessions
}

pub fn spawn(send_event: SendEvent, on_exit: Arc<dyn Fn() + Send + Sync>) -> GreetdController {
    let sock = socket_path();
    let sessions = if sock.is_some() {
        discover_sessions()
    } else {
        Vec::new()
    };
    spawn_with_config(
        sock,
        sessions,
        send_event,
        on_exit,
        Duration::from_millis(50),
    )
}

fn spawn_with_config(
    socket_path: Option<PathBuf>,
    sessions: Vec<GreetdSession>,
    send_event: SendEvent,
    on_exit: Arc<dyn Fn() + Send + Sync>,
    exit_delay: Duration,
) -> GreetdController {
    let Some(path) = socket_path else {
        send_event(BridgeEvent::GreetdState(GreetdState::unavailable()));
        return GreetdController { tx: None };
    };

    let (tx, rx) = mpsc::channel::<GreetdCommand>();
    let initial = GreetdState::inactive(sessions);
    send_event(BridgeEvent::GreetdState(initial.clone()));

    thread::spawn(move || {
        let mut worker = GreetdWorker {
            path,
            stream: None,
            state: initial,
            send_event,
            on_exit,
            exit_delay,
        };
        while let Ok(cmd) = rx.recv() {
            worker.handle(cmd);
        }
    });

    GreetdController { tx: Some(tx) }
}

struct GreetdWorker {
    path: PathBuf,
    stream: Option<UnixStream>,
    state: GreetdState,
    send_event: SendEvent,
    on_exit: Arc<dyn Fn() + Send + Sync>,
    exit_delay: Duration,
}

impl GreetdWorker {
    fn emit(&self) {
        (self.send_event)(BridgeEvent::GreetdState(self.state.clone()));
    }

    fn reset_inactive(&mut self, error: Option<String>) {
        self.stream = None;
        self.state.phase = GreetdPhase::Inactive;
        self.state.user = None;
        self.state.prompt = None;
        self.state.error = error;
        self.emit();
    }

    fn roundtrip(&mut self, req: &Request) -> Result<Response, String> {
        let Some(stream) = self.stream.as_mut() else {
            return Err("no active greetd session".to_string());
        };
        req.write_to(stream)
            .map_err(|err| format!("failed to write greetd request: {err}"))?;
        Response::read_from(stream).map_err(|err| format!("failed to read greetd response: {err}"))
    }

    fn auth_roundtrip(&mut self, id: u32, req: &Request, reply: &SendEvent) {
        match self.roundtrip(req) {
            Ok(Response::AuthMessage {
                auth_message_type,
                auth_message,
            }) => {
                self.state.phase = GreetdPhase::Authenticating;
                self.state.prompt = Some(GreetdAuthPrompt {
                    kind: auth_message_type.into(),
                    message: auth_message,
                });
                self.state.error = None;
                self.emit();
                reply_data(reply, id, &self.state);
            }
            Ok(Response::Success) => {
                self.state.phase = GreetdPhase::ReadyToLaunch;
                self.state.prompt = None;
                self.state.error = None;
                self.emit();
                reply_data(reply, id, &self.state);
            }
            Ok(Response::Error { description, .. }) => {
                self.reset_inactive(Some(description.clone()));
                reply_err(reply, id, description);
            }
            Err(err) => {
                self.reset_inactive(Some(err.clone()));
                reply_err(reply, id, err);
            }
        }
    }

    fn handle(&mut self, cmd: GreetdCommand) {
        match cmd {
            GreetdCommand::CreateSession { id, user, reply } => {
                if let Some(mut existing) = self.stream.take() {
                    let _ = Request::CancelSession.write_to(&mut existing);
                    let _ = Response::read_from(&mut existing);
                }

                let stream = match UnixStream::connect(&self.path) {
                    Ok(s) => s,
                    Err(err) => {
                        let msg = format!(
                            "failed to connect to greetd socket {}: {err}",
                            self.path.display()
                        );
                        self.reset_inactive(Some(msg.clone()));
                        reply_err(&reply, id, msg);
                        return;
                    }
                };
                self.stream = Some(stream);
                self.state.phase = GreetdPhase::Authenticating;
                self.state.user = Some(user.clone());
                self.state.prompt = None;
                self.state.error = None;
                self.emit();

                let req = Request::CreateSession { username: user };
                self.auth_roundtrip(id, &req, &reply);
            }
            GreetdCommand::Respond {
                id,
                response,
                reply,
            } => {
                let req = Request::PostAuthMessageResponse { response };
                self.auth_roundtrip(id, &req, &reply);
            }
            GreetdCommand::CancelSession { id, reply } => {
                if let Some(mut stream) = self.stream.take() {
                    let _ = Request::CancelSession.write_to(&mut stream);
                    let _ = Response::read_from(&mut stream);
                }
                self.reset_inactive(None);
                reply_ok(&reply, id);
            }
            GreetdCommand::Launch {
                id,
                command,
                env,
                exit,
                reply,
            } => {
                if self.state.phase != GreetdPhase::ReadyToLaunch || self.stream.is_none() {
                    reply_err(&reply, id, "greetd session is not ready to launch");
                    return;
                }
                if command.is_empty() {
                    reply_err(&reply, id, "launch command must not be empty");
                    return;
                }
                self.state.phase = GreetdPhase::Launching;
                self.state.error = None;
                self.emit();

                let req = Request::StartSession { cmd: command, env };
                match self.roundtrip(&req) {
                    Ok(Response::Success) => {
                        self.stream = None;
                        self.state.phase = GreetdPhase::Launched;
                        self.state.prompt = None;
                        self.state.error = None;
                        self.emit();
                        reply_ok(&reply, id);
                        if exit {
                            if !self.exit_delay.is_zero() {
                                thread::sleep(self.exit_delay);
                            }
                            (self.on_exit)();
                        }
                    }
                    Ok(Response::Error { description, .. }) => {
                        self.reset_inactive(Some(description.clone()));
                        reply_err(&reply, id, description);
                    }
                    Ok(Response::AuthMessage { .. }) => {
                        let msg = "unexpected auth message during start_session".to_string();
                        self.reset_inactive(Some(msg.clone()));
                        reply_err(&reply, id, msg);
                    }
                    Err(err) => {
                        self.reset_inactive(Some(err.clone()));
                        reply_err(&reply, id, err);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use greetd_ipc::ErrorType;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn temp_sock_path(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "webbar-greetd-test-{}-{}-{label}.sock",
            std::process::id(),
            n
        ))
    }

    fn spawn_mock_greetd<F>(path: &PathBuf, handler: F) -> thread::JoinHandle<()>
    where
        F: Fn(UnixStream) + Send + 'static,
    {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path).expect("bind mock greetd socket");
        let path_cleanup = path.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    break;
                };
                handler(stream);
            }
            let _ = std::fs::remove_file(&path_cleanup);
        })
    }

    fn collect_events() -> (SendEvent, Arc<Mutex<Vec<BridgeEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let tx_events = Arc::clone(&events);
        let send: SendEvent = Arc::new(move |ev| {
            tx_events.lock().unwrap().push(ev);
        });
        (send, events)
    }

    fn rpc_channel() -> (SendEvent, mpsc::Receiver<BridgeEvent>) {
        let (tx, rx) = mpsc::channel();
        let send: SendEvent = Arc::new(move |ev| {
            let _ = tx.send(ev);
        });
        (send, rx)
    }

    #[test]
    fn unavailable_when_no_socket_path() {
        let (send, events) = collect_events();
        let exited = Arc::new(AtomicBool::new(false));
        let ctrl = spawn_with_config(
            None,
            Vec::new(),
            send,
            Arc::new({
                let exited = Arc::clone(&exited);
                move || exited.store(true, Ordering::SeqCst)
            }),
            Duration::ZERO,
        );

        let initial = events.lock().unwrap();
        assert_eq!(initial.len(), 1);
        assert!(matches!(
            &initial[0],
            BridgeEvent::GreetdState(s) if !s.available && s.phase == GreetdPhase::Inactive
        ));

        let (reply_send, rx) = rpc_channel();
        ctrl.create_session(1, "alice".into(), reply_send);
        let ev = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            ev,
            BridgeEvent::RpcReply {
                id: 1,
                ok: false,
                ..
            }
        ));
        assert!(!exited.load(Ordering::SeqCst));
    }

    #[test]
    fn full_happy_path_auth_launch_and_exit() {
        let path = temp_sock_path("happy");
        let recorded = Arc::new(Mutex::new(Vec::<String>::new()));
        let recorded_srv = Arc::clone(&recorded);
        let _srv = spawn_mock_greetd(&path, move |mut stream| {
            while let Ok(req) = Request::read_from(&mut stream) {
                match req {
                    Request::CreateSession { username } => {
                        recorded_srv
                            .lock()
                            .unwrap()
                            .push(format!("create:{username}"));
                        Response::AuthMessage {
                            auth_message_type: AuthMessageType::Secret,
                            auth_message: "Password:".into(),
                        }
                        .write_to(&mut stream)
                        .unwrap();
                    }
                    Request::PostAuthMessageResponse { response } => {
                        recorded_srv.lock().unwrap().push(format!(
                            "respond:{}",
                            response.as_deref().unwrap_or("<none>")
                        ));
                        Response::Success.write_to(&mut stream).unwrap();
                    }
                    Request::StartSession { cmd, env } => {
                        recorded_srv
                            .lock()
                            .unwrap()
                            .push(format!("start:{}:{}", cmd.join(" "), env.join(",")));
                        Response::Success.write_to(&mut stream).unwrap();
                        break;
                    }
                    Request::CancelSession => {
                        recorded_srv.lock().unwrap().push("cancel".into());
                        Response::Success.write_to(&mut stream).unwrap();
                        break;
                    }
                }
            }
        });

        let (send, events) = collect_events();
        let (exit_tx, exit_rx) = mpsc::channel();
        let ctrl = spawn_with_config(
            Some(path.clone()),
            Vec::new(),
            send,
            Arc::new(move || {
                let _ = exit_tx.send(());
            }),
            Duration::ZERO,
        );

        // Launch before authentication is rejected without mutating state
        let (reply_send, rx) = rpc_channel();
        ctrl.launch(99, vec!["sway".into()], Vec::new(), true, reply_send);
        let r_early = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            r_early,
            BridgeEvent::RpcReply {
                id: 99,
                ok: false,
                ..
            }
        ));

        // 1. createSession -> Authenticating with Secret prompt
        let (reply_send, rx) = rpc_channel();
        ctrl.create_session(1, "alice".into(), reply_send);
        let r1 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(r1, BridgeEvent::RpcReply { id: 1, ok: true, .. }));

        // 2. respond -> ReadyToLaunch
        let (reply_send, rx) = rpc_channel();
        ctrl.respond(2, Some("s3cr3t".into()), reply_send);
        let r2 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(r2, BridgeEvent::RpcReply { id: 2, ok: true, .. }));

        // 3. launch -> Launched + on_exit
        let (reply_send, rx) = rpc_channel();
        ctrl.launch(
            3,
            vec!["sway".into()],
            vec!["FOO=bar".into()],
            true,
            reply_send,
        );
        let r3 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(r3, BridgeEvent::RpcReply { id: 3, ok: true, .. }));
        exit_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("expected on_exit to be invoked");

        assert_eq!(
            *recorded.lock().unwrap(),
            vec!["create:alice", "respond:s3cr3t", "start:sway:FOO=bar"]
        );

        let phases: Vec<GreetdPhase> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|ev| match ev {
                BridgeEvent::GreetdState(s) => Some(s.phase),
                _ => None,
            })
            .collect();
        assert_eq!(
            phases,
            vec![
                GreetdPhase::Inactive,
                GreetdPhase::Authenticating,
                GreetdPhase::Authenticating,
                GreetdPhase::ReadyToLaunch,
                GreetdPhase::Launching,
                GreetdPhase::Launched,
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn auth_error_recovers_and_cancel_resets_to_inactive() {
        let path = temp_sock_path("err-cancel");
        let _srv = spawn_mock_greetd(&path, move |mut stream| {
            while let Ok(req) = Request::read_from(&mut stream) {
                match req {
                    Request::CreateSession { .. } => {
                        Response::AuthMessage {
                            auth_message_type: AuthMessageType::Info,
                            auth_message: "Touch security key".into(),
                        }
                        .write_to(&mut stream)
                        .unwrap();
                    }
                    Request::PostAuthMessageResponse { response: None } => {
                        Response::AuthMessage {
                            auth_message_type: AuthMessageType::Secret,
                            auth_message: "Password:".into(),
                        }
                        .write_to(&mut stream)
                        .unwrap();
                    }
                    Request::PostAuthMessageResponse {
                        response: Some(ref pw),
                    } if pw == "wrong" => {
                        Response::Error {
                            error_type: ErrorType::AuthError,
                            description: "Authentication failed".into(),
                        }
                        .write_to(&mut stream)
                        .unwrap();
                        break;
                    }
                    Request::PostAuthMessageResponse { .. } => {
                        Response::Success.write_to(&mut stream).unwrap();
                    }
                    Request::CancelSession => {
                        Response::Success.write_to(&mut stream).unwrap();
                        break;
                    }
                    Request::StartSession { .. } => {
                        Response::Success.write_to(&mut stream).unwrap();
                        break;
                    }
                }
            }
        });

        let (send, events) = collect_events();
        let ctrl = spawn_with_config(
            Some(path.clone()),
            Vec::new(),
            send,
            Arc::new(|| {}),
            Duration::ZERO,
        );

        // Multi-step PAM: Info -> respond(None) -> Secret -> wrong password -> AuthError
        let (reply_send, rx) = rpc_channel();
        ctrl.create_session(1, "alice".into(), reply_send);
        let _ = rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let (reply_send, rx) = rpc_channel();
        ctrl.respond(2, None, reply_send);
        let r2 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(r2, BridgeEvent::RpcReply { id: 2, ok: true, .. }));

        let (reply_send, rx) = rpc_channel();
        ctrl.respond(3, Some("wrong".into()), reply_send);
        let r3 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            r3,
            BridgeEvent::RpcReply {
                id: 3,
                ok: false,
                error: Some(ref e),
                ..
            } if e == "Authentication failed"
        ));

        // Start a new session and cancel it mid-auth
        let (reply_send, rx) = rpc_channel();
        ctrl.create_session(4, "alice".into(), reply_send);
        let _ = rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let (reply_send, rx) = rpc_channel();
        ctrl.cancel_session(5, reply_send);
        let r5 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(r5, BridgeEvent::RpcReply { id: 5, ok: true, .. }));

        let last = events.lock().unwrap().last().cloned().unwrap();
        assert!(matches!(
            last,
            BridgeEvent::GreetdState(GreetdState {
                available: true,
                phase: GreetdPhase::Inactive,
                user: None,
                prompt: None,
                error: None,
                ..
            })
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn discovers_and_deduplicates_desktop_sessions() {
        let base = std::env::temp_dir().join(format!(
            "webbar-greetd-sessions-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let hi = base.join("hi");
        let lo = base.join("lo");
        fs::create_dir_all(hi.join("wayland-sessions")).unwrap();
        fs::create_dir_all(lo.join("wayland-sessions")).unwrap();
        fs::create_dir_all(lo.join("xsessions")).unwrap();

        let valid_bin = base.join("valid-bin");
        fs::write(&valid_bin, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&valid_bin, fs::Permissions::from_mode(0o755)).unwrap();

        fs::write(
            hi.join("wayland-sessions/sway.desktop"),
            format!(
                "[Desktop Entry]\nName=Sway Override\nExec=sway --unsupported-gpu\nTryExec={}\nType=Application\n",
                valid_bin.display()
            ),
        )
        .unwrap();
        fs::write(
            hi.join("wayland-sessions/quoted.desktop"),
            "[Desktop Entry]\nName=Quoted Session\nExec=env \"FOO=bar baz\" sway\nType=Application\n",
        )
        .unwrap();
        fs::write(
            hi.join("wayland-sessions/hidden.desktop"),
            "[Desktop Entry]\nName=Hidden\nExec=sway\nHidden=true\n",
        )
        .unwrap();
        fs::write(
            hi.join("wayland-sessions/nodisplay.desktop"),
            "[Desktop Entry]\nName=NoDisplay\nExec=sway\nNoDisplay=true\n",
        )
        .unwrap();
        fs::write(
            hi.join("wayland-sessions/missing.desktop"),
            "[Desktop Entry]\nName=Missing\nExec=sway\nTryExec=/nonexistent/webbar-missing-bin\n",
        )
        .unwrap();
        fs::write(
            lo.join("wayland-sessions/sway.desktop"),
            "[Desktop Entry]\nName=Sway Low\nExec=sway\nType=Application\n",
        )
        .unwrap();
        fs::write(
            lo.join("xsessions/i3.desktop"),
            "[Desktop Entry]\nName=i3\nExec=i3\nType=Application\n",
        )
        .unwrap();

        let sessions = discover_sessions_in_dirs(&[hi, lo], &[]);
        assert_eq!(
            sessions,
            vec![
                GreetdSession {
                    id: "wayland:quoted".into(),
                    name: "Quoted Session".into(),
                    command: vec!["env".into(), "FOO=bar baz".into(), "sway".into()],
                    kind: GreetdSessionKind::Wayland,
                },
                GreetdSession {
                    id: "wayland:sway".into(),
                    name: "Sway Override".into(),
                    command: vec!["sway".into(), "--unsupported-gpu".into()],
                    kind: GreetdSessionKind::Wayland,
                },
                GreetdSession {
                    id: "x11:i3".into(),
                    name: "i3".into(),
                    command: vec!["i3".into()],
                    kind: GreetdSessionKind::X11,
                },
            ]
        );
        let _ = fs::remove_dir_all(&base);
    }
}

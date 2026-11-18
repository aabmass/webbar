//! Sway IPC client built on the `swayipc` crate.
//!
//! State: on every relevant Sway event (bursts are coalesced) we re-fetch the
//! full tree, workspaces, and binding mode and publish them as one
//! [`SwayState`] snapshot. Events are only used as triggers, never patched in.
//!
//! Commands: run on a dedicated thread with its own connection and answered
//! with an `RpcReply` so the frontend gets a typed success/error result.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use swayipc::{Connection, Error, EventType, Fallible};

use crate::bridge::{reply_ok, BridgeEvent, SendEvent, SwayCommandError};
use crate::sway_state::SwayState;

const SUBSCRIPTIONS: [EventType; 4] = [
    EventType::Workspace,
    EventType::Window,
    EventType::Mode,
    EventType::Output,
];

/// How long to wait for further events before re-fetching state.
const COALESCE_WINDOW: Duration = Duration::from_millis(16);
const BACKOFF_MIN: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(5);
/// A session that lived at least this long resets the reconnect backoff.
const HEALTHY_SESSION: Duration = Duration::from_secs(10);

/// Message from the event-reader thread to the state thread.
#[derive(Debug)]
enum Trigger {
    Changed,
    Disconnected(Error),
}

/// Why a state session ended.
#[derive(Debug)]
enum SessionEnd {
    /// Socket-level failure; reconnecting may help.
    Lost(Error),
    /// Sway IPC is unavailable (e.g. no socket path); give up.
    Unavailable(Error),
}

impl From<Error> for SessionEnd {
    fn from(err: Error) -> Self {
        match err {
            Error::SocketNotFound => SessionEnd::Unavailable(err),
            other => SessionEnd::Lost(other),
        }
    }
}

/// True for errors where the payload was read completely but didn't match
/// swayipc's types (e.g. a value added in a newer Sway). The connection is
/// still in sync, so we can keep using it.
fn is_decode_error(err: &Error) -> bool {
    matches!(err, Error::SerdeJson(_) | Error::UnimplementedEvent(..))
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Spawn the state supervisor: connects, publishes snapshots, and reconnects
/// with exponential backoff when the connection drops (e.g. `swaymsg reload`).
pub fn spawn(send: SendEvent) {
    thread::spawn(move || {
        let mut publisher = Publisher::default();
        let mut backoff = BACKOFF_MIN;
        loop {
            let started = Instant::now();
            match run_session(&send, &mut publisher) {
                SessionEnd::Unavailable(err) => {
                    eprintln!("webbar: Sway IPC disabled: {err}");
                    return;
                }
                SessionEnd::Lost(err) => {
                    eprintln!("webbar: Sway IPC connection lost: {err}; reconnecting");
                }
            }
            if started.elapsed() >= HEALTHY_SESSION {
                backoff = BACKOFF_MIN;
            }
            thread::sleep(backoff);
            backoff = (backoff * 2).min(BACKOFF_MAX);
        }
    });
}

fn run_session(send: &SendEvent, publisher: &mut Publisher) -> SessionEnd {
    match run_session_inner(send, publisher) {
        Ok(never) => match never {},
        Err(end) => end,
    }
}

fn run_session_inner(
    send: &SendEvent,
    publisher: &mut Publisher,
) -> Result<std::convert::Infallible, SessionEnd> {
    let mut query = Connection::new()?;
    // Subscribe before the first fetch so no change can slip in between.
    let events = Connection::new()?.subscribe(SUBSCRIPTIONS)?;

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || read_events(events, tx));

    publisher.publish(&mut query, send)?;
    loop {
        wait_for_change(&rx, COALESCE_WINDOW)?;
        publisher.publish(&mut query, send)?;
    }
}

fn read_events(events: swayipc::EventStream, tx: Sender<Trigger>) {
    let mut warned = false;
    for event in events {
        let trigger = match event {
            Ok(_) => Trigger::Changed,
            Err(err) if is_decode_error(&err) => {
                if !warned {
                    eprintln!("webbar: ignoring undecodable Sway event: {err}");
                    warned = true;
                }
                // Still a sign something changed; refresh anyway.
                Trigger::Changed
            }
            Err(err) => Trigger::Disconnected(err),
        };
        let disconnected = matches!(trigger, Trigger::Disconnected(_));
        if tx.send(trigger).is_err() || disconnected {
            return;
        }
    }
}

/// Block until at least one change arrives, then keep draining until the
/// stream has been quiet for `window`, so a burst causes a single refresh.
fn wait_for_change(rx: &Receiver<Trigger>, window: Duration) -> Result<(), SessionEnd> {
    let closed = || SessionEnd::Lost(Error::SubscriptionFailed("event stream closed".into()));
    match rx.recv() {
        Ok(Trigger::Changed) => {}
        Ok(Trigger::Disconnected(err)) => return Err(SessionEnd::Lost(err)),
        Err(_) => return Err(closed()),
    }
    loop {
        match rx.recv_timeout(window) {
            Ok(Trigger::Changed) => continue,
            Ok(Trigger::Disconnected(err)) => return Err(SessionEnd::Lost(err)),
            Err(RecvTimeoutError::Timeout) => return Ok(()),
            Err(RecvTimeoutError::Disconnected) => return Err(closed()),
        }
    }
}

/// Fetches snapshots and only sends them when they actually changed.
#[derive(Default)]
struct Publisher {
    last_json: Option<String>,
    warned_decode: bool,
}

impl Publisher {
    fn publish(&mut self, conn: &mut Connection, send: &SendEvent) -> Result<(), SessionEnd> {
        match fetch_state(conn) {
            Ok(state) => {
                if self.should_send(&state) {
                    send(BridgeEvent::SwayState(Box::new(state)));
                }
                Ok(())
            }
            Err(err) if is_decode_error(&err) => {
                // Keep the last good snapshot rather than blanking the bar.
                if !self.warned_decode {
                    eprintln!(
                        "webbar: could not decode Sway state (newer Sway than swayipc supports?): {err}"
                    );
                    self.warned_decode = true;
                }
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Returns true if `state` differs from the last snapshot sent.
    fn should_send(&mut self, state: &SwayState) -> bool {
        let Ok(json) = serde_json::to_string(state) else {
            return true;
        };
        if self.last_json.as_deref() == Some(json.as_str()) {
            return false;
        }
        self.last_json = Some(json);
        true
    }
}

fn fetch_state(conn: &mut Connection) -> Fallible<SwayState> {
    Ok(SwayState {
        tree: conn.get_tree()?,
        workspaces: conn.get_workspaces()?,
        mode: conn.get_binding_state()?,
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

struct CommandJob {
    id: u32,
    command: String,
    reply: SendEvent,
}

/// Handle to the Sway command thread.
pub struct SwayCommands {
    tx: Sender<CommandJob>,
}

impl SwayCommands {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<CommandJob>();
        thread::spawn(move || {
            let mut conn: Option<Connection> = None;
            for job in rx {
                let result = run_command_with_retry(&mut conn, &job.command);
                send_command_reply(&job.reply, job.id, result);
            }
        });
        Self { tx }
    }

    pub fn run(&self, id: u32, command: String, reply: SendEvent) {
        let job = CommandJob { id, command, reply };
        if let Err(mpsc::SendError(job)) = self.tx.send(job) {
            send_command_reply(
                &job.reply,
                job.id,
                Err(Error::CommandFailed("Sway command thread stopped".into())),
            );
        }
    }
}

type CommandResult = Fallible<Vec<Fallible<()>>>;

/// Run on a cached connection, reconnecting once if the socket went stale.
fn run_command_with_retry(conn: &mut Option<Connection>, command: &str) -> CommandResult {
    for attempt in 0..2 {
        let c = match conn {
            Some(c) => c,
            None => conn.insert(Connection::new()?),
        };
        match c.run_command(command) {
            Err(Error::Io(err)) if attempt == 0 => {
                eprintln!("webbar: Sway command connection lost ({err}); reconnecting");
                *conn = None;
            }
            other => return other,
        }
    }
    unreachable!("loop returns on the second attempt")
}

fn command_error(result: CommandResult) -> Option<SwayCommandError> {
    let err = match result {
        Ok(outcomes) => outcomes.into_iter().find_map(Result::err)?,
        Err(err) => err,
    };
    Some(match err {
        Error::CommandParse(message) => SwayCommandError {
            message,
            parse_error: true,
        },
        Error::CommandFailed(message) => SwayCommandError {
            message,
            parse_error: false,
        },
        other => SwayCommandError {
            message: other.to_string(),
            parse_error: false,
        },
    })
}

fn send_command_reply(send: &SendEvent, id: u32, result: CommandResult) {
    match command_error(result) {
        None => reply_ok(send, id),
        Some(err) => send(BridgeEvent::RpcReply {
            id,
            ok: false,
            error: Some(err.message.clone()),
            data: serde_json::to_value(&err).ok(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A minimal valid snapshot (empty root). swayipc's types are
/// `#[non_exhaustive]`, so they can only be built by deserializing.
#[cfg(test)]
pub fn test_state() -> SwayState {
    serde_json::from_value(serde_json::json!({
        "tree": {
            "id": 1, "name": "root", "type": "root", "border": "none",
            "current_border_width": 0, "layout": "splith", "orientation": "horizontal",
            "percent": null,
            "rect": { "x": 0, "y": 0, "width": 1920, "height": 1080 },
            "window_rect": { "x": 0, "y": 0, "width": 0, "height": 0 },
            "deco_rect": { "x": 0, "y": 0, "width": 0, "height": 0 },
            "geometry": { "x": 0, "y": 0, "width": 0, "height": 0 },
            "urgent": false, "focused": false, "focus": [], "floating": null,
            "nodes": [], "floating_nodes": [], "sticky": false
        },
        "workspaces": [],
        "mode": "default"
    }))
    .expect("valid test SwayState")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn coalesces_a_burst_into_one_refresh() {
        let (tx, rx) = mpsc::channel();
        for _ in 0..5 {
            tx.send(Trigger::Changed).unwrap();
        }
        assert!(wait_for_change(&rx, Duration::from_millis(5)).is_ok());
        // The whole burst was consumed by one wait.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn disconnect_mid_burst_ends_session() {
        let (tx, rx) = mpsc::channel();
        tx.send(Trigger::Changed).unwrap();
        tx.send(Trigger::Disconnected(Error::Io(std::io::ErrorKind::BrokenPipe.into())))
            .unwrap();
        assert!(matches!(
            wait_for_change(&rx, Duration::from_millis(5)),
            Err(SessionEnd::Lost(Error::Io(_)))
        ));
    }

    #[test]
    fn closed_channel_ends_session() {
        let (tx, rx) = mpsc::channel::<Trigger>();
        drop(tx);
        assert!(matches!(
            wait_for_change(&rx, Duration::from_millis(5)),
            Err(SessionEnd::Lost(_))
        ));
    }

    #[test]
    fn skips_unchanged_snapshots() {
        let mut publisher = Publisher::default();
        let mut state = test_state();
        assert!(publisher.should_send(&state));
        assert!(!publisher.should_send(&state));
        state.mode = "resize".into();
        assert!(publisher.should_send(&state));
    }

    #[test]
    fn missing_socket_is_unavailable_not_retried() {
        assert!(matches!(
            SessionEnd::from(Error::SocketNotFound),
            SessionEnd::Unavailable(_)
        ));
        assert!(matches!(
            SessionEnd::from(Error::Io(std::io::ErrorKind::BrokenPipe.into())),
            SessionEnd::Lost(_)
        ));
    }

    #[test]
    fn maps_command_outcomes() {
        assert_eq!(command_error(Ok(vec![Ok(()), Ok(())])), None);
        assert_eq!(
            command_error(Ok(vec![Ok(()), Err(Error::CommandParse("bad".into()))])),
            Some(SwayCommandError {
                message: "bad".into(),
                parse_error: true
            })
        );
        assert_eq!(
            command_error(Ok(vec![Err(Error::CommandFailed("nope".into()))])),
            Some(SwayCommandError {
                message: "nope".into(),
                parse_error: false
            })
        );
        assert!(matches!(
            command_error(Err(Error::SocketNotFound)),
            Some(SwayCommandError {
                parse_error: false,
                ..
            })
        ));
    }

    #[test]
    fn command_reply_carries_typed_error() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let sink = sent.clone();
        let send: SendEvent = Arc::new(move |ev| sink.lock().unwrap().push(ev));

        send_command_reply(&send, 7, Ok(vec![Err(Error::CommandParse("x".into()))]));
        let json = serde_json::to_value(&sent.lock().unwrap()[0]).unwrap();
        assert_eq!(json["type"], "rpc_reply");
        assert_eq!(json["id"], 7);
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"], "x");
        assert_eq!(json["data"]["parse_error"], true);
    }

    #[test]
    fn sway_state_event_inlines_fields() {
        let json = serde_json::to_value(BridgeEvent::SwayState(Box::new(test_state()))).unwrap();
        assert_eq!(json["type"], "sway_state");
        assert_eq!(json["mode"], "default");
        assert_eq!(json["tree"]["type"], "root");
        assert!(json["workspaces"].is_array());
    }
}

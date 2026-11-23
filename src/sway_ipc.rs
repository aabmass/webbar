//! Sway IPC client built on the `swayipc` crate.
//!
//! State: on every relevant Sway event (bursts are coalesced) we re-fetch the
//! full tree, workspaces, and binding mode and publish them as one
//! [`SwayState`] snapshot. Events are only used as triggers, never patched in.
//!
//! Commands: run on a dedicated thread with its own connection and answered
//! with an `RpcReply` so the frontend gets a typed success/error result.

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use swayipc::{Connection, Error, EventType, Fallible};
use swayipc_types::{Node, NodeType};

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
                    send(BridgeEvent::SwayState(Box::new(SwayState {
                        available: false,
                        tree: None,
                        workspaces: Vec::new(),
                        mode: "default".into(),
                    })));
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
        available: true,
        tree: Some(conn.get_tree()?),
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
// Notification Window Matching & Focus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct AppMatchHints {
    /// Direct sender PID first, followed by parent/ancestor PIDs (closest first).
    pub pids: Vec<i32>,
    pub desktop_entry: String,
    pub app_name: String,
    pub startup_wm_class: String,
    pub exec_program: String,
}

#[derive(Debug, Clone)]
pub struct PreActivationState {
    pub already_urgent_ids: HashSet<i64>,
    pub previously_focused_id: Option<i64>,
}

pub fn capture_pre_activation_state() -> Option<PreActivationState> {
    let mut conn = Connection::new().ok()?;
    let tree = conn.get_tree().ok()?;
    let mut already_urgent_ids = HashSet::new();
    let mut previously_focused_id = None;
    for (node, _) in iter_leaf_windows(&tree) {
        if node.urgent {
            already_urgent_ids.insert(node.id);
        }
        if node.focused && previously_focused_id.is_none() {
            previously_focused_id = Some(node.id);
        }
    }
    Some(PreActivationState {
        already_urgent_ids,
        previously_focused_id,
    })
}

fn is_leaf_window(node: &Node) -> bool {
    matches!(node.node_type, NodeType::Con | NodeType::FloatingCon)
        && node.nodes.is_empty()
        && node.floating_nodes.is_empty()
        && (node.app_id.is_some() || node.window.is_some() || node.pid.is_some())
}

fn iter_leaf_windows(root: &Node) -> Vec<(&Node, bool)> {
    fn visit<'a>(node: &'a Node, in_scratchpad: bool, out: &mut Vec<(&'a Node, bool)>) {
        let scratch = in_scratchpad
            || matches!(node.name.as_deref(), Some("__i3" | "__i3_scratch"));
        if is_leaf_window(node) {
            out.push((node, scratch));
            return;
        }
        let mut seen = HashSet::new();
        for &child_id in &node.focus {
            if let Some(child) = node
                .nodes
                .iter()
                .chain(node.floating_nodes.iter())
                .find(|c| c.id == child_id)
            {
                seen.insert(child.id);
                visit(child, scratch, out);
            }
        }
        for child in node.nodes.iter().chain(node.floating_nodes.iter()) {
            if seen.insert(child.id) {
                visit(child, scratch, out);
            }
        }
    }

    let mut out = Vec::new();
    visit(root, false, &mut out);
    out
}

fn normalize_id(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = trimmed.strip_suffix(".desktop").unwrap_or(trimmed);
    stripped.to_ascii_lowercase()
}

fn reverse_dns_tail(id: &str) -> Option<&str> {
    let (_, tail) = id.rsplit_once('.')?;
    if tail.len() < 3
        || matches!(tail, "desktop" | "app" | "client" | "browser")
    {
        return None;
    }
    Some(tail)
}

fn matches_suffix_component(full: &str, part: &str) -> bool {
    if part.len() < 3 {
        return false;
    }
    if let Some(prefix) = full.strip_suffix(part) {
        if prefix.ends_with('.') {
            return !matches!(part, "desktop" | "app" | "client" | "browser");
        }
    }
    full.strip_prefix(part)
        .is_some_and(|rest| matches!(rest, "-browser" | "-desktop"))
}

fn is_variant_match(candidate: &str, key: &str) -> bool {
    !candidate.is_empty()
        && !key.is_empty()
        && (matches_suffix_component(candidate, key) || matches_suffix_component(key, candidate))
}

pub fn score_window(node: &Node, hints: &AppMatchHints, in_scratchpad: bool) -> u32 {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(ref id) = node.app_id {
        let n = normalize_id(id);
        if !n.is_empty() {
            candidates.push(n);
        }
    }
    if let Some(ref id) = node.sandbox_app_id {
        let n = normalize_id(id);
        if !n.is_empty() {
            candidates.push(n);
        }
    }
    if let Some(ref props) = node.window_properties {
        if let Some(ref class) = props.class {
            let n = normalize_id(class);
            if !n.is_empty() {
                candidates.push(n);
            }
        }
        if let Some(ref instance) = props.instance {
            let n = normalize_id(instance);
            if !n.is_empty() {
                candidates.push(n);
            }
        }
    }

    let desktop_entry = normalize_id(&hints.desktop_entry);
    let startup_wm_class = normalize_id(&hints.startup_wm_class);
    let exec_program = normalize_id(&hints.exec_program);
    let app_name = normalize_id(&hints.app_name);
    let app_slug = app_name.contains(' ').then(|| app_name.replace(' ', "-"));
    let desktop_tail = reverse_dns_tail(&desktop_entry);

    let mut base: u32 = 0;

    if let Some(direct_pid) = hints.pids.first().copied() {
        if node.pid == Some(direct_pid) {
            base = base.max(1000);
        }
    }

    for cand in &candidates {
        if (!desktop_entry.is_empty() && cand == &desktop_entry)
            || (!startup_wm_class.is_empty() && cand == &startup_wm_class)
        {
            base = base.max(800);
        }
        if (!exec_program.is_empty() && cand == &exec_program)
            || (!app_name.is_empty() && cand == &app_name)
            || app_slug.as_deref().is_some_and(|slug| cand == slug)
            || desktop_tail.is_some_and(|tail| cand == tail)
        {
            base = base.max(600);
        }
        for key in [
            desktop_entry.as_str(),
            startup_wm_class.as_str(),
            exec_program.as_str(),
            app_name.as_str(),
            app_slug.as_deref().unwrap_or(""),
        ] {
            if is_variant_match(cand, key) {
                base = base.max(450);
            }
        }
    }

    for (depth, pid) in hints.pids.iter().enumerate().skip(1) {
        if node.pid == Some(*pid) {
            base = base.max(300u32.saturating_sub((depth as u32) * 10));
        }
    }

    if base == 0 {
        return 0;
    }

    let mut score = base;
    if node.urgent {
        score += 500;
    }
    if !in_scratchpad {
        score += 20;
    }
    score
}

pub fn find_matching_window(root: &Node, hints: &AppMatchHints) -> Option<i64> {
    let mut best: Option<(i64, u32)> = None;
    for (node, in_scratchpad) in iter_leaf_windows(root) {
        let score = score_window(node, hints, in_scratchpad);
        if score == 0 {
            continue;
        }
        if best.is_none_or(|(_, best_score)| score > best_score) {
            best = Some((node.id, score));
        }
    }
    best.map(|(id, _)| id)
}

pub fn find_newly_urgent_window(
    root: &Node,
    already_urgent: &HashSet<i64>,
    hints: &AppMatchHints,
) -> Option<i64> {
    let mut first_urgent: Option<i64> = None;
    let mut best_scored: Option<(i64, u32)> = None;
    for (node, in_scratchpad) in iter_leaf_windows(root) {
        if !node.urgent || already_urgent.contains(&node.id) {
            continue;
        }
        if first_urgent.is_none() {
            first_urgent = Some(node.id);
        }
        let score = score_window(node, hints, in_scratchpad);
        if score > 0 && best_scored.is_none_or(|(_, best_score)| score > best_score) {
            best_scored = Some((node.id, score));
        }
    }
    best_scored.map(|(id, _)| id).or(first_urgent)
}

fn find_focused_window(root: &Node) -> Option<(&Node, bool)> {
    iter_leaf_windows(root)
        .into_iter()
        .find(|(node, _)| node.focused)
}

pub enum DefaultFocusOutcome {
    FocusedBeforeAction,
    NeedsPostAction(Option<PreActivationState>),
}

pub fn prepare_default_action_focus(hints: &AppMatchHints) -> DefaultFocusOutcome {
    let Ok(mut conn) = Connection::new() else {
        return DefaultFocusOutcome::NeedsPostAction(None);
    };
    let Ok(tree) = conn.get_tree() else {
        return DefaultFocusOutcome::NeedsPostAction(None);
    };
    let mut already_urgent_ids = HashSet::new();
    let mut previously_focused_id = None;
    let mut matched: Vec<(i64, bool)> = Vec::new();
    for (node, in_scratchpad) in iter_leaf_windows(&tree) {
        if node.urgent {
            already_urgent_ids.insert(node.id);
        }
        if node.focused && previously_focused_id.is_none() {
            previously_focused_id = Some(node.id);
        }
        if score_window(node, hints, in_scratchpad) > 0 {
            matched.push((node.id, node.focused));
        }
    }
    if let [(con_id, already_focused)] = matched.as_slice() {
        let con_id = *con_id;
        // Allow any open Notification Center popover to release KeyboardMode::Exclusive.
        thread::sleep(Duration::from_millis(15));
        if !already_focused {
            let _ = conn.run_command(format!("[con_id={con_id}] focus"));
            for _ in 0..5 {
                thread::sleep(Duration::from_millis(12));
                if let Ok(t) = conn.get_tree() {
                    if find_focused_window(&t).is_some_and(|(n, _)| n.id == con_id) {
                        break;
                    }
                }
            }
            thread::sleep(Duration::from_millis(15));
        }
        return DefaultFocusOutcome::FocusedBeforeAction;
    }
    DefaultFocusOutcome::NeedsPostAction(Some(PreActivationState {
        already_urgent_ids,
        previously_focused_id,
    }))
}

pub fn focus_window_immediately<F: FnOnce()>(hints: &AppMatchHints, on_focus: F) -> bool {
    let Ok(mut conn) = Connection::new() else {
        return false;
    };
    let Ok(tree) = conn.get_tree() else {
        return false;
    };
    let Some(con_id) = find_matching_window(&tree, hints) else {
        return false;
    };
    on_focus();
    command_error(conn.run_command(format!("[con_id={con_id}] focus"))).is_none()
}

pub fn run_post_action_focus<F: FnOnce()>(
    pre: Option<PreActivationState>,
    hints: &AppMatchHints,
    fallback_to_hints: bool,
    on_focus: F,
) {
    let Ok(mut conn) = Connection::new() else {
        return;
    };
    let pre = pre.unwrap_or(PreActivationState {
        already_urgent_ids: HashSet::new(),
        previously_focused_id: None,
    });
    let mut latest_tree: Option<Node> = None;
    for delay_ms in [25u64, 35, 50, 70] {
        thread::sleep(Duration::from_millis(delay_ms));
        let Ok(tree) = conn.get_tree() else {
            continue;
        };
        if let Some(con_id) =
            find_newly_urgent_window(&tree, &pre.already_urgent_ids, hints)
        {
            on_focus();
            let _ = conn.run_command(format!("[con_id={con_id}] focus"));
            return;
        }
        if let Some((focused_node, in_scratchpad)) = find_focused_window(&tree) {
            if pre
                .previously_focused_id
                .is_some_and(|prev| prev != focused_node.id)
                && score_window(focused_node, hints, in_scratchpad) > 0
            {
                on_focus();
                return;
            }
        }
        latest_tree = Some(tree);
    }

    if !fallback_to_hints {
        return;
    }
    let Some(tree) = latest_tree else {
        return;
    };
    if let Some(con_id) = find_matching_window(&tree, hints) {
        let current_focused = find_focused_window(&tree).map(|(n, _)| n.id);
        on_focus();
        if Some(con_id) != current_focused {
            let _ = conn.run_command(format!("[con_id={con_id}] focus"));
        }
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
        "available": true,
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

    fn make_tree_with_windows(windows: serde_json::Value) -> Node {
        serde_json::from_value(serde_json::json!({
            "id": 1, "name": "root", "type": "root", "border": "none",
            "current_border_width": 0, "layout": "splith", "orientation": "horizontal",
            "percent": null,
            "rect": { "x": 0, "y": 0, "width": 1920, "height": 1080 },
            "window_rect": { "x": 0, "y": 0, "width": 0, "height": 0 },
            "deco_rect": { "x": 0, "y": 0, "width": 0, "height": 0 },
            "geometry": { "x": 0, "y": 0, "width": 0, "height": 0 },
            "urgent": false, "focused": false, "focus": [], "floating": null,
            "nodes": windows, "floating_nodes": [], "sticky": false
        }))
        .expect("valid test Node")
    }

    fn make_leaf(
        id: i64,
        app_id: Option<&str>,
        pid: Option<i32>,
        urgent: bool,
        class: Option<&str>,
    ) -> serde_json::Value {
        let window_properties = class.map(|c| {
            serde_json::json!({
                "title": "Test",
                "instance": c,
                "class": c,
                "window_role": null,
                "window_type": null,
                "transient_for": null
            })
        });
        serde_json::json!({
            "id": id, "name": "win", "type": "con", "border": "none",
            "current_border_width": 0, "layout": "none", "orientation": "none",
            "percent": null,
            "rect": { "x": 0, "y": 0, "width": 800, "height": 600 },
            "window_rect": { "x": 0, "y": 0, "width": 800, "height": 600 },
            "deco_rect": { "x": 0, "y": 0, "width": 0, "height": 0 },
            "geometry": { "x": 0, "y": 0, "width": 0, "height": 0 },
            "urgent": urgent, "focused": false, "focus": [], "floating": null,
            "nodes": [], "floating_nodes": [], "sticky": false,
            "app_id": app_id, "pid": pid,
            "window": if class.is_some() { Some(id * 10) } else { None },
            "window_properties": window_properties
        })
    }

    #[test]
    fn matches_direct_pid_before_ancestor_pid() {
        let tree = make_tree_with_windows(serde_json::json!([
            make_leaf(10, Some("foot"), Some(100), false, None),
            make_leaf(20, Some("signal"), Some(200), false, None),
        ]));
        let hints = AppMatchHints {
            pids: vec![200, 100],
            ..Default::default()
        };
        assert_eq!(find_matching_window(&tree, &hints), Some(20));

        let ancestor_only = AppMatchHints {
            pids: vec![999, 100],
            ..Default::default()
        };
        assert_eq!(find_matching_window(&tree, &ancestor_only), Some(10));
    }

    #[test]
    fn newly_urgent_window_wins_over_mru_sibling() {
        let tree = make_tree_with_windows(serde_json::json!([
            make_leaf(10, Some("google-chrome"), Some(500), false, None),
            make_leaf(20, Some("google-chrome"), Some(500), true, None),
        ]));
        let hints = AppMatchHints {
            pids: vec![500],
            desktop_entry: "google-chrome".into(),
            app_name: "Google Chrome".into(),
            ..Default::default()
        };
        let already_urgent = HashSet::new();
        assert_eq!(
            find_newly_urgent_window(&tree, &already_urgent, &hints),
            Some(20)
        );
    }

    #[test]
    fn matches_slugified_app_name_and_xwayland_class() {
        let tree = make_tree_with_windows(serde_json::json!([
            make_leaf(11, Some("google-chrome"), Some(500), false, None),
            make_leaf(12, None, Some(600), false, Some("Slack")),
        ]));
        let chrome_hints = AppMatchHints {
            app_name: "Google Chrome".into(),
            ..Default::default()
        };
        assert_eq!(find_matching_window(&tree, &chrome_hints), Some(11));

        let slack_hints = AppMatchHints {
            startup_wm_class: "Slack".into(),
            ..Default::default()
        };
        assert_eq!(find_matching_window(&tree, &slack_hints), Some(12));
    }
}

//! Full `org.freedesktop.Notifications` D-Bus daemon using `zbus`.

use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gtk4::gdk;
use gtk4::gio::prelude::AppLaunchContextExt;
use gtk4::glib;
use gtk4::prelude::DisplayExt;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Serialize;
use zbus::blocking::connection::Builder as ConnectionBuilder;
use zbus::blocking::Connection;
use zbus::interface;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};

use crate::bridge::{BridgeEvent, SendEvent};
use crate::icons::{
    lookup_desktop_entry, lookup_desktop_entry_by_app_name, resolve_icon_or_path,
    rgba_image_data_to_png_data_url, DesktopEntryInfo, IconPalette, SharedIconPalette,
};
use crate::sway_ipc::{self, AppMatchHints};

const MAX_HISTORY: usize = 50;
const DEFAULT_EXPIRE_MS: u64 = 5000;
const RESUME_MIN_MS: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
#[serde(rename_all = "lowercase")]
pub enum NotificationUrgency {
    Low,
    Normal,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct NotificationAction {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct NotificationItem {
    pub id: u32,
    pub app_name: String,
    pub app_icon_name: String,
    pub app_icon_url: String,
    pub image_url: String,
    pub summary: String,
    pub body: String,
    pub urgency: NotificationUrgency,
    pub has_default_action: bool,
    pub actions: Vec<NotificationAction>,
    #[ts(type = "number")]
    pub timestamp_ms: u64,
    pub category: String,
    pub desktop_entry: String,
    pub progress: Option<u8>,
    pub transient: bool,
    pub resident: bool,
    pub popup: bool,
    pub read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct NotificationsState {
    pub available: bool,
    pub do_not_disturb: bool,
    pub unread_count: u32,
    pub items: Vec<NotificationItem>,
}

#[derive(Debug, Clone)]
enum TimerState {
    Sticky,
    Running { deadline: Instant },
    Paused { remaining: Duration },
}

#[derive(Clone)]
struct NotificationEntry {
    item: NotificationItem,
    sync_tag: String,
    raw_app_icon: String,
    raw_image_path: String,
    sender_pids: Vec<i32>,
    startup_wm_class: String,
    exec_program: String,
    resolved_desktop_id: String,
    timer: TimerState,
}

impl NotificationEntry {
    fn match_hints(&self) -> AppMatchHints {
        AppMatchHints {
            pids: self.sender_pids.clone(),
            desktop_entry: self.resolved_desktop_id.clone(),
            app_name: self.item.app_name.clone(),
            startup_wm_class: self.startup_wm_class.clone(),
            exec_program: self.exec_program.clone(),
        }
    }
}

struct StoreData {
    available: bool,
    do_not_disturb: bool,
    entries: Vec<NotificationEntry>,
    shutdown: bool,
}

impl StoreData {
    fn snapshot(&self) -> NotificationsState {
        let items: Vec<NotificationItem> =
            self.entries.iter().rev().map(|e| e.item.clone()).collect();
        let unread_count = items.iter().filter(|item| !item.read).count() as u32;
        NotificationsState {
            available: self.available,
            do_not_disturb: self.do_not_disturb,
            unread_count,
            items,
        }
    }
}

struct StoreInner {
    next_id: AtomicU32,
    data: Mutex<StoreData>,
    cvar: Condvar,
    send: SendEvent,
    conn: OnceLock<Connection>,
    theme: SharedIconPalette,
    focus_windows: bool,
}

struct StoreHandle {
    inner: Arc<StoreInner>,
}

impl Drop for StoreHandle {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.inner.data.lock() {
            guard.shutdown = true;
            self.inner.cvar.notify_all();
        }
    }
}

#[derive(Clone)]
pub struct NotificationStore {
    handle: Arc<StoreHandle>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn hint_string(hints: &HashMap<String, OwnedValue>, keys: &[&str]) -> String {
    for key in keys {
        if let Some(val) = hints.get(*key) {
            if let Value::Str(s) = &**val {
                return s.as_str().to_string();
            }
        }
    }
    String::new()
}

fn hint_bool(hints: &HashMap<String, OwnedValue>, key: &str) -> bool {
    hints.get(key).is_some_and(|v| match &**v {
        Value::Bool(b) => *b,
        Value::U8(n) => *n != 0,
        Value::I32(n) => *n != 0,
        Value::U32(n) => *n != 0,
        _ => false,
    })
}

fn hint_progress(hints: &HashMap<String, OwnedValue>) -> Option<u8> {
    let raw = hints.get("value").and_then(|v| match &**v {
        Value::U8(n) => Some(i64::from(*n)),
        Value::I32(n) => Some(i64::from(*n)),
        Value::U32(n) => Some(i64::from(*n)),
        Value::I64(n) => Some(*n),
        Value::U64(n) => i64::try_from(*n).ok(),
        _ => None,
    })?;
    Some(raw.clamp(0, 100) as u8)
}

fn hint_sender_pid(hints: &HashMap<String, OwnedValue>) -> Option<u32> {
    hints.get("sender-pid").and_then(|v| match &**v {
        Value::U32(n) if *n > 0 => Some(*n),
        Value::I32(n) if *n > 0 => Some(*n as u32),
        Value::U64(n) if *n > 0 => u32::try_from(*n).ok(),
        Value::I64(n) if *n > 0 => u32::try_from(*n).ok(),
        _ => None,
    })
}

fn parse_proc_stat_ppid(stat: &str) -> Option<i32> {
    let close_paren = stat.rfind(')')?;
    let rest = stat.get(close_paren + 1..)?.trim_start();
    // Fields after `(comm)` are: state (0), ppid (1), ...
    let ppid = rest.split_whitespace().nth(1)?.parse::<i32>().ok()?;
    (ppid > 1).then_some(ppid)
}

fn collect_pid_chain(initial_pid: u32) -> Vec<i32> {
    let mut chain = Vec::new();
    let Some(mut pid) = i32::try_from(initial_pid).ok().filter(|&p| p > 1) else {
        return chain;
    };
    for _ in 0..16 {
        if pid <= 1 || chain.contains(&pid) {
            break;
        }
        chain.push(pid);
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            break;
        };
        let Some(ppid) = parse_proc_stat_ppid(&stat) else {
            break;
        };
        pid = ppid;
    }
    chain
}

fn mint_wayland_activation_token() -> Option<String> {
    if !gtk4::is_initialized() {
        return None;
    }
    let mint_on_main = || {
        let display = gdk::Display::default()?;
        let ctx = display.app_launch_context();
        ctx.startup_notify_id(None::<&gtk4::gio::AppInfo>, &[])
            .map(Into::into)
            .filter(|s: &String| !s.is_empty())
    };
    if glib::MainContext::default().is_owner() {
        mint_on_main()
    } else {
        let (tx, rx) = mpsc::sync_channel(1);
        glib::idle_add_once(move || {
            let _ = tx.send(mint_on_main());
        });
        rx.recv_timeout(Duration::from_millis(200)).ok().flatten()
    }
}

pub fn parse_urgency(hints: &HashMap<String, OwnedValue>) -> NotificationUrgency {
    let raw = hints.get("urgency").and_then(|v| match &**v {
        Value::U8(n) => Some(*n),
        Value::I32(n) => u8::try_from(*n).ok(),
        Value::U32(n) => u8::try_from(*n).ok(),
        _ => None,
    });
    match raw {
        Some(0) => NotificationUrgency::Low,
        Some(2) => NotificationUrgency::Critical,
        _ => NotificationUrgency::Normal,
    }
}

pub fn parse_actions(raw: &[String]) -> (bool, Vec<NotificationAction>) {
    let mut has_default = false;
    let mut actions = Vec::new();
    for pair in raw.as_chunks::<2>().0 {
        let key = &pair[0];
        if key.is_empty() {
            continue;
        }
        if key == "default" {
            has_default = true;
        } else {
            actions.push(NotificationAction {
                key: key.clone(),
                label: pair[1].clone(),
            });
        }
    }
    (has_default, actions)
}

type RawImageData = (i32, i32, i32, bool, i32, i32, Vec<u8>);

fn parse_image_data_hint(hints: &HashMap<String, OwnedValue>, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(val) = hints.get(*key) else {
            continue;
        };
        if let Ok((w, h, rowstride, has_alpha, bps, channels, bytes)) =
            RawImageData::try_from(val.clone())
        {
            if let Some(url) =
                rgba_image_data_to_png_data_url(w, h, rowstride, has_alpha, bps, channels, &bytes)
            {
                return Some(url);
            }
        }
    }
    None
}

pub fn strip_markup(raw: &str) -> String {
    if !raw.contains('<') && !raw.contains('&') {
        return raw.to_string();
    }
    let mut reader = Reader::from_str(raw);
    reader.config_mut().check_end_names = false;
    reader.config_mut().check_comments = false;

    let mut out = String::with_capacity(raw.len());
    loop {
        match reader.read_event() {
            Ok(Event::Text(t)) => {
                if let Ok(unescaped) = t.unescape() {
                    out.push_str(&unescaped);
                } else {
                    out.push_str(&String::from_utf8_lossy(t.as_ref()));
                }
            }
            Ok(Event::CData(c)) => {
                out.push_str(&String::from_utf8_lossy(c.as_ref()));
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if e.local_name().as_ref().eq_ignore_ascii_case(b"br") {
                    out.push('\n');
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return raw.to_string(),
        }
    }
    out
}

fn initial_timer(urgency: NotificationUrgency, expire_timeout: i32, popup: bool) -> TimerState {
    if !popup || urgency == NotificationUrgency::Critical || expire_timeout == 0 {
        return TimerState::Sticky;
    }
    let ms = if expire_timeout > 0 {
        expire_timeout as u64
    } else {
        DEFAULT_EXPIRE_MS
    };
    TimerState::Running {
        deadline: Instant::now() + Duration::from_millis(ms),
    }
}

fn resolve_entry_icons(
    raw_app_icon: &str,
    desktop_entry: &str,
    raw_image_path: &str,
    palette: &IconPalette,
) -> (String, String, Option<String>, Option<DesktopEntryInfo>) {
    let desktop_info = if !desktop_entry.is_empty() {
        lookup_desktop_entry(desktop_entry)
    } else {
        None
    };

    let app_icon_name = if !raw_app_icon.is_empty() {
        raw_app_icon.to_string()
    } else {
        desktop_info
            .as_ref()
            .and_then(|info| info.icon.clone())
            .unwrap_or_default()
    };

    let app_icon_url =
        resolve_icon_or_path(&app_icon_name, "", 24, palette).unwrap_or_default();

    let image_url = if !raw_image_path.is_empty() {
        resolve_icon_or_path(raw_image_path, "", 64, palette)
    } else {
        None
    };

    (app_icon_name, app_icon_url, image_url, desktop_info)
}

impl NotificationStore {
    pub fn new(send: SendEvent, theme: SharedIconPalette) -> Self {
        Self::with_focus_windows(send, theme, true)
    }

    fn with_focus_windows(send: SendEvent, theme: SharedIconPalette, focus_windows: bool) -> Self {
        let inner = Arc::new(StoreInner {
            next_id: AtomicU32::new(1),
            data: Mutex::new(StoreData {
                available: false,
                do_not_disturb: false,
                entries: Vec::new(),
                shutdown: false,
            }),
            cvar: Condvar::new(),
            send,
            conn: OnceLock::new(),
            theme,
            focus_windows,
        });
        let store = Self {
            handle: Arc::new(StoreHandle {
                inner: Arc::clone(&inner),
            }),
        };
        spawn_timer_thread(inner);
        store
    }

    #[cfg(test)]
    pub fn test_new(send: SendEvent) -> Self {
        Self::with_focus_windows(send, SharedIconPalette::default(), false)
    }

    fn emit_snapshot(&self, snapshot: NotificationsState) {
        (self.handle.inner.send)(BridgeEvent::NotificationsState(snapshot));
    }

    fn emit_close_center(&self) {
        (self.handle.inner.send)(BridgeEvent::HostAction {
            action: "notifications_close_center".to_string(),
        });
    }

    fn emit_closed(&self, id: u32, reason: u32) {
        emit_closed_on(&self.handle.inner.conn, id, reason);
    }

    pub fn set_available(&self, available: bool) {
        let mut guard = self.handle.inner.data.lock().unwrap();
        guard.available = available;
        let snapshot = guard.snapshot();
        drop(guard);
        self.emit_snapshot(snapshot);
    }

    fn alloc_id(&self, replaces_id: u32) -> u32 {
        if replaces_id != 0 {
            self.handle
                .inner
                .next_id
                .fetch_max(replaces_id.saturating_add(1), Ordering::Relaxed);
            replaces_id
        } else {
            self.handle.inner.next_id.fetch_add(1, Ordering::Relaxed)
        }
    }

    #[cfg(test)]
    pub fn push(&self, replaces_id: u32, app_name: String, summary: String, body: String) -> u32 {
        self.push_notification(
            app_name,
            replaces_id,
            String::new(),
            summary,
            body,
            Vec::new(),
            HashMap::new(),
            -1,
        )
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn push_notification(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        self.push_notification_with_sender(
            app_name,
            replaces_id,
            app_icon,
            summary,
            body,
            actions,
            hints,
            expire_timeout,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_notification_with_sender(
        &self,
        mut app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
        sender_pid: Option<u32>,
    ) -> u32 {
        let urgency = parse_urgency(&hints);
        let (has_default_action, actions) = parse_actions(&actions);
        let category = hint_string(&hints, &["category"]);
        let desktop_entry = hint_string(&hints, &["desktop-entry"]);
        let transient = hint_bool(&hints, "transient");
        let resident = hint_bool(&hints, "resident");
        let progress = hint_progress(&hints);
        let sync_tag = hint_string(&hints, &["synchronous", "x-canonical-private-synchronous"]);
        let raw_image_path = hint_string(&hints, &["image-path", "image_path"]);
        let image_data_url = parse_image_data_hint(&hints, &["image-data", "image_data"]);

        let palette = self.handle.inner.theme.palette();
        let (app_icon_name, app_icon_url, resolved_image_path_url, desktop_info) =
            resolve_entry_icons(&app_icon, &desktop_entry, &raw_image_path, &palette);
        if app_name.is_empty() {
            if let Some(name) = desktop_info.as_ref().and_then(|info| info.name.clone()) {
                app_name = name;
            }
        }
        let match_desktop_info =
            desktop_info.or_else(|| lookup_desktop_entry_by_app_name(&app_name));
        let startup_wm_class = match_desktop_info
            .as_ref()
            .and_then(|info| info.startup_wm_class.clone())
            .unwrap_or_default();
        let exec_program = match_desktop_info
            .as_ref()
            .and_then(|info| info.exec_program.clone())
            .unwrap_or_default();
        let resolved_desktop_id = match_desktop_info
            .and_then(|info| info.desktop_id)
            .unwrap_or_else(|| desktop_entry.clone());

        let mut sender_pids = sender_pid
            .or_else(|| hint_sender_pid(&hints))
            .map(collect_pid_chain)
            .unwrap_or_default();

        let stored_image_path = if image_data_url.is_none() && resolved_image_path_url.is_some() {
            raw_image_path
        } else {
            String::new()
        };
        let image_url = image_data_url
            .or(resolved_image_path_url)
            .or_else(|| {
                if app_icon_url.is_empty() {
                    parse_image_data_hint(&hints, &["icon_data"])
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let clean_body = strip_markup(&body);
        let ts = now_ms();

        let mut guard = self.handle.inner.data.lock().unwrap();
        let popup = !guard.do_not_disturb || urgency == NotificationUrgency::Critical;

        let existing_idx = if replaces_id != 0 {
            guard.entries.iter().position(|e| e.item.id == replaces_id)
        } else if !sync_tag.is_empty() {
            guard.entries.iter().position(|e| e.sync_tag == sync_tag)
        } else {
            None
        };

        if !popup && transient {
            let id = if let Some(idx) = existing_idx {
                let removed = guard.entries.remove(idx).item.id;
                let snapshot = guard.snapshot();
                self.handle.inner.cvar.notify_all();
                drop(guard);
                self.emit_snapshot(snapshot);
                self.emit_closed(removed, 1);
                return removed;
            } else {
                self.alloc_id(replaces_id)
            };
            drop(guard);
            self.emit_closed(id, 1);
            return id;
        }

        let mut timer = initial_timer(urgency, expire_timeout, popup);
        let id = existing_idx
            .map(|idx| guard.entries[idx].item.id)
            .unwrap_or_else(|| self.alloc_id(replaces_id));

        if let Some(idx) = existing_idx {
            if sender_pids.is_empty()
                && !guard.entries[idx].sender_pids.is_empty()
                && guard.entries[idx].item.app_name == app_name
            {
                sender_pids = guard.entries[idx].sender_pids.clone();
            }
            if matches!(guard.entries[idx].timer, TimerState::Paused { .. }) {
                if let TimerState::Running { deadline } = timer {
                    timer = TimerState::Paused {
                        remaining: deadline.saturating_duration_since(Instant::now()),
                    };
                }
            }
        }

        let entry = NotificationEntry {
            item: NotificationItem {
                id,
                app_name,
                app_icon_name,
                app_icon_url,
                image_url,
                summary,
                body: clean_body,
                urgency,
                has_default_action,
                actions,
                timestamp_ms: ts,
                category,
                desktop_entry,
                progress,
                transient,
                resident,
                popup,
                read: false,
            },
            sync_tag,
            raw_app_icon: app_icon,
            raw_image_path: stored_image_path,
            sender_pids,
            startup_wm_class,
            exec_program,
            resolved_desktop_id,
            timer,
        };

        let mut evicted_ids = Vec::new();
        if let Some(idx) = existing_idx {
            guard.entries[idx] = entry;
        } else {
            guard.entries.push(entry);
            if guard.entries.len() > MAX_HISTORY {
                let drain = guard.entries.len() - MAX_HISTORY;
                evicted_ids.extend(guard.entries.drain(0..drain).map(|e| e.item.id));
            }
        }

        let snapshot = guard.snapshot();
        self.handle.inner.cvar.notify_all();
        drop(guard);

        self.emit_snapshot(snapshot);
        for evicted_id in evicted_ids {
            self.emit_closed(evicted_id, 4);
        }
        id
    }

    pub fn hide_popup(&self, id: u32) {
        let mut guard = self.handle.inner.data.lock().unwrap();
        let Some(idx) = guard.entries.iter().position(|e| e.item.id == id) else {
            return;
        };
        if guard.entries[idx].item.transient {
            guard.entries.remove(idx);
            let snapshot = guard.snapshot();
            self.handle.inner.cvar.notify_all();
            drop(guard);
            self.emit_snapshot(snapshot);
            self.emit_closed(id, 2);
            return;
        }
        if !guard.entries[idx].item.popup {
            return;
        }
        guard.entries[idx].item.popup = false;
        guard.entries[idx].timer = TimerState::Sticky;
        let snapshot = guard.snapshot();
        self.handle.inner.cvar.notify_all();
        drop(guard);
        self.emit_snapshot(snapshot);
    }

    pub fn dismiss(&self, id: u32) -> bool {
        self.remove_with_reason(id, 2)
    }

    pub fn close_by_app(&self, id: u32) -> bool {
        self.remove_with_reason(id, 3)
    }

    fn remove_with_reason(&self, id: u32, reason: u32) -> bool {
        let mut guard = self.handle.inner.data.lock().unwrap();
        let before = guard.entries.len();
        guard.entries.retain(|e| e.item.id != id);
        let removed = guard.entries.len() != before;
        if removed {
            let snapshot = guard.snapshot();
            self.handle.inner.cvar.notify_all();
            drop(guard);
            self.emit_snapshot(snapshot);
            self.emit_closed(id, reason);
        }
        removed
    }

    pub fn dismiss_latest(&self) {
        let latest_id = {
            let guard = self.handle.inner.data.lock().unwrap();
            guard
                .entries
                .iter()
                .rev()
                .find(|e| e.item.popup)
                .or_else(|| guard.entries.last())
                .map(|e| e.item.id)
        };
        if let Some(id) = latest_id {
            self.dismiss(id);
        }
    }

    fn emit_action_signals_and_dismiss(
        &self,
        id: u32,
        action_key: &str,
        token: Option<&str>,
        is_resident: bool,
    ) {
        if let Some(iface) = server_iface(&self.handle.inner.conn) {
            if let Some(token) = token {
                let _ = async_io::block_on(NotificationsServer::activation_token(
                    iface.signal_emitter(),
                    id,
                    token,
                ));
            }
            let _ = async_io::block_on(NotificationsServer::action_invoked(
                iface.signal_emitter(),
                id,
                action_key,
            ));
        }
        if !is_resident {
            self.dismiss(id);
        }
    }

    pub fn invoke_action(&self, id: u32, action_key: &str) {
        let Some((is_resident, match_hints)) = ({
            let guard = self.handle.inner.data.lock().unwrap();
            guard
                .entries
                .iter()
                .find(|e| e.item.id == id)
                .map(|e| (e.item.resident, e.match_hints()))
        }) else {
            return;
        };

        let is_default = action_key == "default";
        if !self.handle.inner.focus_windows {
            if is_default {
                self.emit_close_center();
            }
            self.emit_action_signals_and_dismiss(id, action_key, None, is_resident);
            return;
        }

        let token = mint_wayland_activation_token();
        if is_default {
            self.emit_close_center();
        }
        let store = self.clone();
        let action_key = action_key.to_string();
        thread::spawn(move || {
            let outcome = if is_default {
                sway_ipc::prepare_default_action_focus(&match_hints)
            } else {
                sway_ipc::DefaultFocusOutcome::NeedsPostAction(
                    sway_ipc::capture_pre_activation_state(),
                )
            };
            store.emit_action_signals_and_dismiss(
                id,
                &action_key,
                token.as_deref(),
                is_resident,
            );
            if let sway_ipc::DefaultFocusOutcome::NeedsPostAction(pre) = outcome {
                sway_ipc::run_post_action_focus(pre, &match_hints, is_default, || {
                    if !is_default {
                        store.emit_close_center();
                    }
                });
            }
        });
    }

    pub fn activate(&self, id: u32) {
        let target = {
            let guard = self.handle.inner.data.lock().unwrap();
            guard
                .entries
                .iter()
                .find(|e| e.item.id == id)
                .map(|e| (e.item.has_default_action, e.item.popup, e.match_hints()))
        };
        match target {
            Some((true, _, _)) => self.invoke_action(id, "default"),
            Some((false, popup, match_hints)) => {
                if popup {
                    self.hide_popup(id);
                }
                if self.handle.inner.focus_windows {
                    let store = self.clone();
                    thread::spawn(move || {
                        sway_ipc::focus_window_immediately(&match_hints, || {
                            store.emit_close_center();
                        });
                    });
                }
            }
            _ => {}
        }
    }

    pub fn set_do_not_disturb(&self, enabled: bool) {
        let mut guard = self.handle.inner.data.lock().unwrap();
        if guard.do_not_disturb == enabled {
            return;
        }
        guard.do_not_disturb = enabled;
        let snapshot = guard.snapshot();
        drop(guard);
        self.emit_snapshot(snapshot);
    }

    pub fn toggle_do_not_disturb(&self) {
        let mut guard = self.handle.inner.data.lock().unwrap();
        guard.do_not_disturb = !guard.do_not_disturb;
        let snapshot = guard.snapshot();
        drop(guard);
        self.emit_snapshot(snapshot);
    }

    pub fn mark_all_read(&self) {
        let mut guard = self.handle.inner.data.lock().unwrap();
        let mut changed = false;
        for entry in &mut guard.entries {
            if !entry.item.read {
                entry.item.read = true;
                changed = true;
            }
        }
        if !changed {
            return;
        }
        let snapshot = guard.snapshot();
        drop(guard);
        self.emit_snapshot(snapshot);
    }

    pub fn clear_all(&self) {
        let mut guard = self.handle.inner.data.lock().unwrap();
        if guard.entries.is_empty() {
            return;
        }
        let ids: Vec<u32> = guard.entries.iter().map(|e| e.item.id).collect();
        guard.entries.clear();
        let snapshot = guard.snapshot();
        self.handle.inner.cvar.notify_all();
        drop(guard);
        self.emit_snapshot(snapshot);
        for id in ids {
            self.emit_closed(id, 2);
        }
    }

    pub fn pause_timeout(&self, id: u32) {
        let mut guard = self.handle.inner.data.lock().unwrap();
        let Some(entry) = guard.entries.iter_mut().find(|e| e.item.id == id) else {
            return;
        };
        if let TimerState::Running { deadline } = entry.timer {
            entry.timer = TimerState::Paused {
                remaining: deadline.saturating_duration_since(Instant::now()),
            };
        }
    }

    pub fn resume_timeout(&self, id: u32) {
        let mut guard = self.handle.inner.data.lock().unwrap();
        let Some(entry) = guard.entries.iter_mut().find(|e| e.item.id == id) else {
            return;
        };
        if resume_paused_entries(std::slice::from_mut(entry)) {
            self.handle.inner.cvar.notify_all();
        }
    }

    pub fn resume_all_paused(&self) {
        let mut guard = self.handle.inner.data.lock().unwrap();
        if resume_paused_entries(&mut guard.entries) {
            self.handle.inner.cvar.notify_all();
        }
    }

    pub fn refresh_icons(&self) {
        let sources: Vec<(u32, String, String, String)> = {
            let guard = self.handle.inner.data.lock().unwrap();
            guard
                .entries
                .iter()
                .map(|e| {
                    (
                        e.item.id,
                        e.raw_app_icon.clone(),
                        e.item.desktop_entry.clone(),
                        e.raw_image_path.clone(),
                    )
                })
                .collect()
        };
        if sources.is_empty() {
            return;
        }

        let palette = self.handle.inner.theme.palette();
        let resolved: Vec<(u32, String, String, Option<String>)> = sources
            .into_iter()
            .map(|(id, raw_app, desktop_entry, raw_img)| {
                let (icon_name, icon_url, img_url, _) =
                    resolve_entry_icons(&raw_app, &desktop_entry, &raw_img, &palette);
                (id, icon_name, icon_url, img_url)
            })
            .collect();

        let mut guard = self.handle.inner.data.lock().unwrap();
        let mut changed = false;
        for (id, icon_name, icon_url, img_url) in resolved {
            if let Some(entry) = guard.entries.iter_mut().find(|e| e.item.id == id) {
                if entry.item.app_icon_name != icon_name || entry.item.app_icon_url != icon_url {
                    entry.item.app_icon_name = icon_name;
                    entry.item.app_icon_url = icon_url;
                    changed = true;
                }
                if !entry.raw_image_path.is_empty() {
                    let next_img = img_url.unwrap_or_default();
                    if entry.item.image_url != next_img {
                        entry.item.image_url = next_img;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            return;
        }
        let snapshot = guard.snapshot();
        drop(guard);
        self.emit_snapshot(snapshot);
    }
}

fn resume_paused_entries(entries: &mut [NotificationEntry]) -> bool {
    let now = Instant::now();
    let mut resumed = false;
    for entry in entries {
        if let TimerState::Paused { remaining } = entry.timer {
            if entry.item.popup {
                let dur = remaining.max(Duration::from_millis(RESUME_MIN_MS));
                entry.timer = TimerState::Running {
                    deadline: now + dur,
                };
                resumed = true;
            }
        }
    }
    resumed
}

fn server_iface(
    conn_lock: &OnceLock<Connection>,
) -> Option<zbus::blocking::object_server::InterfaceRef<NotificationsServer>> {
    conn_lock
        .get()?
        .object_server()
        .interface::<_, NotificationsServer>("/org/freedesktop/Notifications")
        .ok()
}

fn emit_closed_on(conn_lock: &OnceLock<Connection>, id: u32, reason: u32) {
    if let Some(iface) = server_iface(conn_lock) {
        let _ = async_io::block_on(NotificationsServer::notification_closed(
            iface.signal_emitter(),
            id,
            reason,
        ));
    }
}

fn spawn_timer_thread(inner: Arc<StoreInner>) {
    thread::spawn(move || {
        let mut guard = inner.data.lock().unwrap();
        loop {
            if guard.shutdown {
                break;
            }
            let now = Instant::now();
            let mut expired_transient_ids = Vec::new();
            let mut changed = false;

            let mut idx = 0;
            while idx < guard.entries.len() {
                let expired = matches!(
                    guard.entries[idx].timer,
                    TimerState::Running { deadline } if guard.entries[idx].item.popup && deadline <= now
                );
                if expired {
                    changed = true;
                    if guard.entries[idx].item.transient {
                        let id = guard.entries.remove(idx).item.id;
                        expired_transient_ids.push(id);
                        continue;
                    } else {
                        guard.entries[idx].item.popup = false;
                        guard.entries[idx].timer = TimerState::Sticky;
                    }
                }
                idx += 1;
            }

            if changed {
                let snapshot = guard.snapshot();
                drop(guard);
                (inner.send)(BridgeEvent::NotificationsState(snapshot));
                for id in expired_transient_ids {
                    emit_closed_on(&inner.conn, id, 1);
                }
                guard = inner.data.lock().unwrap();
                continue;
            }

            let next_deadline = guard
                .entries
                .iter()
                .filter(|e| e.item.popup)
                .filter_map(|e| match e.timer {
                    TimerState::Running { deadline } => Some(deadline),
                    _ => None,
                })
                .min();

            guard = match next_deadline {
                Some(deadline) => {
                    let wait = deadline.saturating_duration_since(Instant::now());
                    if wait.is_zero() {
                        continue;
                    }
                    inner.cvar.wait_timeout(guard, wait).unwrap().0
                }
                None => inner.cvar.wait(guard).unwrap(),
            };
        }
    });
}

struct NotificationsServer {
    store: NotificationStore,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationsServer {
    fn get_capabilities(&self) -> Vec<String> {
        vec![
            "body".to_string(),
            "body-markup".to_string(),
            "persistence".to_string(),
            "actions".to_string(),
            "icon-static".to_string(),
        ]
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> u32 {
        let sender_pid = if let Some(pid) = hint_sender_pid(&hints) {
            Some(pid)
        } else if let Some(sender) = header.sender() {
            conn.call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "GetConnectionUnixProcessID",
                &(sender.as_str(),),
            )
            .await
            .ok()
            .and_then(|msg| msg.body().deserialize::<u32>().ok())
        } else {
            None
        };
        self.store.push_notification_with_sender(
            app_name,
            replaces_id,
            app_icon,
            summary,
            body,
            actions,
            hints,
            expire_timeout,
            sender_pid,
        )
    }

    fn close_notification(&self, id: u32) {
        self.store.close_by_app(id);
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "webbar".to_string(),
            "webbar".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            "1.2".to_string(),
        )
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn activation_token(
        emitter: &SignalEmitter<'_>,
        id: u32,
        activation_token: &str,
    ) -> zbus::Result<()>;
}

pub fn spawn_server(store: NotificationStore) {
    thread::spawn(move || {
        let server = NotificationsServer {
            store: store.clone(),
        };
        match ConnectionBuilder::session()
            .and_then(|b| b.name("org.freedesktop.Notifications"))
            .and_then(|b| b.serve_at("/org/freedesktop/Notifications", server))
            .and_then(|b| b.build())
        {
            Ok(conn) => {
                let _ = store.handle.inner.conn.set(conn);
                store.set_available(true);
            }
            Err(err) => {
                store.set_available(false);
                eprintln!(
                    "webbar: could not claim org.freedesktop.Notifications ({err}). \
                     If another daemon (mako/dunst/swaync) is running, stop it first to see notifications here."
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture() -> (SendEvent, Arc<Mutex<Vec<NotificationsState>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let log_clone = log.clone();
        let send: SendEvent = Arc::new(move |event| {
            if let BridgeEvent::NotificationsState(state) = event {
                log_clone.lock().unwrap().push(state);
            }
        });
        (send, log)
    }

    #[test]
    fn assigns_sequential_ids_and_emits_snapshots() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        let id1 = store.push(0, "app1".into(), "sum1".into(), "body1".into());
        let id2 = store.push(0, "app2".into(), "sum2".into(), "body2".into());
        let id3 = store.push(0, "app3".into(), "sum3".into(), "body3".into());

        assert_eq!((id1, id2, id3), (1, 2, 3));

        let snapshots = log.lock().unwrap();
        assert_eq!(snapshots.len(), 3);
        let ids: Vec<u32> = snapshots[2].items.iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![3, 2, 1]);
        assert_eq!(snapshots[2].unread_count, 3);
    }

    #[test]
    fn replaces_id_and_synchronous_tags_update_in_place() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        let id1 = store.push(0, "mail".into(), "New mail".into(), "From Alice".into());
        let id2 = store.push(0, "chat".into(), "Ping".into(), "Hey".into());
        let replaced = store.push(
            id1,
            "mail".into(),
            "2 new mails".into(),
            "From Alice & Bob".into(),
        );
        assert_eq!(replaced, id1);

        let mut hints1 = HashMap::new();
        hints1.insert(
            "synchronous".into(),
            OwnedValue::from(zbus::zvariant::Str::from("volume")),
        );
        hints1.insert("value".into(), OwnedValue::from(40i32));
        let sync1 = store.push_notification(
            "vol".into(),
            0,
            String::new(),
            "Volume".into(),
            String::new(),
            Vec::new(),
            hints1,
            1000,
        );

        let mut hints2 = HashMap::new();
        hints2.insert(
            "x-canonical-private-synchronous".into(),
            OwnedValue::from(zbus::zvariant::Str::from("volume")),
        );
        hints2.insert("value".into(), OwnedValue::from(150i32));
        let sync2 = store.push_notification(
            "vol".into(),
            0,
            String::new(),
            "Volume".into(),
            String::new(),
            Vec::new(),
            hints2,
            1000,
        );
        assert_eq!(sync2, sync1);

        let snapshots = log.lock().unwrap();
        let latest = snapshots.last().unwrap();
        assert_eq!(latest.items.len(), 3);
        assert_eq!(latest.items[0].id, sync1);
        assert_eq!(latest.items[0].progress, Some(100));
        assert_eq!(latest.items[1].id, id2);
        assert_eq!(latest.items[2].id, id1);
        assert_eq!(latest.items[2].summary, "2 new mails");
    }

    #[test]
    fn dismiss_and_clear_all_and_mark_read_work() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        assert!(!store.dismiss(42));
        store.clear_all();
        assert_eq!(log.lock().unwrap().len(), 0);

        let id1 = store.push(0, "a".into(), "s1".into(), "b1".into());
        let id2 = store.push(0, "b".into(), "s2".into(), "b2".into());

        store.mark_all_read();
        assert_eq!(log.lock().unwrap().last().unwrap().unread_count, 0);

        assert!(store.dismiss(id1));
        assert!(!store.dismiss(id1));
        assert_eq!(log.lock().unwrap().last().unwrap().items[0].id, id2);

        store.clear_all();
        assert!(log.lock().unwrap().last().unwrap().items.is_empty());
    }

    #[test]
    fn caps_history_at_max_history_dropping_oldest() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        let total = MAX_HISTORY + 5;
        for i in 0..total {
            store.push(0, "app".into(), format!("sum {i}"), String::new());
        }

        let snapshots = log.lock().unwrap();
        let latest = snapshots.last().unwrap();
        assert_eq!(latest.items.len(), MAX_HISTORY);
        assert_eq!(latest.items.first().unwrap().id, total as u32);
        assert_eq!(latest.items.last().unwrap().id, 6);
    }

    #[test]
    fn dnd_suppresses_normal_popups_but_allows_critical() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        store.set_do_not_disturb(true);
        store.push_notification(
            "mail".into(),
            0,
            String::new(),
            "Normal".into(),
            String::new(),
            Vec::new(),
            HashMap::new(),
            5000,
        );

        let mut crit_hints = HashMap::new();
        crit_hints.insert("urgency".into(), OwnedValue::from(2u8));
        store.push_notification(
            "power".into(),
            0,
            String::new(),
            "Critical".into(),
            String::new(),
            Vec::new(),
            crit_hints,
            5000,
        );

        let snapshots = log.lock().unwrap();
        let latest = snapshots.last().unwrap();
        assert!(latest.do_not_disturb);
        assert_eq!(latest.items.len(), 2);
        assert!(latest.items[0].popup);
        assert!(!latest.items[1].popup);
    }

    #[test]
    fn popup_expires_keeps_persistent_and_drops_transient_and_supports_pause() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        // Persistent with short timeout
        let id_persist = store.push_notification(
            "app".into(),
            0,
            String::new(),
            "Persist".into(),
            String::new(),
            Vec::new(),
            HashMap::new(),
            40,
        );

        // Transient with short timeout
        let mut t_hints = HashMap::new();
        t_hints.insert("transient".into(), OwnedValue::from(true));
        let id_transient = store.push_notification(
            "app".into(),
            0,
            String::new(),
            "Transient".into(),
            String::new(),
            Vec::new(),
            t_hints,
            40,
        );

        // Paused item should survive past its initial 150ms timeout
        let id_paused = store.push_notification(
            "app".into(),
            0,
            String::new(),
            "Paused".into(),
            String::new(),
            Vec::new(),
            HashMap::new(),
            150,
        );
        store.pause_timeout(id_paused);

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let done = {
                let snapshots = log.lock().unwrap();
                let latest = snapshots.last().unwrap();
                let persist_expired = latest
                    .items
                    .iter()
                    .find(|n| n.id == id_persist)
                    .is_some_and(|n| !n.popup);
                let transient_gone = latest.items.iter().all(|n| n.id != id_transient);
                if persist_expired && transient_gone {
                    let paused = latest.items.iter().find(|n| n.id == id_paused).unwrap();
                    assert!(paused.popup);
                    true
                } else {
                    false
                }
            };
            if done {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "short-timeout notifications did not expire within 2s"
            );
            thread::sleep(Duration::from_millis(20));
        }

        store.resume_timeout(id_paused);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let expired = {
                let snapshots = log.lock().unwrap();
                snapshots
                    .last()
                    .and_then(|s| s.items.iter().find(|n| n.id == id_paused))
                    .is_some_and(|n| !n.popup)
            };
            if expired {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "paused notification did not expire within 2s after resume"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn parses_hints_image_data_and_strips_markup() {
        let mut hints = HashMap::new();
        hints.insert("urgency".into(), OwnedValue::from(2u8));
        let pixel_data = vec![255u8; 4 * 4 * 4];
        hints.insert(
            "image-data".into(),
            OwnedValue::try_from(Value::from((4i32, 4i32, 16i32, true, 8i32, 4i32, pixel_data)))
                .unwrap(),
        );

        let (send, log) = capture();
        let store = NotificationStore::test_new(send);
        store.push_notification(
            "chat".into(),
            0,
            String::new(),
            "Hi".into(),
            "<b>Hello</b> &amp; <a href=\"https://example.com\">link</a><br/>next".into(),
            vec![
                "default".into(),
                "Open".into(),
                "reply".into(),
                "Reply".into(),
                "dangling".into(),
            ],
            hints,
            -1,
        );

        let snapshots = log.lock().unwrap();
        let item = &snapshots.last().unwrap().items[0];
        assert_eq!(item.urgency, NotificationUrgency::Critical);
        assert_eq!(item.body, "Hello & link\nnext");
        assert!(item.image_url.starts_with("data:image/png;base64,"));
        assert!(item.has_default_action);
        assert_eq!(
            item.actions,
            vec![NotificationAction {
                key: "reply".into(),
                label: "Reply".into(),
            }]
        );
    }

    #[test]
    fn hide_popup_keeps_persistent_and_removes_transient() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        let id_persist = store.push(0, "app".into(), "Persist".into(), String::new());
        let mut t_hints = HashMap::new();
        t_hints.insert("transient".into(), OwnedValue::from(true));
        let id_transient = store.push_notification(
            "app".into(),
            0,
            String::new(),
            "Transient".into(),
            String::new(),
            Vec::new(),
            t_hints,
            -1,
        );

        store.hide_popup(id_persist);
        store.hide_popup(id_transient);

        let snapshots = log.lock().unwrap();
        let latest = snapshots.last().unwrap();
        assert_eq!(latest.items.len(), 1);
        assert_eq!(latest.items[0].id, id_persist);
        assert!(!latest.items[0].popup);
    }

    #[test]
    fn invoke_action_dismisses_non_resident_and_preserves_resident() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        let id_ephemeral = store.push(0, "app".into(), "Normal".into(), String::new());
        let mut r_hints = HashMap::new();
        r_hints.insert("resident".into(), OwnedValue::from(true));
        let id_resident = store.push_notification(
            "app".into(),
            0,
            String::new(),
            "Resident".into(),
            String::new(),
            vec!["open".into(), "Open".into()],
            r_hints,
            -1,
        );

        store.invoke_action(id_ephemeral, "open");
        store.invoke_action(id_resident, "open");

        let snapshots = log.lock().unwrap();
        let latest = snapshots.last().unwrap();
        assert_eq!(latest.items.len(), 1);
        assert_eq!(latest.items[0].id, id_resident);
    }

    #[test]
    fn activate_invokes_default_or_hides_popup_without_default() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        let id_default = store.push_notification(
            "chat".into(),
            0,
            String::new(),
            "With default".into(),
            String::new(),
            vec!["default".into(), "Open".into()],
            HashMap::new(),
            -1,
        );
        let id_no_default = store.push(0, "app".into(), "No default".into(), String::new());

        // Activating with default action invokes & dismisses (non-resident)
        store.activate(id_default);
        // Activating popup without default hides popup while keeping history
        store.activate(id_no_default);
        // Activating again when no longer popup is a no-op
        store.activate(id_no_default);

        let snapshots = log.lock().unwrap();
        let latest = snapshots.last().unwrap();
        assert_eq!(latest.items.len(), 1);
        assert_eq!(latest.items[0].id, id_no_default);
        assert!(!latest.items[0].popup);
    }

    #[test]
    fn resume_all_paused_restarts_paused_timers() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        let id = store.push_notification(
            "app".into(),
            0,
            String::new(),
            "Paused".into(),
            String::new(),
            Vec::new(),
            HashMap::new(),
            150,
        );
        store.pause_timeout(id);
        thread::sleep(Duration::from_millis(200));
        assert!(log.lock().unwrap().last().unwrap().items[0].popup);

        // Replacing a paused notification preserves its paused state
        store.push_notification(
            "app".into(),
            id,
            String::new(),
            "Paused Updated".into(),
            String::new(),
            Vec::new(),
            HashMap::new(),
            150,
        );
        thread::sleep(Duration::from_millis(200));
        assert!(log.lock().unwrap().last().unwrap().items[0].popup);

        store.resume_all_paused();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if !log.lock().unwrap().last().unwrap().items[0].popup {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "resume_all_paused did not expire notification within 2s"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn dismiss_latest_prefers_active_popup_then_falls_back_to_history() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        let _id1 = store.push(0, "app".into(), "First".into(), String::new());
        let id2 = store.push(0, "app".into(), "Second".into(), String::new());
        store.hide_popup(id2);

        // id1 is still an active popup while id2 is hidden in history -> id1 dismissed first
        store.dismiss_latest();
        {
            let snapshots = log.lock().unwrap();
            let latest = snapshots.last().unwrap();
            assert_eq!(latest.items.len(), 1);
            assert_eq!(latest.items[0].id, id2);
        }

        // No active popups remain -> falls back to newest history item (id2)
        store.dismiss_latest();
        assert!(log.lock().unwrap().last().unwrap().items.is_empty());
    }

    #[test]
    fn dnd_drops_transient_and_removes_existing_replaced_entry() {
        let (send, log) = capture();
        let store = NotificationStore::test_new(send);

        let id1 = store.push(0, "app".into(), "Existing".into(), String::new());
        store.set_do_not_disturb(true);

        // New transient under DND is dropped without entering history
        let mut t1 = HashMap::new();
        t1.insert("transient".into(), OwnedValue::from(true));
        store.push_notification(
            "app".into(),
            0,
            String::new(),
            "Dropped".into(),
            String::new(),
            Vec::new(),
            t1,
            -1,
        );
        assert_eq!(log.lock().unwrap().last().unwrap().items.len(), 1);

        // Replacing existing entry with transient:true under DND removes the old entry
        let mut t2 = HashMap::new();
        t2.insert("transient".into(), OwnedValue::from(true));
        store.push_notification(
            "app".into(),
            id1,
            String::new(),
            "Replaced Transient".into(),
            String::new(),
            Vec::new(),
            t2,
            -1,
        );
        assert!(log.lock().unwrap().last().unwrap().items.is_empty());
    }

    #[test]
    fn refresh_icons_recolors_svg_app_icon_on_theme_change() {
        use std::fs;
        use base64::engine::general_purpose::STANDARD as BASE64;
        use base64::Engine;
        use crate::theme::ThemeState;

        let dir = std::env::temp_dir().join(format!("webbar-notif-icon-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let svg_path = dir.join("notif-test.svg");
        fs::write(
            &svg_path,
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><style id="current-color-scheme">.ColorScheme-Text { color:#dfdfdf; }</style><path class="ColorScheme-Text" fill="currentColor" d="M0 0h24v24H0z"/></svg>"#,
        )
        .unwrap();

        let (send, log) = capture();
        let palette = SharedIconPalette::new(&ThemeState {
            dark: true,
            ..ThemeState::default()
        });
        let store = NotificationStore::new(send, palette.clone());

        store.push_notification(
            "app".into(),
            0,
            svg_path.to_string_lossy().to_string(),
            "Icon Test".into(),
            String::new(),
            Vec::new(),
            HashMap::new(),
            -1,
        );

        let initial_url = log.lock().unwrap().last().unwrap().items[0]
            .app_icon_url
            .clone();
        assert!(palette.update(&ThemeState {
            dark: false,
            ..ThemeState::default()
        }));
        store.refresh_icons();

        let updated_url = log.lock().unwrap().last().unwrap().items[0]
            .app_icon_url
            .clone();
        assert_ne!(initial_url, updated_url);
        let b64 = updated_url
            .strip_prefix("data:image/svg+xml;base64,")
            .expect("svg data url");
        let decoded = String::from_utf8(BASE64.decode(b64).unwrap()).unwrap();
        assert!(decoded.contains(".ColorScheme-Text { color: #18181f; }"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_proc_stat_ppid_with_spaces_and_parens() {
        assert_eq!(
            parse_proc_stat_ppid("1234 (Web Content (2)) S 999 100 100 0 -1 4194304"),
            Some(999)
        );
        assert_eq!(parse_proc_stat_ppid("1 (systemd) S 0 1 1"), None);
        assert_eq!(parse_proc_stat_ppid("malformed"), None);
    }

    #[test]
    fn extracts_sender_pid_hint_variants() {
        let mut hints = HashMap::new();
        hints.insert("sender-pid".into(), OwnedValue::from(4242u32));
        assert_eq!(hint_sender_pid(&hints), Some(4242));

        hints.insert("sender-pid".into(), OwnedValue::from(5151i32));
        assert_eq!(hint_sender_pid(&hints), Some(5151));

        hints.insert("sender-pid".into(), OwnedValue::from(0i32));
        assert_eq!(hint_sender_pid(&hints), None);
    }
}

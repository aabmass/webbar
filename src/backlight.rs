//! Monitor and internal panel brightness control.
//!
//! - External monitors: `com.ddcutil.DdcutilService` on the session D-Bus,
//!   enumerating all connected displays via `Detect` and matching EDIDs to DRM
//!   connector names (`/sys/class/drm/card*-<connector>/edid`).
//! - Internal laptop panels: `/sys/class/backlight/*` for reading state and
//!   `org.freedesktop.login1.Session.SetBrightness` on the system D-Bus for
//!   unprivileged writes, watched via `udevadm monitor`.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use futures_util::stream::StreamExt;
use serde::Serialize;
use ts_rs::TS;
use zbus::{Connection, MatchRule, MessageStream};

use crate::bridge::{reply_err, reply_ok, BridgeEvent, SendEvent};

const DDC_BUS: &str = "com.ddcutil.DdcutilService";
const DDC_PATH: &str = "/com/ddcutil/DdcutilObject";
const DDC_IFACE: &str = "com.ddcutil.DdcutilInterface";
const VCP_BRIGHTNESS: u8 = 0x10;
const DDC_FLAG_NO_VERIFY: u32 = 4;

const LOGIN1_BUS: &str = "org.freedesktop.login1";
const LOGIN1_SESSION_PATH: &str = "/org/freedesktop/login1/session/auto";
const LOGIN1_SESSION_IFACE: &str = "org.freedesktop.login1.Session";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../preload/src/generated/")]
pub enum BacklightKind {
    Ddc,
    Panel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct BacklightDisplay {
    pub id: String,
    pub name: String,
    pub kind: BacklightKind,
    pub output: Option<String>,
    pub brightness: Option<u8>,
    pub available: bool,
    pub error: Option<String>,
}

#[derive(Clone)]
enum DisplayBackend {
    Panel {
        device_name: String,
        sysfs_dir: PathBuf,
        max_brightness: u32,
    },
    Ddc {
        display_number: i32,
        edid_txt: String,
        max_vcp: u16,
    },
}

#[derive(Clone)]
struct DisplayEntry {
    info: BacklightDisplay,
    backend: DisplayBackend,
}

#[derive(Default)]
struct BacklightRegistry {
    panels: Vec<DisplayEntry>,
    ddc: Vec<DisplayEntry>,
    ddc_error: Option<String>,
}

impl BacklightRegistry {
    fn all_entries(&self) -> impl Iterator<Item = &DisplayEntry> {
        self.panels.iter().chain(self.ddc.iter())
    }

    fn find_mut(&mut self, id: &str) -> Option<&mut DisplayEntry> {
        self.panels
            .iter_mut()
            .chain(self.ddc.iter_mut())
            .find(|e| e.info.id == id)
    }

    fn snapshot(&self) -> BridgeEvent {
        let displays: Vec<BacklightDisplay> =
            self.all_entries().map(|e| e.info.clone()).collect();
        let primary = displays.iter().find(|d| d.available);
        let available = primary.is_some();
        let brightness = primary.and_then(|d| d.brightness);
        let error = if available {
            None
        } else {
            displays
                .iter()
                .find_map(|d| d.error.clone())
                .or_else(|| self.ddc_error.clone())
        };

        BridgeEvent::BacklightState {
            available,
            brightness,
            error,
            displays,
        }
    }

    fn resolve_target(&self, display_id: Option<&str>) -> Option<(String, DisplayBackend)> {
        let entry = match display_id {
            Some(id) => self.all_entries().find(|e| e.info.id == id),
            None => self.all_entries().find(|e| e.info.available),
        }?;
        Some((entry.info.id.clone(), entry.backend.clone()))
    }
}

struct SetRequest {
    req_id: u32,
    percent: u8,
    display_id: Option<String>,
    reply_send: SendEvent,
}

#[derive(Clone)]
pub struct BacklightController {
    tx: Sender<SetRequest>,
}

fn raw_to_percent(current: u32, max: u32) -> u8 {
    if max == 0 {
        return 0;
    }
    ((f64::from(current) / f64::from(max)) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u8
}

fn percent_to_raw(percent: u8, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    let clamped = f64::from(percent.min(100));
    ((clamped / 100.0) * f64::from(max))
        .round()
        .clamp(0.0, f64::from(max)) as u32
}

fn extract_drm_connector(dir_name: &str) -> Option<String> {
    let rest = dir_name.strip_prefix("card")?;
    let dash = rest.find('-')?;
    let card_num = &rest[..dash];
    if card_num.is_empty() || !card_num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let connector = &rest[dash + 1..];
    if connector.is_empty() {
        None
    } else {
        Some(connector.to_string())
    }
}

fn decode_edid_txt(edid_txt: &str) -> Option<Vec<u8>> {
    let trimmed = edid_txt.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Check hex first (256+ pure hex chars is also valid base64, so check hex before base64).
    if trimmed.len() >= 256
        && trimmed.len().is_multiple_of(2)
        && trimmed.bytes().all(|b| b.is_ascii_hexdigit())
    {
        let mut out = Vec::with_capacity(trimmed.len() / 2);
        for chunk in trimmed.as_bytes().as_chunks::<2>().0 {
            let s = std::str::from_utf8(chunk).ok()?;
            out.push(u8::from_str_radix(s, 16).ok()?);
        }
        return Some(out);
    }
    // ddcutil-service returns base64-encoded EDID bytes.
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(trimmed) {
        if bytes.len() >= 128 {
            return Some(bytes);
        }
    }
    None
}

fn scan_drm_edids(drm_root: &Path) -> Vec<(String, Vec<u8>)> {
    let Ok(entries) = fs::read_dir(drm_root) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let Some(connector) = extract_drm_connector(name_str) else {
            continue;
        };
        let edid_path = entry.path().join("edid");
        if let Ok(bytes) = fs::read(&edid_path) {
            if bytes.len() >= 128 {
                result.push((connector, bytes));
            }
        }
    }
    result
}

fn match_edid_to_connector(edid_txt: &str, drm_edids: &[(String, Vec<u8>)]) -> Option<String> {
    let decoded = decode_edid_txt(edid_txt)?;
    let prefix_len = decoded.len().min(128);
    let prefix = &decoded[..prefix_len];
    drm_edids
        .iter()
        .find(|(_, edid)| edid.len() >= prefix_len && &edid[..prefix_len] == prefix)
        .map(|(connector, _)| connector.clone())
}

fn read_u32_file(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse::<u32>().ok()
}

fn read_panel_brightness(panel_dir: &Path, max_brightness: u32) -> Option<u8> {
    let current = read_u32_file(&panel_dir.join("actual_brightness"))
        .or_else(|| read_u32_file(&panel_dir.join("brightness")))?;
    Some(raw_to_percent(current, max_brightness))
}

fn infer_panel_output(panel_dir: &Path) -> Option<String> {
    let device_path = fs::canonicalize(panel_dir.join("device")).ok()?;
    let file_name = device_path.file_name()?.to_str()?;
    extract_drm_connector(file_name)
}

fn read_panel_entry(panel_dir: &Path) -> Option<DisplayEntry> {
    let device_name = panel_dir.file_name()?.to_str()?.to_string();
    let max_brightness = read_u32_file(&panel_dir.join("max_brightness"))?;
    if max_brightness == 0 {
        return None;
    }
    let brightness = read_panel_brightness(panel_dir, max_brightness);
    Some(DisplayEntry {
        info: BacklightDisplay {
            id: format!("panel:{device_name}"),
            name: device_name.clone(),
            kind: BacklightKind::Panel,
            output: infer_panel_output(panel_dir),
            brightness,
            available: brightness.is_some(),
            error: None,
        },
        backend: DisplayBackend::Panel {
            device_name,
            sysfs_dir: panel_dir.to_path_buf(),
            max_brightness,
        },
    })
}

fn scan_sysfs_panels(backlight_root: &Path) -> Vec<DisplayEntry> {
    let Ok(entries) = fs::read_dir(backlight_root) else {
        return Vec::new();
    };
    let mut panels: Vec<DisplayEntry> = entries
        .flatten()
        .filter_map(|entry| read_panel_entry(&entry.path()))
        .collect();
    panels.sort_by(|a, b| a.info.name.cmp(&b.info.name));
    panels
}

fn refresh_panels(registry: &Arc<Mutex<BacklightRegistry>>) -> bool {
    let mut guard = registry.lock().unwrap();
    let mut changed = false;
    for entry in &mut guard.panels {
        let DisplayBackend::Panel {
            sysfs_dir,
            max_brightness,
            ..
        } = &entry.backend
        else {
            continue;
        };
        let next = read_panel_brightness(sysfs_dir, *max_brightness);
        if entry.info.brightness != next {
            entry.info.brightness = next;
            entry.info.available = next.is_some();
            changed = true;
        }
    }
    changed
}

fn emit_snapshot(registry: &Arc<Mutex<BacklightRegistry>>, send: &SendEvent) {
    let event = registry.lock().unwrap().snapshot();
    send(event);
}

type DetectedDisplayTuple = (i32, i32, i32, String, String, String, u16, String, u32);

async fn ddc_detect(conn: &Connection) -> Result<Vec<DetectedDisplayTuple>, String> {
    let reply = conn
        .call_method(Some(DDC_BUS), DDC_PATH, Some(DDC_IFACE), "Detect", &(0u32,))
        .await
        .map_err(|e| e.to_string())?;
    let (_count, displays, err_status, err_msg) = reply
        .body()
        .deserialize::<(i32, Vec<DetectedDisplayTuple>, i32, String)>()
        .map_err(|e| e.to_string())?;
    if err_status != 0 {
        return Err(format!("ddcutil Detect ({err_status}): {err_msg}"));
    }
    Ok(displays)
}

async fn detect_and_query_ddc(
    conn: &Connection,
    registry: &Arc<Mutex<BacklightRegistry>>,
    send: &SendEvent,
) {
    let drm_edids = scan_drm_edids(Path::new("/sys/class/drm"));

    let detected = match ddc_detect(conn).await {
        Ok(displays) => displays,
        Err(err) => {
            let mut guard = registry.lock().unwrap();
            guard.ddc.clear();
            guard.ddc_error = Some(err);
            drop(guard);
            emit_snapshot(registry, send);
            return;
        }
    };

    let mut ddc_entries = Vec::new();
    for (disp_num, _usb_bus, _usb_dev, _mfg, model_name, _serial, _prod, edid_txt, _bin_serial) in
        detected
    {
        if disp_num <= 0 {
            continue;
        }
        let output = match_edid_to_connector(&edid_txt, &drm_edids);
        let name = if model_name.trim().is_empty() {
            format!("Display {disp_num}")
        } else {
            model_name.trim().to_string()
        };

        let mut max_vcp = 100u16;
        let mut brightness = None;
        let mut available = false;
        let mut error = None;

        match conn
            .call_method(
                Some(DDC_BUS),
                DDC_PATH,
                Some(DDC_IFACE),
                "GetVcp",
                &(disp_num, "", VCP_BRIGHTNESS, 0u32),
            )
            .await
        {
            Ok(reply) => {
                if let Ok((current, max, _fmt, err_status, err_msg)) =
                    reply.body().deserialize::<(u16, u16, String, i32, String)>()
                {
                    if err_status != 0 {
                        error = Some(format!("ddcutil GetVcp ({err_status}): {err_msg}"));
                    } else {
                        max_vcp = if max > 0 { max } else { 100 };
                        brightness = Some(raw_to_percent(u32::from(current), u32::from(max_vcp)));
                        available = true;
                    }
                }
            }
            Err(err) => {
                error = Some(err.to_string());
            }
        }

        ddc_entries.push(DisplayEntry {
            info: BacklightDisplay {
                id: format!("ddc:{disp_num}"),
                name,
                kind: BacklightKind::Ddc,
                output,
                brightness,
                available,
                error,
            },
            backend: DisplayBackend::Ddc {
                display_number: disp_num,
                edid_txt,
                max_vcp,
            },
        });
    }

    {
        let mut guard = registry.lock().unwrap();
        guard.ddc = ddc_entries;
        guard.ddc_error = None;
    }
    emit_snapshot(registry, send);
}

fn spawn_panel_watcher(registry: Arc<Mutex<BacklightRegistry>>, send: SendEvent) {
    thread::spawn(move || {
        let Ok(mut child) = Command::new("udevadm")
            .args(["monitor", "--udev", "--subsystem-match=backlight"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            return;
        };
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if line.contains("backlight") && line.contains("change") && refresh_panels(&registry) {
                emit_snapshot(&registry, &send);
            }
        }
        let _ = child.wait();
    });
}

fn apply_brightness(
    backend: &DisplayBackend,
    percent: u8,
    session_conn: Option<&zbus::blocking::Connection>,
    system_conn: Option<&zbus::blocking::Connection>,
) -> Result<(), String> {
    match backend {
        DisplayBackend::Panel {
            device_name,
            max_brightness,
            ..
        } => {
            let conn = system_conn.ok_or_else(|| "system D-Bus connection unavailable".to_string())?;
            let raw = percent_to_raw(percent, *max_brightness);
            conn.call_method(
                Some(LOGIN1_BUS),
                LOGIN1_SESSION_PATH,
                Some(LOGIN1_SESSION_IFACE),
                "SetBrightness",
                &("backlight", device_name.as_str(), raw),
            )
            .map(|_| ())
            .map_err(|e| format!("logind SetBrightness: {e}"))
        }
        DisplayBackend::Ddc {
            display_number,
            max_vcp,
            ..
        } => {
            let conn =
                session_conn.ok_or_else(|| "session D-Bus connection unavailable".to_string())?;
            let value = percent_to_raw(percent, u32::from(*max_vcp)) as u16;
            let reply = conn
                .call_method(
                    Some(DDC_BUS),
                    DDC_PATH,
                    Some(DDC_IFACE),
                    "SetVcp",
                    &(
                        *display_number,
                        "",
                        VCP_BRIGHTNESS,
                        value,
                        DDC_FLAG_NO_VERIFY,
                    ),
                )
                .map_err(|e| e.to_string())?;
            let (status, msg) = reply
                .body()
                .deserialize::<(i32, String)>()
                .map_err(|e| e.to_string())?;
            if status == 0 {
                Ok(())
            } else {
                Err(format!("ddcutil SetVcp ({status}): {msg}"))
            }
        }
    }
}

pub fn spawn(send: SendEvent) -> BacklightController {
    let (tx, rx) = mpsc::channel::<SetRequest>();
    let registry = Arc::new(Mutex::new(BacklightRegistry::default()));

    // Scan internal panels synchronously so initial snapshot is ready right away
    let panels = scan_sysfs_panels(Path::new("/sys/class/backlight"));
    if !panels.is_empty() {
        registry.lock().unwrap().panels = panels;
        emit_snapshot(&registry, &send);
        spawn_panel_watcher(registry.clone(), send.clone());
    }

    // DDC watcher thread: Detect + initial GetVcp + VcpValueChanged & ConnectedDisplaysChanged
    {
        let send = send.clone();
        let registry = registry.clone();
        thread::spawn(move || {
            async_io::block_on(async move {
                let conn = match Connection::session().await {
                    Ok(c) => c,
                    Err(err) => {
                        registry.lock().unwrap().ddc_error = Some(err.to_string());
                        emit_snapshot(&registry, &send);
                        return;
                    }
                };

                detect_and_query_ddc(&conn, &registry, &send).await;

                let Ok(rule) = MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .sender(DDC_BUS)
                    .and_then(|b| b.path(DDC_PATH))
                    .and_then(|b| b.interface(DDC_IFACE))
                    .map(|b| b.build())
                else {
                    return;
                };

                let Ok(mut stream) = MessageStream::for_match_rule(rule, &conn, Some(64)).await
                else {
                    return;
                };

                while let Some(Ok(msg)) = stream.next().await {
                    let member = msg.header().member().map(|m| m.as_str().to_string());
                    match member.as_deref() {
                        Some("VcpValueChanged") => {
                            if let Ok((disp, edid, vcp_code, new_val, _s1, _s2, _flags)) = msg
                                .body()
                                .deserialize::<(i32, String, u8, u16, String, String, u32)>()
                            {
                                if vcp_code == VCP_BRIGHTNESS {
                                    let updated = {
                                        let mut guard = registry.lock().unwrap();
                                        let target = guard.ddc.iter_mut().find(|e| {
                                            matches!(
                                                &e.backend,
                                                DisplayBackend::Ddc {
                                                    display_number,
                                                    edid_txt,
                                                    ..
                                                } if (disp > 0 && *display_number == disp)
                                                    || (!edid.is_empty() && edid_txt == &edid)
                                            )
                                        });
                                        if let Some(entry) = target {
                                            if let DisplayBackend::Ddc { max_vcp, .. } =
                                                entry.backend
                                            {
                                                let pct = raw_to_percent(
                                                    u32::from(new_val),
                                                    u32::from(max_vcp),
                                                );
                                                entry.info.brightness = Some(pct);
                                                entry.info.available = true;
                                                entry.info.error = None;
                                                true
                                            } else {
                                                false
                                            }
                                        } else {
                                            false
                                        }
                                    };
                                    if updated {
                                        emit_snapshot(&registry, &send);
                                    }
                                }
                            }
                        }
                        Some("ConnectedDisplaysChanged") => {
                            detect_and_query_ddc(&conn, &registry, &send).await;
                        }
                        _ => {}
                    }
                }
            });
        });
    }

    // Debounced setter thread (coalesces per target display over 80ms)
    {
        let send = send.clone();
        let registry = registry.clone();
        thread::spawn(move || {
            let session_conn = zbus::blocking::Connection::session().ok();
            let system_conn = zbus::blocking::Connection::system().ok();

            type PendingBatch = (u8, Vec<(u32, SendEvent)>);
            while let Ok(first) = rx.recv() {
                let mut batches: HashMap<Option<String>, PendingBatch> = HashMap::new();
                batches.insert(
                    first.display_id,
                    (first.percent, vec![(first.req_id, first.reply_send)]),
                );

                let deadline = Instant::now() + Duration::from_millis(80);
                loop {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match rx.recv_timeout(remaining) {
                        Ok(req) => {
                            let entry = batches
                                .entry(req.display_id)
                                .or_insert_with(|| (req.percent, Vec::new()));
                            entry.0 = req.percent;
                            entry.1.push((req.req_id, req.reply_send));
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }

                for (display_id, (percent, pending_reqs)) in batches {
                    let target = registry.lock().unwrap().resolve_target(display_id.as_deref());

                    let Some((target_id, backend)) = target else {
                        let msg = match display_id {
                            Some(id) => format!("backlight display not found: {id}"),
                            None => "no available backlight display".to_string(),
                        };
                        for (id, reply_send) in pending_reqs {
                            reply_err(&reply_send, id, &msg);
                        }
                        continue;
                    };

                    let result = apply_brightness(
                        &backend,
                        percent,
                        session_conn.as_ref(),
                        system_conn.as_ref(),
                    );
                    {
                        let mut guard = registry.lock().unwrap();
                        if let Some(entry) = guard.find_mut(&target_id) {
                            match &result {
                                Ok(()) => {
                                    entry.info.brightness = Some(percent);
                                    entry.info.available = true;
                                    entry.info.error = None;
                                }
                                Err(err) => {
                                    entry.info.error = Some(err.clone());
                                }
                            }
                        }
                    }
                    emit_snapshot(&registry, &send);

                    for (id, reply_send) in pending_reqs {
                        match &result {
                            Ok(()) => reply_ok(&reply_send, id),
                            Err(err) => reply_err(&reply_send, id, err),
                        }
                    }
                }
            }
        });
    }

    BacklightController { tx }
}

impl BacklightController {
    pub fn set_brightness(
        &self,
        req_id: u32,
        percent: f64,
        display_id: Option<String>,
        reply_send: SendEvent,
    ) {
        let clamped = if percent.is_nan() {
            0
        } else {
            percent.round().clamp(0.0, 100.0) as u8
        };
        let _ = self.tx.send(SetRequest {
            req_id,
            percent: clamped,
            display_id,
            reply_send,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_raw_and_percent_with_clamping() {
        assert_eq!(raw_to_percent(50, 100), 50);
        assert_eq!(raw_to_percent(128, 255), 50);
        assert_eq!(raw_to_percent(48000, 96000), 50);
        assert_eq!(raw_to_percent(10, 0), 0);

        assert_eq!(percent_to_raw(50, 200), 100);
        assert_eq!(percent_to_raw(50, 96000), 48000);
        assert_eq!(percent_to_raw(150, 1000), 1000);
        assert_eq!(percent_to_raw(25, 0), 0);
    }

    #[test]
    fn extracts_drm_connector_names() {
        assert_eq!(
            extract_drm_connector("card0-eDP-1").as_deref(),
            Some("eDP-1")
        );
        assert_eq!(
            extract_drm_connector("card1-HDMI-A-1").as_deref(),
            Some("HDMI-A-1")
        );
        assert_eq!(extract_drm_connector("card0"), None);
        assert_eq!(extract_drm_connector("renderD128"), None);
    }

    #[test]
    fn matches_base64_and_hex_edids_to_drm_connectors() {
        let raw_edid: Vec<u8> = (0u8..128u8).collect();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&raw_edid);
        let hex: String = raw_edid.iter().map(|b| format!("{b:02x}")).collect();
        let drm = vec![("DP-2".to_string(), raw_edid)];

        assert_eq!(match_edid_to_connector(&b64, &drm).as_deref(), Some("DP-2"));
        assert_eq!(match_edid_to_connector(&hex, &drm).as_deref(), Some("DP-2"));
        assert_eq!(match_edid_to_connector("not-an-edid", &drm), None);
    }

    #[test]
    fn registry_snapshot_and_target_resolution() {
        let mut reg = BacklightRegistry {
            ddc_error: Some("ddc unavailable".to_string()),
            ..Default::default()
        };

        // No displays available -> reports ddc_error and no default target
        let snap = reg.snapshot();
        assert!(matches!(
            snap,
            BridgeEvent::BacklightState {
                available: false,
                brightness: None,
                error: Some(ref err),
                ref displays,
            } if err == "ddc unavailable" && displays.is_empty()
        ));
        assert!(reg.resolve_target(None).is_none());

        // Add unavailable panel + available DDC display
        reg.panels.push(DisplayEntry {
            info: BacklightDisplay {
                id: "panel:intel_backlight".to_string(),
                name: "intel_backlight".to_string(),
                kind: BacklightKind::Panel,
                output: Some("eDP-1".to_string()),
                brightness: None,
                available: false,
                error: Some("unreadable".to_string()),
            },
            backend: DisplayBackend::Panel {
                device_name: "intel_backlight".to_string(),
                sysfs_dir: PathBuf::from("/sys/class/backlight/intel_backlight"),
                max_brightness: 1000,
            },
        });
        reg.ddc.push(DisplayEntry {
            info: BacklightDisplay {
                id: "ddc:2".to_string(),
                name: "Dell U2723QE".to_string(),
                kind: BacklightKind::Ddc,
                output: Some("DP-1".to_string()),
                brightness: Some(72),
                available: true,
                error: None,
            },
            backend: DisplayBackend::Ddc {
                display_number: 2,
                edid_txt: String::new(),
                max_vcp: 100,
            },
        });

        let snap = reg.snapshot();
        assert!(matches!(
            snap,
            BridgeEvent::BacklightState {
                available: true,
                brightness: Some(72),
                error: None,
                ref displays,
            } if displays.len() == 2
        ));

        // Default target picks first available display (ddc:2)
        let (default_id, _) = reg.resolve_target(None).expect("default target");
        assert_eq!(default_id, "ddc:2");

        // Explicit display_id targets that display even if panel is unavailable
        let (explicit_id, _) = reg
            .resolve_target(Some("panel:intel_backlight"))
            .expect("explicit panel target");
        assert_eq!(explicit_id, "panel:intel_backlight");

        // Unknown display_id returns None
        assert!(reg.resolve_target(Some("ddc:99")).is_none());
    }

    #[test]
    fn scans_sysfs_panels_and_falls_back_from_actual_brightness() {
        let root = std::env::temp_dir().join(format!("webbar-backlight-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        let p1 = root.join("amdgpu_bl0");
        fs::create_dir_all(&p1).unwrap();
        fs::write(p1.join("max_brightness"), "255\n").unwrap();
        fs::write(p1.join("brightness"), "10\n").unwrap();
        fs::write(p1.join("actual_brightness"), "128\n").unwrap();

        let p2 = root.join("intel_backlight");
        fs::create_dir_all(&p2).unwrap();
        fs::write(p2.join("max_brightness"), "1000\n").unwrap();
        fs::write(p2.join("brightness"), "250\n").unwrap();

        let p_zero = root.join("broken_bl");
        fs::create_dir_all(&p_zero).unwrap();
        fs::write(p_zero.join("max_brightness"), "0\n").unwrap();
        fs::write(p_zero.join("brightness"), "0\n").unwrap();

        let panels = scan_sysfs_panels(&root);
        assert_eq!(panels.len(), 2);

        // Sorted by device name; actual_brightness takes precedence over brightness
        assert_eq!(panels[0].info.id, "panel:amdgpu_bl0");
        assert_eq!(panels[0].info.brightness, Some(50));
        assert!(panels[0].info.available);

        // Falls back to brightness when actual_brightness is absent
        assert_eq!(panels[1].info.id, "panel:intel_backlight");
        assert_eq!(panels[1].info.brightness, Some(25));
        assert!(panels[1].info.available);

        let _ = fs::remove_dir_all(&root);
    }
}

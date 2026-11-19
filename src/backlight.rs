//! Monitor brightness via `com.ddcutil.DdcutilService` on the session D-Bus.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures_util::stream::StreamExt;
use zbus::{Connection, MatchRule, MessageStream};

use crate::bridge::{reply_err, reply_ok, BridgeEvent, SendEvent};

const DDC_BUS: &str = "com.ddcutil.DdcutilService";
const DDC_PATH: &str = "/com/ddcutil/DdcutilObject";
const DDC_IFACE: &str = "com.ddcutil.DdcutilInterface";
const VCP_BRIGHTNESS: u8 = 0x10;
const DISPLAY_NUMBER: i32 = 1;
const DDC_FLAG_NO_VERIFY: u32 = 4;

#[derive(Clone)]
pub struct BacklightController {
    tx: Sender<(u32, u8, SendEvent)>,
}

pub fn vcp_to_percent(current: u16, max: u16) -> u8 {
    let denom = if max == 0 { 100.0 } else { f64::from(max) };
    ((f64::from(current) / denom) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u8
}

pub fn percent_to_vcp(percent: u8, max: u16) -> u16 {
    let max_val = if max == 0 { 100 } else { max };
    let clamped = f64::from(percent.min(100));
    ((clamped / 100.0) * f64::from(max_val))
        .round()
        .clamp(0.0, f64::from(max_val)) as u16
}

pub fn spawn(send: SendEvent) -> BacklightController {
    let (tx, rx) = mpsc::channel::<(u32, u8, SendEvent)>();
    let max_vcp = Arc::new(Mutex::new(100u16));
    let current_brightness = Arc::new(Mutex::new(None::<u8>));

    // Watcher thread: initial GetVcp + VcpValueChanged signal stream
    {
        let send = send.clone();
        let max_vcp = max_vcp.clone();
        let current_brightness = current_brightness.clone();
        thread::spawn(move || {
            async_io::block_on(async move {
                let conn = match Connection::session().await {
                    Ok(c) => c,
                    Err(err) => {
                        send(BridgeEvent::BacklightState {
                            available: false,
                            brightness: None,
                            error: Some(err.to_string()),
                        });
                        return;
                    }
                };

                // Fetch initial brightness
                match conn
                    .call_method(
                        Some(DDC_BUS),
                        DDC_PATH,
                        Some(DDC_IFACE),
                        "GetVcp",
                        &(DISPLAY_NUMBER, "", VCP_BRIGHTNESS, 0u32),
                    )
                    .await
                {
                    Ok(reply) => {
                        if let Ok((current, max, _fmt, err_status, err_msg)) =
                            reply.body().deserialize::<(u16, u16, String, i32, String)>()
                        {
                            if err_status != 0 {
                                send(BridgeEvent::BacklightState {
                                    available: false,
                                    brightness: None,
                                    error: Some(format!("ddcutil GetVcp ({err_status}): {err_msg}")),
                                });
                            } else {
                                let resolved_max = if max > 0 { max } else { 100 };
                                let pct = vcp_to_percent(current, resolved_max);
                                *max_vcp.lock().unwrap() = resolved_max;
                                *current_brightness.lock().unwrap() = Some(pct);
                                send(BridgeEvent::BacklightState {
                                    available: true,
                                    brightness: Some(pct),
                                    error: None,
                                });
                            }
                        }
                    }
                    Err(err) => {
                        send(BridgeEvent::BacklightState {
                            available: false,
                            brightness: None,
                            error: Some(err.to_string()),
                        });
                    }
                }

                // Listen for VcpValueChanged signals
                let Ok(rule) = MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .sender(DDC_BUS)
                    .and_then(|b| b.path(DDC_PATH))
                    .and_then(|b| b.interface(DDC_IFACE))
                    .and_then(|b| b.member("VcpValueChanged"))
                    .map(|b| b.build())
                else {
                    return;
                };

                let Ok(mut stream) = MessageStream::for_match_rule(rule, &conn, Some(64)).await
                else {
                    return;
                };

                while let Some(Ok(msg)) = stream.next().await {
                    if let Ok((_disp, _edid, vcp_code, new_val, _s1, _s2, _flags)) =
                        msg.body().deserialize::<(i32, String, u8, u16, String, String, u32)>()
                    {
                        if vcp_code == VCP_BRIGHTNESS {
                            let max = *max_vcp.lock().unwrap();
                            let pct = vcp_to_percent(new_val, max);
                            *current_brightness.lock().unwrap() = Some(pct);
                            send(BridgeEvent::BacklightState {
                                available: true,
                                brightness: Some(pct),
                                error: None,
                            });
                        }
                    }
                }
            });
        });
    }

    // Debounced setter thread
    {
        let send = send.clone();
        let max_vcp = max_vcp.clone();
        let current_brightness = current_brightness.clone();
        thread::spawn(move || {
            let conn = match zbus::blocking::Connection::session() {
                Ok(c) => c,
                Err(err) => {
                    while let Ok((req_id, _, reply_send)) = rx.recv() {
                        reply_err(&reply_send, req_id, err.to_string());
                    }
                    return;
                }
            };

            while let Ok((first_req, mut latest_pct, first_reply)) = rx.recv() {
                let mut pending_reqs = vec![(first_req, first_reply)];
                let deadline = Instant::now() + Duration::from_millis(80);

                // Coalesce rapid slider/wheel updates for 80ms
                loop {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match rx.recv_timeout(remaining) {
                        Ok((req_id, pct, reply_send)) => {
                            latest_pct = pct;
                            pending_reqs.push((req_id, reply_send));
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }

                let clamped_pct = latest_pct.min(100);
                let max = *max_vcp.lock().unwrap();
                let value = percent_to_vcp(clamped_pct, max);

                let result = conn.call_method(
                    Some(DDC_BUS),
                    DDC_PATH,
                    Some(DDC_IFACE),
                    "SetVcp",
                    &(DISPLAY_NUMBER, "", VCP_BRIGHTNESS, value, DDC_FLAG_NO_VERIFY),
                );

                match result {
                    Ok(reply) => {
                        match reply.body().deserialize::<(i32, String)>() {
                            Ok((status, msg)) => {
                                // On failure keep reporting the last known brightness
                                // rather than the target that was never applied.
                                let mut current = current_brightness.lock().unwrap();
                                let result = if status == 0 {
                                    *current = Some(clamped_pct);
                                    Ok(())
                                } else {
                                    Err(format!("ddcutil SetVcp ({status}): {msg}"))
                                };
                                send(BridgeEvent::BacklightState {
                                    available: true,
                                    brightness: *current,
                                    error: result.clone().err(),
                                });
                                drop(current);
                                for (id, reply_send) in pending_reqs {
                                    match &result {
                                        Ok(()) => reply_ok(&reply_send, id),
                                        Err(err) => reply_err(&reply_send, id, err),
                                    }
                                }
                            }
                            Err(err) => {
                                for (id, reply_send) in pending_reqs {
                                    reply_err(&reply_send, id, err.to_string());
                                }
                            }
                        }
                    }
                    Err(err) => {
                        for (id, reply_send) in pending_reqs {
                            reply_err(&reply_send, id, err.to_string());
                        }
                    }
                }
            }
        });
    }

    BacklightController { tx }
}

impl BacklightController {
    pub fn set_brightness(&self, req_id: u32, percent: f64, reply_send: SendEvent) {
        let clamped = if percent.is_nan() {
            0
        } else {
            percent.round().clamp(0.0, 100.0) as u8
        };
        let _ = self.tx.send((req_id, clamped, reply_send));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_vcp_and_percent_with_clamping() {
        assert_eq!(vcp_to_percent(50, 100), 50);
        assert_eq!(vcp_to_percent(128, 255), 50);
        assert_eq!(vcp_to_percent(0, 0), 0);

        assert_eq!(percent_to_vcp(50, 200), 100);
        assert_eq!(percent_to_vcp(150, 100), 100);
        assert_eq!(percent_to_vcp(25, 0), 25);
    }
}

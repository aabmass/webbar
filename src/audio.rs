//! PulseAudio / PipeWire audio state & control via `libpulse-binding`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::introspect::{ServerInfo, SinkInfo, SinkInputInfo, SourceInfo};
use pulse::context::subscribe::{Facility, InterestMaskSet, Operation as SubscribeOp};
use pulse::context::{Context, FlagSet as ContextFlagSet, State as ContextState};
use pulse::mainloop::threaded::Mainloop;
use pulse::proplist::Proplist;
use pulse::volume::{ChannelVolumes, Volume};
use serde::{Deserialize, Serialize};

use crate::bridge::{reply_err, reply_ok, BridgeEvent, SendEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../preload/src/generated/")]
pub enum AudioDeviceKind {
    Output,
    Input,
}

#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct AudioDevice {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub volume: f64,
    pub muted: bool,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct AudioStream {
    pub id: u32,
    pub name: String,
    pub app_name: String,
    pub device_id: u32,
    pub volume: f64,
    pub muted: bool,
}

#[derive(Clone)]
pub struct AudioController {
    tx: Sender<AudioCommand>,
}

enum AudioCommand {
    SetVolume {
        req_id: u32,
        device_id: u32,
        volume: f64,
        kind: Option<AudioDeviceKind>,
        reply_send: SendEvent,
    },
    SetMuted {
        req_id: u32,
        device_id: u32,
        muted: bool,
        kind: Option<AudioDeviceKind>,
        reply_send: SendEvent,
    },
    SetDefault {
        req_id: u32,
        device_id: u32,
        kind: Option<AudioDeviceKind>,
        reply_send: SendEvent,
    },
    SetStreamVolume {
        req_id: u32,
        stream_id: u32,
        volume: f64,
        reply_send: SendEvent,
    },
    SetStreamMuted {
        req_id: u32,
        stream_id: u32,
        muted: bool,
        reply_send: SendEvent,
    },
    Refresh,
    Disconnected,
}

type AudioSnapshot = (Vec<AudioDevice>, Vec<AudioDevice>, Vec<AudioStream>);
type LastEmittedAudio = Rc<RefCell<Option<AudioSnapshot>>>;

#[derive(Default)]
struct RawAudioState {
    server_info_loaded: bool,
    default_sink: Option<String>,
    default_source: Option<String>,
    sinks: Vec<RawDevice>,
    sources: Vec<RawDevice>,
    streams: Vec<AudioStream>,
    sink_channels: Vec<(u32, u8)>,
    source_channels: Vec<(u32, u8)>,
    stream_channels: Vec<(u32, u8)>,
}

#[derive(Clone)]
struct RawDevice {
    id: u32,
    name: String,
    description: String,
    volume: f64,
    muted: bool,
}

pub fn volume_to_float(volumes: &ChannelVolumes) -> f64 {
    let avg = volumes.avg();
    (avg.0 as f64) / (Volume::NORMAL.0 as f64)
}

pub fn float_to_channel_volumes(volume: f64, channels: u8) -> ChannelVolumes {
    let clamped = volume.clamp(0.0, 1.5);
    let raw = (clamped * (Volume::NORMAL.0 as f64)).round() as u32;
    let mut cv = ChannelVolumes::default();
    cv.set(channels.max(1), Volume(raw));
    cv
}

fn build_devices(raw: &[RawDevice], default_name: Option<&str>) -> Vec<AudioDevice> {
    raw.iter()
        .map(|d| AudioDevice {
            id: d.id,
            name: d.name.clone(),
            description: d.description.clone(),
            volume: d.volume,
            muted: d.muted,
            default: default_name.is_some_and(|name| name == d.name),
        })
        .collect()
}

fn resolve_device_channels(
    guard: &RawAudioState,
    device_id: u32,
    kind: Option<AudioDeviceKind>,
) -> Option<(bool, u8)> {
    match kind {
        Some(AudioDeviceKind::Output) => guard
            .sink_channels
            .iter()
            .find(|(id, _)| *id == device_id)
            .map(|(_, ch)| (true, *ch)),
        Some(AudioDeviceKind::Input) => guard
            .source_channels
            .iter()
            .find(|(id, _)| *id == device_id)
            .map(|(_, ch)| (false, *ch)),
        None => {
            if let Some((_, ch)) = guard.sink_channels.iter().find(|(id, _)| *id == device_id) {
                Some((true, *ch))
            } else {
                guard
                    .source_channels
                    .iter()
                    .find(|(id, _)| *id == device_id)
                    .map(|(_, ch)| (false, *ch))
            }
        }
    }
}

fn resolve_device_target(
    guard: &RawAudioState,
    device_id: u32,
    kind: Option<AudioDeviceKind>,
) -> Option<(bool, String)> {
    match kind {
        Some(AudioDeviceKind::Output) => guard
            .sinks
            .iter()
            .find(|d| d.id == device_id)
            .map(|d| (true, d.name.clone())),
        Some(AudioDeviceKind::Input) => guard
            .sources
            .iter()
            .find(|d| d.id == device_id)
            .map(|d| (false, d.name.clone())),
        None => {
            if let Some(d) = guard.sinks.iter().find(|d| d.id == device_id) {
                Some((true, d.name.clone()))
            } else {
                guard
                    .sources
                    .iter()
                    .find(|d| d.id == device_id)
                    .map(|d| (false, d.name.clone()))
            }
        }
    }
}

fn format_stream_labels(
    app_prop: Option<String>,
    media_prop: Option<String>,
    fallback_name: Option<String>,
) -> (String, String) {
    let app = app_prop
        .filter(|s| !s.trim().is_empty())
        .or_else(|| fallback_name.clone().filter(|s| !s.trim().is_empty()))
        .or_else(|| media_prop.clone().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "Application".to_string());
    let media = media_prop
        .filter(|s| !s.trim().is_empty())
        .or_else(|| fallback_name.filter(|s| !s.trim().is_empty()))
        .unwrap_or_default();
    let name = if media == app { String::new() } else { media };
    (app, name)
}

fn reject_while_disconnected(cmd: AudioCommand) {
    match cmd {
        AudioCommand::SetVolume {
            req_id, reply_send, ..
        }
        | AudioCommand::SetMuted {
            req_id, reply_send, ..
        }
        | AudioCommand::SetDefault {
            req_id, reply_send, ..
        }
        | AudioCommand::SetStreamVolume {
            req_id, reply_send, ..
        }
        | AudioCommand::SetStreamMuted {
            req_id, reply_send, ..
        } => reply_err(&reply_send, req_id, "PulseAudio/PipeWire unavailable"),
        AudioCommand::Refresh | AudioCommand::Disconnected => {}
    }
}

fn emit_unavailable(send: &SendEvent) {
    send(BridgeEvent::AudioState {
        available: false,
        outputs: Vec::new(),
        inputs: Vec::new(),
        streams: Vec::new(),
    });
}

fn run_session(send: &SendEvent, cmd_tx: &Sender<AudioCommand>, rx: &Receiver<AudioCommand>) -> bool {
    let mut proplist = Proplist::new().expect("pulse proplist");
    let _ = proplist.set_str(pulse::proplist::properties::APPLICATION_NAME, "webbar");

    let Some(mut mainloop) = Mainloop::new() else {
        eprintln!("webbar: failed to create PulseAudio mainloop");
        return false;
    };

    let Some(mut context) = Context::new_with_proplist(&mainloop, "webbar", &proplist) else {
        eprintln!("webbar: failed to create PulseAudio context");
        return false;
    };

    if context
        .connect(None, ContextFlagSet::NOFLAGS, None)
        .is_err()
    {
        return false;
    }

    if mainloop.start().is_err() {
        return false;
    }

    // Wait for context ready
    loop {
        mainloop.lock();
        let state = context.get_state();
        mainloop.unlock();
        match state {
            ContextState::Ready => break,
            ContextState::Failed | ContextState::Terminated => {
                mainloop.stop();
                return false;
            }
            _ => {
                while let Ok(cmd) = rx.try_recv() {
                    reject_while_disconnected(cmd);
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
    }

    let raw_state = Arc::new(Mutex::new(RawAudioState::default()));
    let last_emitted: LastEmittedAudio = Rc::new(RefCell::new(None));

    // Watch context state so we reconnect if PipeWire/PulseAudio restarts
    {
        let disconnect_tx = cmd_tx.clone();
        mainloop.lock();
        let ctx_ptr = &context as *const Context as usize;
        context.set_state_callback(Some(Box::new(move || {
            // Safe: callback runs on mainloop thread while context is alive
            let ctx = unsafe { &*(ctx_ptr as *const Context) };
            if matches!(
                ctx.get_state(),
                ContextState::Failed | ContextState::Terminated
            ) {
                let _ = disconnect_tx.send(AudioCommand::Disconnected);
            }
        })));
        mainloop.unlock();
    }

    // Subscribe to PulseAudio events
    {
        let refresh_tx = cmd_tx.clone();
        mainloop.lock();
        context.set_subscribe_callback(Some(Box::new(move |facility, op, _| {
            if matches!(
                facility,
                Some(
                    Facility::Sink
                        | Facility::Source
                        | Facility::SinkInput
                        | Facility::Server
                )
            ) && matches!(
                op,
                Some(SubscribeOp::New | SubscribeOp::Changed | SubscribeOp::Removed)
            ) {
                let _ = refresh_tx.send(AudioCommand::Refresh);
            }
        })));
        context.subscribe(
            InterestMaskSet::SINK
                | InterestMaskSet::SOURCE
                | InterestMaskSet::SINK_INPUT
                | InterestMaskSet::SERVER,
            |_| {},
        );
        mainloop.unlock();
    }

    let trigger_refresh = |ml: &mut Mainloop, ctx: &mut Context| {
        ml.lock();
        let introspect = ctx.introspect();

        // Server info (defaults)
        {
            let raw_state = raw_state.clone();
            let send = send.clone();
            let last_emitted = last_emitted.clone();
            introspect.get_server_info(move |info: &ServerInfo<'_>| {
                let mut guard = raw_state.lock().unwrap();
                guard.server_info_loaded = true;
                guard.default_sink = info.default_sink_name.as_ref().map(|s| s.to_string());
                guard.default_source = info.default_source_name.as_ref().map(|s| s.to_string());
                emit_if_changed(&guard, &send, &last_emitted);
            });
        }

        // Sinks (outputs)
        {
            let raw_state = raw_state.clone();
            let send = send.clone();
            let last_emitted = last_emitted.clone();
            let pending = Rc::new(RefCell::new(Vec::<(RawDevice, u8)>::new()));
            introspect.get_sink_info_list(move |res: ListResult<&SinkInfo<'_>>| match res {
                ListResult::Item(info) => {
                    let desc = info
                        .description
                        .as_ref()
                        .map(|s| s.to_string())
                        .or_else(|| info.name.as_ref().map(|s| s.to_string()))
                        .unwrap_or_else(|| format!("Sink {}", info.index));
                    pending.borrow_mut().push((
                        RawDevice {
                            id: info.index,
                            name: info.name.as_ref().map(|s| s.to_string()).unwrap_or_default(),
                            description: desc,
                            volume: volume_to_float(&info.volume),
                            muted: info.mute,
                        },
                        info.volume.len(),
                    ));
                }
                ListResult::End => {
                    let items = std::mem::take(&mut *pending.borrow_mut());
                    let mut guard = raw_state.lock().unwrap();
                    guard.sink_channels = items.iter().map(|(d, c)| (d.id, *c)).collect();
                    guard.sinks = items.into_iter().map(|(d, _)| d).collect();
                    emit_if_changed(&guard, &send, &last_emitted);
                }
                ListResult::Error => {}
            });
        }

        // Sources (inputs, excluding monitor sources)
        {
            let raw_state = raw_state.clone();
            let send = send.clone();
            let last_emitted = last_emitted.clone();
            let pending = Rc::new(RefCell::new(Vec::<(RawDevice, u8)>::new()));
            introspect.get_source_info_list(move |res: ListResult<&SourceInfo<'_>>| match res {
                ListResult::Item(info) => {
                    if info.monitor_of_sink.is_some() {
                        return;
                    }
                    let desc = info
                        .description
                        .as_ref()
                        .map(|s| s.to_string())
                        .or_else(|| info.name.as_ref().map(|s| s.to_string()))
                        .unwrap_or_else(|| format!("Source {}", info.index));
                    pending.borrow_mut().push((
                        RawDevice {
                            id: info.index,
                            name: info.name.as_ref().map(|s| s.to_string()).unwrap_or_default(),
                            description: desc,
                            volume: volume_to_float(&info.volume),
                            muted: info.mute,
                        },
                        info.volume.len(),
                    ));
                }
                ListResult::End => {
                    let items = std::mem::take(&mut *pending.borrow_mut());
                    let mut guard = raw_state.lock().unwrap();
                    guard.source_channels = items.iter().map(|(d, c)| (d.id, *c)).collect();
                    guard.sources = items.into_iter().map(|(d, _)| d).collect();
                    emit_if_changed(&guard, &send, &last_emitted);
                }
                ListResult::Error => {}
            });
        }

        // Sink inputs (per-application playback streams)
        {
            let raw_state = raw_state.clone();
            let send = send.clone();
            let last_emitted = last_emitted.clone();
            let pending = Rc::new(RefCell::new(Vec::<(AudioStream, u8)>::new()));
            introspect.get_sink_input_info_list(move |res: ListResult<&SinkInputInfo<'_>>| {
                match res {
                    ListResult::Item(info) => {
                        if !info.has_volume || !info.volume_writable {
                            return;
                        }
                        let app_prop = info
                            .proplist
                            .get_str(pulse::proplist::properties::APPLICATION_NAME);
                        let media_prop = info
                            .proplist
                            .get_str(pulse::proplist::properties::MEDIA_NAME);
                        let fallback = info.name.as_ref().map(|s| s.to_string());
                        if media_prop.as_deref() == Some("Peak detect") {
                            return;
                        }
                        let (app_name, name) =
                            format_stream_labels(app_prop, media_prop, fallback);
                        pending.borrow_mut().push((
                            AudioStream {
                                id: info.index,
                                name,
                                app_name,
                                device_id: info.sink,
                                volume: volume_to_float(&info.volume),
                                muted: info.mute,
                            },
                            info.volume.len(),
                        ));
                    }
                    ListResult::End => {
                        let items = std::mem::take(&mut *pending.borrow_mut());
                        let mut guard = raw_state.lock().unwrap();
                        guard.stream_channels = items.iter().map(|(s, c)| (s.id, *c)).collect();
                        guard.streams = items.into_iter().map(|(s, _)| s).collect();
                        emit_if_changed(&guard, &send, &last_emitted);
                    }
                    ListResult::Error => {}
                }
            });
        }

        ml.unlock();
    };

    trigger_refresh(&mut mainloop, &mut context);

    let mut disconnected = false;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            AudioCommand::Disconnected => {
                disconnected = true;
                break;
            }
            AudioCommand::Refresh => {
                // Drain any queued Refresh events to coalesce rapid bursts
                let mut pending_cmds = Vec::new();
                while let Ok(next) = rx.try_recv() {
                    match next {
                        AudioCommand::Refresh => {}
                        AudioCommand::Disconnected => {
                            disconnected = true;
                            break;
                        }
                        other => pending_cmds.push(other),
                    }
                }
                if disconnected {
                    for c in pending_cmds {
                        reject_while_disconnected(c);
                    }
                    break;
                }
                trigger_refresh(&mut mainloop, &mut context);
                for c in pending_cmds {
                    handle_command(&mut mainloop, &mut context, &raw_state, cmd_tx, c);
                }
            }
            other => handle_command(&mut mainloop, &mut context, &raw_state, cmd_tx, other),
        }
    }

    mainloop.lock();
    context.set_state_callback(None);
    context.set_subscribe_callback(None);
    context.disconnect();
    mainloop.unlock();
    mainloop.stop();

    disconnected
}

pub fn spawn(send: SendEvent) -> AudioController {
    let (tx, rx) = mpsc::channel::<AudioCommand>();
    let cmd_tx = tx.clone();

    thread::spawn(move || {
        emit_unavailable(&send);
        let mut retry_delay = Duration::from_millis(500);
        loop {
            let was_connected = run_session(&send, &cmd_tx, &rx);
            emit_unavailable(&send);
            if was_connected {
                retry_delay = Duration::from_millis(500);
            }
            let deadline = std::time::Instant::now() + retry_delay;
            while std::time::Instant::now() < deadline {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(cmd) => reject_while_disconnected(cmd),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            retry_delay = (retry_delay * 2).min(Duration::from_secs(5));
        }
    });

    AudioController { tx }
}

fn emit_if_changed(
    guard: &RawAudioState,
    send: &SendEvent,
    last_emitted: &LastEmittedAudio,
) {
    if !guard.server_info_loaded {
        return;
    }
    let outputs = build_devices(&guard.sinks, guard.default_sink.as_deref());
    let inputs = build_devices(&guard.sources, guard.default_source.as_deref());
    let streams = guard.streams.clone();
    let mut prev = last_emitted.borrow_mut();
    if prev
        .as_ref()
        .is_some_and(|(o, i, s)| o == &outputs && i == &inputs && s == &streams)
    {
        return;
    }
    *prev = Some((outputs.clone(), inputs.clone(), streams.clone()));
    send(BridgeEvent::AudioState {
        available: true,
        outputs,
        inputs,
        streams,
    });
}

fn handle_command(
    ml: &mut Mainloop,
    ctx: &mut Context,
    raw_state: &Arc<Mutex<RawAudioState>>,
    cmd_tx: &Sender<AudioCommand>,
    cmd: AudioCommand,
) {
    match cmd {
        AudioCommand::SetVolume {
            req_id,
            device_id,
            volume,
            kind,
            reply_send,
        } => {
            let Some((is_sink, channels)) = ({
                let guard = raw_state.lock().unwrap();
                resolve_device_channels(&guard, device_id, kind)
            }) else {
                reply_err(&reply_send, req_id, format!("unknown audio device {device_id}"));
                return;
            };
            let cv = float_to_channel_volumes(volume, channels);
            ml.lock();
            let mut introspect = ctx.introspect();
            let cb = move |ok: bool| {
                if ok {
                    reply_ok(&reply_send, req_id);
                } else {
                    reply_err(&reply_send, req_id, "PulseAudio rejected volume change");
                }
            };
            if is_sink {
                introspect.set_sink_volume_by_index(device_id, &cv, Some(Box::new(cb)));
            } else {
                introspect.set_source_volume_by_index(device_id, &cv, Some(Box::new(cb)));
            }
            ml.unlock();
        }
        AudioCommand::SetMuted {
            req_id,
            device_id,
            muted,
            kind,
            reply_send,
        } => {
            let Some((is_sink, _)) = ({
                let guard = raw_state.lock().unwrap();
                resolve_device_target(&guard, device_id, kind)
            }) else {
                reply_err(&reply_send, req_id, format!("unknown audio device {device_id}"));
                return;
            };
            ml.lock();
            let mut introspect = ctx.introspect();
            let cb = move |ok: bool| {
                if ok {
                    reply_ok(&reply_send, req_id);
                } else {
                    reply_err(&reply_send, req_id, "PulseAudio rejected mute change");
                }
            };
            if is_sink {
                introspect.set_sink_mute_by_index(device_id, muted, Some(Box::new(cb)));
            } else {
                introspect.set_source_mute_by_index(device_id, muted, Some(Box::new(cb)));
            }
            ml.unlock();
        }
        AudioCommand::SetDefault {
            req_id,
            device_id,
            kind,
            reply_send,
        } => {
            let Some((is_sink, name)) = ({
                let guard = raw_state.lock().unwrap();
                resolve_device_target(&guard, device_id, kind)
            }) else {
                reply_err(&reply_send, req_id, format!("unknown audio device {device_id}"));
                return;
            };
            let refresh_tx = cmd_tx.clone();
            ml.lock();
            let cb = move |ok: bool| {
                if ok {
                    let _ = refresh_tx.send(AudioCommand::Refresh);
                    reply_ok(&reply_send, req_id);
                } else {
                    reply_err(
                        &reply_send,
                        req_id,
                        "PulseAudio rejected default device change",
                    );
                }
            };
            if is_sink {
                ctx.set_default_sink(&name, cb);
            } else {
                ctx.set_default_source(&name, cb);
            }
            ml.unlock();
        }
        AudioCommand::SetStreamVolume {
            req_id,
            stream_id,
            volume,
            reply_send,
        } => {
            let channels = {
                let guard = raw_state.lock().unwrap();
                guard
                    .stream_channels
                    .iter()
                    .find(|(id, _)| *id == stream_id)
                    .map(|(_, ch)| *ch)
            };
            let Some(channels) = channels else {
                reply_err(&reply_send, req_id, format!("unknown audio stream {stream_id}"));
                return;
            };
            let cv = float_to_channel_volumes(volume, channels);
            ml.lock();
            let mut introspect = ctx.introspect();
            introspect.set_sink_input_volume(
                stream_id,
                &cv,
                Some(Box::new(move |ok: bool| {
                    if ok {
                        reply_ok(&reply_send, req_id);
                    } else {
                        reply_err(
                            &reply_send,
                            req_id,
                            "PulseAudio rejected stream volume change",
                        );
                    }
                })),
            );
            ml.unlock();
        }
        AudioCommand::SetStreamMuted {
            req_id,
            stream_id,
            muted,
            reply_send,
        } => {
            let exists = {
                let guard = raw_state.lock().unwrap();
                guard.streams.iter().any(|s| s.id == stream_id)
            };
            if !exists {
                reply_err(&reply_send, req_id, format!("unknown audio stream {stream_id}"));
                return;
            }
            ml.lock();
            let mut introspect = ctx.introspect();
            introspect.set_sink_input_mute(
                stream_id,
                muted,
                Some(Box::new(move |ok: bool| {
                    if ok {
                        reply_ok(&reply_send, req_id);
                    } else {
                        reply_err(&reply_send, req_id, "PulseAudio rejected stream mute change");
                    }
                })),
            );
            ml.unlock();
        }
        AudioCommand::Refresh | AudioCommand::Disconnected => {}
    }
}

impl AudioController {
    pub fn set_volume(
        &self,
        req_id: u32,
        device_id: u32,
        volume: f64,
        kind: Option<AudioDeviceKind>,
        reply_send: SendEvent,
    ) {
        let _ = self.tx.send(AudioCommand::SetVolume {
            req_id,
            device_id,
            volume,
            kind,
            reply_send,
        });
    }

    pub fn set_muted(
        &self,
        req_id: u32,
        device_id: u32,
        muted: bool,
        kind: Option<AudioDeviceKind>,
        reply_send: SendEvent,
    ) {
        let _ = self.tx.send(AudioCommand::SetMuted {
            req_id,
            device_id,
            muted,
            kind,
            reply_send,
        });
    }

    pub fn set_default(
        &self,
        req_id: u32,
        device_id: u32,
        kind: Option<AudioDeviceKind>,
        reply_send: SendEvent,
    ) {
        let _ = self.tx.send(AudioCommand::SetDefault {
            req_id,
            device_id,
            kind,
            reply_send,
        });
    }

    pub fn set_stream_volume(
        &self,
        req_id: u32,
        stream_id: u32,
        volume: f64,
        reply_send: SendEvent,
    ) {
        let _ = self.tx.send(AudioCommand::SetStreamVolume {
            req_id,
            stream_id,
            volume,
            reply_send,
        });
    }

    pub fn set_stream_muted(
        &self,
        req_id: u32,
        stream_id: u32,
        muted: bool,
        reply_send: SendEvent,
    ) {
        let _ = self.tx.send(AudioCommand::SetStreamMuted {
            req_id,
            stream_id,
            muted,
            reply_send,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_volume_to_float_and_back() {
        let cv = float_to_channel_volumes(0.5, 2);
        assert_eq!(cv.len(), 2);
        let back = volume_to_float(&cv);
        assert!((back - 0.5).abs() < 0.001);

        let clamped = float_to_channel_volumes(2.5, 1);
        assert!((volume_to_float(&clamped) - 1.5).abs() < 0.001);
    }

    #[test]
    fn marks_default_device_correctly() {
        let raw = vec![
            RawDevice {
                id: 1,
                name: "alsa_1".into(),
                description: "Speakers".into(),
                volume: 0.7,
                muted: false,
            },
            RawDevice {
                id: 2,
                name: "alsa_2".into(),
                description: "Headphones".into(),
                volume: 0.4,
                muted: true,
            },
        ];
        let devices = build_devices(&raw, Some("alsa_2"));
        assert!(!devices[0].default);
        assert!(devices[1].default);
    }

    #[test]
    fn disambiguates_overlapping_sink_and_source_ids_with_kind() {
        let state = RawAudioState {
            sinks: vec![RawDevice {
                id: 42,
                name: "sink_42".into(),
                description: "Speakers".into(),
                volume: 0.5,
                muted: false,
            }],
            sources: vec![RawDevice {
                id: 42,
                name: "source_42".into(),
                description: "Mic".into(),
                volume: 0.8,
                muted: false,
            }],
            sink_channels: vec![(42, 2)],
            source_channels: vec![(42, 1)],
            ..Default::default()
        };

        assert_eq!(
            resolve_device_target(&state, 42, Some(AudioDeviceKind::Output)),
            Some((true, "sink_42".into()))
        );
        assert_eq!(
            resolve_device_target(&state, 42, Some(AudioDeviceKind::Input)),
            Some((false, "source_42".into()))
        );
        assert_eq!(
            resolve_device_channels(&state, 42, Some(AudioDeviceKind::Input)),
            Some((false, 1))
        );
    }

    #[test]
    fn waits_for_server_info_before_emitting_snapshot() {
        let events: Arc<Mutex<Vec<BridgeEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let send: SendEvent = {
            let events = events.clone();
            Arc::new(move |ev| events.lock().unwrap().push(ev))
        };
        let last_emitted: LastEmittedAudio = Rc::new(RefCell::new(None));
        let mut state = RawAudioState {
            sinks: vec![RawDevice {
                id: 1,
                name: "sink_1".into(),
                description: "Speakers".into(),
                volume: 0.5,
                muted: false,
            }],
            ..Default::default()
        };

        emit_if_changed(&state, &send, &last_emitted);
        assert!(events.lock().unwrap().is_empty());

        state.server_info_loaded = true;
        state.default_sink = Some("sink_1".into());
        emit_if_changed(&state, &send, &last_emitted);
        assert_eq!(events.lock().unwrap().len(), 1);
    }

    #[test]
    fn formats_stream_labels_without_duplicating_app_name() {
        assert_eq!(
            format_stream_labels(
                Some("Firefox".into()),
                Some("Lo-Fi Beats".into()),
                Some("AudioStream".into())
            ),
            ("Firefox".into(), "Lo-Fi Beats".into())
        );
        assert_eq!(
            format_stream_labels(Some("Spotify".into()), Some("Spotify".into()), None),
            ("Spotify".into(), String::new())
        );
    }
}

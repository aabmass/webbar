//! PulseAudio / PipeWire audio state & control via `libpulse-binding`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::introspect::{ServerInfo, SinkInfo, SourceInfo};
use pulse::context::subscribe::{Facility, InterestMaskSet, Operation as SubscribeOp};
use pulse::context::{Context, FlagSet as ContextFlagSet, State as ContextState};
use pulse::mainloop::threaded::Mainloop;
use pulse::proplist::Proplist;
use pulse::volume::{ChannelVolumes, Volume};
use serde::Serialize;

use crate::bridge::{reply_err, reply_ok, BridgeEvent, SendEvent};

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

#[derive(Clone)]
pub struct AudioController {
    tx: Sender<AudioCommand>,
}

enum AudioCommand {
    SetVolume {
        req_id: u32,
        device_id: u32,
        volume: f64,
    },
    SetMuted {
        req_id: u32,
        device_id: u32,
        muted: bool,
    },
    SetDefault {
        req_id: u32,
        device_id: u32,
    },
    Refresh,
}

type AudioSnapshot = (Vec<AudioDevice>, Vec<AudioDevice>);
type LastEmittedAudio = Rc<RefCell<Option<AudioSnapshot>>>;

#[derive(Default)]
struct RawAudioState {
    default_sink: Option<String>,
    default_source: Option<String>,
    sinks: Vec<RawDevice>,
    sources: Vec<RawDevice>,
    sink_channels: Vec<(u32, u8)>,
    source_channels: Vec<(u32, u8)>,
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

pub fn spawn(send: SendEvent) -> AudioController {
    let (tx, rx) = mpsc::channel::<AudioCommand>();
    let cmd_tx = tx.clone();

    thread::spawn(move || {
        send(BridgeEvent::AudioState {
            available: false,
            outputs: Vec::new(),
            inputs: Vec::new(),
        });

        let mut proplist = Proplist::new().expect("pulse proplist");
        let _ = proplist.set_str(pulse::proplist::properties::APPLICATION_NAME, "webbar");

        let mut mainloop = match Mainloop::new() {
            Some(ml) => ml,
            None => {
                eprintln!("webbar: failed to create PulseAudio mainloop");
                return;
            }
        };

        let mut context = match Context::new_with_proplist(&mainloop, "webbar", &proplist) {
            Some(ctx) => ctx,
            None => {
                eprintln!("webbar: failed to create PulseAudio context");
                return;
            }
        };

        if context
            .connect(None, ContextFlagSet::NOFLAGS, None)
            .is_err()
        {
            eprintln!("webbar: failed to connect to PulseAudio/PipeWire");
            return;
        }

        if mainloop.start().is_err() {
            eprintln!("webbar: failed to start PulseAudio mainloop");
            return;
        }

        // Wait for context ready
        loop {
            mainloop.lock();
            let state = context.get_state();
            mainloop.unlock();
            match state {
                ContextState::Ready => break,
                ContextState::Failed | ContextState::Terminated => {
                    eprintln!("webbar: PulseAudio context failed during startup");
                    mainloop.stop();
                    return;
                }
                _ => thread::sleep(Duration::from_millis(20)),
            }
        }

        let raw_state = Arc::new(Mutex::new(RawAudioState::default()));
        let last_emitted: LastEmittedAudio = Rc::new(RefCell::new(None));

        // Subscribe to PulseAudio events
        {
            let refresh_tx = cmd_tx.clone();
            mainloop.lock();
            context.set_subscribe_callback(Some(Box::new(move |facility, op, _| {
                if matches!(
                    facility,
                    Some(Facility::Sink | Facility::Source | Facility::Server)
                ) && matches!(
                    op,
                    Some(SubscribeOp::New | SubscribeOp::Changed | SubscribeOp::Removed)
                ) {
                    let _ = refresh_tx.send(AudioCommand::Refresh);
                }
            })));
            context.subscribe(
                InterestMaskSet::SINK | InterestMaskSet::SOURCE | InterestMaskSet::SERVER,
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

            ml.unlock();
        };

        trigger_refresh(&mut mainloop, &mut context);

        while let Ok(cmd) = rx.recv() {
            match cmd {
                AudioCommand::Refresh => {
                    // Drain any queued Refresh events to coalesce rapid bursts
                    let mut pending_cmds = Vec::new();
                    while let Ok(next) = rx.try_recv() {
                        if !matches!(next, AudioCommand::Refresh) {
                            pending_cmds.push(next);
                        }
                    }
                    trigger_refresh(&mut mainloop, &mut context);
                    for c in pending_cmds {
                        handle_command(&mut mainloop, &mut context, &raw_state, &send, c);
                    }
                }
                other => handle_command(&mut mainloop, &mut context, &raw_state, &send, other),
            }
        }

        mainloop.stop();
    });

    AudioController { tx }
}

fn emit_if_changed(
    guard: &RawAudioState,
    send: &SendEvent,
    last_emitted: &LastEmittedAudio,
) {
    let outputs = build_devices(&guard.sinks, guard.default_sink.as_deref());
    let inputs = build_devices(&guard.sources, guard.default_source.as_deref());
    let mut prev = last_emitted.borrow_mut();
    if prev
        .as_ref()
        .is_some_and(|(o, i)| o == &outputs && i == &inputs)
    {
        return;
    }
    *prev = Some((outputs.clone(), inputs.clone()));
    send(BridgeEvent::AudioState {
        available: true,
        outputs,
        inputs,
    });
}

fn handle_command(
    ml: &mut Mainloop,
    ctx: &mut Context,
    raw_state: &Arc<Mutex<RawAudioState>>,
    send: &SendEvent,
    cmd: AudioCommand,
) {
    match cmd {
        AudioCommand::SetVolume {
            req_id,
            device_id,
            volume,
        } => {
            let (is_sink, channels) = {
                let guard = raw_state.lock().unwrap();
                if let Some((_, ch)) = guard.sink_channels.iter().find(|(id, _)| *id == device_id) {
                    (true, *ch)
                } else if let Some((_, ch)) =
                    guard.source_channels.iter().find(|(id, _)| *id == device_id)
                {
                    (false, *ch)
                } else {
                    reply_err(send, req_id, format!("unknown audio device {device_id}"));
                    return;
                }
            };
            let cv = float_to_channel_volumes(volume, channels);
            let send = send.clone();
            ml.lock();
            let mut introspect = ctx.introspect();
            let cb = move |ok: bool| {
                if ok {
                    reply_ok(&send, req_id);
                } else {
                    reply_err(&send, req_id, "PulseAudio rejected volume change");
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
        } => {
            let is_sink = {
                let guard = raw_state.lock().unwrap();
                if guard.sinks.iter().any(|d| d.id == device_id) {
                    true
                } else if guard.sources.iter().any(|d| d.id == device_id) {
                    false
                } else {
                    reply_err(send, req_id, format!("unknown audio device {device_id}"));
                    return;
                }
            };
            let send = send.clone();
            ml.lock();
            let mut introspect = ctx.introspect();
            let cb = move |ok: bool| {
                if ok {
                    reply_ok(&send, req_id);
                } else {
                    reply_err(&send, req_id, "PulseAudio rejected mute change");
                }
            };
            if is_sink {
                introspect.set_sink_mute_by_index(device_id, muted, Some(Box::new(cb)));
            } else {
                introspect.set_source_mute_by_index(device_id, muted, Some(Box::new(cb)));
            }
            ml.unlock();
        }
        AudioCommand::SetDefault { req_id, device_id } => {
            let target = {
                let guard = raw_state.lock().unwrap();
                if let Some(d) = guard.sinks.iter().find(|d| d.id == device_id) {
                    (true, d.name.clone())
                } else if let Some(d) = guard.sources.iter().find(|d| d.id == device_id) {
                    (false, d.name.clone())
                } else {
                    reply_err(send, req_id, format!("unknown audio device {device_id}"));
                    return;
                }
            };
            let send = send.clone();
            ml.lock();
            let cb = move |ok: bool| {
                if ok {
                    reply_ok(&send, req_id);
                } else {
                    reply_err(&send, req_id, "PulseAudio rejected default device change");
                }
            };
            if target.0 {
                ctx.set_default_sink(&target.1, cb);
            } else {
                ctx.set_default_source(&target.1, cb);
            }
            ml.unlock();
        }
        AudioCommand::Refresh => {}
    }
}

impl AudioController {
    pub fn set_volume(&self, req_id: u32, device_id: u32, volume: f64) {
        let _ = self.tx.send(AudioCommand::SetVolume {
            req_id,
            device_id,
            volume,
        });
    }

    pub fn set_muted(&self, req_id: u32, device_id: u32, muted: bool) {
        let _ = self.tx.send(AudioCommand::SetMuted {
            req_id,
            device_id,
            muted,
        });
    }

    pub fn set_default(&self, req_id: u32, device_id: u32) {
        let _ = self.tx.send(AudioCommand::SetDefault { req_id, device_id });
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
}

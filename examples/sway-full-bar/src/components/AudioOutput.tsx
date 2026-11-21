import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import type { AudioDevice, AudioStream } from "@webbar/types";
import {
  fillPercentForVolume,
  getActiveMic,
  getActiveSpeaker,
  openAudioMixer,
  sliderMaxForVolume,
} from "@webbar/sway-full-bar-shared/audio";
import { MicIcon, MixerIcon, SpeakerIcon } from "./Icons";
import { LevelReadout } from "./LevelReadout";
import { Popover } from "./Popover";

function DeviceRow({
  device,
  kind,
}: {
  device: AudioDevice;
  kind: "output" | "input";
}) {
  const pct = Math.round(device.volume * 100);
  return (
    <button
      type="button"
      className={`audio-device-btn${device.default ? " is-default" : ""}`}
      title={device.description}
      aria-pressed={device.default}
      onClick={() => {
        if (!device.default) {
          void window.webbar.audio.setDefault(device.id, kind);
        }
      }}
    >
      <span className="audio-device-dot" aria-hidden="true" />
      <span className="audio-device-name">{device.description}</span>
      <span className={`audio-device-meta${device.muted ? " is-muted" : ""}`}>
        {device.muted ? "Muted" : `${pct}%`}
      </span>
    </button>
  );
}

function StreamRow({ stream }: { stream: AudioStream }) {
  const pct = Math.round(stream.volume * 100);
  const sliderMax = sliderMaxForVolume(stream.volume);
  const fillPct = fillPercentForVolume(stream.volume);

  return (
    <div className="audio-stream-card">
      <div className="audio-stream-head">
        <span className="audio-stream-app">{stream.app_name}</span>
        {stream.name && (
          <span className="audio-stream-media" title={stream.name}>
            {stream.name}
          </span>
        )}
      </div>
      <div className="audio-slider-row">
        <button
          type="button"
          className={`audio-mute-btn${stream.muted ? " is-muted" : ""}`}
          title={stream.muted ? `Unmute ${stream.app_name}` : `Mute ${stream.app_name}`}
          aria-label={stream.muted ? `Unmute ${stream.app_name}` : `Mute ${stream.app_name}`}
          onClick={() => {
            void window.webbar.audio.setStreamMuted(stream.id, !stream.muted);
          }}
        >
          <SpeakerIcon volume={stream.volume} muted={stream.muted} />
        </button>
        <input
          type="range"
          className={`bar-slider audio-slider${stream.muted ? " is-muted" : ""}`}
          style={{ "--fill": `${fillPct}%` } as React.CSSProperties}
          min={0}
          max={sliderMax}
          step={0.01}
          value={Math.min(sliderMax, stream.volume)}
          aria-label={`${stream.app_name} volume`}
          onChange={(e) => {
            void window.webbar.audio.setStreamVolume(
              stream.id,
              Number(e.target.value),
            );
          }}
        />
        <span className="drawer-value">{pct}%</span>
      </div>
    </div>
  );
}

export function AudioOutput() {
  const audioState = useSyncExternalStore(
    window.webbar.audio.subscribe,
    window.webbar.audio.get,
  );
  const [open, setOpen] = useState(false);
  const [outputsExpanded, setOutputsExpanded] = useState(false);
  const [inputsExpanded, setInputsExpanded] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

  const speaker = getActiveSpeaker(audioState);
  const mic = getActiveMic(audioState);

  const volume = speaker?.volume ?? 0;
  const muted = speaker?.muted ?? false;
  const pct = Math.round(volume * 100);
  const outputSliderMax = sliderMaxForVolume(volume);
  const outputFillPct = fillPercentForVolume(volume);

  const micVolume = mic?.volume ?? 0;
  const micMuted = mic?.muted ?? false;
  const micPct = Math.round(micVolume * 100);
  const micSliderMax = sliderMaxForVolume(micVolume);
  const micFillPct = fillPercentForVolume(micVolume);

  function openMixer() {
    close();
    openAudioMixer();
  }

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={`bar-btn level-btn${muted ? " is-muted" : ""}${!audioState.available ? " is-dimmed" : ""}${open ? " is-open" : ""}`}
        title={
          speaker
            ? `${speaker.description}: ${muted ? "Muted" : `${pct}%`} (click for mixer, right-click to mute, scroll to adjust)`
            : "No audio output"
        }
        onClick={() => setOpen((prev) => !prev)}
        onContextMenu={(e) => {
          e.preventDefault();
          if (!speaker) return;
          void window.webbar.audio.setMuted(speaker.id, !speaker.muted, "output");
        }}
        onWheel={(e) => {
          if (!speaker) return;
          e.preventDefault();
          const step = e.deltaY < 0 ? 0.05 : -0.05;
          const next = Math.max(0, Math.min(1.5, speaker.volume + step));
          void window.webbar.audio.setVolume(speaker.id, next, "output");
        }}
      >
        <LevelReadout
          icon={<SpeakerIcon volume={volume} muted={muted} />}
          percent={speaker ? pct : null}
        />
      </button>

      <Popover open={open} onClose={close} anchorRef={btnRef} align="end">
        <div className="audio-popover">
          <div className="qs-header audio-header">
            <div className="qs-heading audio-heading">
              <span className="qs-title">
                <SpeakerIcon volume={volume} muted={muted} /> Audio
              </span>
              <span
                className="qs-subtitle audio-subtitle"
                title={speaker?.description}
              >
                {!audioState.available
                  ? "Audio unavailable"
                  : speaker
                    ? speaker.description
                    : "No output device"}
              </span>
            </div>
            <button
              type="button"
              className="audio-mixer-btn"
              title="Open pwvucontrol"
              onClick={openMixer}
            >
              <MixerIcon />
              <span>Mixer</span>
            </button>
          </div>

          <div className="audio-body">
            <div className="audio-section">
              <div className="audio-section-head">
                <span className="audio-section-label">Output</span>
                {audioState.outputs.length > 1 && (
                  <button
                    type="button"
                    className="audio-collapse-btn"
                    aria-expanded={outputsExpanded}
                    onClick={() => setOutputsExpanded((prev) => !prev)}
                  >
                    {outputsExpanded
                      ? "Hide devices"
                      : `Switch (${audioState.outputs.length})`}
                  </button>
                )}
              </div>
              <div className="audio-slider-card">
                {speaker && (
                  <div className="audio-active-name" title={speaker.description}>
                    {speaker.description}
                  </div>
                )}
                <div className="audio-slider-row">
                  <button
                    type="button"
                    className={`audio-mute-btn${muted ? " is-muted" : ""}`}
                    disabled={!speaker}
                    title={muted ? "Unmute output" : "Mute output"}
                    aria-label={muted ? "Unmute output" : "Mute output"}
                    onClick={() => {
                      if (!speaker) return;
                      void window.webbar.audio.setMuted(
                        speaker.id,
                        !speaker.muted,
                        "output",
                      );
                    }}
                  >
                    <SpeakerIcon volume={volume} muted={muted} />
                  </button>
                  <input
                    type="range"
                    className={`bar-slider audio-slider${muted ? " is-muted" : ""}`}
                    style={{ "--fill": `${outputFillPct}%` } as React.CSSProperties}
                    min={0}
                    max={outputSliderMax}
                    step={0.01}
                    value={Math.min(outputSliderMax, volume)}
                    disabled={!speaker}
                    aria-label="Output volume"
                    onChange={(e) => {
                      if (!speaker) return;
                      void window.webbar.audio.setVolume(
                        speaker.id,
                        Number(e.target.value),
                        "output",
                      );
                    }}
                  />
                  <span className="drawer-value">
                    {speaker ? `${pct}%` : "--"}
                  </span>
                </div>
              </div>

              {outputsExpanded && audioState.outputs.length > 1 && (
                <div className="audio-device-list" role="radiogroup" aria-label="Output devices">
                  {audioState.outputs.map((device) => (
                    <DeviceRow key={device.id} device={device} kind="output" />
                  ))}
                </div>
              )}
            </div>

            {mic && (
              <div className="audio-section">
                <div className="audio-section-head">
                  <span className="audio-section-label">Input</span>
                  {audioState.inputs.length > 1 && (
                    <button
                      type="button"
                      className="audio-collapse-btn"
                      aria-expanded={inputsExpanded}
                      onClick={() => setInputsExpanded((prev) => !prev)}
                    >
                      {inputsExpanded
                        ? "Hide devices"
                        : `Switch (${audioState.inputs.length})`}
                    </button>
                  )}
                </div>
                <div className="audio-slider-card">
                  <div className="audio-active-name" title={mic.description}>
                    {mic.description}
                  </div>
                  <div className="audio-slider-row">
                    <button
                      type="button"
                      className={`audio-mute-btn${micMuted ? " is-muted" : ""}`}
                      title={micMuted ? "Unmute input" : "Mute input"}
                      aria-label={micMuted ? "Unmute input" : "Mute input"}
                      onClick={() => {
                        void window.webbar.audio.setMuted(
                          mic.id,
                          !mic.muted,
                          "input",
                        );
                      }}
                    >
                      <MicIcon muted={micMuted} />
                    </button>
                    <input
                      type="range"
                      className={`bar-slider audio-slider${micMuted ? " is-muted" : ""}`}
                      style={{ "--fill": `${micFillPct}%` } as React.CSSProperties}
                      min={0}
                      max={micSliderMax}
                      step={0.01}
                      value={Math.min(micSliderMax, micVolume)}
                      aria-label="Input volume"
                      onChange={(e) => {
                        void window.webbar.audio.setVolume(
                          mic.id,
                          Number(e.target.value),
                          "input",
                        );
                      }}
                    />
                    <span className="drawer-value">{micPct}%</span>
                  </div>
                </div>

                {inputsExpanded && audioState.inputs.length > 1 && (
                  <div className="audio-device-list" role="radiogroup" aria-label="Input devices">
                    {audioState.inputs.map((device) => (
                      <DeviceRow key={device.id} device={device} kind="input" />
                    ))}
                  </div>
                )}
              </div>
            )}

            <div className="audio-section">
              <div className="audio-section-label">Applications</div>
              {audioState.streams.length === 0 ? (
                <div className="drawer-empty">No apps playing audio</div>
              ) : (
                <div className="audio-stream-list">
                  {audioState.streams.map((stream) => (
                    <StreamRow key={stream.id} stream={stream} />
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      </Popover>
    </>
  );
}

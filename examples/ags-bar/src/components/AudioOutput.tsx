import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import type { AudioDevice } from "../global";
import { SpeakerIcon } from "./Icons";
import { Popover } from "./Popover";

export function AudioOutput() {
  const audioState = useSyncExternalStore(
    window.webbar.audio.subscribe,
    window.webbar.audio.get,
  );
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

  const speaker: AudioDevice | undefined =
    audioState.outputs.find((o) => o.default) ?? audioState.outputs[0];

  const volume = speaker?.volume ?? 0;
  const muted = speaker?.muted ?? false;
  const pct = Math.round(volume * 100);

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={`bar-btn icon-btn${muted ? " is-muted" : ""}${open ? " is-open" : ""}`}
        title={
          speaker
            ? `${speaker.description}: ${muted ? "Muted" : `${pct}%`} (click for slider, right-click to mute, scroll to adjust)`
            : "No audio output"
        }
        onClick={() => setOpen((prev) => !prev)}
        onContextMenu={(e) => {
          e.preventDefault();
          if (!speaker) return;
          void window.webbar.audio.setMuted(speaker.id, !speaker.muted);
        }}
        onWheel={(e) => {
          if (!speaker) return;
          e.preventDefault();
          const step = e.deltaY < 0 ? 0.05 : -0.05;
          const next = Math.max(0, Math.min(1.5, speaker.volume + step));
          void window.webbar.audio.setVolume(speaker.id, next);
        }}
      >
        <SpeakerIcon volume={volume} muted={muted} />
      </button>

      <Popover open={open} onClose={close} anchorRef={btnRef} align="end">
        <div className="popover-slider-row">
          <SpeakerIcon volume={volume} muted={muted} />
          <input
            type="range"
            className="bar-slider popover-slider"
            min={0}
            max={1}
            step={0.01}
            value={Math.min(1, volume)}
            disabled={!speaker}
            aria-label="Output volume"
            onChange={(e) => {
              if (!speaker) return;
              void window.webbar.audio.setVolume(
                speaker.id,
                Number(e.target.value),
              );
            }}
          />
          <span className="drawer-value">{speaker ? `${pct}%` : "--"}</span>
        </div>
      </Popover>
    </>
  );
}

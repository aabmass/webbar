import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Audio, AudioDevice } from "@domicile-desktop/system-audio/audio";
import { soundServer } from "@domicile-desktop/system-audio/sound-server";
import { SpeakerIcon } from "./Icons";
import { Popover } from "./Popover";

export function AudioOutput() {
  const [audioState, setAudioState] = useState<Audio | null>(null);
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

  const audio = useMemo(
    () =>
      window.domicile?.processes
        ? soundServer({ processes: window.domicile.processes })
        : null,
    [],
  );

  useEffect(() => {
    if (!audio) return;
    return audio.watch(setAudioState);
  }, [audio]);

  const speaker: AudioDevice | undefined =
    audioState?.outputs.find((o) => o.default) ?? audioState?.outputs[0];

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
          if (!speaker || !audio) return;
          void audio.setMuted(speaker.id, !speaker.muted);
        }}
        onWheel={(e) => {
          if (!speaker || !audio) return;
          e.preventDefault();
          const step = e.deltaY < 0 ? 0.05 : -0.05;
          const next = Math.max(0, Math.min(1.5, speaker.volume + step));
          void audio.setVolume(speaker.id, next);
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
              void audio?.setVolume(speaker.id, Number(e.target.value));
            }}
          />
          <span className="drawer-value">{speaker ? `${pct}%` : "--"}</span>
        </div>
      </Popover>
    </>
  );
}

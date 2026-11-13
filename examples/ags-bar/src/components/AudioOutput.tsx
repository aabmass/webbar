import { useEffect, useMemo, useState } from "react";
import type { Audio, AudioDevice } from "@domicile-desktop/system-audio/audio";
import { soundServer } from "@domicile-desktop/system-audio/sound-server";
import { SpeakerIcon } from "./Icons";

export function AudioOutput() {
  const [audioState, setAudioState] = useState<Audio | null>(null);
  const [expanded, setExpanded] = useState(false);

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
    <div className="widget-group">
      {expanded && speaker && (
        <div className="inline-drawer">
          <input
            type="range"
            className="bar-slider"
            min={0}
            max={1}
            step={0.01}
            value={Math.min(1, volume)}
            aria-label="Output volume"
            onChange={(e) => {
              const next = Number(e.target.value);
              void audio?.setVolume(speaker.id, next);
            }}
          />
          <span className="drawer-value">{pct}%</span>
        </div>
      )}

      <button
        type="button"
        className={`bar-btn icon-btn${muted ? " is-muted" : ""}`}
        title={
          speaker
            ? `${speaker.description}: ${muted ? "Muted" : `${pct}%`} (click for slider, right-click to mute, scroll to adjust)`
            : "No audio output"
        }
        onClick={() => setExpanded((prev) => !prev)}
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
    </div>
  );
}

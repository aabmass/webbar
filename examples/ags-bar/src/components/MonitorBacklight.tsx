import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import { DisplayIcon } from "./Icons";
import { Popover } from "./Popover";

export function MonitorBacklight() {
  const { available, brightness, error } = useSyncExternalStore(
    window.webbar.backlight.subscribe,
    window.webbar.backlight.get,
  );
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

  const applyBrightness = useCallback((percent: number) => {
    void window.webbar.backlight.set(percent);
  }, []);

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={`bar-btn icon-btn${error || !available ? " is-dimmed" : ""}${open ? " is-open" : ""}`}
        title={
          error
            ? `Monitor brightness unavailable: ${error}`
            : `Monitor brightness: ${brightness !== null ? `${brightness}%` : "--"} (click for slider, scroll to adjust)`
        }
        onClick={() => setOpen((prev) => !prev)}
        onWheel={(e) => {
          if (brightness === null) return;
          e.preventDefault();
          const step = e.deltaY < 0 ? 5 : -5;
          applyBrightness(brightness + step);
        }}
      >
        <DisplayIcon />
      </button>

      <Popover open={open} onClose={close} anchorRef={btnRef} align="end">
        <div className="popover-slider-row">
          <DisplayIcon />
          <input
            type="range"
            className="bar-slider popover-slider"
            style={{ "--fill": `${brightness ?? 50}%` } as React.CSSProperties}
            min={0}
            max={100}
            step={1}
            value={brightness ?? 50}
            aria-label="Monitor brightness"
            onChange={(e) => applyBrightness(Number(e.target.value))}
          />
          <span className="drawer-value">
            {brightness !== null ? `${brightness}%` : "--"}
          </span>
        </div>
      </Popover>
    </>
  );
}

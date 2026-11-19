import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import { DisplayIcon } from "./Icons";
import { LevelReadout } from "./LevelReadout";
import { Popover } from "./Popover";

type SliderRowProps = {
  label?: string;
  labelTitle?: string;
  ariaLabel: string;
  brightness: number | null;
  disabled: boolean;
  onChange: (percent: number) => void;
};

function BacklightSliderRow({
  label,
  labelTitle,
  ariaLabel,
  brightness,
  disabled,
  onChange,
}: SliderRowProps) {
  return (
    <div className="popover-slider-row">
      <DisplayIcon />
      {label && (
        <span className="drawer-label" title={labelTitle}>
          {label}
        </span>
      )}
      <input
        type="range"
        className="bar-slider popover-slider"
        style={{ "--fill": `${brightness ?? 50}%` } as React.CSSProperties}
        min={0}
        max={100}
        step={1}
        disabled={disabled}
        value={brightness ?? 50}
        aria-label={ariaLabel}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      <span className="drawer-value">
        {brightness !== null ? `${brightness}%` : "--"}
      </span>
    </div>
  );
}

export function MonitorBacklight() {
  const { available, brightness, error, displays } = useSyncExternalStore(
    window.webbar.backlight.subscribe,
    window.webbar.backlight.get,
  );
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

  const currentOutput = window.webbar.output;
  const activeDisplay =
    (currentOutput
      ? displays.find((d) => d.output === currentOutput)
      : undefined) ?? displays[0];
  const activeBrightness = activeDisplay?.brightness ?? brightness;
  const activeAvailable = activeDisplay?.available ?? available;
  const activeError = activeDisplay?.error ?? error;

  const applyBrightness = useCallback((percent: number, displayId?: string) => {
    void window.webbar.backlight.set(percent, displayId);
  }, []);

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={`bar-btn level-btn${activeError || !activeAvailable ? " is-dimmed" : ""}${open ? " is-open" : ""}`}
        title={
          activeError
            ? `Monitor brightness unavailable: ${activeError}`
            : `Monitor brightness: ${activeBrightness !== null ? `${activeBrightness}%` : "--"} (click for slider, scroll to adjust)`
        }
        onClick={() => setOpen((prev) => !prev)}
        onWheel={(e) => {
          if (activeBrightness === null) return;
          e.preventDefault();
          const step = e.deltaY < 0 ? 5 : -5;
          applyBrightness(activeBrightness + step, activeDisplay?.id);
        }}
      >
        <LevelReadout icon={<DisplayIcon />} percent={activeBrightness} />
      </button>

      <Popover open={open} onClose={close} anchorRef={btnRef} align="end">
        {displays.length > 1 ? (
          <div className="drawer-section">
            {displays.map((display) => (
              <BacklightSliderRow
                key={display.id}
                label={display.output ?? display.name}
                labelTitle={display.name}
                ariaLabel={`Brightness for ${display.output ?? display.name}`}
                brightness={display.brightness}
                disabled={!display.available}
                onChange={(pct) => applyBrightness(pct, display.id)}
              />
            ))}
          </div>
        ) : (
          <BacklightSliderRow
            ariaLabel="Monitor brightness"
            brightness={activeBrightness}
            disabled={!activeAvailable}
            onChange={(pct) => applyBrightness(pct, activeDisplay?.id)}
          />
        )}
      </Popover>
    </>
  );
}

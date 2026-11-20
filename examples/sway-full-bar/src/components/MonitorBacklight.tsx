import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import { DisplayIcon, MoonIcon, SunIcon } from "./Icons";
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
  const theme = useSyncExternalStore(
    window.webbar.theme.subscribe,
    window.webbar.theme.get,
  );
  const [open, setOpen] = useState(false);
  const [themeError, setThemeError] = useState<string | null>(null);
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

  const setSystemTheme = useCallback(
    async (dark: boolean) => {
      if (theme.dark === dark) return;
      setThemeError(null);
      try {
        const res = await window.webbar.exec([
          "gsettings",
          "set",
          "org.gnome.desktop.interface",
          "color-scheme",
          dark ? "prefer-dark" : "prefer-light",
        ]);
        if (res.code !== 0) {
          const msg =
            res.stderr.trim() ||
            res.stdout.trim() ||
            `gsettings exited with ${res.code ?? "error"}`;
          setThemeError(msg);
        }
      } catch (err) {
        setThemeError(err instanceof Error ? err.message : String(err));
      }
    },
    [theme.dark],
  );

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={`bar-btn level-btn${activeError || !activeAvailable ? " is-dimmed" : ""}${open ? " is-open" : ""}`}
        title={
          activeError
            ? `Monitor brightness unavailable: ${activeError} (click for theme)`
            : `Monitor brightness: ${activeBrightness !== null ? `${activeBrightness}%` : "--"} (click for display & theme, scroll to adjust)`
        }
        onClick={() => {
          setThemeError(null);
          setOpen((prev) => !prev);
        }}
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
        <div className="display-popover">
          <div className="display-section">
            <span className="display-section-label">Brightness</span>
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
          </div>

          <div className="display-divider" />

          <div className="display-section">
            <span className="display-section-label">Appearance</span>
            <div
              className="theme-segmented"
              role="radiogroup"
              aria-label="System theme"
            >
              <button
                type="button"
                role="radio"
                aria-checked={!theme.dark}
                className={`theme-option${!theme.dark ? " is-active" : ""}`}
                onClick={() => void setSystemTheme(false)}
              >
                <SunIcon />
                <span>Light</span>
              </button>
              <button
                type="button"
                role="radio"
                aria-checked={theme.dark}
                className={`theme-option${theme.dark ? " is-active" : ""}`}
                onClick={() => void setSystemTheme(true)}
              >
                <MoonIcon />
                <span>Dark</span>
              </button>
            </div>
            {themeError && <div className="theme-error">{themeError}</div>}
          </div>
        </div>
      </Popover>
    </>
  );
}

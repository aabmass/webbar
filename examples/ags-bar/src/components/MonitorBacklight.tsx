import { useCallback, useEffect, useRef, useState } from "react";
import { Bus, ListenedKind } from "@domicile-desktop/sdk/system";
import { DisplayIcon } from "./Icons";
import { Popover } from "./Popover";

const DDC_BUS = "com.ddcutil.DdcutilService";
const DDC_PATH = "/com/ddcutil/DdcutilObject";
const DDC_IFACE = "com.ddcutil.DdcutilInterface";
const VCP_BRIGHTNESS = 0x10;
const DISPLAY_NUMBER = 1;

export function MonitorBacklight() {
  const [brightness, setBrightness] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  const maxRef = useRef(100);
  const debounceRef = useRef<number | null>(null);

  const applyBrightness = useCallback((percent: number) => {
    const clamped = Math.max(0, Math.min(100, Math.round(percent)));
    setBrightness(clamped);

    if (debounceRef.current !== null) {
      window.clearTimeout(debounceRef.current);
    }
    debounceRef.current = window.setTimeout(async () => {
      const dbus = window.domicile?.dbus;
      if (!dbus) return;
      const max = maxRef.current;
      const value = Math.max(0, Math.min(max, Math.round((clamped / 100) * max)));
      const res = await dbus.call({
        bus: Bus.Session,
        destination: DDC_BUS,
        path: DDC_PATH,
        interface: DDC_IFACE,
        member: "SetVcp",
        signature: "isyqu",
        body: [DISPLAY_NUMBER, "", VCP_BRIGHTNESS, value, 0],
      });
      res.match({
        Ok: ({ body }) => {
          const [errStatus, errMsg] = body as [number, string];
          if (errStatus !== 0) {
            setError(`ddcutil SetVcp (${errStatus}): ${errMsg}`);
          } else {
            setError(null);
          }
        },
        Err: (err) => setError(err.message),
      });
    }, 100);
  }, []);

  useEffect(() => {
    const dbus = window.domicile?.dbus;
    if (!dbus) return;

    let cancelled = false;
    let stopMatch: (() => void) | undefined;

    (async () => {
      const res = await dbus.call({
        bus: Bus.Session,
        destination: DDC_BUS,
        path: DDC_PATH,
        interface: DDC_IFACE,
        member: "GetVcp",
        signature: "isyu",
        body: [DISPLAY_NUMBER, "", VCP_BRIGHTNESS, 0],
      });
      if (cancelled) return;
      res.match({
        Ok: ({ body }) => {
          const [current, max, , errStatus, errMsg] = body as [
            number,
            number,
            string,
            number,
            string,
          ];
          if (errStatus !== 0) {
            setError(`ddcutil GetVcp (${errStatus}): ${errMsg}`);
            return;
          }
          maxRef.current = max > 0 ? max : 100;
          setBrightness(Math.round((current / maxRef.current) * 100));
          setError(null);
        },
        Err: (err) => setError(err.message),
      });

      const matchRes = await dbus.match({
        bus: Bus.Session,
        sender: DDC_BUS,
        path: DDC_PATH,
        interface: DDC_IFACE,
        member: "VcpValueChanged",
      });
      if (cancelled) {
        matchRes.match({ Ok: (m) => m.stop(), Err: () => {} });
        return;
      }
      matchRes.match({
        Ok: (listening) => {
          stopMatch = listening.stop;
          const reader = listening.items.getReader();
          (async () => {
            while (!cancelled) {
              const { done, value } = await reader.read();
              if (done || !value) break;
              if (value.kind === ListenedKind.Heard) {
                const [, , vcpCode, vcpNewValue] = value.item.body as [
                  number,
                  string,
                  number,
                  number,
                  string,
                  string,
                ];
                if (vcpCode === VCP_BRIGHTNESS) {
                  setBrightness(
                    Math.round((vcpNewValue / maxRef.current) * 100),
                  );
                }
              }
            }
          })();
        },
        Err: () => {},
      });
    })();

    return () => {
      cancelled = true;
      stopMatch?.();
      if (debounceRef.current !== null) {
        window.clearTimeout(debounceRef.current);
      }
    };
  }, []);

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={`bar-btn icon-btn${error ? " is-dimmed" : ""}${open ? " is-open" : ""}`}
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

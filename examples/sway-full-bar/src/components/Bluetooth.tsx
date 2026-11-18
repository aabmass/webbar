import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import type { BluetoothDevice } from "@webbar/types";
import { BluetoothIcon } from "./Icons";
import { Popover } from "./Popover";

function BatteryGlyph({ percent }: { percent: number }) {
  const level = percent > 50 ? "high" : percent > 20 ? "mid" : "low";
  return (
    <span className={`battery-glyph is-${level}`} aria-hidden="true">
      <span style={{ width: `${Math.max(8, Math.min(100, percent))}%` }} />
    </span>
  );
}

export function Bluetooth() {
  const bt = useSyncExternalStore(
    window.webbar.bluetooth.subscribe,
    window.webbar.bluetooth.get,
  );
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  const [connectingPaths, setConnectingPaths] = useState<Set<string>>(
    () => new Set(),
  );

  const adapter = bt.adapters[0];
  const pairedDevices = bt.devices.filter((d) => d.paired);
  const connectedCount = pairedDevices.filter((d) => d.connected).length;

  async function toggleDevice(device: BluetoothDevice) {
    setConnectingPaths((prev) => new Set(prev).add(device.path));
    setError(null);
    try {
      if (device.connected) {
        await window.webbar.bluetooth.disconnect(device.path);
      } else {
        await window.webbar.bluetooth.connect(device.path);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setConnectingPaths((prev) => {
        const next = new Set(prev);
        next.delete(device.path);
        return next;
      });
    }
  }

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={`bar-btn icon-btn${connectedCount > 0 ? " is-active" : ""}${error || !bt.available ? " is-dimmed" : ""}${open ? " is-open" : ""}`}
        title={
          error
            ? `Bluetooth error: ${error}`
            : !bt.available
              ? "Bluetooth unavailable"
              : connectedCount > 0
                ? `Bluetooth: ${connectedCount} connected`
                : "Bluetooth"
        }
        onClick={() => setOpen((prev) => !prev)}
      >
        <BluetoothIcon />
      </button>

      <Popover open={open} onClose={close} anchorRef={btnRef} align="end">
        <div className="qs-header">
          <div className="qs-heading">
            <span className="qs-title">
              <BluetoothIcon /> Bluetooth
            </span>
            <span className="qs-subtitle">
              {!bt.available
                ? "Unavailable"
                : adapter && !adapter.powered
                  ? "Off"
                  : connectedCount > 0
                    ? `${connectedCount} connected`
                    : "On"}
            </span>
          </div>
          {adapter && (
            <button
              type="button"
              role="switch"
              aria-checked={adapter.powered}
              aria-label="Bluetooth power"
              className={`qs-switch${adapter.powered ? " is-on" : ""}`}
              onClick={() => {
                setError(null);
                window.webbar.bluetooth
                  .setPowered(adapter.path, !adapter.powered)
                  .catch((err: unknown) =>
                    setError(err instanceof Error ? err.message : String(err)),
                  );
              }}
            />
          )}
        </div>
        <div className="bt-popover-list">
          {pairedDevices.length === 0 ? (
            <span className="drawer-empty">No paired devices</span>
          ) : (
            pairedDevices.map((device) => {
              const isConnecting = connectingPaths.has(device.path);
              const stateClass = isConnecting
                ? "connecting"
                : device.connected
                  ? "connected"
                  : "paired";
              return (
                <button
                  key={device.path}
                  type="button"
                  className={`bar-btn bt-device ${stateClass}`}
                  title={`${device.name} (${device.address})`}
                  onClick={() => void toggleDevice(device)}
                >
                  <span className="bt-dot" aria-hidden="true" />
                  <span className="bt-name">{device.name}</span>
                  {device.battery != null && (
                    <span className="bt-battery">
                      {device.battery}%
                      <BatteryGlyph percent={device.battery} />
                    </span>
                  )}
                </button>
              );
            })
          )}
        </div>
      </Popover>
    </>
  );
}

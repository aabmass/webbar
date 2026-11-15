import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import type { BluetoothDevice } from "../global";
import { BluetoothIcon } from "./Icons";
import { Popover } from "./Popover";

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
                  {device.name}
                  {device.battery != null ? ` (${device.battery}%)` : ""}
                </button>
              );
            })
          )}
        </div>
      </Popover>
    </>
  );
}

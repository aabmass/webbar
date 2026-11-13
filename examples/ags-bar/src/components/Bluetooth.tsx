import { useEffect, useState } from "react";
import {
  type Bluetooth as BluetoothState,
  type Device,
  connect,
  disconnect,
  watchBluetooth,
} from "@domicile-desktop/system-bluetooth/bluetooth";
import { BluetoothIcon } from "./Icons";

export function Bluetooth() {
  const [bt, setBt] = useState<BluetoothState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [connectingPaths, setConnectingPaths] = useState<Set<string>>(
    () => new Set(),
  );

  useEffect(() => {
    const dbus = window.domicile?.dbus;
    if (!dbus) return;
    return watchBluetooth({ dbus }, (result) => {
      result.match({
        Ok: (next) => {
          setBt(next);
          setError(null);
        },
        Err: (err) => setError(err.message),
      });
    });
  }, []);

  const pairedDevices = bt?.devices.filter((d) => d.paired) ?? [];
  const connectedCount = pairedDevices.filter((d) => d.connected).length;

  async function toggleDevice(device: Device) {
    const dbus = window.domicile?.dbus;
    if (!dbus) return;

    setConnectingPaths((prev) => new Set(prev).add(device.path));
    try {
      if (device.connected) {
        await disconnect({ dbus }, device.path);
      } else {
        await connect({ dbus }, device.path);
      }
    } finally {
      setConnectingPaths((prev) => {
        const next = new Set(prev);
        next.delete(device.path);
        return next;
      });
    }
  }

  return (
    <div className="widget-group">
      {expanded && (
        <div className="inline-drawer bt-devices">
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
                  {device.battery !== undefined ? ` (${device.battery}%)` : ""}
                </button>
              );
            })
          )}
        </div>
      )}

      <button
        type="button"
        className={`bar-btn icon-btn${connectedCount > 0 ? " is-active" : ""}${error ? " is-dimmed" : ""}`}
        title={
          error
            ? `Bluetooth error: ${error}`
            : connectedCount > 0
              ? `Bluetooth: ${connectedCount} connected`
              : "Bluetooth"
        }
        onClick={() => setExpanded((prev) => !prev)}
      >
        <BluetoothIcon />
      </button>
    </div>
  );
}

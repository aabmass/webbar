import { useEffect, useState } from "react";
import type { TrayItem } from "../global";

function trayShortLabel(item: TrayItem): string {
  const raw = item.title || item.icon_name || item.bus;
  return raw.length > 14 ? `${raw.slice(0, 12)}…` : raw;
}

export function Tray() {
  const [items, setItems] = useState<TrayItem[]>([]);

  useEffect(() => {
    if (!window.tray) return;
    return window.tray.onChange(setItems);
  }, []);

  if (items.length === 0) {
    return null;
  }

  return (
    <div className="tray" role="list" aria-label="System tray">
      {items.map((item) => {
        const key = item.id || `${item.bus}${item.path}`;
        const label = item.title || item.icon_name || item.bus;
        return (
          <button
            key={key}
            type="button"
            className={`bar-btn tray-item${item.icon_url ? " icon-btn" : ""}`}
            title={`${label} (${item.bus}${item.path})`}
            onClick={() => window.tray?.activate(item.bus, item.path)}
          >
            {item.icon_url ? (
              <img className="tray-icon" src={item.icon_url} alt={label} />
            ) : (
              trayShortLabel(item)
            )}
          </button>
        );
      })}
    </div>
  );
}

import { useEffect, useState } from "react";
import type { TrayItem } from "./global";
import "./App.css";

function useTray(): TrayItem[] {
  const [items, setItems] = useState<TrayItem[]>([]);

  useEffect(() => {
    if (!window.tray) return;
    return window.tray.onChange(setItems);
  }, []);

  return items;
}

export function App() {
  const trayItems = useTray();

  return (
    <header className="bar">
      <span className="brand">react-tray</span>

      <div className="tray" role="list">
        {trayItems.length === 0 ? (
          <span className="empty">No tray items</span>
        ) : (
          trayItems.map((item) => (
            <button
              key={item.id}
              className="tray-item"
              title={item.title || item.icon_name || item.id}
              onClick={() => window.tray?.activate(item.bus, item.path)}
            >
              {item.title || item.icon_name || item.bus}
            </button>
          ))
        )}
      </div>
    </header>
  );
}

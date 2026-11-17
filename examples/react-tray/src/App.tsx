import { useSyncExternalStore } from "react";
import type { TrayItem } from "@webbar/types";
import "./App.css";

export function App() {
  const trayItems = useSyncExternalStore(
    window.webbar.tray.subscribe,
    window.webbar.tray.get,
  );

  return (
    <header className="bar">
      <span className="brand">react-tray</span>

      <div className="tray" role="list">
        {trayItems.length === 0 ? (
          <span className="empty">No tray items</span>
        ) : (
          trayItems.map((item) => {
            const key = `${item.bus}${item.path}`;
            return (
              <button
                key={key}
                className="tray-item"
                title={item.title || item.icon_name || key}
                onClick={() => window.webbar.tray.activate(item.bus, item.path)}
              >
                {item.title || item.icon_name || item.bus}
              </button>
            );
          })
        )}
      </div>
    </header>
  );
}

import { useState } from "react";
import { AudioOutput } from "./components/AudioOutput";
import { Bluetooth } from "./components/Bluetooth";
import { Clock } from "./components/Clock";
import { MonitorBacklight } from "./components/MonitorBacklight";
import { Notifications } from "./components/Notifications";
import { NotificationToasts } from "./components/NotificationToasts";
import { Stasis } from "./components/Stasis";
import { Tray } from "./components/Tray";
import { Workspaces } from "./components/Workspaces";

export function App() {
  const [notifOpen, setNotifOpen] = useState(false);

  return (
    <>
      <div className="bar-shell">
        <header className="centerbox">
          <div className="bar-start">
            <Workspaces />
          </div>

          <div className="bar-center">
            <Clock />
            <Notifications open={notifOpen} onOpenChange={setNotifOpen} />
          </div>

          <div className="bar-end">
            <Tray />
            <div
              className="widget-cluster"
              role="group"
              aria-label="System controls"
            >
              <Stasis />
              <AudioOutput />
              <MonitorBacklight />
              <Bluetooth />
            </div>
          </div>
        </header>
      </div>
      <NotificationToasts hidden={notifOpen} />
    </>
  );
}

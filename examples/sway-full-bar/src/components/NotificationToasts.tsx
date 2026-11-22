import { useEffect, useState, useSyncExternalStore } from "react";
import {
  activePopups,
  isFocusedOutput,
} from "@webbar/sway-full-bar-shared/notifications";
import { NotificationCard } from "./NotificationCard";

export function NotificationToasts({ hidden }: { hidden: boolean }) {
  const notifState = useSyncExternalStore(
    window.webbar.notifications.subscribe,
    window.webbar.notifications.get,
  );
  const sway = useSyncExternalStore(
    window.webbar.sway.subscribe,
    window.webbar.sway.get,
  );
  const [nowMs, setNowMs] = useState(() => Date.now());

  const popups = activePopups(notifState.items);
  const show =
    !hidden &&
    popups.length > 0 &&
    isFocusedOutput(sway, window.webbar.output);

  useEffect(() => {
    if (!show) return;
    setNowMs(Date.now());
    const timer = window.setInterval(() => setNowMs(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, [show]);

  if (!show) return null;

  return (
    <div
      className="notif-toast-stack"
      role="region"
      aria-label="Notification popups"
    >
      {popups.map((item) => (
        <NotificationCard key={item.id} item={item} nowMs={nowMs} isToast />
      ))}
    </div>
  );
}

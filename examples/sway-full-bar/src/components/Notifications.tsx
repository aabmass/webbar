import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  historyItems,
  isFocusedOutput,
  notificationButtonTitle,
  notificationCenterSubtitle,
} from "@webbar/sway-full-bar-shared/notifications";
import { BellIcon } from "./Icons";
import { NotificationCard } from "./NotificationCard";
import { Popover } from "./Popover";

export function Notifications({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (next: boolean) => void;
}) {
  const notifState = useSyncExternalStore(
    window.webbar.notifications.subscribe,
    window.webbar.notifications.get,
  );
  const [nowMs, setNowMs] = useState(() => Date.now());
  const btnRef = useRef<HTMLButtonElement>(null);
  const openRef = useRef(open);
  openRef.current = open;

  const close = useCallback(() => onOpenChange(false), [onOpenChange]);

  useEffect(() => {
    return window.webbar.onHostAction((action) => {
      if (action !== "notifications_toggle_center") return;
      if (!isFocusedOutput(window.webbar.sway.get(), window.webbar.output)) {
        return;
      }
      onOpenChange(!openRef.current);
    });
  }, [onOpenChange]);

  useEffect(() => {
    if (!open) return;
    setNowMs(Date.now());
    const timer = window.setInterval(() => setNowMs(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, [open]);

  const unread = notifState.unread_count;
  useEffect(() => {
    if (open && unread > 0) {
      window.webbar.notifications.markAllRead();
    }
  }, [open, unread]);

  const items = historyItems(notifState.items);

  const btnClass = [
    "bar-btn",
    "icon-btn",
    "notif-btn",
    notifState.do_not_disturb ? "is-dnd" : "",
    unread > 0 && !notifState.do_not_disturb ? "is-active" : "",
    !notifState.available ? "is-dimmed" : "",
    open ? "is-open" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={btnClass}
        aria-label="Notifications"
        title={notificationButtonTitle(notifState)}
        onClick={() => onOpenChange(!open)}
        onContextMenu={(e) => {
          e.preventDefault();
          window.webbar.notifications.setDoNotDisturb(!notifState.do_not_disturb);
        }}
      >
        <BellIcon dnd={notifState.do_not_disturb} />
        {unread > 0 && (
          <span className="notif-badge" aria-hidden="true">
            {unread > 99 ? "99+" : unread}
          </span>
        )}
      </button>

      <Popover open={open} onClose={close} anchorRef={btnRef} align="end">
        <div className="notif-popover">
          <div className="notif-header">
            <div className="qs-heading">
              <span className="qs-title">
                <BellIcon dnd={notifState.do_not_disturb} /> Notifications
              </span>
              <span className="qs-subtitle">
                {notificationCenterSubtitle(
                  notifState.do_not_disturb,
                  items.length,
                )}
              </span>
            </div>
            <div className="notif-header-controls">
              <span
                className={`notif-dnd-control${notifState.do_not_disturb ? " is-on" : ""}`}
                title="Do Not Disturb"
              >
                <BellIcon dnd />
                <button
                  type="button"
                  role="switch"
                  aria-checked={notifState.do_not_disturb}
                  aria-label="Do Not Disturb"
                  title="Do Not Disturb"
                  className={`qs-switch${notifState.do_not_disturb ? " is-on" : ""}`}
                  onClick={() =>
                    window.webbar.notifications.setDoNotDisturb(
                      !notifState.do_not_disturb,
                    )
                  }
                />
              </span>
              <button
                type="button"
                className="notif-clear-btn"
                disabled={items.length === 0}
                onClick={() => window.webbar.notifications.clearAll()}
              >
                Clear
              </button>
            </div>
          </div>

          <div className="notif-list">
            {items.length === 0 ? (
              <span className="drawer-empty">No notifications</span>
            ) : (
              items.map((item) => (
                <NotificationCard key={item.id} item={item} nowMs={nowMs} />
              ))
            )}
          </div>
        </div>
      </Popover>
    </>
  );
}

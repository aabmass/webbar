# Notifications in `webbar`

`webbar` includes a built-in [`org.freedesktop.Notifications`](https://specifications.freedesktop.org/notification-spec/latest/) (v1.2) daemon in Rust and exposes reactive notification state on `window.webbar.notifications`.

Because `webbar` manages expiry timers, Do Not Disturb suppression, `replaces_id`, synchronous tag replacement (`synchronous` / `x-canonical-private-synchronous`), transient/resident rules, markup stripping, icon resolution, and the 50-item history cap on the host, your bar UI only needs minimal view-local state.

---

## Store & Actions Reference

`window.webbar.notifications` is a reactive `Store<NotificationsState>` (`{ get, subscribe, onChange }`) with action methods:

```ts
type NotificationsState = {
  available: boolean;      // false if another daemon owns org.freedesktop.Notifications or --no-notifications is set
  do_not_disturb: boolean;
  unread_count: number;
  items: NotificationItem[];
};
```

Each `NotificationItem` provides:

- **Identity & metadata:** `id`, `app_name`, `desktop_entry`, `category`, `timestamp_ms`, `urgency` (`"low" | "normal" | "critical"`)
- **Content & icons:** `summary`, `body` (markup stripped), `app_icon_url`, `app_icon_name`, `image_url`, `progress` (`number | null`, `0..=100`), `actions` (`{ key, label }[]`)
- **Lifecycle flags:** `popup` (currently showing as a toast), `read`, `transient`, `resident`

### Methods on `window.webbar.notifications`

| Method | Description |
|---|---|
| `dismiss(id)` | Remove from history & popups and emit `NotificationClosed` (reason `2`: dismissed by user) |
| `hidePopup(id)` | Hide the floating toast while keeping persistent items in history (transient items are removed) |
| `invokeAction(id, actionKey)` | Emit `ActionInvoked(id, actionKey)` over D-Bus; dismisses unless `resident: true` |
| `clearAll()` | Dismiss all notifications |
| `setDoNotDisturb(enabled)` | Enable or disable Do Not Disturb (`critical` notifications still pop up) |
| `markAllRead()` | Mark all current items as read (`unread_count` becomes `0`) |
| `pauseTimeout(id)` | Pause a toast's auto-expire timer (e.g. on pointer hover) |
| `resumeTimeout(id)` | Resume a toast's auto-expire timer |

---

## Notification Center (`[popover]`) vs. Floating Toasts (`[data-webbar-interactive]`)

`webbar` renders your bar into a transparent full-screen `gtk4-layer-shell` overlay with `exclusive_zone` set to `--height`. How Wayland input regions behave depends on whether a notification UI is a **center popover** or a **floating toast**:

| Surface | HTML / Attribute | Wayland Input Region | Keyboard Focus | Items to Render |
|---|---|---|---|---|
| **Notification Center** | Standard `[popover]` | Expands to the full overlay while open | `KeyboardMode::Exclusive` (`Escape` and click-outside light-dismiss) | `[...state.items].reverse()` |
| **Floating Toasts** | `data-webbar-interactive` on each toast | Unions toast bounding rects with the bar strip | `KeyboardMode::None` (clickable & hoverable without stealing Sway focus) | `state.items.filter((i) => i.popup)` |

> [!IMPORTANT]
> Do **not** wrap floating toasts in `[popover]`, or they will grab keyboard focus and light-dismiss when you click another window. Put `data-webbar-interactive` on each clickable toast card instead—`webbar`'s preload automatically tracks them with `MutationObserver` and `ResizeObserver`.

### Floating Toast Stack Example (React)

```tsx
import { useEffect, useRef, useSyncExternalStore } from "react";
import type { NotificationItem } from "@webbar/types";

function ToastCard({ item }: { item: NotificationItem }) {
  const hoveredRef = useRef(false);
  const hasDefault = item.actions.some((a) => a.key === "default");
  const buttons = item.actions.filter((a) => a.key !== "default");

  // If the toast unmounts while hovered (e.g. hidden when the center opens),
  // resume its timeout so the timer is not left paused.
  useEffect(() => {
    return () => {
      if (hoveredRef.current) {
        hoveredRef.current = false;
        window.webbar.notifications.resumeTimeout(item.id);
      }
    };
  }, [item.id]);

  return (
    <article
      className="notif-toast"
      data-webbar-interactive=""
      onPointerEnter={() => {
        hoveredRef.current = true;
        window.webbar.notifications.pauseTimeout(item.id);
      }}
      onPointerLeave={() => {
        hoveredRef.current = false;
        window.webbar.notifications.resumeTimeout(item.id);
      }}
      onClick={() =>
        hasDefault
          ? window.webbar.notifications.invokeAction(item.id, "default")
          : window.webbar.notifications.hidePopup(item.id)
      }
    >
      <div className="notif-summary">{item.summary}</div>
      {item.body && <div className="notif-body">{item.body}</div>}
      {buttons.map((a) => (
        <button
          key={a.key}
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            window.webbar.notifications.invokeAction(item.id, a.key);
          }}
        >
          {a.label}
        </button>
      ))}
    </article>
  );
}

export function NotificationToasts({ centerOpen }: { centerOpen: boolean }) {
  const notifState = useSyncExternalStore(
    window.webbar.notifications.subscribe,
    window.webbar.notifications.get,
  );
  const popups = notifState.items.filter((i) => i.popup);
  if (centerOpen || popups.length === 0) return null;

  return (
    <div className="notif-toast-stack">
      {popups.map((item) => (
        <ToastCard key={item.id} item={item} />
      ))}
    </div>
  );
}
```

### Notification Center Popover Example (React)

Use a native HTML `[popover]` element (via `showPopover()` / `hidePopover()`). Call `markAllRead()` when opened, wire `setDoNotDisturb` and `clearAll`, and listen for host toggle actions triggered via `gapplication`:

```tsx
useEffect(() => {
  if (open && notifState.unread_count > 0) {
    window.webbar.notifications.markAllRead();
  }
}, [open, notifState.unread_count]);

useEffect(() => {
  return window.webbar.onHostAction((action) => {
    if (action === "notifications_toggle_center") {
      setOpen((prev) => !prev);
    }
  });
}, []);
```

See [examples/sway-full-bar](examples/sway-full-bar/src/components/Notifications.tsx) (React) and [examples/sway-full-bar-svelte](examples/sway-full-bar-svelte/src/components/Notifications.svelte) (Svelte 5) for complete implementations including icons, progress bars, and relative timestamps.

---

## Multi-Monitor Setups

`webbar` runs one WebView per monitor, each sharing the same `notifications` store. To avoid showing duplicate toasts or opening the notification center on every monitor at once, check whether `window.webbar.output` matches the focused Sway output:

```ts
export function isFocusedOutput(): boolean {
  const output = window.webbar.output;
  if (!output) return true;
  const focused = window.webbar.sway.get().workspaces.find((ws) => ws.focused)?.output;
  return !focused || focused === output;
}
```

---

## Remote Control & Daemon Ownership

- **Keybindings & remote control:** `webbar` exposes `notifications-toggle-center`, `notifications-toggle-do-not-disturb`, `notifications-dismiss-latest`, and `notifications-clear-all` over session D-Bus (`gapplication action dev.local.webbar <action>`). See [README.md#remote-control-d-bus--gapplication](README.md#remote-control-d-bus--gapplication) for the full action table and Sway `bindsym` examples.
- **Single-daemon ownership:** Only one daemon can own `org.freedesktop.Notifications` on your session D-Bus at a time. If another daemon (`mako`, `swaync`, `waybar`) is already running, `webbar` logs a warning and sets `available: false` in `window.webbar.notifications`. Pass `--no-notifications` if you want `webbar` to leave `org.freedesktop.Notifications` unclaimed for another daemon.

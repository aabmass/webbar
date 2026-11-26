# Notifications in `webbar`

`webbar` includes a built-in [`org.freedesktop.Notifications`](https://specifications.freedesktop.org/notification-spec/latest/) (v1.2) daemon in Rust and exposes reactive notification state on `window.webbar.notifications`.

Because `webbar` manages expiry timers (including auto-resuming paused hover timers), Do Not Disturb suppression, `replaces_id`, synchronous tag replacement (`synchronous` / `x-canonical-private-synchronous`), transient/resident rules, markup stripping, icon resolution, newest-first ordering, and the 50-item history cap on the host, your bar UI only needs minimal view-local state.

---

## Store & Actions Reference

`window.webbar.notifications` is a reactive `Store<NotificationsState>` (`{ get, subscribe, onChange }`) with action methods:

```ts
type NotificationsState = {
  available: boolean;      // false if another daemon owns org.freedesktop.Notifications or --no-notifications is set
  do_not_disturb: boolean;
  unread_count: number;
  items: NotificationItem[]; // newest-first
};
```

Each `NotificationItem` provides:

- **Identity & metadata:** `id`, `app_name`, `desktop_entry`, `category`, `timestamp_ms`, `urgency` (`"low" | "normal" | "critical"`)
- **Content & icons:** `summary`, `body` (markup stripped), `app_icon_url`, `app_icon_name`, `image_url`, `progress` (`number | null`, `0..=100`), `has_default_action`, `actions` (`{ key, label }[]`, excluding `"default"` so every entry maps 1:1 to a button)
- **Lifecycle flags:** `popup` (currently showing as a toast), `read`, `transient`, `resident`

### Methods on `window.webbar.notifications`

| Method | Description |
|---|---|
| `activate(id)` | Emit a Wayland `ActivationToken` and invoke the `"default"` action if `has_default_action` is true (which lets browsers like Chrome switch to the originating tab and raise its window), then focus/jump to the activated or matching Sway window across workspaces; if `has_default_action` is false, hide the floating popup if `popup` is true and focus the matching Sway window |
| `dismiss(id)` | Remove from history & popups and emit `NotificationClosed` (reason `2`: dismissed by user) |
| `hidePopup(id)` | Hide the floating toast while keeping persistent items in history (transient items are removed) |
| `invokeAction(id, actionKey)` | Emit a Wayland `ActivationToken` and `ActionInvoked(id, actionKey)` over D-Bus, focus any newly activated/urgent Sway window, and dismiss unless `resident: true` |
| `clearAll()` | Dismiss all notifications |
| `setDoNotDisturb(enabled)` | Enable or disable Do Not Disturb (`critical` notifications still pop up) |
| `markAllRead()` | Mark all current items as read (`unread_count` becomes `0`) |
| `pauseTimeout(id)` | Pause a toast's auto-expire timer (e.g. on pointer hover) |
| `resumeTimeout(id)` | Resume a toast's auto-expire timer (auto-resumed on overlay open, focus change, or disconnect) |

---

## Notification Center (`popover="auto"`) vs. Floating Toasts (`popover="manual"`)

`webbar` renders a bar surface into a transparent `gtk4-layer-shell` overlay with `exclusive_zone` set from `webbar.json` (your bar's `height` or `width`). Wayland input regions work in two clean modes:

| Surface | HTML / Attribute | Wayland Input Region | Keyboard Focus | Items to Render |
|---|---|---|---|---|
| **Notification Center** | Standard `[popover]` (`popover="auto"`) or `acquireOverlay()` | Expands to the full overlay while open | `KeyboardMode::Exclusive` (`Escape` and click-outside light-dismiss) | `state.items` |
| **Floating Toasts** | `popover="manual"` (or legacy `data-webbar-interactive`) | Unions clickable toast bounding rects with the bar strip | `KeyboardMode::None` (clickable & hoverable without stealing Sway focus) | `state.items.filter((i) => i.popup)` |

> [!IMPORTANT]
> Put your floating toast stack in `<div popover="manual" style="pointer-events: none">` and give each toast card `pointer-events: auto`—`webbar`'s preload automatically tracks the outermost clickable children with `MutationObserver`, `ResizeObserver`, and live animation sync (`document.getAnimations()`), while letting clicks pass through transparent gaps and releasing capture immediately when `pointer-events: none` or `visibility: hidden` is set during exit animations. See [SURFACES.md](SURFACES.md) for full input-masking details.

### Floating Toast Stack Example (React)

```tsx
import { useSyncExternalStore } from "react";
import type { NotificationItem } from "@webbar/types";

function ToastCard({ item }: { item: NotificationItem }) {
  return (
    <article
      className="notif-toast"
      data-webbar-interactive=""
      onPointerEnter={() => window.webbar.notifications.pauseTimeout(item.id)}
      onPointerLeave={() => window.webbar.notifications.resumeTimeout(item.id)}
      onClick={() => window.webbar.notifications.activate(item.id)}
    >
      <div className="notif-summary">{item.summary}</div>
      {item.body && <div className="notif-body">{item.body}</div>}
      {item.actions.map((a) => (
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
  const focused = useSyncExternalStore(
    window.webbar.focused.subscribe,
    window.webbar.focused.get,
  );
  const popups = notifState.items.filter((i) => i.popup);
  if (centerOpen || !focused || popups.length === 0) return null;

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
    } else if (action === "notifications_close_center") {
      setOpen(false);
    }
  });
}, []);
```

See [examples/sway-full-bar](examples/sway-full-bar/src/components/Notifications.tsx) (React) and [examples/sway-full-bar-svelte](examples/sway-full-bar-svelte/src/components/Notifications.svelte) (Svelte 5) for complete implementations including icons, progress bars, and relative timestamps.

---

## Multi-Monitor Setups

`webbar` runs one WebView per monitor, each sharing the same `notifications` store:

- **Toasts:** Subscribe to `window.webbar.focused` (`Store<boolean>`) so floating popups only appear on the currently focused monitor.
- **Toggle Center action:** `notifications-toggle-center` is automatically routed by the Rust host to the focused Sway monitor's WebView (falling back to broadcast when single-monitor or Sway is unavailable).

---

## Remote Control & Daemon Ownership

- **Keybindings & remote control:** `webbar` exposes `notifications-toggle-center`, `notifications-toggle-do-not-disturb`, `notifications-dismiss-latest`, and `notifications-clear-all` over session D-Bus (`gapplication action dev.local.webbar <action>`). See [README.md#remote-control-d-bus--gapplication](README.md#remote-control-d-bus--gapplication) for the full action table and Sway `bindsym` examples.
- **Single-daemon ownership:** Only one daemon can own `org.freedesktop.Notifications` on your session D-Bus at a time. If another daemon (`mako`, `swaync`, `dunst`) is already running, `webbar` logs a warning and sets `available: false` in `window.webbar.notifications`. Pass `--no-notifications` if you want `webbar` to leave `org.freedesktop.Notifications` unclaimed for another daemon.

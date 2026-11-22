# `webbar` Design Doc

## Goal
Let users build a Sway status bar in plain HTML, CSS, and TypeScript/React—without switching away from Sway as their Wayland compositor and without fighting imperative D-Bus or CLI plumbing in frontend code.

## Architecture

`webbar` splits responsibilities across three layers:

```
┌──────────────────────────────────────────────────────────────────────┐
│  User Bar UI (examples/minimal/, examples/sway-full-bar/, etc.)      │
│  HTML / CSS / TS / React using window.webbar.*                       │
│  (sway, tray, notifications, audio, bluetooth, network, backlight,   │
│   theme)                                                             │
└─────────────────────────────────▲────────────────────────────────────┘
                                  │ injected at DocumentStart (or Vite entry)
┌─────────────────────────────────┴────────────────────────────────────┐
│  Preload Bridge (preload/ -> generated/preload.js + @webbar/types)   │
│  Exposes reactive { get, subscribe, onChange } stores + Promise      │
│  actions over a token-authenticated localhost WebSocket              │
└─────────────────────────────────▲────────────────────────────────────┘
                                  │ JSON over local WebSocket (ws_bridge.rs)
┌─────────────────────────────────┴────────────────────────────────────┐
│  Rust Host (src/*.rs) — 1 shared instance across all monitors        │
│  gtk4-layer-shell (1 bar/monitor) + shared WebKitWebProcess WebViews │
│  - ws_bridge.rs (localhost WS server + SnapshotCache replay on open) │
│  - bridge.rs (protocol types + SnapshotCache + ts-rs bindings)       │
│  - sway_ipc.rs (swayipc: tree, workspaces, mode, commands)           │
│  - tray.rs (StatusNotifierWatcher + DBusMenu) + notifications.rs     │
│  - icons.rs (shared IconTheme lookup, SVG recoloring, .desktop)      │
│  - audio.rs (libpulse) + bluetooth.rs (BlueZ) + network.rs (NM)      │
│  - backlight.rs (ddcutil D-Bus + /sys/class/backlight + logind)      │
│  - theme.rs (adw::StyleManager + gtk4::Settings + StyleContext)      │
│  - dbus_bridge.rs / process_bridge.rs (Promise-based escape hatches) │
└──────────────────────────────────────────────────────────────────────┘
```

## Key Design Decisions

1. **Unified local WebSocket bridge (`src/ws_bridge.rs`) + TypeScript preload**
   - `preload/src/preload.ts` and `preload/src/index.ts` compile to `generated/preload.js` (injected at `UserScriptInjectionTime::Start` in WebKitGTK) and expose the `@webbar/types` workspace package (shared by all TS/React frontends).
   - Both WebKitGTK bar windows and external browsers (via `--ws-bridge <port>`) talk to the same loopback WebSocket server authenticated by a per-session token (`wsPort`/`wsToken` query params), so every page immediately has `window.webbar` with live Rust backend data.
   - Additional monitor `WebView` instances attach to an existing live `WebView` via `related_view` so multi-monitor setups share one `WebKitWebProcess`.

2. **System state aggregation in Rust, web-native stores in JS**
   - Instead of pushing raw D-Bus signals or subprocesses into every monitor's `WebView`, Rust runs one shared watcher per subsystem (`audio`, `bluetooth`, `network`, `backlight`, `sway`, `tray`, `notifications`), deduplicates/debounces updates, and caches the latest snapshot in `SnapshotCache` for instant replay on WebSocket connect or Vite HMR reload.
   - Each store exposes `{ get(), subscribe(cb), onChange(cb) }`, working directly with React's `useSyncExternalStore(store.subscribe, store.get)` as well as vanilla TS listeners, while mutations return standard `Promise`s.
   - `window.webbar.dbus`, `window.webbar.exec`, and `window.webbar.execStream` remain available as Promise-based escape hatches for custom widgets.

3. **Sway IPC in Rust (`src/sway_ipc.rs`, `src/sway_state.rs`, `build.rs`)**
   - `sway_ipc.rs` uses `swayipc` to subscribe to `workspace`, `window`, `mode`, and `output` events with exponential-backoff reconnect, coalesces event bursts, and emits full `SwayState` snapshots (`tree`, `workspaces`, `mode`) using Sway's own IPC types.
   - `build.rs` traces `SwayState` via `serde-reflection` to generate `preload/src/generated/sway.ts` at build time.
   - Commands run on a dedicated thread and return an `RpcReply` (`SwayCommandError` on failure), exposed via `sway.command`.

4. **Explicit UI source (`--dir` / `--url` / `--ws-bridge`) + Nix**
   - `webbar` takes `--dir <path>` (served over a hardened `webbar://app/...` URI scheme), `--url <url>` (e.g., a Vite dev server for live reload), or `--ws-bridge <port>` alone for headless browser-dev mode.
   - `flake.nix` provides a reproducible `devShell`, `packages.default` (the `webbar` binary), and `sway-full-bar` / `sway-full-bar-dev` / `sway-full-bar-browser-dev` wrappers.

5. **Tray & notification icons resolved in Rust to `data:` URLs (`src/icons.rs`, `src/tray.rs`, `src/notifications.rs`)**
   - Tray items and notifications expose icons over D-Bus in multiple formats: theme icon names (`IconName` / `app_icon`), `.desktop` entry fallbacks (`desktop-entry`), `file://` URIs (percent-decoded, 4 MiB cap), raw ARGB32 pixmaps (`IconPixmap`), or raw RGBA/RGB `image-data` (bounds-checked and downscaled to 128px max).
   - Rust resolves all of these on the host in `src/icons.rs` (using GTK4's `IconTheme` on the GLib main thread and `gdk_pixbuf` for raw pixel buffers), recolors `current-color-scheme` and `currentColor` SVGs from `SharedIconPalette` (`ThemeState`) with live refreshes on theme/icon-theme changes, and sends ready-to-render base64 `data:` URLs (`icon_url` / `app_icon_url` / `image_url`).
   - `icon_name` / `app_icon_name` are also passed through so frontends can identify freedesktop `-symbolic` icons and style them with CSS `mask-image`.

6. **OS theming via `SnapshotCache` + `:root` CSS variables (`src/theme.rs`, `preload/src/preload.ts`)**
   - WebKitGTK does not expose GTK4/Libadwaita colors or `gtk-font-name` to web content. `src/theme.rs` uses `libadwaita::StyleManager`, `gtk4::Settings`, `gtk4::pango::FontDescription`, and `gtk4::StyleContext::lookup_color` to watch dark/light mode, accent color, font, and GTK/Libadwaita named colors.
   - `ThemeState` is broadcast as `BridgeEvent::Theme` through `SnapshotCache` and `ws_bridge.rs`, so both WebKitGTK bar windows and `--ws-bridge` external browsers receive live updates on `window.webbar.theme` and `:root` (`--desktop-*` semantic tokens, `--gtk-*` GTK named colors, `color-scheme`, and `data-desktop-theme`).
   - In WebKitGTK mode, `src/main.rs` also injects the initial `:root` CSS block as a `UserStyleSheet` (`UserStyleLevel::Author`) so the first paint has zero flash before the WebSocket connects, after which `preload.ts` updates `document.documentElement.style` in place. See [THEMING.md](THEMING.md) for the user-facing variable reference.

7. **Web-standard Popovers and Composable Wayland Input Regions (`src/main.rs`, `preload/src/index.ts`)**
   - Each monitor's `gtk4-layer-shell` window is sized to full monitor height (or `--overlay-height`) with `exclusive_zone` set to `--height` and a transparent WebView. At `UserStyleLevel::User`, `src/main.rs` injects `--webbar-height: <height>px` on `:root` and `html, body { background: transparent; }` so bars get a transparent canvas and the configured height by default while remaining overridable by author CSS.
   - `WindowInputState` composes two orthogonal layers:
     - **Base bar strip + `interactive_rects`** (`[data-webbar-interactive]` / `setInteractiveRegions()`): the top bar strip unioned with floating interactive regions (such as notification toasts, including non-modal `[popover][data-webbar-interactive]`) under `KeyboardMode::None`, keeping them clickable and hoverable without stealing Sway focus.
     - **Active modal overlay** (`[popover]:not([data-webbar-interactive])` / `acquireOverlay()`): expands to `(0, 0, monitor_width, overlay_height)` under `KeyboardMode::Exclusive` for clicks, `Escape`, and click-outside light-dismiss.
   - `preload/src/index.ts` automatically tracks native modal `[popover]` elements (via capturing `toggle` events and `:popover-open:not([data-webbar-interactive])`, with a lazy `MutationObserver` while open) and `[data-webbar-interactive]` elements (via `MutationObserver`, `ResizeObserver`, `transitionend`/`animationend`, and `requestAnimationFrame` coalescing), replaying `interactive_rects` on WebSocket reconnect.

8. **Notification Daemon (`org.freedesktop.Notifications` v1.2) & Host Actions (`src/notifications.rs`, `src/main.rs`)**
   - By default in bar mode, `webbar` claims `org.freedesktop.Notifications` (supporting `GetCapabilities`, `Notify`, `CloseNotification`, `GetServerInformation`, `NotificationClosed` reasons 1–4, `ActionInvoked`, `replaces_id`, `synchronous` / `x-canonical-private-synchronous`, `transient`, `resident`, `value` progress, urgency/Do Not Disturb, 50-item history cap, `quick-xml` markup stripping, newest-first snapshots, `"default"` split to `has_default_action`, and a `Condvar` popup-expiry thread with hover pause/resume that auto-resumes on overlay open, focused-output change, and disconnect).
   - `window.webbar.notifications` exposes a reactive `Store<NotificationsState>` (`{ available, do_not_disturb, unread_count, items }`) plus `activate`, `dismiss`, `hidePopup`, `invokeAction`, `clearAll`, `setDoNotDisturb`, `markAllRead`, `pauseTimeout`, and `resumeTimeout`; `window.webbar.focused` exposes a per-monitor `Store<boolean>` derived from the focused Sway output.
   - Passing `--no-notifications` leaves `org.freedesktop.Notifications` unclaimed (`available: false`); headless `--ws-bridge` skips claiming both D-Bus daemons by default (using `tray::spawn_observer`) unless `--ws-claim-notifications` is passed.
   - `dev.local.webbar` exposes `gio::SimpleAction`s for remote control (`devtools`, `notifications-toggle-center`, `notifications-toggle-do-not-disturb`, `notifications-dismiss-latest`, `notifications-clear-all`); `notifications-toggle-center` routes `BridgeEvent::HostAction` to the focused Sway output's WebView (falling back to broadcast) via `window.webbar.onHostAction(cb)`. See [NOTIFICATIONS.md](NOTIFICATIONS.md) for the user-facing authoring guide.

# `webbar` Design Doc

## Goal
Let users build a Sway status bar in plain HTML, CSS, and TypeScript/React—without switching away from Sway as their Wayland compositor and without fighting imperative D-Bus or CLI plumbing in frontend code.

## Architecture

`webbar` splits responsibilities across three layers:

```
┌──────────────────────────────────────────────────────────────────────┐
│  User Bar UI (web/, examples/sway-full-bar/, or ~/.config/webbar/)   │
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
   - `window.webbar.dbus` and `window.webbar.exec` remain available as Promise-based escape hatches for custom widgets.

3. **Sway IPC in Rust (`src/sway_ipc.rs`, `src/sway_state.rs`, `build.rs`)**
   - `sway_ipc.rs` uses `swayipc` to subscribe to `workspace`, `window`, `mode`, and `output` events with exponential-backoff reconnect, coalesces event bursts, and emits full `SwayState` snapshots (`tree`, `workspaces`, `mode`) using Sway's own IPC types.
   - `build.rs` traces `SwayState` via `serde-reflection` to generate `preload/src/generated/sway.ts` at build time.
   - Commands run on a dedicated thread and return an `RpcReply` (`SwayCommandError` on failure), exposed via `sway.command`.

4. **Explicit UI source (`--dir` / `--url` / `--ws-bridge`) + Nix**
   - `webbar` takes `--dir <path>` (served over a hardened `webbar://app/...` URI scheme), `--url <url>` (e.g., a Vite dev server for live reload), or `--ws-bridge <port>` alone for headless browser-dev mode.
   - `flake.nix` provides a reproducible `devShell`, `packages.default` (installing `web/dist` into `$out/share/webbar`), and `sway-full-bar` / `sway-full-bar-dev` / `sway-full-bar-browser-dev` wrappers.

5. **Tray & DBusMenu icons resolved in Rust to `data:` URLs (`src/tray.rs`)**
   - Tray apps expose icons over D-Bus in multiple formats: theme icon names (`IconName` / `icon-name`), file paths, raw ARGB32 pixmaps (`IconPixmap`), or raw PNG bytes (`icon-data`).
   - Rust resolves all of these on the host (using GTK4's `IconTheme` for theme names and `gdk_pixbuf` for ARGB pixmaps) and sends the frontend a ready-to-render base64 `data:` URL in `icon_url`.
   - `icon_name` is also passed through so frontends can identify freedesktop `-symbolic` icons and recolor them with CSS `mask-image` to match the bar theme.

6. **OS theming via `SnapshotCache` + `:root` CSS variables (`src/theme.rs`, `preload/src/preload.ts`)**
   - WebKitGTK does not expose GTK4/Libadwaita colors or `gtk-font-name` to web content. `src/theme.rs` uses `libadwaita::StyleManager`, `gtk4::Settings`, `gtk4::pango::FontDescription`, and `gtk4::StyleContext::lookup_color` to watch dark/light mode, accent color, font, and GTK/Libadwaita named colors.
   - `ThemeState` is broadcast as `BridgeEvent::Theme` through `SnapshotCache` and `ws_bridge.rs`, so both WebKitGTK bar windows and `--ws-bridge` external browsers receive live updates on `window.webbar.theme` and `:root` (`--desktop-*` semantic tokens, `--gtk-*` GTK named colors, `color-scheme`, and `data-desktop-theme`).
   - In WebKitGTK mode, `src/main.rs` also injects the initial `:root` CSS block as a `UserStyleSheet` (`UserStyleLevel::Author`) so the first paint has zero flash before the WebSocket connects, after which `preload.ts` updates `document.documentElement.style` in place. See [THEMING.md](THEMING.md) for the user-facing variable reference.

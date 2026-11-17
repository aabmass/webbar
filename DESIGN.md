# `webbar` Design Doc

## Goal
Let users build a Sway status bar in plain HTML, CSS, and TypeScript/React—without switching away from Sway as their Wayland compositor and without fighting imperative D-Bus or CLI plumbing in frontend code.

## Architecture

`webbar` splits responsibilities across three layers:

```
┌──────────────────────────────────────────────────────────────────────┐
│  User Bar UI (web/, examples/ags-bar/, or ~/.config/webbar/)         │
│  HTML / CSS / TS / React using window.webbar.*                       │
│  (sway, tray, notifications, audio, bluetooth, network, backlight)   │
└─────────────────────────────────▲────────────────────────────────────┘
                                  │ injected at DocumentStart
┌─────────────────────────────────┴────────────────────────────────────┐
│  Preload Bridge (preload/ -> generated/preload.js + @webbar/types workspace)    │
│  Exposes reactive { get, subscribe, onChange } stores + Promise      │
│  actions over WebKitGTK UserContentManager                           │
└─────────────────────────────────▲────────────────────────────────────┘
                                  │ JSON over webkit.messageHandlers
┌─────────────────────────────────┴────────────────────────────────────┐
│  Rust Host (src/*.rs) — 1 shared instance across all monitors        │
│  gtk4-layer-shell (1 bar/monitor) + webkit6 WebView                  │
│  - bridge.rs (protocol + SnapshotCache replay on LoadEvent::Finished)│
│  - sway_ipc.rs ($SWAYSOCK workspaces, title, mode)                   │
│  - tray.rs (StatusNotifierWatcher + DBusMenu) + notifications.rs     │
│  - audio.rs (libpulse) + bluetooth.rs (BlueZ) + network.rs (NM)      │
│  - backlight.rs (ddcutil D-Bus)                                      │
│  - dbus_bridge.rs / process_bridge.rs (Promise-based escape hatches) │
└──────────────────────────────────────────────────────────────────────┘
```

## Key Design Decisions

1. **WebKitGTK `UserScript` injection with a TypeScript preload**
   - WebKitGTK runs each bar in a sandboxed `WebKitWebProcess` separate from the GTK4 UI process.
   - `preload/src/preload.ts` and `preload/src/index.ts` compile to `generated/preload.js` (embedded at `UserScriptInjectionTime::Start`) and expose the `@webbar/types` workspace package (shared by all TS/React frontends).
   - Every page immediately has `window.webbar` without per-bar bridge boilerplate.

2. **System state aggregation in Rust, web-native stores in JS**
   - Instead of pushing raw D-Bus signals or subprocesses into every monitor's `WebView`, Rust runs one shared watcher per subsystem (`audio`, `bluetooth`, `network`, `backlight`, `sway`, `tray`, `notifications`), deduplicates/debounces updates, and caches the latest snapshot in `SnapshotCache` for instant replay on page load or Vite HMR reload.
   - Each store exposes `{ get(), subscribe(cb), onChange(cb) }`, working directly with React's `useSyncExternalStore(store.subscribe, store.get)` as well as vanilla TS listeners, while mutations return standard `Promise`s.
   - `window.webbar.dbus` and `window.webbar.exec` remain available as Promise-based escape hatches for custom widgets.

3. **Sway IPC in Rust (`src/sway_ipc.rs`)**
   - `sway_ipc.rs` subscribes directly to `$SWAYSOCK` (`workspace`, `window`, `mode`) and forwards commands like workspace switches without spawning `swaymsg`.

4. **Explicit UI source (`--dir` / `--url`) + Nix**
   - `webbar` takes either `--dir <path>` (served over a custom `webbar://app/...` URI scheme) or `--url <url>` (e.g., a Vite dev server for live reload).
   - `flake.nix` provides both a reproducible `devShell` and `packages.default` (which installs the sample `web/dist` into `$out/share/webbar`).

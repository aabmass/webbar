# `webbar` Design Doc

## Goal
Let users build a Sway status bar in plain HTML, CSS, and TypeScript/React—without switching away from Sway as their Wayland compositor and without rewriting Linux D-Bus/Audio/Network integrations from scratch.

## Architecture

`webbar` splits responsibilities across three layers:

```
┌────────────────────────────────────────────────────────────────────┐
│  User Bar UI (web/ or ~/.config/webbar/)                      │
│  HTML / CSS / TS using window.domicile, window.sway,               │
│  window.tray, window.notifications, and @domicile-desktop/system-* │
└─────────────────────────────────▲──────────────────────────────────┘
                                  │ injected at DocumentStart
┌─────────────────────────────────┴──────────────────────────────────┐
│  Runtime Preload (runtime/preload.ts -> embedded src/preload.js)   │
│  Exposes Domicile-compatible dbus/processes/files + sway/tray/     │
│  notifications over WebKitGTK UserContentManager                   │
└─────────────────────────────────▲──────────────────────────────────┘
                                  │ JSON over webkit.messageHandlers
┌─────────────────────────────────┴──────────────────────────────────┐
│  Rust Host (src/*.rs)                                              │
│  gtk4-layer-shell (1 bar/monitor) + webkit6 WebView                │
│  - dbus_json.rs / dbus_bridge.rs  (zbus D-Bus call & async match)  │
│  - process_bridge.rs / file_bridge.rs (OS primitives)              │
│  - sway_ipc.rs ($SWAYSOCK workspaces, title, mode)                 │
│  - tray.rs (StatusNotifierWatcher) + notifications.rs              │
└────────────────────────────────────────────────────────────────────┘
```

## Key Design Decisions

1. **WebKitGTK `UserScript` injection instead of per-bar bridge code**
   - WebKitGTK runs the web page in a sandboxed `WebKitWebProcess` separate from the GTK4 UI process.
   - Instead of requiring every bar project to copy bridge code, `src/main.rs` injects `src/preload.js` at `UserScriptInjectionTime::Start` into every page.
   - Any HTML/CSS/JS (including a plain `index.html` in `~/.config/webbar/` or a Vite+React app) immediately has `window.domicile`, `window.sway`, `window.tray`, and `window.notifications` available.

2. **Reusing `@domicile-desktop/system-*` instead of reimplementing Linux plumbing**
   - In [`cprussin/domicile`](https://github.com/cprussin/domicile), subsystem logic (BlueZ Bluetooth, PipeWire/PulseAudio, NetworkManager) lives in TypeScript npm packages (`@domicile-desktop/system-*`), while Rust (`domicile-host`) provides generic `dbus`, `processes`, and `files` primitives.
   - `webbar` copies `dbus_json.rs` and adapts `system.rs`/`tray.rs` from `domicile-host`, swapping Chromium Mojo IPC for WebKitGTK `UserContentManager` so those npm packages run unmodified.

3. **Sway IPC in Rust (`src/sway_ipc.rs`)**
   - Domicile is its own Wayland compositor and has no Sway client. `sway_ipc.rs` bridges that gap by subscribing to `$SWAYSOCK` (`workspace`, `window`, `mode`) and forwarding commands like workspace switches.

4. **Explicit UI source (`--dir` / `--url`) + Nix**
   - `webbar` takes either `--dir <path>` (served over a custom `webbar://app/...` URI scheme) or `--url <url>` (e.g., a Vite dev server for live reload).
   - `flake.nix` provides both a reproducible `devShell` and `packages.default` (which also installs the sample `web/dist` into `$out/share/webbar`).

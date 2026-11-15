# webbar

Build your Sway status bar with plain HTML, CSS, and TypeScript/React—without switching away from Sway or reinventing Linux D-Bus plumbing.

`webbar` is a Rust Wayland host (`gtk4` + `gtk4-layer-shell` + `webkit6`) that docks a transparent WebView at the top of each monitor and automatically injects `window.webbar` into every page with reactive stores (`sway`, `tray`, `notifications`, `audio`, `bluetooth`, `network`, `backlight`) plus Promise-based `dbus` and `exec` escape hatches.

## Usage

Pass either a directory containing `index.html` or a dev server URL, and optionally `--height <px>` (default `34`):

```bash
webbar --dir /path/to/dist
webbar --url http://localhost:5173
webbar --dir ./examples/ags-bar/dist --height 86
```

## Running with Nix

```bash
# Run with the built-in web/dist example
nix run . -- --dir ./web/dist

# Run with the React tray example
nix run . -- --dir ./examples/react-tray/dist

# Run the React AGS-style bar example (built by Nix)
nix run .#ags-bar

# Enter a dev shell with Rust, Node, GTK4, gtk4-layer-shell, and WebKitGTK 6.0
nix develop
```

## Running without Nix

Requires `rustc`/`cargo`, `node`/`npm`, and system dev libraries for `gtk4`, `gtk4-layer-shell-0`, `webkitgtk-6.0`, and `libpulse`.

```bash
# 1. Build the default bar UI (or examples/react-tray)
cd web
npm install
npm run build

# 2. Build and run webbar
cd ..
cargo run -- --dir ./web/dist
```

### Live Reload with Vite (`examples/ags-bar`)

Run both the Vite dev server and `webbar` with a single command from the repo root:

```bash
nix run .#ags-bar-dev
```

Or start them in two terminals manually (`--replace` swaps out any already-running `webbar` instance):

```bash
# Terminal 1: start Vite dev server (inside nix develop if you don't have Node locally)
cd examples/ags-bar
npm install
npm run dev

# Terminal 2: launch webbar against the Vite dev server
nix run . -- --url http://localhost:5173 --height 86 --replace
# Or with Cargo directly:
# cargo run -- --url http://localhost:5173 --height 86 --replace
```

Edits to `examples/ags-bar/src/` will hot-reload in the bar, and `webbar` automatically replays the latest system snapshots on reload.

> **Note:** Only one daemon can own `org.freedesktop.Notifications` or `org.kde.StatusNotifierWatcher` on your session D-Bus at a time. If another bar or notification daemon (`waybar`, `mako`, `swaync`) is already running, `webbar` logs a warning and still runs, but notifications/tray items stay with the existing daemon until it is stopped.

## Testing & Checks

Run all Rust, preload, and frontend checks reproducibly with Nix:

```bash
nix flake check
```

Or run individual suites locally (or inside `nix develop`):

```bash
# Rust unit tests, lint, and ts-rs binding generation
cargo test
cargo clippy --all-targets

# Preload bridge build, typecheck, and contract tests
cd preload && npm install && npm run build && npm run typecheck && npm test

# Frontend typecheck and production builds
(cd web && npm run typecheck && npm run build)
(cd examples/ags-bar && npm run typecheck && npm run build)
(cd examples/react-tray && npm run typecheck && npm run build)
```

## Architecture & Credits

- Rust payload types use `ts-rs` to generate `preload/src/generated/*.ts` (verified in sync by `nix flake check`).
- `preload/src/preload.ts` and `preload/src/index.ts` compile to `generated/preload.js` and `generated/types/` (`@webbar/types`), providing `{ get, subscribe, onChange }` stores compatible with React's `useSyncExternalStore` and vanilla TypeScript, `window.webbar.output` per monitor, and `AbortSignal` cancellation on `exec` and `dbus.watch`.
- System state (`sway`, `tray`, `notifications`, `audio`, `bluetooth`, `network`, `backlight`) is aggregated once in the Rust host and shared across all monitor WebViews.
- [`src/dbus_json.rs`](src/dbus_json.rs) and [`src/tray.rs`](src/tray.rs) were originally inspired by [domicile](https://github.com/cprussin/domicile) (MIT License).

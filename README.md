# webbar

Build your Sway status bar with plain HTML, CSS, and TypeScript/React—without switching away from Sway or reinventing Linux D-Bus plumbing.

`webbar` is a Rust Wayland host (`gtk4` + `gtk4-layer-shell` + `webkit6`) that docks a transparent WebView at the top of each monitor and automatically injects `window.domicile`, `window.sway`, `window.tray`, and `window.notifications` into every page.

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

Requires `rustc`/`cargo`, `node`/`npm`, and system dev libraries for `gtk4`, `gtk4-layer-shell-0`, and `webkitgtk-6.0`.

```bash
# 1. Build the default bar UI (or examples/react-tray)
cd web
npm install
npm run build

# 2. Build and run webbar
cd ..
cargo run -- --dir ./web/dist
```

Or with Vite live reload on `examples/react-tray` or `examples/ags-bar`:

```bash
# Terminal 1
cd examples/ags-bar && npm install && npm run dev

# Terminal 2
cargo run -- --url http://localhost:5173 --height 86
```

> **Note:** Only one daemon can own `org.freedesktop.Notifications` or `org.kde.StatusNotifierWatcher` on your session D-Bus at a time. If another bar or notification daemon (`waybar`, `mako`, `swaync`) is already running, `webbar` logs a warning and still runs, but notifications/tray items stay with the existing daemon until it is stopped.

---
name: webbar-browser-dev
description: Use when a coding agent or developer needs to preview and inspect the webbar ags-bar UI in a browser with live Rust backend data via --ws-bridge.
---

# Webbar Browser Dev Cycle

Use this workflow to visually inspect `examples/ags-bar` in a browser (with real `sway`, `stasis`, `audio`, `bluetooth`, and `backlight` data) without replacing the user's running desktop bar. In Antigravity, pair this with the `browser` subagent (`/browser`).

> Note: Headless `--ws-bridge` mode does not claim `org.kde.StatusNotifierWatcher` or `org.freedesktop.Notifications` on D-Bus, so `tray` and `notifications` remain empty when running alongside your live bar.

## 1. Start the Dev Stack

From the `webbar` repository root, run the combined Nix target in the background:

```bash
nix run .#ags-bar-browser-dev
```

Or run the two processes separately (useful when iterating on Rust changes too):

```bash
# Terminal 1: headless Rust backend bridge (prints a per-session ws://...?token=... URL)
nix run . -- --ws-bridge 9123

# Terminal 2: Vite dev server
npm --workspace=@webbar/types run build
npm --workspace=examples/ags-bar run dev -- --port 5173 --strictPort
```

Wait until it prints `open http://localhost:5173?wsPort=9123&wsToken=...`.

## 2. Inspect with the Browser Subagent

Open the printed `http://localhost:5173?wsPort=9123&wsToken=...` URL in the browser subagent:
- **Verify live data**: Check that Sway workspaces, clock, Stasis pill, and audio/brightness/bluetooth pills populate and update without console or WebSocket errors (`tray` and `notifications` are expected to be empty in headless mode).
- **Inspect popovers & layout**: Click pills (e.g. the Stasis pill) to open popovers, check `getBoundingClientRect()` coordinates, verify there is no overlap/clipping, and review screenshots.
- **Iterate**: Edit `examples/ags-bar/src/` files — Vite hot-reloads immediately in the browser while keeping the WebSocket connection to `webbar --ws-bridge`.

## 3. Clean Up

Stop the background `ags-bar-browser-dev` (or `webbar` / `vite`) processes when finished.

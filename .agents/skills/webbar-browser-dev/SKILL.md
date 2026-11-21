---
name: webbar-browser-dev
description: Use when a coding agent or developer needs to preview and inspect the webbar sway-full-bar UI in a browser with live Rust backend data via --ws-bridge.
---

# Webbar Browser Dev Cycle

Use this workflow to visually inspect `examples/sway-full-bar` in a browser (with real `sway`, `stasis`, `audio`, `bluetooth`, and `backlight` data) without replacing the user's running desktop bar. In Antigravity, pair this with the `browser` subagent (`/browser`).

> Note: Headless `--ws-bridge` mode does not claim `org.kde.StatusNotifierWatcher` or `org.freedesktop.Notifications` on D-Bus, so `tray` and `notifications` remain empty when running alongside your live bar. Append `&mockTray` to the dev URL to populate mock tray items/menus (`examples/sway-full-bar-shared/src/devMocks.ts`) when inspecting tray styling or popovers.

## 1. Start the Dev Stack

From the `webbar` repository root, run the combined Nix target in the background:

```bash
nix run .#sway-full-bar-browser-dev
# Or for the Svelte alternative (Vite on port 5174):
nix run .#sway-full-bar-svelte-browser-dev
```

Or run the two processes separately (useful when iterating on Rust changes too):

```bash
# Terminal 1: headless Rust backend bridge (prints a per-session ws://...?token=... URL)
nix run . -- --ws-bridge 9123

# Terminal 2: Vite dev server
npm --workspace=@webbar/types run build
npm --workspace=examples/sway-full-bar run dev -- --port 5173 --strictPort
```

Wait until it prints `open http://localhost:5173?wsPort=9123&wsToken=...`.

> Note: The backend needs the host session (Sway IPC socket, session/system D-Bus, PulseAudio), so run the `webbar` / `nix run` process outside any agent sandbox. If a prebuilt `webbar` binary rejects `--ws-bridge`, it is stale — rebuild it (e.g. `nix build`).

## 2. Inspect with the Browser Subagent

Open the printed `http://localhost:5173?wsPort=9123&wsToken=...` URL in the browser subagent:
- **Verify live data**: Check that Sway workspaces, clock, Stasis pill, and audio/brightness/bluetooth pills populate and update without console or WebSocket errors (`tray` and `notifications` are expected to be empty in headless mode).
- **Inspect popovers & layout**: Click pills (e.g. the Stasis pill) to open popovers; use accessibility snapshots for functional checks and `take_screenshot` for visual layout/caret/clipping checks (only call `evaluate_script` / `getBoundingClientRect()` when exact pixel measurements are needed, as it may prompt for MCP permission).
- **Iterate**: Edit `examples/sway-full-bar/src/` files — Vite hot-reloads immediately in the browser while keeping the WebSocket connection to `webbar --ws-bridge`.

## 3. Clean Up

Stop the background `sway-full-bar-browser-dev` (or `webbar` / `vite`) processes when finished.

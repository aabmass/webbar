# Design Learnings from the Quickshell Comparison Experiment

To evaluate `webbar`'s frontend ergonomics and identify real architectural gaps, we recreated `examples/sway-full-bar` (bar, workspaces, clock/calendar, tray, stasis, audio, display/theme, Bluetooth, notification center, and floating toasts) in **Quickshell 0.3.2** (QML) and compared the client-side codebases (excluding the Rust host, `preload/`, and Quickshell's C++ core).

This document captures the **non-obvious design learnings and architectural gaps** uncovered by that experiment. Additive backend stores that follow `webbar`'s existing Rust watcher + `SnapshotCache` pattern (e.g. MPRIS, UPower) are intentionally omitted.

---

## 1. Summary of Empirical Findings

| Implementation | Client Components / Logic | Shared TS (excl. mocks) | Shared CSS | Total Client LOC |
|---|---:|---:|---:|---:|
| **Quickshell (QML)** *(excl. `DemoFixture.qml`)* | 3,359 | — | — | **3,359** |
| **Svelte (`webbar`)** | 2,347 | 676 | 2,382 | **5,405** |
| **React (`webbar`)** | 2,426 | 676 | 2,382 | **5,484** |

Key takeaways from the code evaluation:
- **Component-for-component (excluding CSS)**, Svelte (`3,023` LOC) and React (`3,102` LOC) are *shorter* than Quickshell (`3,359` LOC), even though React/Svelte include ~900 LOC of extra polish (custom D-Bus tray menus, Stasis inhibitor blame, multi-monitor sliders, frosted toast sheen, ARIA/reduced-motion).
- **CSS + TypeScript + Playwright is a major advantage for both humans and coding agents.** Static type-checking (`tsc` / `svelte-check`) catches prop, type, and shape errors in ~1 second without a Wayland session, whereas QML's untyped `property var` and runtime-only errors (e.g. fractional `font.pixelSize`, invalid attached-property lookups, unmapped nested `PanelWindow`s) only surfaced inside headless Sway.
- **Where Quickshell genuinely stood out:** native Wayland input-region masking per floating card, lightweight multi-window `PanelWindow`s, native tray menu delegation (`SystemTrayItem.display()`), and ~3–4× lower RSS (~40–80 MB vs. ~150–300 MB).

---

## 2. Gap 1: Native HTML `popover="manual"` & Live Wayland Input Masking

### What Quickshell showed
In Quickshell, floating notification toasts live in their own `WlrLayer.Overlay` `PanelWindow` with native Wayland input masking (`mask: Region`). Gaps between stacked cards pass clicks straight through, hover events work natively per card, and dismissing a card never leaves a stale click-blocking region.

### Current gap in `webbar`
In `preload/src/index.ts`, `webbar` currently:
1. Treats every `:popover-open` as a full-screen modal overlay (`KeyboardMode::Exclusive`) unless the client adds a custom `[data-webbar-interactive]` attribute.
2. Collects `getBoundingClientRect()` for `[data-webbar-interactive]` without checking computed `pointer-events` or `visibility`—forcing clients like `NotificationToasts.svelte` to imperatively strip `data-webbar-interactive` in `onoutrostart` during exit fades.
3. Re-syncs interactive rects only on `MutationObserver`, `ResizeObserver`, and `transitionend`/`animationend`, so during a 300ms `animate:flip` slide the Wayland `cairo::Region` lags behind the moving toast until the transition finishes.

### Design changes for `webbar` (zero custom HTML required)

1. **Use HTML's built-in `popover="manual"` vs. `popover="auto"` distinction:**
   - `:popover-open:not([popover="manual"]):not([data-webbar-interactive])` → **modal overlay** (`set_overlay(true)`, `KeyboardMode::Exclusive`, light-dismiss on outside click / `Escape`).
   - `:popover-open[popover="manual"]` (plus legacy `[data-webbar-interactive]`) → **non-modal interactive rects** (`setInteractiveRegions(rects)`, `KeyboardMode::None`).
   - This lets clients author floating toasts and OSDs with 100% standard HTML (`popover="manual"`), dropping custom `data-webbar-*` attributes while staying backwards-compatible.

2. **Honor CSS `pointer-events: none` and `visibility: hidden` in `syncInteractiveRegions()`:**
   - Before pushing a rect to `api.setInteractiveRegions(rects)`, check `const cs = getComputedStyle(el); if (cs.pointerEvents === "none" || cs.visibility === "hidden") continue;`.
   - Clients can then simply set `pointer-events: none` on exiting toasts in CSS (standard web practice) and the Wayland input hole updates immediately.

3. **Frame-accurate input-region sync during active animations:**
   - Listen for capturing `transitionrun` and `animationstart` events on tracked floating elements.
   - Maintain a small `activeAnimations` counter; while `> 0`, schedule `syncInteractiveRegions()` on `requestAnimationFrame` (still deduplicated by the existing `lastInteractiveKey` string so no IPC message is sent if rects didn't change).
   - Stop the loop as soon as `transitionend`/`transitioncancel`/`animationend`/`animationcancel` brings the counter back to `0`. Zero idle overhead, frame-locked hit regions during `flip`/slide animations.

---

## 3. Non-Obvious Learning: Why `webbar`'s Overlay Popover Model Beats `xdg_popup` on Sway

Going into the experiment, it was easy to assume Quickshell's native `PopupWindow` (`xdg_popup`) would be strictly better than `webbar`'s full-screen transparent overlay + HTML `[popover]`. In practice, **the opposite happened on Sway**:

- Wayland `xdg_popup` auto-dismiss depends on a compositor pointer grab tied to a user click serial. Opening a popover programmatically (via IPC/keybind like `toggleNotifications`, or at startup in headless screenshot tests) is fragile on Sway/wlroots.
- Because Quickshell `PanelWindow`s must live directly in the `ShellRoot` `Reloadable` tree rather than inside widget `Item`s, switching to an overlay `PanelWindow` forced centralizing popover hosting in `shell.qml` and prop-drilling `barWindow`/`shellRoot` into every widget.
- Worse, tying `PanelWindow.visible` to `open` immediately unmaps the window on close, cutting off exit animations.

**Takeaway for `webbar`:** Keep the single transparent overlay `WebView` + native HTML `<div popover>` model. It gives:
- Local, inline `<Popover>` composition per widget (no prop-drilling).
- Identical behavior whether opened by mouse click, Wayland keybind/IPC (`host_action`), or headless screenshot harness.
- Full CSS/Svelte enter *and* exit animations before the overlay releases input.

---

## 4. Gap 2: Multi-Surface Architecture & Inherent Tradeoffs

### Where Quickshell has a real structural edge
- **Cheap per-item Wayland windows:** Spawning 10 `PanelWindow`s in Quickshell shares one QtQuick engine (~1–2 MB incremental cost). Spawning 10 `WebView`s in WebKitGTK is much heavier.
- **In-process cross-window reactivity:** Quickshell singletons update all windows synchronously in one JS/QML heap.
- **Zero-copy `wlr-screencopy` textures:** QtQuick can bind compositor DMA-BUF frames directly as GPU textures; WebKitGTK has no direct host-to-DOM DMA-BUF texture hook.

### Design guidance for expanding `webbar` to multi-surface shells
If `webbar` adds support for additional surfaces (docks, launchers, OSDs, lock screens):
1. **Favor a small number of long-lived `WebView`s** (one per shell role/edge) rather than ephemeral per-toast or per-tooltip WebViews.
2. **Always attach additional `WebView`s via `related_view`** (as `src/main.rs` already does for multi-monitor bars) so they share one `WebKitWebProcess` and minimize RAM overhead.
3. **Reuse the existing `ws_bridge.rs` + `SnapshotCache` for cross-window state** (every surface already gets identical live system stores over localhost WS), and use standard browser `BroadcastChannel` for client-only UI coordination across same-origin surfaces.

---

## 5. Gap 3: Frontend CSS Architecture in Reference Examples

The subagent code-elegance audit highlighted one surprising weakness in `examples/sway-full-bar-svelte`: while Svelte's component code was cleaner and denser than QML, the shared **2,246-line global `bar.css`** was the biggest source of friction:
- Styles were disconnected from `.svelte` components, making edits require jumping between files.
- Light-theme variables were duplicated across `[data-desktop-theme="light"]` and `@media (prefers-color-scheme: light)`.
- An unchecked CSS variable typo (`var(--fg)` instead of `var(--fg-color)`) went unnoticed by `svelte-check`.

By contrast, Quickshell's `Theme.qml` (105 LOC) kept all tokens in one concise table with zero duplication.

**Takeaway for `webbar` examples:** For framework-specific bars (especially Svelte), prefer **component-scoped `<style>` blocks** paired with a small shared `theme.css` token file, and avoid duplicating light-mode blocks by letting `webbar`'s `data-desktop-theme` (with a tiny JS fallback in standalone browser dev) be the single theme switch.

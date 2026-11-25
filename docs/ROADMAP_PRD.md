# `webbar` Product Requirements & Enhancements

This document tracks planned product requirements and enhancements for `webbar`. It focuses strictly on desired capabilities, author experience, and user-facing behavior rather than implementation design.

---

## Product Principles

1. **Standard web authoring first:** Bar UIs should be written in standard HTML, CSS, and TypeScript/JavaScript without `webbar`-specific DOM attributes or imperative workarounds wherever a standard web API already expresses the intent.
2. **Browser-compatible & testable:** Any bar UI should remain inspectable and testable in a regular browser (including automated end-to-end tests via Playwright and `--ws-bridge`).
3. **Predictable Wayland desktop behavior:** Floating UI elements must never steal compositor focus unexpectedly, leave dead click-blocking regions behind during animations, or interfere with normal window interaction underneath.

---

## Requirements & Enhancements

### 1. Native HTML Floating Surfaces & Click-Through

- **R1.1 — Standard HTML non-modal popovers:** Authors must be able to declare floating, non-modal surfaces (such as notification toasts, volume/brightness OSDs, or pinned widgets) using standard HTML `popover="manual"` without adding custom `data-webbar-*` attributes.
- **R1.2 — Automatic click-through for gaps and transparent regions:** Only the visible, interactive floating elements themselves should capture pointer events; empty space around or between stacked elements (e.g. gaps between notification toasts) must pass clicks and hover events through to underlying Sway windows.
- **R1.3 — Standard CSS `pointer-events` and visibility respect:** When a floating element is styled with `pointer-events: none` or `visibility: hidden` (for example, while fading out on dismissal), it must immediately stop capturing Wayland pointer input without requiring the client to mutate DOM attributes in lifecycle hooks.
- **R1.4 — Live hit-testing during motion:** When floating elements move or resize during CSS transitions or animations (such as stacked toasts sliding upward after one is dismissed), pointer hit-testing and hover boundaries must track the element's visual position smoothly throughout the animation rather than snapping only after the animation ends.
- **R1.5 — Backwards compatibility:** Existing UIs using `data-webbar-interactive` must continue to work without breaking changes.

### 2. Multi-Surface Shell Capabilities

- **R2.1 — Additional shell surfaces beyond a top bar:** Authors should be able to define and show additional Wayland shell surfaces—such as bottom/side docks, app launchers, standalone OSDs, control centers, or lock screens—using the same HTML/CSS/TS workflow.
- **R2.2 — Per-surface placement & focus policy:** Each surface must support configuring its anchor edges, layer (background, bottom, top, overlay), exclusive zone, target monitor(s), and keyboard focus policy (none, on-demand, exclusive).
- **R2.3 — Synchronized state across surfaces:** System stores (`sway`, `audio`, `notifications`, `theme`, etc.) and user-triggered state changes must stay consistent across all open surfaces on all monitors.

### 3. Additional System Integrations

- **R3.1 — Media playback (MPRIS):** Expose active media players, track metadata, artwork, playback state, and transport controls (`play`/`pause`/`next`/`previous`/seek) so bars and control centers can render media widgets without shelling out to `playerctl`.
- **R3.2 — Power & battery (UPower):** Expose battery percentage, charging state, time remaining, and power profile controls for laptop setups.
- **R3.3 — Expanded network controls:** Expose Wi-Fi scanning, network switching, and VPN/interface status beyond basic connectivity readouts.

### 4. Example UI & Authoring Ergonomics

- **R4.1 — Scoped component styling in examples:** Reference examples (such as `sway-full-bar-svelte`) should demonstrate clean, maintainable styling without relying on a monolithic global stylesheet or duplicated theme blocks.
- **R4.2 — Verified theme & token consistency:** CSS custom properties used by example themes should be checked against typos or stale selectors during CI/build checks.

---

## Non-Goals

- **Replacing WebKitGTK or matching minimal-RSS native toolkits:** `webbar` intentionally trades higher baseline memory (~150–300 MB across WebKit processes) for full HTML/CSS/JS expressiveness, TypeScript safety, and browser/Playwright testability.
- **Zero-copy live compositor window thumbnails (`wlr-screencopy`):** Live 60fps window previews inside the bar are out of scope unless a clear user need emerges.
- **Custom non-standard UI DSLs:** `webbar` will not introduce proprietary templating or layout primitives when standard HTML/CSS suffices.

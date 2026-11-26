# Surfaces & Multi-Surface Shells in `webbar`

Every `webbar` UI declares its Wayland layer-shell surfaces in a `webbar.json` manifest—whether it is a single status bar on any screen edge or a multi-surface shell with a top bar, a bottom dock, an on-screen display, and a toggleable overlay launcher hosted from a single `webbar` process.

For the full machine-readable manifest schema (and editor autocomplete/validation), see [`webbar.schema.json`](webbar.schema.json), generated directly from the Rust configuration structs in `src/surface.rs`.

---

## Quick Start: Single Bar on Any Edge

Place a `webbar.json` at the root of your UI directory (`--dir`), or at `public/webbar.json` in Vite (`--url`, served at `/webbar.json`):

```json
{
  "$schema": "./webbar.schema.json",
  "surfaces": [
    {
      "id": "bar",
      "edge": "top",
      "height": 34
    }
  ]
}
```

Then run `webbar --dir ./dist` (or `webbar --url http://localhost:5173`).

- Set `"edge": "bottom"` for a bottom bar, or `"edge": "left"` / `"edge": "right"` with `"width": 56` for a vertical bar.
- `webbar` automatically injects `--webbar-height` (horizontal bars) or `--webbar-width` (vertical bars) and `--webbar-thickness` onto `:root` so your CSS can match the reserved strip size without duplicating numbers.

---

## Multi-Surface Shells (`webbar.json`)

Add more entries to `surfaces` to host multiple Wayland layer-shell surfaces from the same `webbar` process:

```json
{
  "$schema": "./webbar.schema.json",
  "surfaces": [
    {
      "id": "bar",
      "edge": "top",
      "height": 38,
      "overlay_extent": 38,
      "layer": "top",
      "monitors": "all"
    },
    {
      "id": "dock",
      "edge": "bottom",
      "height": 52,
      "overlay_extent": 52,
      "layer": "top",
      "monitors": "all"
    },
    {
      "id": "launcher",
      "anchors": [],
      "width": 520,
      "height": 300,
      "layer": "overlay",
      "exclusive_zone": "ignore",
      "keyboard": "exclusive",
      "input_mode": "full",
      "monitors": "focused",
      "visible": false
    }
  ]
}
```

See [`examples/multi-surface/`](examples/multi-surface) for a complete runnable example.

### Sharing One App vs. Separate Entry Points

You can structure your frontend either way:

1. **Single bundle (default)**: Every surface loads `index.html` and branches on `window.webbar.surface` (e.g. `"bar"`, `"dock"`, `"launcher"`).
2. **Per-surface HTML files**: Set `"entry": "dock.html"` or `"entry": "launcher.html"` on individual surfaces in `webbar.json`.

All surfaces share the same origin (`webbar://app` in `--dir` mode, or your dev server origin in `--url` mode), so standard web APIs like `BroadcastChannel` and `localStorage` work directly between surfaces.

---

## Common Recipes

### 1. Top Bar + Bottom Dock (Two Exclusive Zones)

Give each edge surface `"edge": "top"` or `"edge": "bottom"` and its `"height"`. By default, `edge` surfaces reserve an exclusive zone equal to their height, so Sway tiles normal windows cleanly between both surfaces.

If your bar or dock has popovers that extend into the screen, omit `"overlay_extent"` (defaults to full monitor height) or set it to the maximum popover reach. If it has no popovers, set `"overlay_extent"` equal to `"height"` to keep the backing surface compact.

### 2. Toggleable Overlay Launcher (Above Fullscreen Apps)

```json
{
  "id": "launcher",
  "anchors": [],
  "width": 520,
  "height": 300,
  "layer": "overlay",
  "exclusive_zone": "ignore",
  "keyboard": "exclusive",
  "input_mode": "full",
  "monitors": "focused",
  "visible": false
}
```

- `"anchors": []` centers the surface on the monitor.
- `"layer": "overlay"` + `"exclusive_zone": "ignore"` renders above fullscreen windows without shifting tiled layouts.
- `"keyboard": "exclusive"` gives your `<input autofocus>` immediate keyboard focus when shown.
- `"monitors": "focused"` opens the launcher on whichever Sway output is currently focused.
- `"visible": false` keeps it hidden on startup until toggled.

Because the launcher starts hidden and grabs exclusive keyboard focus while open, you'll want a trigger to open it (such as a bar button or a Sway keybinding via D-Bus) and a dismiss handler (such as `Escape`):

```sway
# Example ~/.config/sway/config keybinding
bindsym $mod+d exec gapplication action dev.local.webbar surface-toggle "'launcher'"
```

```ts
// Inside your launcher page
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") window.webbar.surfaces.hide("launcher");
});
```

### 3. Floating Toasts & On-Screen Displays (Click-Through)

To show floating UI (toasts, volume/brightness indicators) without stealing keyboard focus or blocking clicks on the desktop behind transparent areas:

1. Put the floating UI in `<div popover="manual">` and call `.showPopover()`.
2. If it's a container holding multiple cards, put `pointer-events: none` on the container and `pointer-events: auto` on each clickable card:

```html
<div id="toasts" popover="manual" style="pointer-events: none">
  <article class="toast" style="pointer-events: auto">Notification</article>
</div>
```

If you're putting this on its own dedicated overlay surface instead of a bar, set `"input_mode": "interactive"` and `"keyboard": "none"` in `webbar.json`.

`webbar` masks pointer input to those clickable elements automatically—including while they animate—and stops capturing clicks as soon as you set `pointer-events: none` or `visibility: hidden`. For a modal menu that *should* grab keyboard focus and light-dismiss on outside clicks, use a regular `<div popover="auto">` (or `window.webbar.acquireOverlay()`).

---

## Controlling Surfaces from JavaScript & D-Bus

### JavaScript API (`window.webbar`)

Every surface receives:

- `window.webbar.surface` — string ID of the current surface (e.g. `"bar"`, `"dock"`, `"launcher"`).
- `window.webbar.surfaces` — reactive store (`get`, `subscribe`, `onChange`) of `SurfaceRuntimeState[]` (`{ id, visible, layer, anchors }`), plus actions:
  - `window.webbar.surfaces.show(id)`
  - `window.webbar.surfaces.hide(id)`
  - `window.webbar.surfaces.toggle(id)`

```ts
// Toggle a launcher from a bar button
button.onclick = () => window.webbar.surfaces.toggle("launcher");

// React to surface visibility changes
const unsub = window.webbar.surfaces.onChange((surfaces) => {
  const launcher = surfaces.find((s) => s.id === "launcher");
  console.log("launcher visible:", launcher?.visible);
});
```

### Remote Control via D-Bus

`webbar` registers on the session D-Bus as `dev.local.webbar` (object path `/dev/local/webbar`, interface `org.gtk.Actions`) with three string-parameterized actions: `surface-show`, `surface-hide`, and `surface-toggle`.

Any D-Bus client (`gapplication`, `busctl`, `gdbus`, etc.) can invoke them. For example, using `gapplication` (note the GVariant string quoting `"'<id>'"`):

```bash
gapplication action dev.local.webbar surface-show "'launcher'"
gapplication action dev.local.webbar surface-hide "'launcher'"
gapplication action dev.local.webbar surface-toggle "'dock'"
```

---

## `webbar.json` Field Summary

Every surface in `surfaces` requires a unique `id` (ASCII alphanumeric, `-`, or `_`). All other fields are optional—see [`webbar.schema.json`](webbar.schema.json) for exact types and enum values:

| Field | Default | Description |
|---|---|---|
| `id` | *(required)* | Unique surface identifier exposed as `window.webbar.surface` |
| `entry` | `index.html` | Relative HTML path for this surface |
| `edge` | `null` | Bar preset (`top`, `bottom`, `left`, `right`); sets edge anchors and `input_mode: "bar"` |
| `anchors` | from `edge`, or `[]` | Explicit layer-shell anchors (`[]` centers the surface) |
| `layer` | `"top"` | Stacking layer: `"background"`, `"bottom"`, `"top"`, or `"overlay"` |
| `width`, `height` | `null` (`34` px for `edge` bars) | Explicit pixel size (positive integer) |
| `overlay_extent` | full monitor | Max popover reach in pixels for `input_mode: "bar"` |
| `margins` | `{ top: 0, right: 0, bottom: 0, left: 0 }` | Outer layer-shell margins in pixels |
| `exclusive_zone` | bar thickness / `0` | Non-negative pixels, `"auto"`, or `"ignore"` (`-1`) |
| `keyboard` | `"none"` | Base keyboard policy: `"none"`, `"on_demand"`, or `"exclusive"` |
| `monitors` | `"all"` | `"all"`, `"primary"`, `"focused"`, or connector list (e.g. `["DP-1"]`) |
| `visible` | `true` | Initial visibility on startup |
| `input_mode` | `"bar"` if `edge`, else `"full"` | Pointer masking: `"bar"`, `"interactive"`, or `"full"` |
| `namespace` | `"webbar-<id>"` | Wayland layer-shell namespace |

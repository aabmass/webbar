# System Theming in `webbar`

`webbar` injects your desktop and GTK theme into every bar UI as `--desktop-*` and `--gtk-*` CSS custom properties on `:root` (`document.documentElement`), plus `color-scheme` and `data-desktop-theme`. These CSS variables are **reactive to system updates**—changing dark/light mode, accent color, or system font updates `:root` live without reloading the bar.

`webbar` does not apply any styles itself—these are just CSS variables you can reference if you want your bar to follow the OS theme, in both WebKitGTK windows and `--ws-bridge` browser-dev mode.

---

## Quick Start

Use `--desktop-*` variables in your CSS with a fallback value:

```css
body {
  font-family: var(--desktop-font-family, system-ui, sans-serif);
  font-size: var(--desktop-font-size, 13px);
  color: var(--desktop-window-fg, #ececf1);
}

.bar {
  background: color-mix(in srgb, var(--desktop-window-bg, #18181c) 90%, transparent);
  border-bottom: 1px solid var(--desktop-border-color, rgba(255, 255, 255, 0.1));
}

.workspace.focused {
  background: var(--desktop-accent-bg, #3584e4);
  color: var(--desktop-accent-fg, #ffffff);
}
```

See [web/style.css](file:///home/aaron/repo/webbar/web/style.css) for a complete minimal bar styled with `--desktop-*` tokens.

---

## `--desktop-*` vs `--gtk-*`

`webbar` exposes two prefixes on `:root`:

1. **`--desktop-*`**: Semantic tokens from [Libadwaita](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/named-colors.html) ([`AdwStyleManager`](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.StyleManager.html)) and `gtk4::Settings`.
2. **`--gtk-*`**: Raw [GTK4 CSS](https://docs.gtk.org/gtk4/css-overview.html) `@define-color` entries. Useful if your theme (e.g. Stylix, Catppuccin, `adw-gtk3`, `matugen`) defines custom `@define-color` names.

---

## CSS Reference

### Mode & Root Attributes

| Property / Attribute | Values | Description |
|---|---|---|
| `color-scheme` (on `:root`) | `dark` \| `light` | Enables native scrollbars, form controls, and CSS `light-dark()` |
| `data-desktop-theme` (on `<html>`) | `"dark"` \| `"light"` | Attribute selector for mode-specific CSS rules |

Example using `data-desktop-theme` or `light-dark()`:

```css
/* Option 1: light-dark() (uses :root color-scheme automatically) */
.pill {
  background: light-dark(rgba(0, 0, 0, 0.06), rgba(255, 255, 255, 0.08));
}

/* Option 2: data-desktop-theme attribute */
:root[data-desktop-theme="light"] .bar {
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
}
```

### Typography & Motion (`--desktop-*`)

| CSS Variable | Source | Example |
|---|---|---|
| `--desktop-font-family` | `gtk-font-name` | `"Cantarell", sans-serif` |
| `--desktop-font-size` | `gtk-font-name` | `11pt` |
| `--desktop-font-weight` | `gtk-font-name` (defaults to `400`) | `500` |
| `--desktop-monospace-font-family` | `org.gnome.desktop.interface monospace-font-name` *(when schema present)* | `"JetBrains Mono", sans-serif` |
| `--desktop-enable-animations` | `gtk-enable-animations` | `1` or `0` |

### Surfaces & Foregrounds (`--desktop-*`)

| CSS Variable | GTK / Libadwaita Fallback Chain |
|---|---|
| `--desktop-window-bg` | `window_bg_color` → `theme_bg_color` → Adwaita default |
| `--desktop-window-fg` | `window_fg_color` → `theme_fg_color` → Adwaita default |
| `--desktop-view-bg` | `view_bg_color` → `theme_base_color` → Adwaita default |
| `--desktop-view-fg` | `view_fg_color` → `theme_text_color` → Adwaita default |
| `--desktop-headerbar-bg` | `headerbar_bg_color` → `window_bg_color` → Adwaita default |
| `--desktop-headerbar-fg` | `headerbar_fg_color` → `window_fg_color` → Adwaita default |
| `--desktop-headerbar-border` | `headerbar_border_color` → `borders` → Adwaita default |
| `--desktop-card-bg` | `card_bg_color` → `view_bg_color` → Adwaita default |
| `--desktop-card-fg` | `card_fg_color` → `window_fg_color` → Adwaita default |
| `--desktop-popover-bg` | `popover_bg_color` → `card_bg_color` → Adwaita default |
| `--desktop-popover-fg` | `popover_fg_color` → `window_fg_color` → Adwaita default |
| `--desktop-sidebar-bg` | `sidebar_bg_color` → `window_bg_color` → Adwaita default |
| `--desktop-sidebar-fg` | `sidebar_fg_color` → `window_fg_color` → Adwaita default |

### Accents, Semantic States & Borders (`--desktop-*`)

| CSS Variable | Source / Fallback Chain |
|---|---|
| `--desktop-accent-bg` | Portal `accent-color` → `accent_bg_color` → `theme_selected_bg_color` → Adwaita default |
| `--desktop-accent-fg` | `accent_fg_color` → `theme_selected_fg_color` → Adwaita default |
| `--desktop-accent-color` | Portal `accent-color` → `accent_color` → `accent_bg_color` → Adwaita default |
| `--desktop-destructive-bg` / `-fg` / `-color` | `destructive_*` → `error_*` → Adwaita default |
| `--desktop-success-bg` / `-fg` / `-color` | `success_*` → Adwaita default |
| `--desktop-warning-bg` / `-fg` / `-color` | `warning_*` → Adwaita default |
| `--desktop-error-bg` / `-fg` / `-color` | `error_*` → Adwaita default |
| `--desktop-border-color` | `borders` → `card_shade_color` → Adwaita default |
| `--desktop-shade-color` | `shade_color` → `card_shade_color` → Adwaita default |

### Raw GTK Variables (`--gtk-*`)

`webbar` looks up standard GTK3/GTK4 named colors via `GtkStyleContext` (including overrides from `gtk-4.0/gtk.css`) and exposes them as `--gtk-<name>`:

- `--gtk-theme-bg-color`
- `--gtk-theme-fg-color`
- `--gtk-theme-base-color`
- `--gtk-theme-text-color`
- `--gtk-theme-selected-bg-color`
- `--gtk-theme-selected-fg-color`
- `--gtk-insensitive-bg-color`
- `--gtk-insensitive-fg-color`
- `--gtk-borders`

---

## Using `window.webbar.theme` in TypeScript / React

For CSS values, prefer standard `var(--desktop-*)` or `getComputedStyle(document.documentElement)`. For reactive metadata (`theme_name`, `icon_theme_name`, `dark`, `high_contrast`), subscribe to `window.webbar.theme`:

```tsx
import { useSyncExternalStore } from "react";

function ThemeBadge() {
  const theme = useSyncExternalStore(
    window.webbar.theme.subscribe,
    window.webbar.theme.get,
  );

  return (
    <span>
      {theme.dark ? "Dark" : "Light"} · {theme.theme_name}
    </span>
  );
}
```

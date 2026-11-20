//! Desktop & GTK theme watcher powered by `libadwaita::StyleManager`,
//! `gtk4::Settings`, `gtk4::pango::FontDescription`, and GTK `StyleContext`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::pango;
use gtk4::prelude::*;
use libadwaita as adw;
use serde::Serialize;
use ts_rs::TS;

use crate::bridge::{BridgeEvent, SendEvent};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct ThemeState {
    pub dark: bool,
    pub high_contrast: bool,
    pub theme_name: String,
    pub icon_theme_name: String,
    pub font_family: String,
    pub font_size: String,
    pub font_weight: u16,
    pub monospace_font_family: Option<String>,
    pub enable_animations: bool,
    pub accent_color: Option<String>,
    pub desktop_colors: BTreeMap<String, String>,
    pub gtk_colors: BTreeMap<String, String>,
}

impl Default for ThemeState {
    fn default() -> Self {
        Self {
            dark: true,
            high_contrast: false,
            theme_name: "Adwaita".to_string(),
            icon_theme_name: "Adwaita".to_string(),
            font_family: "\"Cantarell\", sans-serif".to_string(),
            font_size: "11pt".to_string(),
            font_weight: 400,
            monospace_font_family: None,
            enable_animations: true,
            accent_color: None,
            desktop_colors: BTreeMap::new(),
            gtk_colors: BTreeMap::new(),
        }
    }
}

/// Start watching `adw::StyleManager` and `gtk4::Settings`, returning the
/// initial `ThemeState` synchronously for `SnapshotCache` and `UserStyleSheet`.
pub fn start_theme_watcher(send: SendEvent) -> ThemeState {
    let has_display = gdk::Display::default().is_some();
    if has_display {
        let _ = adw::init();
    }

    let initial = collect_theme_state(has_display);
    let last_emitted = Rc::new(RefCell::new(initial.clone()));
    send(BridgeEvent::Theme(initial.clone()));

    let schedule_emit = {
        let last_emitted = Rc::clone(&last_emitted);
        let send = Arc::clone(&send);
        let pending = Rc::new(Cell::new(false));
        move || {
            if pending.replace(true) {
                return;
            }
            let last_emitted = Rc::clone(&last_emitted);
            let send = Arc::clone(&send);
            let pending = Rc::clone(&pending);
            // Defer until idle so AdwStyleManager finishes swapping GTK's
            // CssProvider before collect_colors() queries StyleContext.
            glib::idle_add_local_once(move || {
                pending.set(false);
                let next = collect_theme_state(has_display);
                let mut last = last_emitted.borrow_mut();
                if *last != next {
                    *last = next.clone();
                    drop(last);
                    send(BridgeEvent::Theme(next));
                }
            });
        }
    };

    if has_display {
        let style_mgr = adw::StyleManager::default();
        for prop in ["dark", "color-scheme", "high-contrast", "accent-color"] {
            let emit = schedule_emit.clone();
            style_mgr.connect_notify_local(Some(prop), move |_, _| emit());
        }
    }

    if let Some(settings) = gtk4::Settings::default() {
        for prop in [
            "gtk-font-name",
            "gtk-theme-name",
            "gtk-icon-theme-name",
            "gtk-application-prefer-dark-theme",
            "gtk-enable-animations",
        ] {
            let emit = schedule_emit.clone();
            settings.connect_notify_local(Some(prop), move |_, _| emit());
        }
    }

    initial
}

fn collect_theme_state(has_display: bool) -> ThemeState {
    let settings = gtk4::Settings::default();
    let theme_name = settings
        .as_ref()
        .and_then(|s| s.gtk_theme_name())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Adwaita".to_string());
    let icon_theme_name = settings
        .as_ref()
        .and_then(|s| s.gtk_icon_theme_name())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Adwaita".to_string());
    let font_name = settings
        .as_ref()
        .and_then(|s| s.gtk_font_name())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Cantarell 11".to_string());
    let enable_animations = settings
        .as_ref()
        .is_none_or(|s| s.is_gtk_enable_animations());

    let (dark, high_contrast, accent_color) = if has_display {
        let style_mgr = adw::StyleManager::default();
        let accent = Some(rgba_to_css(style_mgr.accent_color_rgba()));
        (style_mgr.is_dark(), style_mgr.is_high_contrast(), accent)
    } else {
        let prefer_dark = settings
            .as_ref()
            .is_some_and(|s| s.is_gtk_application_prefer_dark_theme());
        (prefer_dark, false, None)
    };

    let (raw_family, font_size, font_weight) = parse_pango_font(&font_name);
    let font_family = format_css_font_family(&raw_family);
    let monospace_font_family = read_monospace_font_family().map(|f| format_css_font_family(&f));
    let (desktop_colors, gtk_colors) = match accent_color.as_deref() {
        Some(accent) => collect_colors(accent),
        None => (BTreeMap::new(), BTreeMap::new()),
    };

    ThemeState {
        dark,
        high_contrast,
        theme_name,
        icon_theme_name,
        font_family,
        font_size,
        font_weight,
        monospace_font_family,
        enable_animations,
        accent_color,
        desktop_colors,
        gtk_colors,
    }
}

/// Parse a Pango font description string (e.g. `"Cantarell 11"`, `"Inter Medium 10.5"`)
/// into `(family, css_size, css_weight)`.
pub fn parse_pango_font(raw: &str) -> (String, String, u16) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return ("sans-serif".to_string(), "11pt".to_string(), 400);
    }

    let desc = pango::FontDescription::from_string(trimmed);
    let family = desc
        .family()
        .map(|f| f.to_string())
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| "sans-serif".to_string());

    let size = if desc.size() > 0 {
        let value = desc.size() as f64 / pango::SCALE as f64;
        let formatted = format!("{value:.2}");
        let formatted = formatted.trim_end_matches('0').trim_end_matches('.');
        if desc.is_size_absolute() {
            format!("{formatted}px")
        } else {
            format!("{formatted}pt")
        }
    } else {
        "11pt".to_string()
    };

    let weight = match desc.weight() {
        pango::Weight::Thin => 100,
        pango::Weight::Ultralight => 200,
        pango::Weight::Light => 300,
        pango::Weight::Semilight => 350,
        pango::Weight::Medium => 500,
        pango::Weight::Semibold => 600,
        pango::Weight::Bold => 700,
        pango::Weight::Ultrabold => 800,
        pango::Weight::Heavy => 900,
        _ => 400,
    };

    (family, size, weight)
}

fn read_monospace_font_family() -> Option<String> {
    let source = gio::SettingsSchemaSource::default()?;
    source.lookup("org.gnome.desktop.interface", true)?;
    let gsettings = gio::Settings::new("org.gnome.desktop.interface");
    let raw = gsettings.string("monospace-font-name");
    if raw.trim().is_empty() {
        return None;
    }
    let (family, _, _) = parse_pango_font(raw.as_str());
    Some(family)
}

#[allow(deprecated)]
fn collect_colors(
    portal_accent: &str,
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let probe = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    let ctx = probe.style_context();

    let mut desktop = BTreeMap::new();
    for (desktop_key, gtk_name) in DESKTOP_COLOR_MAP {
        if let Some(rgba) = ctx.lookup_color(gtk_name) {
            desktop.insert((*desktop_key).to_string(), rgba_to_css(rgba));
        }
    }
    desktop.insert("accent-bg".to_string(), portal_accent.to_string());
    desktop.insert("accent-color".to_string(), portal_accent.to_string());

    let mut gtk = BTreeMap::new();
    for gtk_name in GTK_COLOR_NAMES {
        if let Some(rgba) = ctx.lookup_color(gtk_name) {
            gtk.insert(gtk_name.replace('_', "-"), rgba_to_css(rgba));
        }
    }

    (desktop, gtk)
}

fn rgba_to_css(rgba: gdk::RGBA) -> String {
    let r = (rgba.red().clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (rgba.green().clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (rgba.blue().clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = rgba.alpha().clamp(0.0, 1.0);
    if (a - 1.0).abs() < 0.002 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("rgba({r}, {g}, {b}, {a:.3})")
    }
}

/// Format `:root` CSS rules for initial `UserStyleSheet` injection in WebKitGTK.
pub fn format_root_css(state: &ThemeState) -> String {
    let mut css = String::from(":root {\n");
    css.push_str(&format!(
        "  color-scheme: {};\n",
        if state.dark { "dark" } else { "light" }
    ));
    css.push_str(&format!(
        "  --desktop-font-family: {};\n",
        state.font_family
    ));
    css.push_str(&format!("  --desktop-font-size: {};\n", state.font_size));
    css.push_str(&format!(
        "  --desktop-font-weight: {};\n",
        state.font_weight
    ));
    if let Some(mono) = &state.monospace_font_family {
        css.push_str(&format!("  --desktop-monospace-font-family: {mono};\n"));
    }
    css.push_str(&format!(
        "  --desktop-enable-animations: {};\n",
        if state.enable_animations { "1" } else { "0" }
    ));
    for (k, v) in &state.desktop_colors {
        css.push_str(&format!("  --desktop-{k}: {v};\n"));
    }
    for (k, v) in &state.gtk_colors {
        css.push_str(&format!("  --gtk-{k}: {v};\n"));
    }
    css.push_str("}\n");
    css
}

fn format_css_font_family(family: &str) -> String {
    if matches!(family, "sans-serif" | "serif" | "monospace" | "system-ui") {
        family.to_string()
    } else {
        format!("\"{}\", sans-serif", family.replace('"', "\\\""))
    }
}

const DESKTOP_COLOR_MAP: &[(&str, &str)] = &[
    ("window-bg", "window_bg_color"),
    ("window-fg", "window_fg_color"),
    ("view-bg", "view_bg_color"),
    ("view-fg", "view_fg_color"),
    ("headerbar-bg", "headerbar_bg_color"),
    ("headerbar-fg", "headerbar_fg_color"),
    ("headerbar-border", "headerbar_border_color"),
    ("card-bg", "card_bg_color"),
    ("card-fg", "card_fg_color"),
    ("popover-bg", "popover_bg_color"),
    ("popover-fg", "popover_fg_color"),
    ("sidebar-bg", "sidebar_bg_color"),
    ("sidebar-fg", "sidebar_fg_color"),
    ("accent-fg", "accent_fg_color"),
    ("destructive-bg", "destructive_bg_color"),
    ("destructive-fg", "destructive_fg_color"),
    ("destructive-color", "destructive_color"),
    ("success-bg", "success_bg_color"),
    ("success-fg", "success_fg_color"),
    ("success-color", "success_color"),
    ("warning-bg", "warning_bg_color"),
    ("warning-fg", "warning_fg_color"),
    ("warning-color", "warning_color"),
    ("error-bg", "error_bg_color"),
    ("error-fg", "error_fg_color"),
    ("error-color", "error_color"),
    ("border-color", "borders"),
    ("shade-color", "shade_color"),
];

const GTK_COLOR_NAMES: &[&str] = &[
    "theme_bg_color",
    "theme_fg_color",
    "theme_base_color",
    "theme_text_color",
    "theme_selected_bg_color",
    "theme_selected_fg_color",
    "insensitive_bg_color",
    "insensitive_fg_color",
    "borders",
];

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn parses_pango_fonts() {
        assert_eq!(
            parse_pango_font("Cantarell 11"),
            ("Cantarell".to_string(), "11pt".to_string(), 400)
        );
        assert_eq!(
            parse_pango_font("Inter Medium Italic 10.5"),
            ("Inter".to_string(), "10.5pt".to_string(), 500)
        );
        assert_eq!(
            parse_pango_font("JetBrains Mono Bold 12px"),
            ("JetBrains Mono".to_string(), "12px".to_string(), 700)
        );
        assert_eq!(
            parse_pango_font(""),
            ("sans-serif".to_string(), "11pt".to_string(), 400)
        );
    }

    #[test]
    fn formats_root_css_variables() {
        let mut state = ThemeState::default();
        state
            .desktop_colors
            .insert("window-bg".to_string(), "#242424".to_string());
        state
            .gtk_colors
            .insert("theme-bg-color".to_string(), "#1e1e2e".to_string());
        let css = format_root_css(&state);
        assert!(css.contains("color-scheme: dark;"));
        assert!(css.contains("--desktop-font-family: \"Cantarell\", sans-serif;"));
        assert!(css.contains("--desktop-window-bg: #242424;"));
        assert!(css.contains("--gtk-theme-bg-color: #1e1e2e;"));
    }

    #[test]
    fn style_manager_color_scheme_switch_updates_colors_without_lag() {
        if gtk4::init().is_err() || gdk::Display::default().is_none() {
            // Skip in headless CI without a Wayland/X11 display.
            return;
        }
        let emitted: Arc<Mutex<Vec<ThemeState>>> = Arc::new(Mutex::new(Vec::new()));
        let send: SendEvent = {
            let emitted = Arc::clone(&emitted);
            Arc::new(move |ev| {
                if let BridgeEvent::Theme(state) = ev {
                    emitted.lock().unwrap().push(state);
                }
            })
        };

        let _initial = start_theme_watcher(send);
        let style_mgr = adw::StyleManager::default();
        let ctx = glib::MainContext::default();
        let pump = || {
            for _ in 0..20 {
                if !ctx.iteration(false) {
                    break;
                }
            }
        };

        let luma = |hex: &str| -> u32 {
            let h = hex.strip_prefix('#').unwrap_or("000000");
            let r = u32::from_str_radix(&h[0..2], 16).unwrap_or(0);
            let g = u32::from_str_radix(&h[2..4], 16).unwrap_or(0);
            let b = u32::from_str_radix(&h[4..6], 16).unwrap_or(0);
            r + g + b
        };

        // Toggle back and forth multiple times to verify zero 1-step lag.
        for (scheme, expect_dark) in [
            (adw::ColorScheme::ForceDark, true),
            (adw::ColorScheme::ForceLight, false),
            (adw::ColorScheme::ForceDark, true),
            (adw::ColorScheme::ForceLight, false),
        ] {
            style_mgr.set_color_scheme(scheme);
            pump();
            let state = emitted.lock().unwrap().last().cloned().unwrap();
            let bg = state
                .desktop_colors
                .get("window-bg")
                .cloned()
                .unwrap_or_default();
            assert_eq!(state.dark, expect_dark, "mismatch for {scheme:?}");
            if expect_dark {
                assert!(
                    luma(&bg) < 200,
                    "expected dark window-bg for {scheme:?}, got {bg}"
                );
            } else {
                assert!(
                    luma(&bg) > 500,
                    "expected light window-bg for {scheme:?}, got {bg}"
                );
            }
        }
    }
}

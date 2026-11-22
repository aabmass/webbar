//! Minimal `org.kde.StatusNotifierWatcher` D-Bus server using `zbus`.
//!
//! Adapted from `cprussin/domicile` (`packages/domicile-host/src/tray.rs`),
//! MIT License — Copyright (c) Connor Prussin (https://github.com/cprussin/domicile).

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use gtk4::gdk;
use gtk4::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{IconLookupFlags, IconTheme, TextDirection};
use futures_channel::oneshot;
use futures_util::{select, FutureExt, StreamExt};
use quick_xml::events::{BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use serde::Serialize;
use zbus::blocking::connection::Builder as ConnectionBuilder;
use zbus::blocking::Connection;
use zbus::interface;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{MatchRule, MessageStream};

use crate::bridge::{BridgeEvent, SendEvent};
use crate::theme::ThemeState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayColorPalette {
    // Included for PartialEq change detection when gtk-icon-theme-name changes.
    pub icon_theme_name: String,
    pub text: String,
    pub background: String,
    pub highlight: String,
    pub accent: String,
    pub button_text: String,
    pub positive_text: String,
    pub neutral_text: String,
    pub negative_text: String,
}

impl Default for TrayColorPalette {
    fn default() -> Self {
        Self::from_theme_state(&ThemeState::default())
    }
}

fn pick_theme_color(
    theme: &ThemeState,
    key: &str,
    dark_fallback: &str,
    light_fallback: &str,
) -> String {
    theme
        .desktop_colors
        .get(key)
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if theme.dark {
                dark_fallback.to_string()
            } else {
                light_fallback.to_string()
            }
        })
}

impl TrayColorPalette {
    pub fn from_theme_state(theme: &ThemeState) -> Self {
        let text = pick_theme_color(theme, "theme_fg_color", "#ececf1", "#18181f");
        let background = pick_theme_color(theme, "theme_bg_color", "#1c1c21", "#f4f4f7");
        let accent = theme
            .accent_color
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if theme.dark {
                    "#8ab4f8".to_string()
                } else {
                    "#2b6de0".to_string()
                }
            });
        let highlight = pick_theme_color(theme, "theme_selected_bg_color", &accent, &accent);

        Self {
            icon_theme_name: theme.icon_theme_name.clone(),
            button_text: text.clone(),
            text,
            background,
            highlight,
            accent,
            positive_text: if theme.dark { "#6fcf97" } else { "#1f8f54" }.to_string(),
            neutral_text: if theme.dark { "#f2c94c" } else { "#c5820a" }.to_string(),
            negative_text: if theme.dark { "#f2726f" } else { "#d63638" }.to_string(),
        }
    }

    pub fn stylesheet(&self) -> String {
        format!(
            ".ColorScheme-Text {{ color: {text}; }} \
             .ColorScheme-Background {{ color: {bg}; }} \
             .ColorScheme-Highlight {{ color: {hl}; }} \
             .ColorScheme-Accent {{ color: {accent}; }} \
             .ColorScheme-ButtonText {{ color: {btn}; }} \
             .ColorScheme-PositiveText {{ color: {pos}; }} \
             .ColorScheme-NeutralText {{ color: {neu}; }} \
             .ColorScheme-NegativeText {{ color: {neg}; }}",
            text = self.text,
            bg = self.background,
            hl = self.highlight,
            accent = self.accent,
            btn = self.button_text,
            pos = self.positive_text,
            neu = self.neutral_text,
            neg = self.negative_text,
        )
    }
}

#[derive(Clone, Default)]
pub struct TrayTheme {
    palette: Arc<Mutex<TrayColorPalette>>,
}

impl TrayTheme {
    #[cfg(test)]
    pub fn new(theme: &ThemeState) -> Self {
        Self {
            palette: Arc::new(Mutex::new(TrayColorPalette::from_theme_state(theme))),
        }
    }

    pub fn palette(&self) -> TrayColorPalette {
        self.palette.lock().unwrap().clone()
    }

    pub fn update(&self, theme: &ThemeState) -> bool {
        let next = TrayColorPalette::from_theme_state(theme);
        let mut guard = self.palette.lock().unwrap();
        if *guard == next {
            return false;
        }
        *guard = next;
        true
    }
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct TrayItem {
    pub bus: String,
    pub path: String,
    pub title: String,
    pub icon_name: String,
    pub icon_url: String,
    pub menu_path: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../preload/src/generated/")]
pub struct TrayMenuItem {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub item_type: String,
    pub toggle_type: String,
    pub toggle_state: i32,
    pub icon_name: String,
    pub icon_url: String,
    pub accessible_desc: String,
    pub children: Vec<TrayMenuItem>,
}

type ArgbPixmap = (i32, i32, Vec<u8>);

struct TrayIconSource {
    bus: String,
    path: String,
    icon_name: String,
    icon_theme_path: String,
    pixmaps: Vec<ArgbPixmap>,
}

#[derive(Clone)]
struct TrayEntry {
    item: TrayItem,
    unique_bus: String,
    icon_theme_path: String,
    pixmaps: Vec<ArgbPixmap>,
}

#[derive(Clone)]
pub struct TrayStore {
    items: Arc<Mutex<Vec<TrayEntry>>>,
    send: SendEvent,
    theme: TrayTheme,
}

impl TrayStore {
    #[cfg(test)]
    fn new(send: SendEvent) -> Self {
        Self::with_theme(send, TrayTheme::default())
    }

    fn with_theme(send: SendEvent, theme: TrayTheme) -> Self {
        Self {
            items: Arc::new(Mutex::new(Vec::new())),
            send,
            theme,
        }
    }

    fn snapshot_locked(entries: &[TrayEntry]) -> Vec<TrayItem> {
        entries.iter().map(|e| e.item.clone()).collect()
    }

    #[cfg(test)]
    fn upsert(&self, item: TrayItem, unique_bus: String) {
        self.upsert_with_sources(item, unique_bus, String::new(), Vec::new());
    }

    fn upsert_from_fetched(&self, bus: String, path: String, unique_bus: String, fetched: FetchedItemProperties) {
        self.upsert_with_sources(
            TrayItem {
                bus,
                path,
                title: fetched.title,
                icon_name: fetched.icon_name,
                icon_url: fetched.icon_url,
                menu_path: fetched.menu_path,
            },
            unique_bus,
            fetched.icon_theme_path,
            fetched.pixmaps,
        );
    }

    fn upsert_with_sources(
        &self,
        item: TrayItem,
        unique_bus: String,
        icon_theme_path: String,
        pixmaps: Vec<ArgbPixmap>,
    ) {
        let mut guard = self.items.lock().unwrap();
        if let Some(existing) = guard
            .iter_mut()
            .find(|e| e.item.bus == item.bus && e.item.path == item.path)
        {
            existing.item = item;
            existing.icon_theme_path = icon_theme_path;
            existing.pixmaps = pixmaps;
            if !unique_bus.is_empty() {
                existing.unique_bus = unique_bus;
            }
        } else {
            guard.push(TrayEntry {
                item,
                unique_bus,
                icon_theme_path,
                pixmaps,
            });
        }
        let snapshot = Self::snapshot_locked(&guard);
        drop(guard);
        (self.send)(BridgeEvent::Tray { items: snapshot });
    }

    #[cfg(test)]
    fn update_if_present(
        &self,
        bus: &str,
        path: &str,
        title: String,
        icon_name: String,
        icon_url: String,
        menu_path: String,
    ) -> bool {
        self.update_from_fetched(
            bus,
            path,
            FetchedItemProperties {
                title,
                icon_name,
                icon_theme_path: String::new(),
                pixmaps: Vec::new(),
                icon_url,
                menu_path,
            },
        )
    }

    fn update_from_fetched(&self, bus: &str, path: &str, fetched: FetchedItemProperties) -> bool {
        let mut guard = self.items.lock().unwrap();
        let Some(existing) = guard
            .iter_mut()
            .find(|e| e.item.bus == bus && e.item.path == path)
        else {
            return false;
        };

        if existing.item.title == fetched.title
            && existing.item.icon_name == fetched.icon_name
            && existing.item.icon_url == fetched.icon_url
            && existing.item.menu_path == fetched.menu_path
            && existing.icon_theme_path == fetched.icon_theme_path
            && existing.pixmaps == fetched.pixmaps
        {
            return false;
        }

        existing.item.title = fetched.title;
        existing.item.icon_name = fetched.icon_name;
        existing.item.icon_url = fetched.icon_url;
        existing.item.menu_path = fetched.menu_path;
        existing.icon_theme_path = fetched.icon_theme_path;
        existing.pixmaps = fetched.pixmaps;

        let snapshot = Self::snapshot_locked(&guard);
        drop(guard);
        (self.send)(BridgeEvent::Tray { items: snapshot });
        true
    }

    pub fn refresh_icons(&self) {
        let palette = self.theme.palette();
        let sources: Vec<TrayIconSource> = {
            let guard = self.items.lock().unwrap();
            guard
                .iter()
                .map(|e| TrayIconSource {
                    bus: e.item.bus.clone(),
                    path: e.item.path.clone(),
                    icon_name: e.item.icon_name.clone(),
                    icon_theme_path: e.icon_theme_path.clone(),
                    pixmaps: e.pixmaps.clone(),
                })
                .collect()
        };

        if sources.is_empty() {
            return;
        }

        let resolved: Vec<(String, String, String)> = sources
            .into_iter()
            .map(|src| {
                let url = resolve_icon_url(
                    &src.icon_name,
                    &src.icon_theme_path,
                    &src.pixmaps,
                    &palette,
                );
                (src.bus, src.path, url)
            })
            .collect();

        let mut guard = self.items.lock().unwrap();
        let mut changed = false;
        for (bus, path, url) in resolved {
            if let Some(entry) = guard
                .iter_mut()
                .find(|e| e.item.bus == bus && e.item.path == path)
            {
                if entry.item.icon_url != url {
                    entry.item.icon_url = url;
                    changed = true;
                }
            }
        }
        if !changed {
            return;
        }
        let snapshot = Self::snapshot_locked(&guard);
        drop(guard);
        (self.send)(BridgeEvent::Tray { items: snapshot });
    }

    fn find_target(&self, sender: &str, path: &str) -> Option<(String, String)> {
        self.items
            .lock()
            .unwrap()
            .iter()
            .find(|e| (e.item.bus == sender || e.unique_bus == sender) && e.item.path == path)
            .map(|e| (e.item.bus.clone(), e.item.path.clone()))
    }

    fn remove_by_bus(&self, bus: &str) -> Vec<String> {
        let mut guard = self.items.lock().unwrap();
        let mut removed = Vec::new();
        guard.retain(|entry| {
            if entry.item.bus == bus || entry.unique_bus == bus {
                removed.push(format!("{}{}", entry.item.bus, entry.item.path));
                false
            } else {
                true
            }
        });
        if removed.is_empty() {
            return removed;
        }
        let snapshot = Self::snapshot_locked(&guard);
        drop(guard);
        (self.send)(BridgeEvent::Tray { items: snapshot });
        removed
    }

    fn registered_items(&self) -> Vec<String> {
        self.items
            .lock()
            .unwrap()
            .iter()
            .map(|e| format!("{}{}", e.item.bus, e.item.path))
            .collect()
    }
}

struct StatusNotifierWatcher {
    store: TrayStore,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl StatusNotifierWatcher {
    fn register_status_notifier_item(&self, service: &str, #[zbus(header)] header: Header<'_>) {
        let sender = header
            .sender()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let (bus, path) = if service.starts_with('/') {
            (sender.clone(), service.to_string())
        } else {
            (service.to_string(), "/StatusNotifierItem".to_string())
        };

        if bus.is_empty() {
            return;
        }

        let unique_bus = if bus.starts_with(':') {
            bus.clone()
        } else {
            sender
        };

        let store = self.store.clone();
        thread::spawn(move || {
            let palette = store.theme.palette();
            let fetched = fetch_item_properties(&bus, &path, &palette);
            store.upsert_from_fetched(bus, path, unique_bus, fetched);
        });
    }

    fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.store.registered_items()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
}

#[derive(Default)]
struct FetchedItemProperties {
    title: String,
    icon_name: String,
    icon_theme_path: String,
    pixmaps: Vec<(i32, i32, Vec<u8>)>,
    icon_url: String,
    menu_path: String,
}

fn fetch_item_properties(
    bus: &str,
    path: &str,
    palette: &TrayColorPalette,
) -> FetchedItemProperties {
    let conn = match Connection::session() {
        Ok(c) => c,
        Err(_) => return FetchedItemProperties::default(),
    };

    let reply = conn.call_method(
        Some(bus),
        path,
        Some("org.freedesktop.DBus.Properties"),
        "GetAll",
        &("org.kde.StatusNotifierItem",),
    );

    let Ok(msg) = reply else {
        return FetchedItemProperties::default();
    };

    let Ok(props) = msg.body().deserialize::<HashMap<String, OwnedValue>>() else {
        return FetchedItemProperties::default();
    };

    let id = props
        .get("Id")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    let tooltip_title = props
        .get("ToolTip")
        .and_then(|v| {
            <(String, Vec<(i32, i32, Vec<u8>)>, String, String)>::try_from(v.clone()).ok()
        })
        .map(|(_, _, t, _)| t)
        .unwrap_or_default();
    let mut title = props
        .get("Title")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    if title.is_empty() {
        title = if !tooltip_title.is_empty() {
            tooltip_title
        } else {
            id
        };
    }
    let icon_name = props
        .get("IconName")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    let icon_theme_path = props
        .get("IconThemePath")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    let pixmaps = props
        .get("IconPixmap")
        .and_then(|v| Vec::<(i32, i32, Vec<u8>)>::try_from(v.clone()).ok())
        .unwrap_or_default();
    let menu_path = props
        .get("Menu")
        .and_then(|v| OwnedObjectPath::try_from(v.clone()).ok())
        .map(|p| p.to_string())
        .unwrap_or_default();

    let icon_url = resolve_icon_url(&icon_name, &icon_theme_path, &pixmaps, palette);
    FetchedItemProperties {
        title,
        icon_name,
        icon_theme_path,
        pixmaps,
        icon_url,
        menu_path,
    }
}

fn resolve_icon_url(
    icon_name: &str,
    icon_theme_path: &str,
    pixmaps: &[ArgbPixmap],
    palette: &TrayColorPalette,
) -> String {
    if !icon_name.is_empty() {
        if Path::new(icon_name).is_file() {
            if let Some(url) = file_to_data_url(Path::new(icon_name), palette) {
                return url;
            }
        }
        if let Some(url) = lookup_theme_icon_data_url(icon_name, icon_theme_path, palette) {
            return url;
        }
    }

    if let Some((w, h, argb)) = select_best_pixmap(pixmaps) {
        if let Some(url) = argb_pixmap_to_png_data_url(w, h, argb) {
            return url;
        }
    }

    String::new()
}

fn select_best_pixmap(pixmaps: &[ArgbPixmap]) -> Option<(i32, i32, &[u8])> {
    pixmaps
        .iter()
        .filter(|(w, h, bytes)| *w > 0 && *h > 0 && bytes.len() == (*w as usize) * (*h as usize) * 4)
        .max_by_key(|(w, h, _)| (*w).min(*h))
        .map(|(w, h, bytes)| (*w, *h, bytes.as_slice()))
}

fn argb_pixmap_to_png_data_url(width: i32, height: i32, argb: &[u8]) -> Option<String> {
    let mut rgba = Vec::with_capacity(argb.len());
    for px in argb.as_chunks::<4>().0 {
        rgba.push(px[1]);
        rgba.push(px[2]);
        rgba.push(px[3]);
        rgba.push(px[0]);
    }
    let pixbuf = Pixbuf::from_bytes(
        &glib::Bytes::from_owned(rgba),
        Colorspace::Rgb,
        true,
        8,
        width,
        height,
        width * 4,
    );
    let png = pixbuf.save_to_bufferv("png", &[]).ok()?;
    Some(format!("data:image/png;base64,{}", BASE64.encode(png)))
}

fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

fn has_id_current_color_scheme(elem: &BytesStart<'_>) -> bool {
    elem.attributes().flatten().any(|attr| {
        attr.key.local_name().as_ref() == b"id"
            && attr.unescape_value().ok().as_deref() == Some("current-color-scheme")
    })
}

fn with_root_svg_color(elem: BytesStart<'_>, color: &str) -> BytesStart<'static> {
    let tag_name = String::from_utf8_lossy(elem.name().as_ref()).into_owned();
    let mut updated = BytesStart::new(tag_name);
    for attr in elem.attributes().flatten() {
        if attr.key.local_name().as_ref() != b"color" {
            updated.push_attribute(attr);
        }
    }
    updated.push_attribute(("color", color));
    updated.into_owned()
}

fn recolor_svg_bytes(bytes: &[u8], palette: &TrayColorPalette) -> Vec<u8> {
    let has_color_scheme = bytes_contain(bytes, b"current-color-scheme");
    let has_current_color = bytes_contain(bytes, b"currentColor");
    if !has_color_scheme && !has_current_color {
        return bytes.to_vec();
    }

    let mut reader = Reader::from_reader(bytes);
    let mut writer = Writer::new(Vec::with_capacity(bytes.len() + 256));
    let mut in_current_color_scheme = false;
    let mut wrote_style_body = false;
    let mut saw_root_svg = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if !saw_root_svg && e.local_name().as_ref() == b"svg" {
                    saw_root_svg = true;
                    if !has_color_scheme && has_current_color {
                        let updated = with_root_svg_color(e, &palette.text);
                        if writer.write_event(Event::Start(updated)).is_err() {
                            return bytes.to_vec();
                        }
                        let mut out = writer.into_inner();
                        out.extend_from_slice(&bytes[reader.buffer_position() as usize..]);
                        return out;
                    }
                } else if e.local_name().as_ref() == b"style" && has_id_current_color_scheme(&e) {
                    in_current_color_scheme = true;
                    wrote_style_body = false;
                }

                if writer.write_event(Event::Start(e)).is_err() {
                    return bytes.to_vec();
                }
            }
            Ok(Event::End(e)) => {
                if in_current_color_scheme && e.local_name().as_ref() == b"style" {
                    if !wrote_style_body
                        && writer
                            .write_event(Event::Text(BytesText::new(&palette.stylesheet())))
                            .is_err()
                    {
                        return bytes.to_vec();
                    }
                    if writer.write_event(Event::End(e)).is_err() {
                        return bytes.to_vec();
                    }
                    let mut out = writer.into_inner();
                    out.extend_from_slice(&bytes[reader.buffer_position() as usize..]);
                    return out;
                }
                if writer.write_event(Event::End(e)).is_err() {
                    return bytes.to_vec();
                }
            }
            Ok(Event::Text(_)) | Ok(Event::CData(_)) if in_current_color_scheme => {
                if !wrote_style_body {
                    if writer
                        .write_event(Event::Text(BytesText::new(&palette.stylesheet())))
                        .is_err()
                    {
                        return bytes.to_vec();
                    }
                    wrote_style_body = true;
                }
            }
            Ok(Event::Eof) => {
                if in_current_color_scheme {
                    return bytes.to_vec();
                }
                break;
            }
            Ok(ev) => {
                if writer.write_event(ev).is_err() {
                    return bytes.to_vec();
                }
            }
            Err(_) => return bytes.to_vec(),
        }
    }

    writer.into_inner()
}

fn file_to_data_url(path: &Path, palette: &TrayColorPalette) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let mime = match path.extension().and_then(|e| e.to_str()) {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("xpm") => "image/x-xpixmap",
        _ => "application/octet-stream",
    };
    let encoded = if mime == "image/svg+xml" {
        BASE64.encode(recolor_svg_bytes(&bytes, palette))
    } else {
        BASE64.encode(bytes)
    };
    Some(format!("data:{mime};base64,{encoded}"))
}

fn lookup_theme_icon_data_url(
    icon_name: &str,
    icon_theme_path: &str,
    palette: &TrayColorPalette,
) -> Option<String> {
    if !gtk4::is_initialized() {
        return None;
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let name = icon_name.to_string();
    let theme_path = icon_theme_path.to_string();

    glib::idle_add_once(move || {
        let theme = gdk::Display::default()
            .map(|d| IconTheme::for_display(&d))
            .unwrap_or_default();
        if !theme_path.is_empty() && !theme.search_path().iter().any(|p| p == Path::new(&theme_path))
        {
            theme.add_search_path(&theme_path);
        }
        let paintable = theme.lookup_icon(
            &name,
            &[],
            24,
            1,
            TextDirection::None,
            IconLookupFlags::empty(),
        );
        let _ = tx.send(paintable.file().and_then(|f| f.path()));
    });

    let file_path = rx.recv().ok().flatten()?;
    file_to_data_url(&file_path, palette)
}

fn handle_name_owner_changed(store: &TrayStore, name: &str, new_owner: &str) -> Vec<String> {
    if !new_owner.is_empty() {
        return Vec::new();
    }
    store.remove_by_bus(name)
}

fn should_refresh_sni_signal(msg: &zbus::Message) -> bool {
    let header = msg.header();
    let iface = header.interface().map(|i| i.as_str());
    let member = header.member().map(|m| m.as_str());

    match (iface, member) {
        (
            Some("org.kde.StatusNotifierItem"),
            Some("NewIcon" | "NewAttentionIcon" | "NewOverlayIcon" | "NewTitle" | "NewStatus"),
        ) => true,
        (Some("org.freedesktop.DBus.Properties"), Some("PropertiesChanged")) => {
            matches!(
                msg.body()
                    .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>(),
                Ok((changed_iface, _, _)) if changed_iface == "org.kde.StatusNotifierItem"
            )
        }
        _ => false,
    }
}

fn spawn_item_refresh(store: TrayStore, bus: String, path: String) {
    thread::spawn(move || {
        let palette = store.theme.palette();
        let fetched = fetch_item_properties(&bus, &path, &palette);
        store.update_from_fetched(&bus, &path, fetched);
    });
}

pub fn spawn_server(send: SendEvent, theme: TrayTheme) -> TrayStore {
    let store = TrayStore::with_theme(send, theme);
    let server_store = store.clone();
    thread::spawn(move || {
        let store = server_store;
        let watcher = StatusNotifierWatcher {
            store: store.clone(),
        };
        let conn = match ConnectionBuilder::session()
            .and_then(|b| b.name("org.kde.StatusNotifierWatcher"))
            .and_then(|b| b.serve_at("/StatusNotifierWatcher", watcher))
            .and_then(|b| b.build())
        {
            Ok(conn) => conn,
            Err(err) => {
                eprintln!(
                    "webbar: could not claim org.kde.StatusNotifierWatcher ({err}). \
                     Another StatusNotifierWatcher may already be running."
                );
                return;
            }
        };

        let Ok(name_owner_rule) = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|b| b.path("/org/freedesktop/DBus"))
            .and_then(|b| b.interface("org.freedesktop.DBus"))
            .and_then(|b| b.member("NameOwnerChanged"))
            .map(|b| b.build())
        else {
            loop {
                thread::park();
            }
        };

        let sni_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.kde.StatusNotifierItem")
            .map(|b| b.build())
            .ok();

        let props_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .and_then(|b| b.member("PropertiesChanged"))
            .and_then(|b| b.arg(0, "org.kde.StatusNotifierItem"))
            .map(|b| b.build())
            .ok();

        async_io::block_on(async {
            let mut streams = Vec::new();
            let Ok(name_stream) =
                MessageStream::for_match_rule(name_owner_rule, conn.inner(), Some(64)).await
            else {
                return;
            };
            streams.push(name_stream);

            if let Some(rule) = sni_rule {
                if let Ok(stream) =
                    MessageStream::for_match_rule(rule, conn.inner(), Some(64)).await
                {
                    streams.push(stream);
                }
            }

            if let Some(rule) = props_rule {
                if let Ok(stream) =
                    MessageStream::for_match_rule(rule, conn.inner(), Some(64)).await
                {
                    streams.push(stream);
                }
            }

            let mut combined = futures_util::stream::select_all(streams);

            while let Some(Ok(msg)) = combined.next().await {
                let header = msg.header();
                if header.interface().map(|i| i.as_str()) == Some("org.freedesktop.DBus")
                    && header.member().map(|m| m.as_str()) == Some("NameOwnerChanged")
                {
                    if let Ok((name, _old_owner, new_owner)) =
                        msg.body().deserialize::<(String, String, String)>()
                    {
                        let removed = handle_name_owner_changed(&store, &name, &new_owner);
                        if !removed.is_empty() {
                            if let Ok(iface) = conn
                                .object_server()
                                .interface::<_, StatusNotifierWatcher>("/StatusNotifierWatcher")
                            {
                                for service in removed {
                                    let _ =
                                        StatusNotifierWatcher::status_notifier_item_unregistered(
                                            iface.signal_emitter(),
                                            &service,
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                    continue;
                }

                if should_refresh_sni_signal(&msg) {
                    let Some(sender) = header.sender().map(|s| s.as_str()) else {
                        continue;
                    };
                    let Some(path) = header.path().map(|p| p.as_str()) else {
                        continue;
                    };
                    if let Some((bus, item_path)) = store.find_target(sender, path) {
                        spawn_item_refresh(store.clone(), bus, item_path);
                    }
                }
            }
        });

        loop {
            thread::park();
        }
    });
    store
}

fn split_registered_service(service: &str) -> (String, String) {
    if let Some((bus, rest)) = service.split_once('/') {
        (bus.to_string(), format!("/{rest}"))
    } else {
        (service.to_string(), "/StatusNotifierItem".to_string())
    }
}

fn resolve_unique_bus(conn: &Connection, bus: &str) -> String {
    if bus.starts_with(':') {
        return bus.to_string();
    }
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "GetNameOwner",
        &(bus,),
    )
    .ok()
    .and_then(|msg| msg.body().deserialize::<String>().ok())
    .unwrap_or_default()
}

fn spawn_observed_item_fetch(store: TrayStore, service: String) {
    let (bus, path) = split_registered_service(&service);
    if bus.is_empty() {
        return;
    }
    thread::spawn(move || {
        let unique_bus = Connection::session()
            .ok()
            .map(|c| resolve_unique_bus(&c, &bus))
            .unwrap_or_default();
        let palette = store.theme.palette();
        let fetched = fetch_item_properties(&bus, &path, &palette);
        store.upsert_from_fetched(bus, path, unique_bus, fetched);
    });
}

/// Observe an existing `org.kde.StatusNotifierWatcher` on the session bus
/// without claiming the well-known name. Used in headless `--ws-bridge` mode
/// so browser dev previews receive live tray items alongside a running bar.
pub fn spawn_observer(send: SendEvent, theme: TrayTheme) -> TrayStore {
    let store = TrayStore::with_theme(send, theme);
    let observer_store = store.clone();
    thread::spawn(move || {
        let store = observer_store;
        let Ok(conn) = Connection::session() else {
            return;
        };

        if let Ok(reply) = conn.call_method(
            Some("org.kde.StatusNotifierWatcher"),
            "/StatusNotifierWatcher",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(
                "org.kde.StatusNotifierWatcher",
                "RegisteredStatusNotifierItems",
            ),
        ) {
            if let Ok(val) = reply.body().deserialize::<OwnedValue>() {
                if let Ok(items) = Vec::<String>::try_from(val) {
                    for service in items {
                        spawn_observed_item_fetch(store.clone(), service);
                    }
                }
            }
        }

        let watcher_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.kde.StatusNotifierWatcher")
            .map(|b| b.build())
            .ok();

        let name_owner_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|b| b.path("/org/freedesktop/DBus"))
            .and_then(|b| b.interface("org.freedesktop.DBus"))
            .and_then(|b| b.member("NameOwnerChanged"))
            .map(|b| b.build())
            .ok();

        let sni_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.kde.StatusNotifierItem")
            .map(|b| b.build())
            .ok();

        let props_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .and_then(|b| b.member("PropertiesChanged"))
            .and_then(|b| b.arg(0, "org.kde.StatusNotifierItem"))
            .map(|b| b.build())
            .ok();

        async_io::block_on(async {
            let mut streams = Vec::new();
            for rule in [watcher_rule, name_owner_rule, sni_rule, props_rule]
                .into_iter()
                .flatten()
            {
                if let Ok(stream) =
                    MessageStream::for_match_rule(rule, conn.inner(), Some(64)).await
                {
                    streams.push(stream);
                }
            }
            if streams.is_empty() {
                return;
            }
            let mut combined = futures_util::stream::select_all(streams);

            while let Some(Ok(msg)) = combined.next().await {
                let header = msg.header();
                let iface = header.interface().map(|i| i.as_str());
                let member = header.member().map(|m| m.as_str());

                if iface == Some("org.kde.StatusNotifierWatcher") {
                    if member == Some("StatusNotifierItemRegistered") {
                        if let Ok(service) = msg.body().deserialize::<String>() {
                            spawn_observed_item_fetch(store.clone(), service);
                        }
                    } else if member == Some("StatusNotifierItemUnregistered") {
                        if let Ok(service) = msg.body().deserialize::<String>() {
                            let (bus, _path) = split_registered_service(&service);
                            store.remove_by_bus(&bus);
                        }
                    }
                    continue;
                }

                if iface == Some("org.freedesktop.DBus") && member == Some("NameOwnerChanged") {
                    if let Ok((name, _old_owner, new_owner)) =
                        msg.body().deserialize::<(String, String, String)>()
                    {
                        handle_name_owner_changed(&store, &name, &new_owner);
                    }
                    continue;
                }

                if should_refresh_sni_signal(&msg) {
                    let Some(sender) = header.sender().map(|s| s.as_str()) else {
                        continue;
                    };
                    let Some(path) = header.path().map(|p| p.as_str()) else {
                        continue;
                    };
                    if let Some((bus, item_path)) = store.find_target(sender, path) {
                        spawn_item_refresh(store.clone(), bus, item_path);
                    }
                }
            }
        });
    });
    store
}

pub fn activate(bus: String, path: String) {
    thread::spawn(move || {
        let conn = match Connection::session() {
            Ok(c) => c,
            Err(err) => {
                eprintln!("webbar: failed to connect to session bus for tray activate: {err}");
                return;
            }
        };
        if let Err(err) = conn.call_method(
            Some(bus.as_str()),
            path.as_str(),
            Some("org.kde.StatusNotifierItem"),
            "Activate",
            &(0i32, 0i32),
        ) {
            eprintln!("webbar: failed to activate tray item {bus}{path}: {err}");
        }
    });
}

type RawMenuNode = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

fn strip_mnemonic(raw: &str) -> String {
    raw.replace("__", "\x00")
        .replace('_', "")
        .replace('\x00', "_")
}

fn parse_menu_node(node: RawMenuNode, palette: &TrayColorPalette) -> TrayMenuItem {
    let (id, props, raw_children) = node;
    let label = props
        .get("label")
        .and_then(|v| String::try_from(v.clone()).ok())
        .map(|s| strip_mnemonic(&s))
        .unwrap_or_default();
    let enabled = props
        .get("enabled")
        .and_then(|v| bool::try_from(v.clone()).ok())
        .unwrap_or(true);
    let visible = props
        .get("visible")
        .and_then(|v| bool::try_from(v.clone()).ok())
        .unwrap_or(true);
    let item_type = props
        .get("type")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_else(|| "standard".to_string());
    let toggle_type = props
        .get("toggle-type")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    let toggle_state = props
        .get("toggle-state")
        .and_then(|v| i32::try_from(v.clone()).ok())
        .unwrap_or(-1);
    let icon_name = props
        .get("icon-name")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();
    let icon_data = props
        .get("icon-data")
        .and_then(|v| Vec::<u8>::try_from(v.clone()).ok())
        .unwrap_or_default();
    let icon_url = if !icon_data.is_empty() {
        format!("data:image/png;base64,{}", BASE64.encode(&icon_data))
    } else if !icon_name.is_empty() {
        resolve_icon_url(&icon_name, "", &[], palette)
    } else {
        String::new()
    };
    let accessible_desc = props
        .get("accessible-desc")
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default();

    let children = raw_children
        .into_iter()
        .filter_map(|child_val| RawMenuNode::try_from(child_val).ok())
        .map(|child| parse_menu_node(child, palette))
        .filter(|item| item.visible)
        .collect();

    TrayMenuItem {
        id,
        label,
        enabled,
        visible,
        item_type,
        toggle_type,
        toggle_state,
        icon_name,
        icon_url,
        accessible_desc,
        children,
    }
}

fn fetch_menu_items_blocking(
    conn: &Connection,
    bus: &str,
    menu_path: &str,
    call_about_to_show: bool,
    palette: &TrayColorPalette,
) -> Result<Vec<TrayMenuItem>, String> {
    if call_about_to_show {
        let _ = conn.call_method(
            Some(bus),
            menu_path,
            Some("com.canonical.dbusmenu"),
            "AboutToShow",
            &(0i32,),
        );
    }

    let msg = conn
        .call_method(
            Some(bus),
            menu_path,
            Some("com.canonical.dbusmenu"),
            "GetLayout",
            &(0i32, -1i32, Vec::<&str>::new()),
        )
        .map_err(|err| err.to_string())?;

    let (_rev, root): (u32, RawMenuNode) = msg
        .body()
        .deserialize()
        .map_err(|err| err.to_string())?;

    Ok(parse_menu_node(root, palette).children)
}

async fn fetch_menu_items_async(
    conn: &zbus::Connection,
    bus: &str,
    menu_path: &str,
    call_about_to_show: bool,
    palette: &TrayColorPalette,
) -> Result<Vec<TrayMenuItem>, String> {
    if call_about_to_show {
        let _ = conn
            .call_method(
                Some(bus),
                menu_path,
                Some("com.canonical.dbusmenu"),
                "AboutToShow",
                &(0i32,),
            )
            .await;
    }

    let msg = conn
        .call_method(
            Some(bus),
            menu_path,
            Some("com.canonical.dbusmenu"),
            "GetLayout",
            &(0i32, -1i32, Vec::<&str>::new()),
        )
        .await
        .map_err(|err| err.to_string())?;

    let (_rev, root): (u32, RawMenuNode) = msg
        .body()
        .deserialize()
        .map_err(|err| err.to_string())?;

    Ok(parse_menu_node(root, palette).children)
}

fn should_refresh_dbusmenu_signal(msg: &zbus::Message) -> bool {
    let header = msg.header();
    matches!(
        (
            header.interface().map(|i| i.as_str()),
            header.member().map(|m| m.as_str()),
        ),
        (
            Some("com.canonical.dbusmenu"),
            Some("LayoutUpdated" | "ItemsPropertiesUpdated"),
        )
    )
}

struct MenuWatchEntry {
    count: usize,
    cancel: Option<oneshot::Sender<()>>,
}

#[derive(Clone)]
pub struct TrayMenuWatcher {
    watched: Arc<Mutex<HashMap<(String, String), MenuWatchEntry>>>,
    send: SendEvent,
    theme: TrayTheme,
}

impl TrayMenuWatcher {
    pub fn new(send: SendEvent, theme: TrayTheme) -> Self {
        Self {
            watched: Arc::new(Mutex::new(HashMap::new())),
            send,
            theme,
        }
    }

    pub fn watch(&self, bus: String, menu_path: String) {
        if bus.is_empty() || menu_path.is_empty() {
            return;
        }
        let key = (bus.clone(), menu_path.clone());
        let cancel_rx = {
            let mut guard = self.watched.lock().unwrap();
            if let Some(entry) = guard.get_mut(&key) {
                entry.count += 1;
                None
            } else {
                let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
                guard.insert(
                    key.clone(),
                    MenuWatchEntry {
                        count: 1,
                        cancel: Some(cancel_tx),
                    },
                );
                Some(cancel_rx)
            }
        };

        let send = self.send.clone();
        let theme = self.theme.clone();
        let Some(cancel_rx) = cancel_rx else {
            // Already watching this menu; still send a fresh snapshot for the new subscriber.
            thread::spawn(move || {
                if let Ok(conn) = Connection::session() {
                    let palette = theme.palette();
                    if let Ok(items) =
                        fetch_menu_items_blocking(&conn, &bus, &menu_path, true, &palette)
                    {
                        send(BridgeEvent::TrayMenu {
                            bus,
                            menu_path,
                            items,
                        });
                    }
                }
            });
            return;
        };

        thread::spawn(move || {
            async_io::block_on(async move {
                let Ok(conn) = zbus::Connection::session().await else {
                    return;
                };

                let rule = MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .sender(bus.as_str())
                    .and_then(|b| b.path(menu_path.as_str()))
                    .and_then(|b| b.interface("com.canonical.dbusmenu"))
                    .map(|b| b.build());

                let signals = match rule {
                    Ok(r) => MessageStream::for_match_rule(r, &conn, Some(32)).await.ok(),
                    Err(_) => None,
                };

                let palette = theme.palette();
                if let Ok(items) =
                    fetch_menu_items_async(&conn, &bus, &menu_path, true, &palette).await
                {
                    send(BridgeEvent::TrayMenu {
                        bus: bus.clone(),
                        menu_path: menu_path.clone(),
                        items,
                    });
                }

                let Some(mut signals) = signals else {
                    return;
                };

                let mut cancel_rx = cancel_rx.fuse();
                loop {
                    select! {
                        _ = cancel_rx => {
                            return;
                        }
                        maybe_msg = signals.next().fuse() => {
                            let Some(Ok(msg)) = maybe_msg else {
                                return;
                            };
                            if should_refresh_dbusmenu_signal(&msg) {
                                // Drain any additional queued signals before re-fetching to
                                // coalesce rapid LayoutUpdated bursts.
                                while let Some(Some(_)) = signals.next().now_or_never() {}
                                let palette = theme.palette();
                                if let Ok(items) =
                                    fetch_menu_items_async(&conn, &bus, &menu_path, false, &palette).await
                                {
                                    send(BridgeEvent::TrayMenu {
                                        bus: bus.clone(),
                                        menu_path: menu_path.clone(),
                                        items,
                                    });
                                }
                            }
                        }
                    }
                }
            });
        });
    }

    pub fn unwatch(&self, bus: &str, menu_path: &str) {
        let cancel = {
            let mut guard = self.watched.lock().unwrap();
            let key = (bus.to_string(), menu_path.to_string());
            let Some(entry) = guard.get_mut(&key) else {
                return;
            };
            entry.count = entry.count.saturating_sub(1);
            if entry.count == 0 {
                guard.remove(&key).and_then(|e| e.cancel)
            } else {
                None
            }
        };
        if let Some(tx) = cancel {
            let _ = tx.send(());
        }
    }
}

pub fn get_menu(id: u32, bus: String, menu_path: String, send: SendEvent, theme: TrayTheme) {
    thread::spawn(move || {
        let send_err = |msg: String| {
            crate::bridge::reply_err(&send, id, msg);
        };

        let conn = match Connection::session() {
            Ok(c) => c,
            Err(err) => return send_err(err.to_string()),
        };

        let palette = theme.palette();
        match fetch_menu_items_blocking(&conn, &bus, &menu_path, true, &palette) {
            Ok(items) => crate::bridge::reply_data(&send, id, items),
            Err(err) => send_err(err),
        }
    });
}

pub fn menu_event(bus: String, menu_path: String, item_id: i32) {
    thread::spawn(move || {
        let conn = match Connection::session() {
            Ok(c) => c,
            Err(err) => {
                eprintln!("webbar: failed to connect to session bus for tray menu event: {err}");
                return;
            }
        };
        if let Err(err) = conn.call_method(
            Some(bus.as_str()),
            menu_path.as_str(),
            Some("com.canonical.dbusmenu"),
            "Event",
            &(item_id, "clicked", Value::from(""), 0u32),
        ) {
            eprintln!("webbar: failed to send tray menu event {bus}{menu_path}#{item_id}: {err}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_dbusmenu_node_and_strips_mnemonics() {
        let mut child_props: HashMap<String, OwnedValue> = HashMap::new();
        child_props.insert(
            "label".to_string(),
            Value::from("Turn Bluetooth _Off").try_into().unwrap(),
        );
        child_props.insert("enabled".to_string(), OwnedValue::from(true));

        let child_tuple: RawMenuNode = (65536, child_props, Vec::new());
        let child_val: OwnedValue = Value::from(child_tuple).try_into().unwrap();

        let root_props: HashMap<String, OwnedValue> = HashMap::new();
        let parsed = parse_menu_node((0, root_props, vec![child_val]), &TrayColorPalette::default());
        assert_eq!(parsed.children.len(), 1);
        assert_eq!(parsed.children[0].id, 65536);
        assert_eq!(parsed.children[0].label, "Turn Bluetooth Off");
        assert!(parsed.children[0].enabled);
    }

    #[test]
    fn removes_tray_items_when_registered_bus_loses_owner() {
        let events: Arc<Mutex<Vec<BridgeEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let send: SendEvent = Arc::new(move |ev| {
            events_clone.lock().unwrap().push(ev);
        });
        let store = TrayStore::new(send);

        store.upsert(
            TrayItem {
                bus: ":1.42".to_string(),
                path: "/StatusNotifierItem".to_string(),
                title: "Stasis".to_string(),
                icon_name: "stasis".to_string(),
                icon_url: String::new(),
                menu_path: "/MenuBar".to_string(),
            },
            ":1.42".to_string(),
        );
        store.upsert(
            TrayItem {
                bus: ":1.99".to_string(),
                path: "/org/ayatana/NotificationItem/foo/Menu".to_string(),
                title: "Other".to_string(),
                icon_name: "other".to_string(),
                icon_url: String::new(),
                menu_path: String::new(),
            },
            ":1.99".to_string(),
        );
        assert_eq!(events.lock().unwrap().len(), 2);

        // Non-empty new_owner (ownership acquired or transferred) must not remove items.
        assert!(handle_name_owner_changed(&store, ":1.42", ":1.100").is_empty());
        assert_eq!(store.registered_items().len(), 2);
        assert_eq!(events.lock().unwrap().len(), 2);

        // Empty new_owner means the bus disconnected/exited.
        let removed = handle_name_owner_changed(&store, ":1.42", "");
        assert_eq!(removed, vec![":1.42/StatusNotifierItem".to_string()]);
        assert_eq!(
            store.registered_items(),
            vec![":1.99/org/ayatana/NotificationItem/foo/Menu".to_string()]
        );

        let guard = events.lock().unwrap();
        assert_eq!(guard.len(), 3);
        match guard.last().unwrap() {
            BridgeEvent::Tray { items } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].bus, ":1.99");
            }
            other => panic!("expected BridgeEvent::Tray, got {other:?}"),
        }
        drop(guard);

        // Repeating for an unknown/already-removed bus is a no-op with no extra event.
        assert!(handle_name_owner_changed(&store, ":1.42", "").is_empty());
        assert_eq!(events.lock().unwrap().len(), 3);
    }

    #[test]
    fn finds_and_updates_existing_tray_item_without_resurrecting_removed_ones() {
        let events: Arc<Mutex<Vec<BridgeEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let send: SendEvent = Arc::new(move |ev| {
            events_clone.lock().unwrap().push(ev);
        });
        let store = TrayStore::new(send);

        store.upsert(
            TrayItem {
                bus: "org.kde.StatusNotifierItem-100-1".to_string(),
                path: "/StatusNotifierItem".to_string(),
                title: "NetworkManager Applet".to_string(),
                icon_name: "nm-no-connection".to_string(),
                icon_url: "data:image/png;base64,AAA".to_string(),
                menu_path: "/org/ayatana/NotificationItem/nm_applet/Menu".to_string(),
            },
            ":1.55".to_string(),
        );
        assert_eq!(events.lock().unwrap().len(), 1);

        // Matches by either unique sender (:1.55) or registered bus name.
        assert_eq!(
            store.find_target(":1.55", "/StatusNotifierItem"),
            Some((
                "org.kde.StatusNotifierItem-100-1".to_string(),
                "/StatusNotifierItem".to_string()
            ))
        );
        assert_eq!(
            store.find_target("org.kde.StatusNotifierItem-100-1", "/StatusNotifierItem"),
            Some((
                "org.kde.StatusNotifierItem-100-1".to_string(),
                "/StatusNotifierItem".to_string()
            ))
        );
        assert_eq!(store.find_target(":1.55", "/OtherPath"), None);

        // No-op update does not emit a duplicate event.
        assert!(!store.update_if_present(
            "org.kde.StatusNotifierItem-100-1",
            "/StatusNotifierItem",
            "NetworkManager Applet".to_string(),
            "nm-no-connection".to_string(),
            "data:image/png;base64,AAA".to_string(),
            "/org/ayatana/NotificationItem/nm_applet/Menu".to_string(),
        ));
        assert_eq!(events.lock().unwrap().len(), 1);

        // Icon update emits a new snapshot with the updated icon.
        assert!(store.update_if_present(
            "org.kde.StatusNotifierItem-100-1",
            "/StatusNotifierItem",
            "NetworkManager Applet".to_string(),
            "nm-signal-75".to_string(),
            "data:image/png;base64,BBB".to_string(),
            "/org/ayatana/NotificationItem/nm_applet/Menu".to_string(),
        ));
        assert_eq!(events.lock().unwrap().len(), 2);
        match events.lock().unwrap().last().unwrap() {
            BridgeEvent::Tray { items } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].icon_name, "nm-signal-75");
                assert_eq!(items[0].icon_url, "data:image/png;base64,BBB");
            }
            other => panic!("expected BridgeEvent::Tray, got {other:?}"),
        }

        // Removing by unique_bus also removes well-known-name items, and
        // update_if_present must not resurrect removed items.
        assert_eq!(
            handle_name_owner_changed(&store, ":1.55", ""),
            vec!["org.kde.StatusNotifierItem-100-1/StatusNotifierItem".to_string()]
        );
        assert!(!store.update_if_present(
            "org.kde.StatusNotifierItem-100-1",
            "/StatusNotifierItem",
            "NetworkManager Applet".to_string(),
            "nm-signal-100".to_string(),
            "data:image/png;base64,CCC".to_string(),
            "/org/ayatana/NotificationItem/nm_applet/Menu".to_string(),
        ));
        assert!(store.registered_items().is_empty());
    }

    #[test]
    fn dbusmenu_signal_filter_matches_layout_and_property_updates() {
        let layout_msg = zbus::Message::signal(
            "/org/ayatana/NotificationItem/nm_applet/Menu",
            "com.canonical.dbusmenu",
            "LayoutUpdated",
        )
        .unwrap()
        .build(&(1u32, 0i32))
        .unwrap();
        assert!(should_refresh_dbusmenu_signal(&layout_msg));

        let props_msg = zbus::Message::signal(
            "/org/ayatana/NotificationItem/nm_applet/Menu",
            "com.canonical.dbusmenu",
            "ItemsPropertiesUpdated",
        )
        .unwrap()
        .build(&())
        .unwrap();
        assert!(should_refresh_dbusmenu_signal(&props_msg));

        let activation_msg = zbus::Message::signal(
            "/org/ayatana/NotificationItem/nm_applet/Menu",
            "com.canonical.dbusmenu",
            "ItemActivationRequested",
        )
        .unwrap()
        .build(&(1i32, 0u32))
        .unwrap();
        assert!(!should_refresh_dbusmenu_signal(&activation_msg));
    }

    #[test]
    fn menu_watcher_refcounts_and_cancels_on_last_unwatch() {
        let send: SendEvent = Arc::new(|_| {});
        let watcher = TrayMenuWatcher::new(send, TrayTheme::default());
        let key = (
            ":1.7".to_string(),
            "/org/ayatana/NotificationItem/nm_applet/Menu".to_string(),
        );

        let (tx, mut rx) = oneshot::channel::<()>();
        watcher.watched.lock().unwrap().insert(
            key.clone(),
            MenuWatchEntry {
                count: 2,
                cancel: Some(tx),
            },
        );

        watcher.unwatch(&key.0, &key.1);
        assert_eq!(watcher.watched.lock().unwrap().get(&key).unwrap().count, 1);
        assert!(rx.try_recv().unwrap().is_none());

        watcher.unwatch(&key.0, &key.1);
        assert!(!watcher.watched.lock().unwrap().contains_key(&key));
        assert_eq!(rx.try_recv().unwrap(), Some(()));

        // Extra unwatch is a safe no-op.
        watcher.unwatch(&key.0, &key.1);
    }

    #[test]
    fn parses_menu_icon_data_icon_name_and_accessible_desc() {
        let mut props = HashMap::new();
        props.insert("label".to_string(), OwnedValue::from(zbus::zvariant::Str::from("MyWiFi")));
        props.insert(
            "icon-data".to_string(),
            OwnedValue::try_from(Value::from(vec![137u8, 80, 78, 71])).unwrap(),
        );
        props.insert(
            "icon-name".to_string(),
            OwnedValue::from(zbus::zvariant::Str::from("network-wireless-symbolic")),
        );
        props.insert(
            "accessible-desc".to_string(),
            OwnedValue::from(zbus::zvariant::Str::from("MyWiFi: 80%, secure.")),
        );

        let item = parse_menu_node((7, props, Vec::new()), &TrayColorPalette::default());
        assert_eq!(item.label, "MyWiFi");
        assert_eq!(item.icon_name, "network-wireless-symbolic");
        assert_eq!(item.icon_url, "data:image/png;base64,iVBORw==");
        assert_eq!(item.accessible_desc, "MyWiFi: 80%, secure.");
    }

    #[test]
    fn splits_registered_status_notifier_service_strings() {
        assert_eq!(
            split_registered_service(":1.7/org/ayatana/NotificationItem/nm_applet"),
            (
                ":1.7".to_string(),
                "/org/ayatana/NotificationItem/nm_applet".to_string()
            )
        );
        assert_eq!(
            split_registered_service("org.kde.StatusNotifierItem-100-1/StatusNotifierItem"),
            (
                "org.kde.StatusNotifierItem-100-1".to_string(),
                "/StatusNotifierItem".to_string()
            )
        );
        assert_eq!(
            split_registered_service("org.kde.StatusNotifierItem-100-1"),
            (
                "org.kde.StatusNotifierItem-100-1".to_string(),
                "/StatusNotifierItem".to_string()
            )
        );
    }

    #[test]
    fn recolors_current_color_scheme_svg_for_light_and_dark_themes() {
        let papirus_svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">
 <defs>
  <style id="current-color-scheme" type="text/css">
   .ColorScheme-Text { color:#dfdfdf; }
   .ColorScheme-Highlight { color:#4285f4; }
  </style>
 </defs>
 <path d="M4 8l8 11 8-11z" style="fill:currentColor" class="ColorScheme-Text"/>
</svg>"#;

        let light_palette = TrayColorPalette::from_theme_state(&ThemeState {
            dark: false,
            ..ThemeState::default()
        });
        let light_out = String::from_utf8(recolor_svg_bytes(papirus_svg, &light_palette)).unwrap();
        assert!(light_out.contains(".ColorScheme-Text { color: #18181f; }"));
        assert!(light_out.contains(".ColorScheme-NegativeText { color: #d63638; }"));
        assert!(!light_out.contains("#dfdfdf"));

        let dark_palette = TrayColorPalette::from_theme_state(&ThemeState {
            dark: true,
            ..ThemeState::default()
        });
        let dark_out = String::from_utf8(recolor_svg_bytes(papirus_svg, &dark_palette)).unwrap();
        assert!(dark_out.contains(".ColorScheme-Text { color: #ececf1; }"));
        assert!(dark_out.contains(".ColorScheme-NegativeText { color: #f2726f; }"));
    }

    #[test]
    fn recolors_cdata_style_and_preserves_full_color_and_malformed_svgs() {
        let cdata_svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">
 <style type="text/css" id="current-color-scheme"><![CDATA[
  .ColorScheme-Text { color:#dfdfdf; }
 ]]></style>
 <path class="ColorScheme-Text" fill="currentColor" d="M0 0h24v24H0z"/>
</svg>"#;
        let palette = TrayColorPalette::from_theme_state(&ThemeState {
            dark: false,
            ..ThemeState::default()
        });
        let cdata_out = String::from_utf8(recolor_svg_bytes(cdata_svg, &palette)).unwrap();
        assert!(cdata_out.contains(".ColorScheme-Text { color: #18181f; }"));
        assert!(!cdata_out.contains("#dfdfdf"));

        let full_color_svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><circle cx="12" cy="12" r="10" fill="#ff5500"/></svg>"##;
        assert_eq!(recolor_svg_bytes(full_color_svg, &palette), full_color_svg);

        let malformed_svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><style id="current-color-scheme">.ColorScheme-Text{color:#dfdfdf}"#;
        assert_eq!(recolor_svg_bytes(malformed_svg, &palette), malformed_svg);
    }

    #[test]
    fn sets_root_color_for_current_color_svg_without_color_scheme() {
        let symbolic_svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><path fill="currentColor" d="M2 2h12v12H2z"/></svg>"#;
        let light_palette = TrayColorPalette::from_theme_state(&ThemeState {
            dark: false,
            ..ThemeState::default()
        });
        let out = String::from_utf8(recolor_svg_bytes(symbolic_svg, &light_palette)).unwrap();
        assert!(out.contains(r##"color="#18181f""##));
    }

    #[test]
    fn tray_store_refreshes_svg_icons_when_theme_changes() {
        let dir = std::env::temp_dir().join(format!("webbar-tray-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let svg_path = dir.join("test-panel.svg");
        fs::write(
            &svg_path,
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><style id="current-color-scheme">.ColorScheme-Text { color:#dfdfdf; }</style><path class="ColorScheme-Text" fill="currentColor" d="M0 0h24v24H0z"/></svg>"#,
        )
        .unwrap();

        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let send: SendEvent = Arc::new(move |ev| {
            events_clone.lock().unwrap().push(ev);
        });

        let tray_theme = TrayTheme::new(&ThemeState {
            dark: true,
            ..ThemeState::default()
        });
        let store = TrayStore::with_theme(send, tray_theme.clone());
        let icon_path_str = svg_path.to_string_lossy().to_string();
        let initial_url = resolve_icon_url(&icon_path_str, "", &[], &tray_theme.palette());
        store.upsert(
            TrayItem {
                bus: ":1.99".to_string(),
                path: "/StatusNotifierItem".to_string(),
                title: "Test".to_string(),
                icon_name: icon_path_str,
                icon_url: initial_url.clone(),
                menu_path: String::new(),
            },
            ":1.99".to_string(),
        );

        assert!(tray_theme.update(&ThemeState {
            dark: false,
            ..ThemeState::default()
        }));
        store.refresh_icons();

        let captured = events.lock().unwrap();
        assert_eq!(captured.len(), 2);
        let updated_url = match &captured[1] {
            BridgeEvent::Tray { items } => items[0].icon_url.clone(),
            other => panic!("expected BridgeEvent::Tray, got {other:?}"),
        };
        assert_ne!(initial_url, updated_url);
        let encoded = updated_url
            .strip_prefix("data:image/svg+xml;base64,")
            .expect("svg data url");
        let decoded = String::from_utf8(BASE64.decode(encoded).unwrap()).unwrap();
        assert!(decoded.contains(".ColorScheme-Text { color: #18181f; }"));

        let _ = fs::remove_dir_all(&dir);
    }
}

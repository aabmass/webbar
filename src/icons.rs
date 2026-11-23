//! Shared icon theme lookup, SVG recoloring, pixbuf/image conversion, and
//! `.desktop` entry fallback helpers used by tray and notifications.

use std::borrow::Cow;
use std::io::Read;
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::{fs, io};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use freedesktop_desktop_entry::unicase::Ascii;
use freedesktop_desktop_entry::{
    desktop_entries, find_app_by_id, get_languages_from_env, DesktopEntry,
};
use gtk4::gdk;
use gtk4::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{IconLookupFlags, IconTheme, TextDirection};
use quick_xml::events::{BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};

use crate::theme::ThemeState;

const MAX_IMAGE_DIM: i32 = 128;

pub type ArgbPixmap = (i32, i32, Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconPalette {
    // Included for PartialEq change detection when gtk-icon-theme-name changes.
    pub icon_theme_name: String,
    pub dark: bool,
    pub text: String,
    pub background: String,
    pub highlight: String,
    pub accent: String,
}

impl Default for IconPalette {
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

impl IconPalette {
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
            dark: theme.dark,
            text,
            background,
            highlight,
            accent,
        }
    }

    pub fn stylesheet(&self) -> String {
        let (pos, neu, neg) = if self.dark {
            ("#6fcf97", "#f2c94c", "#f2726f")
        } else {
            ("#1f8f54", "#c5820a", "#d63638")
        };
        format!(
            ".ColorScheme-Text {{ color: {text}; }} \
             .ColorScheme-Background {{ color: {bg}; }} \
             .ColorScheme-Highlight {{ color: {hl}; }} \
             .ColorScheme-Accent {{ color: {accent}; }} \
             .ColorScheme-ButtonText {{ color: {text}; }} \
             .ColorScheme-PositiveText {{ color: {pos}; }} \
             .ColorScheme-NeutralText {{ color: {neu}; }} \
             .ColorScheme-NegativeText {{ color: {neg}; }}",
            text = self.text,
            bg = self.background,
            hl = self.highlight,
            accent = self.accent,
        )
    }
}

#[derive(Clone, Default)]
pub struct SharedIconPalette {
    palette: Arc<Mutex<IconPalette>>,
}

impl SharedIconPalette {
    #[cfg(test)]
    pub fn new(theme: &ThemeState) -> Self {
        Self {
            palette: Arc::new(Mutex::new(IconPalette::from_theme_state(theme))),
        }
    }

    pub fn palette(&self) -> IconPalette {
        self.palette.lock().unwrap().clone()
    }

    pub fn update(&self, theme: &ThemeState) -> bool {
        let next = IconPalette::from_theme_state(theme);
        let mut guard = self.palette.lock().unwrap();
        if *guard == next {
            return false;
        }
        *guard = next;
        true
    }
}

fn decode_file_uri(raw: &str) -> Cow<'_, str> {
    let Some(rest) = raw.strip_prefix("file://") else {
        return Cow::Borrowed(raw);
    };
    let stripped = rest.strip_prefix("localhost/").map_or(rest, |_| &rest["localhost".len()..]);
    if !stripped.as_bytes().contains(&b'%') {
        return Cow::Borrowed(stripped);
    }
    let bytes = stripped.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(((hi << 4) | lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    match String::from_utf8(out) {
        Ok(decoded) => Cow::Owned(decoded),
        Err(_) => Cow::Borrowed(stripped),
    }
}

pub fn resolve_icon_or_path(
    icon_name: &str,
    icon_theme_path: &str,
    size: i32,
    palette: &IconPalette,
) -> Option<String> {
    let trimmed = icon_name.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path_str = decode_file_uri(trimmed);
    let path = Path::new(path_str.as_ref());
    if path.is_absolute() && path.is_file() {
        if let Some(url) = file_to_data_url(path, palette) {
            return Some(url);
        }
    }
    lookup_theme_icon_data_url(trimmed, icon_theme_path, size, palette)
}

pub fn resolve_icon_url(
    icon_name: &str,
    icon_theme_path: &str,
    size: i32,
    pixmaps: &[ArgbPixmap],
    palette: &IconPalette,
) -> String {
    if let Some(url) = resolve_icon_or_path(icon_name, icon_theme_path, size, palette) {
        return url;
    }

    if let Some((w, h, argb)) = select_best_pixmap(pixmaps) {
        if let Some(url) = argb_pixmap_to_png_data_url(w, h, argb) {
            return url;
        }
    }

    String::new()
}

pub fn select_best_pixmap(pixmaps: &[ArgbPixmap]) -> Option<(i32, i32, &[u8])> {
    pixmaps
        .iter()
        .filter(|(w, h, bytes)| *w > 0 && *h > 0 && bytes.len() == (*w as usize) * (*h as usize) * 4)
        .max_by_key(|(w, h, _)| (*w).min(*h))
        .map(|(w, h, bytes)| (*w, *h, bytes.as_slice()))
}

pub fn argb_pixmap_to_png_data_url(width: i32, height: i32, argb: &[u8]) -> Option<String> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|px| px.checked_mul(4))?;
    if width <= 0 || height <= 0 || argb.len() < expected {
        return None;
    }
    let mut rgba = Vec::with_capacity(expected);
    for px in argb[..expected].as_chunks::<4>().0 {
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
    Some(encode_png_bytes_data_url(&png))
}

pub fn rgba_image_data_to_png_data_url(
    width: i32,
    height: i32,
    rowstride: i32,
    has_alpha: bool,
    bits_per_sample: i32,
    channels: i32,
    data: &[u8],
) -> Option<String> {
    let expected_channels = if has_alpha { 4 } else { 3 };
    if width <= 0 || height <= 0 || bits_per_sample != 8 || channels != expected_channels {
        return None;
    }
    let min_rowstride = width.checked_mul(channels)?;
    if rowstride < min_rowstride {
        return None;
    }
    let required_len = usize::try_from(rowstride)
        .ok()?
        .checked_mul(usize::try_from(height - 1).ok()?)?
        .checked_add(usize::try_from(min_rowstride).ok()?)?;
    if data.len() < required_len {
        return None;
    }

    let pixbuf = Pixbuf::from_bytes(
        &glib::Bytes::from(data),
        Colorspace::Rgb,
        has_alpha,
        bits_per_sample,
        width,
        height,
        rowstride,
    );
    let max_dim = width.max(height);
    let encoded_pixbuf = if max_dim > MAX_IMAGE_DIM {
        let scale = MAX_IMAGE_DIM as f64 / max_dim as f64;
        let tw = ((width as f64) * scale).round().max(1.0) as i32;
        let th = ((height as f64) * scale).round().max(1.0) as i32;
        pixbuf.scale_simple(tw, th, InterpType::Bilinear)?
    } else {
        pixbuf
    };
    let png = encoded_pixbuf.save_to_bufferv("png", &[]).ok()?;
    Some(encode_png_bytes_data_url(&png))
}

pub fn encode_png_bytes_data_url(bytes: &[u8]) -> String {
    format!("data:image/png;base64,{}", BASE64.encode(bytes))
}

#[derive(Debug, Clone, Default)]
pub struct DesktopEntryInfo {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub desktop_id: Option<String>,
    pub startup_wm_class: Option<String>,
    pub exec_program: Option<String>,
}

fn cached_desktop_entries() -> &'static (Vec<String>, Vec<DesktopEntry>) {
    static CACHE: OnceLock<(Vec<String>, Vec<DesktopEntry>)> = OnceLock::new();
    CACHE.get_or_init(|| {
        let locales = get_languages_from_env();
        let entries = desktop_entries(&locales);
        (locales, entries)
    })
}

fn extract_exec_program(exec: &str) -> Option<String> {
    let trimmed = exec.trim_start();
    let first = if let Some(rest) = trimmed.strip_prefix('"') {
        rest.split_once('"')?.0
    } else if let Some(rest) = trimmed.strip_prefix('\'') {
        rest.split_once('\'')?.0
    } else {
        trimmed.split_whitespace().next()?
    };
    if first.is_empty() {
        return None;
    }
    let name = Path::new(first)
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "env" | "sh" | "bash" | "flatpak" | "gtk-launch" | "xdg-open"
    ) {
        return None;
    }
    Some(name.to_string())
}

fn desktop_entry_info(entry: &DesktopEntry, locales: &[String]) -> DesktopEntryInfo {
    DesktopEntryInfo {
        name: entry
            .name(locales)
            .map(|s| s.into_owned())
            .filter(|s| !s.is_empty()),
        icon: entry
            .icon()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        desktop_id: Some(entry.appid.to_string()).filter(|s| !s.is_empty()),
        startup_wm_class: entry
            .desktop_entry("StartupWMClass")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        exec_program: entry.exec().and_then(extract_exec_program),
    }
}

pub fn lookup_desktop_entry(desktop_id: &str) -> Option<DesktopEntryInfo> {
    let trimmed = desktop_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.strip_suffix(".desktop").unwrap_or(trimmed);
    let (locales, entries) = cached_desktop_entries();
    let entry = find_app_by_id(entries, Ascii::new(normalized))?;
    Some(desktop_entry_info(entry, locales))
}

pub fn lookup_desktop_entry_by_app_name(app_name: &str) -> Option<DesktopEntryInfo> {
    let trimmed = app_name.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(info) = lookup_desktop_entry(trimmed) {
        return Some(info);
    }
    if trimmed.contains(' ') {
        let slug = trimmed.to_ascii_lowercase().replace(' ', "-");
        if let Some(info) = lookup_desktop_entry(&slug) {
            return Some(info);
        }
    }
    let (locales, entries) = cached_desktop_entries();
    let needle = trimmed.to_ascii_lowercase();
    for entry in entries {
        if entry
            .name(locales)
            .is_some_and(|n| n.trim().to_ascii_lowercase() == needle)
        {
            return Some(desktop_entry_info(entry, locales));
        }
        if entry
            .desktop_entry("StartupWMClass")
            .is_some_and(|c| c.trim().to_ascii_lowercase() == needle)
        {
            return Some(desktop_entry_info(entry, locales));
        }
    }
    None
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

pub fn recolor_svg_bytes(bytes: &[u8], palette: &IconPalette) -> Vec<u8> {
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

pub fn file_to_data_url(path: &Path, palette: &IconPalette) -> Option<String> {
    let bytes = read_file_capped(path).ok()?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let mime = match ext.as_deref() {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
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

fn read_file_capped(path: &Path) -> io::Result<Vec<u8>> {
    const MAX_ICON_BYTES: u64 = 4 * 1024 * 1024;
    let file = fs::File::open(path)?;
    let mut buf = Vec::new();
    file.take(MAX_ICON_BYTES + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_ICON_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "icon file exceeds size cap",
        ));
    }
    Ok(buf)
}

pub fn lookup_theme_icon_data_url(
    icon_name: &str,
    icon_theme_path: &str,
    size: i32,
    palette: &IconPalette,
) -> Option<String> {
    if !gtk4::is_initialized() {
        return None;
    }

    let lookup_on_main = move |name: &str, theme_path: &str| {
        let theme = gdk::Display::default()
            .map(|d| IconTheme::for_display(&d))
            .unwrap_or_default();
        if !theme_path.is_empty() && !theme.search_path().iter().any(|p| p == Path::new(theme_path))
        {
            theme.add_search_path(theme_path);
        }
        let paintable = theme.lookup_icon(
            name,
            &[],
            size,
            1,
            TextDirection::None,
            IconLookupFlags::empty(),
        );
        paintable.file().and_then(|f| f.path())
    };

    let file_path = if glib::MainContext::default().is_owner() {
        lookup_on_main(icon_name, icon_theme_path)?
    } else {
        let (tx, rx) = mpsc::sync_channel(1);
        let name = icon_name.to_string();
        let theme_path = icon_theme_path.to_string();
        glib::idle_add_once(move || {
            let _ = tx.send(lookup_on_main(&name, &theme_path));
        });
        rx.recv().ok().flatten()?
    };

    file_to_data_url(&file_path, palette)
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let light_palette = IconPalette::from_theme_state(&ThemeState {
            dark: false,
            ..ThemeState::default()
        });
        let light_out = String::from_utf8(recolor_svg_bytes(papirus_svg, &light_palette)).unwrap();
        assert!(light_out.contains(".ColorScheme-Text { color: #18181f; }"));
        assert!(light_out.contains(".ColorScheme-NegativeText { color: #d63638; }"));
        assert!(!light_out.contains("#dfdfdf"));

        let dark_palette = IconPalette::from_theme_state(&ThemeState {
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
        let palette = IconPalette::from_theme_state(&ThemeState {
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
        let light_palette = IconPalette::from_theme_state(&ThemeState {
            dark: false,
            ..ThemeState::default()
        });
        let out = String::from_utf8(recolor_svg_bytes(symbolic_svg, &light_palette)).unwrap();
        assert!(out.contains(r##"color="#18181f""##));
    }

    #[test]
    fn encodes_and_downscales_rgba_image_data() {
        // Invalid inputs are rejected cleanly
        assert_eq!(rgba_image_data_to_png_data_url(0, 10, 40, true, 8, 4, &[]), None);
        assert_eq!(
            rgba_image_data_to_png_data_url(4, 4, 16, true, 8, 4, &[0u8; 10]),
            None
        );

        // Large 256x128 RGBA buffer is downscaled to 128x64 PNG
        let width = 256;
        let height = 128;
        let rowstride = width * 4;
        let data = vec![200u8; (rowstride * height) as usize];
        let url = rgba_image_data_to_png_data_url(width, height, rowstride, true, 8, 4, &data)
            .expect("valid png data url");
        let b64 = url
            .strip_prefix("data:image/png;base64,")
            .expect("png prefix");
        let png_bytes = BASE64.decode(b64).expect("valid base64");
        assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        // PNG IHDR width/height are bytes 16..24 big-endian
        let encoded_w = i32::from_be_bytes(png_bytes[16..20].try_into().unwrap());
        let encoded_h = i32::from_be_bytes(png_bytes[20..24].try_into().unwrap());
        assert_eq!((encoded_w, encoded_h), (128, 64));

        // 3-channel RGB (has_alpha: false) with padded rowstride (> width * 3)
        let rgb_rowstride = 16;
        let rgb_data = vec![120u8; (rgb_rowstride * 3) as usize];
        let rgb_url =
            rgba_image_data_to_png_data_url(4, 3, rgb_rowstride, false, 8, 3, &rgb_data)
                .expect("valid rgb png data url");
        let rgb_b64 = rgb_url
            .strip_prefix("data:image/png;base64,")
            .expect("png prefix");
        let rgb_png = BASE64.decode(rgb_b64).expect("valid base64");
        assert!(rgb_png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(i32::from_be_bytes(rgb_png[16..20].try_into().unwrap()), 4);
        assert_eq!(i32::from_be_bytes(rgb_png[20..24].try_into().unwrap()), 3);
    }

    #[test]
    fn decodes_file_uris_with_percent_encoding_and_localhost() {
        assert_eq!(
            decode_file_uri("file:///tmp/my%20icon.png"),
            "/tmp/my icon.png"
        );
        assert_eq!(
            decode_file_uri("file://localhost/tmp/a%2Fb.svg"),
            "/tmp/a/b.svg"
        );
        assert_eq!(decode_file_uri("/plain/path.png"), "/plain/path.png");
    }

    #[test]
    fn extracts_exec_program_handles_quotes_and_wrappers() {
        assert_eq!(
            extract_exec_program("\"/opt/My App/my-app\" %U").as_deref(),
            Some("my-app")
        );
        assert_eq!(
            extract_exec_program("'/opt/My App/my-app' --flag").as_deref(),
            Some("my-app")
        );
        assert_eq!(
            extract_exec_program("/usr/bin/google-chrome-stable %U").as_deref(),
            Some("google-chrome-stable")
        );
        assert_eq!(extract_exec_program("env FOO=1 bar"), None);
    }
}

//! Multi-surface layer-shell configuration, manifest loading (`webbar.json`),
//! and generalized Wayland input-region state across all attachment edges/layers.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

use gtk4::cairo;
use gtk4::ApplicationWindow;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde::Deserialize;

use crate::bridge::{InputRect, SurfaceRuntimeState};

pub const DEFAULT_BAR_HEIGHT: i32 = 34;

/// Screen edge used for bar presets and explicit layer-shell anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum SurfaceEdge {
    Top,
    Bottom,
    Left,
    Right,
}

impl SurfaceEdge {
    pub fn is_horizontal(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }

    pub fn default_anchors(self) -> Vec<Self> {
        match self {
            Self::Top => vec![Self::Top, Self::Left, Self::Right],
            Self::Bottom => vec![Self::Bottom, Self::Left, Self::Right],
            Self::Left => vec![Self::Left, Self::Top, Self::Bottom],
            Self::Right => vec![Self::Right, Self::Top, Self::Bottom],
        }
    }

    pub fn to_layer_edge(self) -> Edge {
        match self {
            Self::Top => Edge::Top,
            Self::Bottom => Edge::Bottom,
            Self::Left => Edge::Left,
            Self::Right => Edge::Right,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// Wayland `zwlr_layer_shell_v1` stacking layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum LayerKind {
    Background,
    Bottom,
    #[default]
    Top,
    Overlay,
}

impl LayerKind {
    pub fn to_layer(self) -> Layer {
        match self {
            Self::Background => Layer::Background,
            Self::Bottom => Layer::Bottom,
            Self::Top => Layer::Top,
            Self::Overlay => Layer::Overlay,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Bottom => "bottom",
            Self::Top => "top",
            Self::Overlay => "overlay",
        }
    }
}

/// Exclusive zone reservation: non-negative pixel count, `-1` / `"ignore"` to
/// render over other exclusive zones, or `"auto"` for GTK4 layer-shell auto sizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExclusiveZone {
    Px(i32),
    Auto,
    Ignore,
}

#[cfg(test)]
#[derive(schemars::JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum ExclusiveZoneSchema {
    /// Pixel thickness to reserve (0 for floating, -1 to ignore other exclusive zones).
    Px(#[schemars(range(min = -1))] i32),
    /// Named mode: `"auto"` or `"ignore"`.
    Keyword(ExclusiveZoneKeyword),
}

#[cfg(test)]
#[derive(schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
enum ExclusiveZoneKeyword {
    Auto,
    Ignore,
}

#[cfg(test)]
impl schemars::JsonSchema for ExclusiveZone {
    fn schema_name() -> String {
        "ExclusiveZone".to_string()
    }

    fn json_schema(generator: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        ExclusiveZoneSchema::json_schema(generator)
    }
}

impl ExclusiveZone {
    pub fn apply_to_window(self, win: &ApplicationWindow) {
        match self {
            Self::Px(px) => win.set_exclusive_zone(px),
            Self::Auto => win.auto_exclusive_zone_enable(),
            Self::Ignore => win.set_exclusive_zone(-1),
        }
    }
}

impl<'de> Deserialize<'de> for ExclusiveZone {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawZone {
            Int(i32),
            Str(String),
        }
        match RawZone::deserialize(deserializer)? {
            RawZone::Int(-1) => Ok(Self::Ignore),
            RawZone::Int(n) if n >= 0 => Ok(Self::Px(n)),
            RawZone::Int(n) => Err(serde::de::Error::custom(format!(
                "invalid exclusive_zone {n} (expected >= 0, -1, \"auto\", or \"ignore\")"
            ))),
            RawZone::Str(s) => match s.to_ascii_lowercase().as_str() {
                "auto" => Ok(Self::Auto),
                "ignore" => Ok(Self::Ignore),
                other => Err(serde::de::Error::custom(format!(
                    "invalid exclusive_zone '{other}' (expected integer, \"auto\", or \"ignore\")"
                ))),
            },
        }
    }
}

/// Wayland layer-shell keyboard interactivity policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum KeyboardPolicy {
    #[default]
    None,
    OnDemand,
    Exclusive,
}

impl KeyboardPolicy {
    pub fn to_keyboard_mode(self) -> KeyboardMode {
        match self {
            Self::None => KeyboardMode::None,
            Self::OnDemand => KeyboardMode::OnDemand,
            Self::Exclusive => KeyboardMode::Exclusive,
        }
    }
}

/// Which outputs receive an instance of this surface: `"all"`, `"primary"`,
/// `"focused"`, or an explicit list of Wayland connector names (e.g. `["DP-1"]`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MonitorTarget {
    #[default]
    All,
    Primary,
    Focused,
    Connectors(Vec<String>),
}

#[cfg(test)]
#[derive(schemars::JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum MonitorTargetSchema {
    Keyword(MonitorTargetKeyword),
    /// Explicit list of Wayland output connector names (e.g. `["DP-1", "HDMI-A-1"]`).
    Connectors(Vec<String>),
}

#[cfg(test)]
#[derive(schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
enum MonitorTargetKeyword {
    All,
    Primary,
    Focused,
}

#[cfg(test)]
impl schemars::JsonSchema for MonitorTarget {
    fn schema_name() -> String {
        "MonitorTarget".to_string()
    }

    fn json_schema(generator: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        MonitorTargetSchema::json_schema(generator)
    }
}

impl<'de> Deserialize<'de> for MonitorTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawTarget {
            Str(String),
            List(Vec<String>),
        }
        match RawTarget::deserialize(deserializer)? {
            RawTarget::Str(s) => match s.to_ascii_lowercase().as_str() {
                "all" => Ok(Self::All),
                "primary" => Ok(Self::Primary),
                "focused" => Ok(Self::Focused),
                other => Err(serde::de::Error::custom(format!(
                    "invalid monitors target '{other}' (expected \"all\", \"primary\", \"focused\", or connector list)"
                ))),
            },
            RawTarget::List(list) => Ok(Self::Connectors(list)),
        }
    }
}

/// Wayland pointer input-masking strategy for a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum InputMode {
    /// Captures the anchored edge strip plus open `popover="manual"` / `[data-webbar-interactive]` rects.
    Bar,
    /// Click-through everywhere except open `popover="manual"` / `[data-webbar-interactive]` rects.
    Interactive,
    /// Captures pointer input across the entire surface bounds.
    Full,
}

/// Outer layer-shell margins in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Margins {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

fn default_true() -> bool {
    true
}

/// Root manifest (`webbar.json`) describing all layer-shell surfaces hosted by `webbar`.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct ShellConfig {
    /// Ordered list of layer-shell surfaces to host (must be non-empty; unique `id`s).
    #[cfg_attr(test, schemars(length(min = 1)))]
    pub surfaces: Vec<SurfaceSpec>,
}

/// Configuration for a single Wayland layer-shell surface in `webbar.json`.
#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct SurfaceSpec {
    /// Unique surface identifier (ASCII alphanumeric, `-`, or `_`), exposed as `window.webbar.surface`.
    #[cfg_attr(test, schemars(regex(pattern = r"^[A-Za-z0-9_-]+$")))]
    pub id: String,
    /// Optional HTML entry path relative to `--dir` or `--url` (defaults to `index.html`).
    #[serde(default)]
    pub entry: Option<String>,
    /// Convenience edge preset (`top`, `bottom`, `left`, `right`); implies edge anchors and `input_mode: "bar"`.
    #[serde(default)]
    pub edge: Option<SurfaceEdge>,
    /// Explicit layer-shell anchors; overrides `edge` defaults. Empty list centers the surface.
    #[serde(default)]
    pub anchors: Option<Vec<SurfaceEdge>>,
    /// Wayland layer-shell stacking layer (default: `"top"`).
    #[serde(default)]
    pub layer: LayerKind,
    /// Explicit surface width in pixels (positive integer).
    #[serde(default)]
    #[cfg_attr(test, schemars(range(min = 1)))]
    pub width: Option<i32>,
    /// Explicit surface height in pixels (positive integer).
    #[serde(default)]
    #[cfg_attr(test, schemars(range(min = 1)))]
    pub height: Option<i32>,
    /// Maximum popover/overlay extent in pixels for `input_mode: "bar"` (defaults to full monitor dimension).
    #[serde(default)]
    #[cfg_attr(test, schemars(range(min = 1)))]
    pub overlay_extent: Option<i32>,
    /// Outer layer-shell margins in pixels.
    #[serde(default)]
    pub margins: Margins,
    /// Exclusive zone reservation (defaults to bar thickness for `bar` surfaces, `0` otherwise).
    #[serde(default)]
    pub exclusive_zone: Option<ExclusiveZone>,
    /// Keyboard interactivity policy when no modal overlay is active (default: `"none"`).
    #[serde(default)]
    pub keyboard: KeyboardPolicy,
    /// Target monitors for this surface (default: `"all"`).
    #[serde(default)]
    pub monitors: MonitorTarget,
    /// Whether the surface is shown on startup (default: `true`).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Pointer input-masking mode (defaults to `"bar"` when `edge` is set, otherwise `"full"`).
    #[serde(default)]
    pub input_mode: Option<InputMode>,
    /// Wayland layer-shell namespace (defaults to `"webbar-<id>"`).
    #[serde(default)]
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputLayout {
    Bar {
        edge: SurfaceEdge,
        thickness: i32,
        fixed_overlay_extent: Option<i32>,
    },
    Interactive,
    Full,
}

#[derive(Debug, Clone)]
pub struct ResolvedSurface {
    pub id: String,
    pub entry: Option<String>,
    pub anchors: Vec<SurfaceEdge>,
    pub layer: LayerKind,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub margins: Margins,
    pub exclusive_zone: ExclusiveZone,
    pub keyboard: KeyboardPolicy,
    pub monitors: MonitorTarget,
    pub visible: bool,
    pub input_layout: InputLayout,
    pub namespace: String,
}

impl ResolvedSurface {
    pub fn resolve(spec: SurfaceSpec) -> Result<Self, String> {
        let id = spec.id.trim().to_string();
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(format!(
                "invalid surface id '{id}' (must be non-empty ASCII alphanumeric, '-', or '_')"
            ));
        }
        for (name, value) in [
            ("width", spec.width),
            ("height", spec.height),
            ("overlay_extent", spec.overlay_extent),
        ] {
            if value.is_some_and(|v| v <= 0) {
                return Err(format!("surface '{id}': {name} must be positive"));
            }
        }

        let anchors = match (spec.anchors, spec.edge) {
            (Some(a), _) => a,
            (None, Some(edge)) => edge.default_anchors(),
            (None, None) => Vec::new(),
        };

        let mode = spec.input_mode.unwrap_or_else(|| {
            if spec.edge.is_some() {
                InputMode::Bar
            } else {
                InputMode::Full
            }
        });

        let bar_edge = spec.edge.unwrap_or_else(|| infer_bar_edge(&anchors));
        let thickness = if bar_edge.is_horizontal() {
            spec.height.unwrap_or(DEFAULT_BAR_HEIGHT)
        } else {
            spec.width.unwrap_or(DEFAULT_BAR_HEIGHT)
        };

        let input_layout = match mode {
            InputMode::Bar => InputLayout::Bar {
                edge: bar_edge,
                thickness,
                fixed_overlay_extent: spec.overlay_extent,
            },
            InputMode::Interactive => InputLayout::Interactive,
            InputMode::Full => InputLayout::Full,
        };

        let exclusive_zone = spec.exclusive_zone.unwrap_or(match input_layout {
            InputLayout::Bar { thickness, .. } => ExclusiveZone::Px(thickness),
            InputLayout::Interactive | InputLayout::Full => ExclusiveZone::Px(0),
        });

        let namespace = spec
            .namespace
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("webbar-{id}"));

        Ok(Self {
            id,
            entry: spec.entry.filter(|s| !s.trim().is_empty()),
            anchors,
            layer: spec.layer,
            width: spec.width,
            height: spec.height,
            margins: spec.margins,
            exclusive_zone,
            keyboard: spec.keyboard,
            monitors: spec.monitors,
            visible: spec.visible,
            input_layout,
            namespace,
        })
    }

    pub fn runtime_state(&self) -> SurfaceRuntimeState {
        SurfaceRuntimeState {
            id: self.id.clone(),
            visible: self.visible,
            layer: self.layer.as_str().to_string(),
            anchors: self.anchors.iter().map(|a| a.as_str().to_string()).collect(),
        }
    }

    pub fn format_defaults_css(&self) -> String {
        let mut vars = Vec::new();
        match self.input_layout {
            InputLayout::Bar {
                edge, thickness, ..
            } => {
                if edge.is_horizontal() {
                    vars.push(format!("--webbar-height: {thickness}px;"));
                } else {
                    vars.push(format!("--webbar-width: {thickness}px;"));
                }
                vars.push(format!("--webbar-thickness: {thickness}px;"));
            }
            InputLayout::Interactive | InputLayout::Full => {
                if let Some(h) = self.height {
                    vars.push(format!("--webbar-height: {h}px;"));
                }
                if let Some(w) = self.width {
                    vars.push(format!("--webbar-width: {w}px;"));
                }
            }
        }
        let vars_block = if vars.is_empty() {
            String::new()
        } else {
            format!(":root {{ {} }}\n", vars.join(" "))
        };
        format!("{vars_block}html, body {{ background: transparent; }}\n")
    }
}

fn infer_bar_edge(anchors: &[SurfaceEdge]) -> SurfaceEdge {
    let has = |e: SurfaceEdge| anchors.contains(&e);
    if has(SurfaceEdge::Top) && !has(SurfaceEdge::Bottom) {
        SurfaceEdge::Top
    } else if has(SurfaceEdge::Bottom) && !has(SurfaceEdge::Top) {
        SurfaceEdge::Bottom
    } else if has(SurfaceEdge::Left) && !has(SurfaceEdge::Right) {
        SurfaceEdge::Left
    } else if has(SurfaceEdge::Right) && !has(SurfaceEdge::Left) {
        SurfaceEdge::Right
    } else {
        SurfaceEdge::Top
    }
}

pub fn parse_shell_config(json: &str) -> Result<Vec<ResolvedSurface>, String> {
    let cfg: ShellConfig =
        serde_json::from_str(json).map_err(|e| format!("invalid webbar.json: {e}"))?;
    if cfg.surfaces.is_empty() {
        return Err("webbar.json must define at least one surface".to_string());
    }
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(cfg.surfaces.len());
    for spec in cfg.surfaces {
        let resolved = ResolvedSurface::resolve(spec)?;
        if !seen.insert(resolved.id.clone()) {
            return Err(format!("duplicate surface id '{}' in webbar.json", resolved.id));
        }
        out.push(resolved);
    }
    Ok(out)
}

pub fn load_shell_config_from_dir(dir: &Path) -> Result<Vec<ResolvedSurface>, String> {
    let path = dir.join("webbar.json");
    let text = fs::read_to_string(&path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            format!(
                "missing required manifest {} (see SURFACES.md / webbar.schema.json)",
                path.display()
            )
        } else {
            format!("failed to read {}: {err}", path.display())
        }
    })?;
    parse_shell_config(&text)
}

/// Fetches `<url>/webbar.json` via HTTP/1.0 from a loopback dev server.
/// Using HTTP/1.0 avoids chunked transfer encoding on dev servers.
pub fn load_shell_config_from_url(url: &str) -> Result<Vec<ResolvedSurface>, String> {
    const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
    let (host_port, host_header) = parse_loopback_http_origin(url).ok_or_else(|| {
        format!(
            "cannot fetch webbar.json from '{url}' (--url must be a loopback http://localhost, 127.0.0.1, or [::1] server)"
        )
    })?;
    let mut stream = TcpStream::connect(&host_port)
        .map_err(|err| format!("failed to connect to {host_port} to fetch /webbar.json: {err}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

    let req = format!(
        "GET /webbar.json HTTP/1.0\r\nHost: {host_header}\r\nConnection: close\r\nAccept: application/json\r\n\r\n"
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|err| format!("failed to request /webbar.json from {host_port}: {err}"))?;
    let mut raw = Vec::new();
    let _ = stream.take(MAX_MANIFEST_BYTES).read_to_end(&mut raw);
    let response = String::from_utf8_lossy(&raw);
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("malformed HTTP response for /webbar.json from {host_port}"))?;
    let status_line = head.lines().next().unwrap_or("");
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    if status_code != 200 {
        return Err(format!(
            "missing required manifest at http://{host_header}/webbar.json (HTTP {status_code}; see SURFACES.md)"
        ));
    }
    let is_json_header = head.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("content-type:") && lower.contains("json")
    });
    let trimmed = body.trim();
    if !is_json_header && !trimmed.starts_with('{') {
        return Err(format!(
            "missing required manifest at http://{host_header}/webbar.json (server returned non-JSON content; in Vite, place webbar.json in public/webbar.json)"
        ));
    }
    parse_shell_config(trimmed)
}

fn parse_loopback_http_origin(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let (host, explicit_port) = if let Some(after_bracket) = authority.strip_prefix('[') {
        let (ip, tail) = after_bracket.split_once(']')?;
        let port = if tail.is_empty() {
            None
        } else {
            Some(tail.strip_prefix(':')?)
        };
        (ip, port)
    } else if let Some((h, p)) = authority.rsplit_once(':') {
        (h, Some(p))
    } else {
        (authority, None)
    };
    if !matches!(host, "localhost" | "127.0.0.1" | "::1") {
        return None;
    }
    let port: u16 = match explicit_port {
        Some(p) => p.parse::<u16>().ok().filter(|&n| n > 0)?,
        None => 80,
    };
    let bracketed = if host == "::1" { "[::1]" } else { host };
    let connect_host = format!("{bracketed}:{port}");
    let host_header = if explicit_port.is_some() {
        format!("{bracketed}:{port}")
    } else {
        bracketed.to_string()
    };
    Some((connect_host, host_header))
}

pub fn sanitize_rects(rects: Vec<InputRect>) -> Vec<(i32, i32, i32, i32)> {
    rects
        .into_iter()
        .map(|r| {
            (
                r.x.round() as i32,
                r.y.round() as i32,
                r.width.round() as i32,
                r.height.round() as i32,
            )
        })
        .filter(|&(_, _, w, h)| w > 0 && h > 0)
        .collect()
}

#[derive(Debug, Clone)]
pub struct WindowInputState {
    layout: InputLayout,
    base_keyboard: KeyboardMode,
    explicit_width: Option<i32>,
    explicit_height: Option<i32>,
    window_width: i32,
    window_height: i32,
    overlay_active: bool,
    interactive_rects: Vec<(i32, i32, i32, i32)>,
}

impl WindowInputState {
    pub fn from_surface(
        surface: &ResolvedSurface,
        monitor_width: i32,
        monitor_height: i32,
    ) -> Self {
        let mut state = Self {
            layout: surface.input_layout,
            base_keyboard: surface.keyboard.to_keyboard_mode(),
            explicit_width: surface.width,
            explicit_height: surface.height,
            window_width: 1,
            window_height: 1,
            overlay_active: false,
            interactive_rects: Vec::new(),
        };
        state.update_monitor_geometry(monitor_width, monitor_height);
        state
    }

    /// Computes `(default_width, default_height)` to pass to GTK4 `set_default_size`.
    /// Always returns positive dimensions so GTK4 Wayland never computes a 0-width toplevel.
    pub fn default_window_size(&self) -> (i32, i32) {
        (self.window_width.max(1), self.window_height.max(1))
    }

    pub fn update_monitor_geometry(
        &mut self,
        monitor_width: i32,
        monitor_height: i32,
    ) -> (i32, i32) {
        let mw = monitor_width.max(1);
        let mh = monitor_height.max(1);
        match self.layout {
            InputLayout::Bar {
                edge,
                thickness,
                fixed_overlay_extent,
            } => {
                if edge.is_horizontal() {
                    self.window_width = mw;
                    self.window_height = fixed_overlay_extent.unwrap_or(mh).max(thickness);
                } else {
                    self.window_width = fixed_overlay_extent.unwrap_or(mw).max(thickness);
                    self.window_height = mh;
                }
            }
            InputLayout::Interactive | InputLayout::Full => {
                self.window_width = self.explicit_width.unwrap_or(mw).max(1);
                self.window_height = self.explicit_height.unwrap_or(mh).max(1);
            }
        }
        self.default_window_size()
    }

    pub fn set_overlay(&mut self, active: bool) {
        self.overlay_active = active;
    }

    pub fn set_interactive_rects(&mut self, rects: Vec<InputRect>) {
        self.interactive_rects = sanitize_rects(rects);
    }

    pub fn reset(&mut self) {
        self.overlay_active = false;
        self.interactive_rects.clear();
    }

    pub fn keyboard_mode(&self) -> KeyboardMode {
        if self.overlay_active {
            KeyboardMode::Exclusive
        } else {
            self.base_keyboard
        }
    }

    pub fn rect_tuples(&self) -> Vec<(i32, i32, i32, i32)> {
        if self.overlay_active {
            return vec![(0, 0, self.window_width, self.window_height)];
        }
        match self.layout {
            InputLayout::Bar {
                edge, thickness, ..
            } => {
                let mut rects = Vec::with_capacity(1 + self.interactive_rects.len());
                let strip = match edge {
                    SurfaceEdge::Top => (0, 0, self.window_width, thickness),
                    SurfaceEdge::Bottom => (
                        0,
                        (self.window_height - thickness).max(0),
                        self.window_width,
                        thickness,
                    ),
                    SurfaceEdge::Left => (0, 0, thickness, self.window_height),
                    SurfaceEdge::Right => (
                        (self.window_width - thickness).max(0),
                        0,
                        thickness,
                        self.window_height,
                    ),
                };
                rects.push(strip);
                rects.extend_from_slice(&self.interactive_rects);
                rects
            }
            InputLayout::Interactive => self.interactive_rects.clone(),
            InputLayout::Full => vec![(0, 0, self.window_width, self.window_height)],
        }
    }

    pub fn cairo_region(&self) -> cairo::Region {
        let rects: Vec<cairo::RectangleInt> = self
            .rect_tuples()
            .into_iter()
            .map(|(x, y, w, h)| cairo::RectangleInt::new(x, y, w, h))
            .collect();
        cairo::Region::create_rectangles(&rects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multi_surface_manifest_with_presets_and_explicit_anchors() {
        let raw = r#"{
            "surfaces": [
                { "id": "bar", "edge": "top", "height": 86 },
                {
                    "id": "dock",
                    "anchors": ["bottom"],
                    "width": 520,
                    "height": 64,
                    "margins": { "bottom": 12 },
                    "exclusive_zone": 0
                },
                {
                    "id": "osd",
                    "anchors": ["top", "bottom", "left", "right"],
                    "layer": "overlay",
                    "exclusive_zone": "ignore",
                    "input_mode": "interactive",
                    "monitors": "focused"
                },
                {
                    "id": "launcher",
                    "layer": "overlay",
                    "width": 640,
                    "height": 420,
                    "keyboard": "exclusive",
                    "monitors": "focused",
                    "visible": false
                }
            ]
        }"#;

        let surfaces = parse_shell_config(raw).expect("valid manifest");
        assert_eq!(surfaces.len(), 4);

        assert_eq!(surfaces[0].id, "bar");
        assert_eq!(
            surfaces[0].anchors,
            vec![SurfaceEdge::Top, SurfaceEdge::Left, SurfaceEdge::Right]
        );
        assert_eq!(surfaces[0].exclusive_zone, ExclusiveZone::Px(86));
        assert_eq!(surfaces[0].namespace, "webbar-bar");

        assert_eq!(surfaces[1].id, "dock");
        assert_eq!(surfaces[1].anchors, vec![SurfaceEdge::Bottom]);
        assert_eq!(surfaces[1].margins.bottom, 12);
        assert_eq!(surfaces[1].exclusive_zone, ExclusiveZone::Px(0));
        assert_eq!(surfaces[1].input_layout, InputLayout::Full);

        assert_eq!(surfaces[2].id, "osd");
        assert_eq!(surfaces[2].layer, LayerKind::Overlay);
        assert_eq!(surfaces[2].exclusive_zone, ExclusiveZone::Ignore);
        assert_eq!(surfaces[2].input_layout, InputLayout::Interactive);
        assert_eq!(surfaces[2].monitors, MonitorTarget::Focused);

        assert_eq!(surfaces[3].id, "launcher");
        assert!(!surfaces[3].visible);
        assert_eq!(surfaces[3].keyboard, KeyboardPolicy::Exclusive);
    }

    #[test]
    fn rejects_invalid_surface_configs() {
        assert!(parse_shell_config(r#"{"surfaces":[]}"#).is_err());
        assert!(parse_shell_config(r#"{"surfaces":[{"id":"bar"},{"id":"bar"}]}"#).is_err());
        assert!(parse_shell_config(r#"{"surfaces":[{"id":"bad&id"}]}"#).is_err());
        assert!(parse_shell_config(r#"{"surfaces":[{"id":"bar","height":0}]}"#).is_err());
        assert!(parse_shell_config(r#"{"surfaces":[{"id":"bar","width":-5}]}"#).is_err());
    }

    #[test]
    fn input_state_computes_regions_for_all_four_bar_edges_and_modes() {
        // Top bar + reset + explicit overlay height + sanitize_rects
        let top = ResolvedSurface::resolve(SurfaceSpec {
            id: "bar".to_string(),
            edge: Some(SurfaceEdge::Top),
            height: Some(86),
            ..Default::default()
        })
        .unwrap();
        let mut s_top = WindowInputState::from_surface(&top, 1920, 1080);
        assert_eq!(s_top.rect_tuples(), vec![(0, 0, 1920, 86)]);
        assert_eq!(s_top.update_monitor_geometry(2560, 1440), (2560, 1440));
        s_top.set_interactive_rects(vec![
            InputRect { x: 2200.4, y: 95.6, width: 340.2, height: 87.8 },
            InputRect { x: 10.0, y: 10.0, width: 0.0, height: 50.0 },
        ]);
        assert_eq!(s_top.rect_tuples(), vec![(0, 0, 2560, 86), (2200, 96, 340, 88)]);
        s_top.reset();
        assert_eq!(s_top.rect_tuples(), vec![(0, 0, 2560, 86)]);
        assert!(top.format_defaults_css().contains("--webbar-height: 86px;"));

        // Bottom bar
        let bottom = ResolvedSurface::resolve(SurfaceSpec {
            id: "bottom".to_string(),
            edge: Some(SurfaceEdge::Bottom),
            height: Some(64),
            ..Default::default()
        })
        .unwrap();
        let mut s = WindowInputState::from_surface(&bottom, 1920, 1080);
        assert_eq!(s.default_window_size(), (1920, 1080));
        assert_eq!(s.rect_tuples(), vec![(0, 1016, 1920, 64)]);
        s.set_overlay(true);
        assert_eq!(s.rect_tuples(), vec![(0, 0, 1920, 1080)]);
        assert_eq!(s.keyboard_mode(), KeyboardMode::Exclusive);

        // Left vertical bar
        let left = ResolvedSurface::resolve(SurfaceSpec {
            id: "left".to_string(),
            edge: Some(SurfaceEdge::Left),
            width: Some(48),
            overlay_extent: Some(400),
            ..Default::default()
        })
        .unwrap();
        let s_left = WindowInputState::from_surface(&left, 1920, 1080);
        assert_eq!(s_left.default_window_size(), (400, 1080));
        assert_eq!(s_left.rect_tuples(), vec![(0, 0, 48, 1080)]);

        // Right vertical bar
        let right = ResolvedSurface::resolve(SurfaceSpec {
            id: "right".to_string(),
            edge: Some(SurfaceEdge::Right),
            width: Some(50),
            ..Default::default()
        })
        .unwrap();
        let s_right = WindowInputState::from_surface(&right, 1920, 1080);
        assert_eq!(s_right.rect_tuples(), vec![(1870, 0, 50, 1080)]);

        // Interactive-only overlay (empty base region until interactive rects arrive)
        let osd = ResolvedSurface::resolve(SurfaceSpec {
            id: "osd".to_string(),
            anchors: Some(vec![
                SurfaceEdge::Top,
                SurfaceEdge::Bottom,
                SurfaceEdge::Left,
                SurfaceEdge::Right,
            ]),
            layer: LayerKind::Overlay,
            exclusive_zone: Some(ExclusiveZone::Ignore),
            monitors: MonitorTarget::Focused,
            visible: true,
            input_mode: Some(InputMode::Interactive),
            ..Default::default()
        })
        .unwrap();
        let mut s_osd = WindowInputState::from_surface(&osd, 1920, 1080);
        assert!(s_osd.rect_tuples().is_empty());
        s_osd.set_interactive_rects(vec![InputRect {
            x: 800.0,
            y: 900.0,
            width: 320.0,
            height: 48.0,
        }]);
        assert_eq!(s_osd.rect_tuples(), vec![(800, 900, 320, 48)]);
        assert_eq!(s_osd.keyboard_mode(), KeyboardMode::None);
    }

    #[test]
    fn loopback_origin_parser_allows_only_valid_loopback_http() {
        assert_eq!(
            parse_loopback_http_origin("http://localhost:5173/index.html"),
            Some(("localhost:5173".to_string(), "localhost:5173".to_string()))
        );
        assert_eq!(
            parse_loopback_http_origin("http://127.0.0.1:8080"),
            Some(("127.0.0.1:8080".to_string(), "127.0.0.1:8080".to_string()))
        );
        assert_eq!(
            parse_loopback_http_origin("http://[::1]:5173/app"),
            Some(("[::1]:5173".to_string(), "[::1]:5173".to_string()))
        );
        assert_eq!(
            parse_loopback_http_origin("http://[::1]/app"),
            Some(("[::1]:80".to_string(), "[::1]".to_string()))
        );
        assert_eq!(parse_loopback_http_origin("http://[::1]\r\nX-Injected: 1"), None);
        assert_eq!(parse_loopback_http_origin("http://localhost:99999"), None);
        assert_eq!(parse_loopback_http_origin("https://example.com"), None);
    }

    #[test]
    fn export_bindings_webbar_schema() {
        let schema = schemars::schema_for!(ShellConfig);
        let json = format!(
            "{}\n",
            serde_json::to_string_pretty(&schema).expect("serialize webbar.json schema")
        );
        let out_path = std::env::var("WEBBAR_SCHEMA_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("webbar.schema.json"));
        if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).expect("create schema output dir");
        }
        fs::write(&out_path, json).expect("write webbar.schema.json");
    }
}

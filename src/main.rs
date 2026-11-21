mod audio;
mod backlight;
mod bluetooth;
mod bridge;
mod dbus_bridge;
mod dbus_json;
mod network;
mod notifications;
mod process_bridge;
mod sway_ipc;
mod sway_state;
mod theme;
mod tray;
mod ws_bridge;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gtk4::cairo;
use gtk4::gdk::{self, Monitor, RGBA};
use gtk4::gio::{self, MemoryInputStream};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use webkit6::prelude::*;
use webkit6::{
    LoadEvent, UserContentInjectedFrames, UserContentManager, UserScript,
    UserScriptInjectionTime, UserStyleLevel, UserStyleSheet, WebContext, WebView,
};

use crate::audio::AudioController;
use crate::backlight::BacklightController;
use crate::bluetooth::BluetoothController;
use crate::bridge::{BridgeEvent, BridgeRequest, SendEvent, SnapshotCache};
use crate::dbus_bridge::DbusBridge;
use crate::notifications::NotificationStore;
use crate::process_bridge::ProcessBridge;

const DEFAULT_BAR_HEIGHT: i32 = 34;
const PRELOAD_JS: &str = include_str!("../generated/preload.js");

struct WindowHandle {
    window: glib::WeakRef<ApplicationWindow>,
    input_state: Rc<RefCell<WindowInputState>>,
}

thread_local! {
    static WEB_VIEWS: RefCell<Vec<glib::WeakRef<WebView>>> = const { RefCell::new(Vec::new()) };
    static WINDOWS: RefCell<HashMap<String, WindowHandle>> = RefCell::new(HashMap::new());
}

#[derive(Clone)]
enum Source {
    Dir(PathBuf),
    Url(String),
}

#[derive(Clone)]
struct Config {
    source: Option<Source>,
    height: i32,
    overlay_height: Option<i32>,
    replace: bool,
    devtools: bool,
    ws_bridge: Option<u16>,
    ws_token: Option<String>,
}

impl Config {
    fn from_args() -> Result<Self, String> {
        Self::from_iter(std::env::args().skip(1))
    }

    fn from_iter<I: IntoIterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut dir: Option<PathBuf> = None;
        let mut url: Option<String> = None;
        let mut height: i32 = DEFAULT_BAR_HEIGHT;
        let mut overlay_height: Option<i32> = None;
        let mut replace = false;
        let mut devtools = false;
        let mut ws_bridge: Option<u16> = None;
        let mut ws_token: Option<String> = None;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--dir" => {
                    let val = args.next().ok_or("missing value for --dir")?;
                    dir = Some(PathBuf::from(val));
                }
                "--url" => {
                    let val = args.next().ok_or("missing value for --url")?;
                    url = Some(val);
                }
                "--height" => {
                    let val = args.next().ok_or("missing value for --height")?;
                    height = val
                        .parse::<i32>()
                        .ok()
                        .filter(|&h| h > 0)
                        .ok_or("invalid --height (must be a positive integer)")?;
                }
                "--overlay-height" => {
                    let val = args.next().ok_or("missing value for --overlay-height")?;
                    let parsed = val
                        .parse::<i32>()
                        .ok()
                        .filter(|&h| h > 0)
                        .ok_or("invalid --overlay-height (must be a positive integer)")?;
                    overlay_height = Some(parsed);
                }
                "--ws-bridge" => {
                    let val = args.next().ok_or("missing value for --ws-bridge")?;
                    let parsed = val
                        .parse::<u16>()
                        .ok()
                        .filter(|&p| p > 0)
                        .ok_or("invalid --ws-bridge (must be a valid TCP port)")?;
                    ws_bridge = Some(parsed);
                }
                "--ws-token" => {
                    let val = args.next().ok_or("missing value for --ws-token")?;
                    if val.is_empty() {
                        return Err("--ws-token must not be empty".to_string());
                    }
                    ws_token = Some(val);
                }
                "--replace" => replace = true,
                "--devtools" => devtools = true,
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        let source = match (dir, url) {
            (Some(d), None) => Some(Source::Dir(d)),
            (None, Some(u)) => Some(Source::Url(u)),
            (None, None) if ws_bridge.is_some() => None,
            _ => {
                return Err(
                    "pass either --dir <path> or --url <url> (or --ws-bridge <port>)".to_string(),
                )
            }
        };
        Ok(Self {
            source,
            height,
            overlay_height,
            replace,
            devtools,
            ws_bridge,
            ws_token,
        })
    }

    fn application_flags(&self) -> gio::ApplicationFlags {
        let mut flags = gio::ApplicationFlags::ALLOW_REPLACEMENT;
        if self.replace {
            flags |= gio::ApplicationFlags::REPLACE;
        }
        flags
    }
}

struct HostServices {
    notifications: NotificationStore,
    audio: AudioController,
    bluetooth: BluetoothController,
    backlight: BacklightController,
    sway: sway_ipc::SwayCommands,
    tray_menus: tray::TrayMenuWatcher,
}

impl HostServices {
    fn start(send_event: SendEvent, claim_dbus_servers: bool) -> (Arc<Self>, theme::ThemeState) {
        let initial_theme = theme::start_theme_watcher(send_event.clone());
        let notifications = NotificationStore::new(send_event.clone());
        let tray_menus = tray::TrayMenuWatcher::new(send_event.clone());
        if claim_dbus_servers {
            notifications::spawn_server(notifications.clone());
            tray::spawn_server(send_event.clone());
        } else {
            tray::spawn_observer(send_event.clone());
        }
        sway_ipc::spawn(send_event.clone());
        let audio = audio::spawn(send_event.clone());
        let bluetooth = bluetooth::spawn(send_event.clone());
        network::spawn(send_event.clone());
        let backlight = backlight::spawn(send_event);

        (
            Arc::new(Self {
                notifications,
                audio,
                bluetooth,
                backlight,
                sway: sway_ipc::SwayCommands::spawn(),
                tray_menus,
            }),
            initial_theme,
        )
    }

    fn dispatch(
        &self,
        output: Option<&str>,
        req: BridgeRequest,
        dbus: &DbusBridge,
        processes: &ProcessBridge,
        reply_send: &SendEvent,
    ) {
        match req {
            BridgeRequest::DbusCall {
                id,
                bus,
                destination,
                path,
                interface,
                member,
                signature,
                body,
            } => dbus.call(id, bus, destination, path, interface, member, signature, body),
            BridgeRequest::DbusMatch {
                id,
                bus,
                sender,
                path,
                interface,
                member,
            } => dbus.match_signals(
                id,
                bus,
                crate::dbus_bridge::MatchCriteria {
                    sender,
                    path,
                    interface,
                    member,
                },
            ),
            BridgeRequest::DbusUnwatch { id } => dbus.unwatch(id),
            BridgeRequest::DismissNotification { id } => {
                self.notifications.dismiss(id);
            }
            BridgeRequest::InvokeNotificationAction { id, action_key } => {
                self.notifications.invoke_action(id, &action_key);
            }
            BridgeRequest::ClearNotifications => {
                self.notifications.clear_all();
            }
            BridgeRequest::ProcessSpawn { id, argv, cwd, env } => {
                processes.spawn(id, argv, cwd, env);
            }
            BridgeRequest::ProcessStreamSpawn { id, argv, cwd, env } => {
                processes.spawn_stream(id, argv, cwd, env);
            }
            BridgeRequest::ProcessKill { id, signal } => {
                processes.kill(id, signal);
            }
            BridgeRequest::AudioSetVolume {
                id,
                device_id,
                volume,
                kind,
            } => self
                .audio
                .set_volume(id, device_id, volume, kind, reply_send.clone()),
            BridgeRequest::AudioSetMuted {
                id,
                device_id,
                muted,
                kind,
            } => self
                .audio
                .set_muted(id, device_id, muted, kind, reply_send.clone()),
            BridgeRequest::AudioSetDefault {
                id,
                device_id,
                kind,
            } => {
                self.audio
                    .set_default(id, device_id, kind, reply_send.clone());
            }
            BridgeRequest::AudioSetStreamVolume {
                id,
                stream_id,
                volume,
            } => self
                .audio
                .set_stream_volume(id, stream_id, volume, reply_send.clone()),
            BridgeRequest::AudioSetStreamMuted {
                id,
                stream_id,
                muted,
            } => self
                .audio
                .set_stream_muted(id, stream_id, muted, reply_send.clone()),
            BridgeRequest::BluetoothSetPowered { id, path, powered } => {
                self.bluetooth
                    .set_powered(id, path, powered, reply_send.clone());
            }
            BridgeRequest::BluetoothConnect { id, path } => {
                self.bluetooth.connect(id, path, reply_send.clone());
            }
            BridgeRequest::BluetoothDisconnect { id, path } => {
                self.bluetooth.disconnect(id, path, reply_send.clone());
            }
            BridgeRequest::BacklightSet {
                id,
                percent,
                display_id,
            } => {
                self.backlight
                    .set_brightness(id, percent, display_id, reply_send.clone());
            }
            BridgeRequest::SwayCommand { id, command } => {
                self.sway.run(id, command, reply_send.clone());
            }
            BridgeRequest::TrayActivate { bus, path } => {
                tray::activate(bus, path);
            }
            BridgeRequest::TrayGetMenu { id, bus, menu_path } => {
                tray::get_menu(id, bus, menu_path, reply_send.clone());
            }
            BridgeRequest::TrayWatchMenu { bus, menu_path } => {
                self.tray_menus.watch(bus, menu_path);
            }
            BridgeRequest::TrayUnwatchMenu { bus, menu_path } => {
                self.tray_menus.unwatch(&bus, &menu_path);
            }
            BridgeRequest::TrayMenuEvent {
                bus,
                menu_path,
                item_id,
            } => {
                tray::menu_event(bus, menu_path, item_id);
            }
            BridgeRequest::SetInputRegion { rects } => {
                if let Some(output) = output.map(str::to_owned) {
                    glib::idle_add_once(move || {
                        WINDOWS.with(|cell| {
                            if let Some(handle) = cell.borrow().get(&output) {
                                handle.input_state.borrow_mut().set_custom_rects(rects);
                                if let Some(win) = handle.window.upgrade() {
                                    apply_window_input_state(&win, &handle.input_state.borrow());
                                }
                            }
                        });
                    });
                }
            }
            BridgeRequest::SetOverlay { active } => {
                if let Some(output) = output.map(str::to_owned) {
                    glib::idle_add_once(move || {
                        WINDOWS.with(|cell| {
                            if let Some(handle) = cell.borrow().get(&output) {
                                handle.input_state.borrow_mut().set_overlay(active);
                                if let Some(win) = handle.window.upgrade() {
                                    apply_window_input_state(&win, &handle.input_state.borrow());
                                }
                            }
                        });
                    });
                }
            }
        }
    }
}

fn start_ws_bridge(config: &Config, claim_dbus_servers: bool) -> (u16, String, theme::ThemeState) {
    let snapshots = SnapshotCache::new();
    let broadcaster = ws_bridge::WsBroadcaster::new();
    let send_event: SendEvent = {
        let snapshots = snapshots.clone();
        let broadcaster = broadcaster.clone();
        Arc::new(move |event: BridgeEvent| {
            snapshots.record(&event);
            match serde_json::to_string(&event) {
                Ok(json) => broadcaster.broadcast_json(&json),
                Err(err) => eprintln!("webbar: failed to serialize event: {err}"),
            }
        })
    };

    let (services, initial_theme) = HostServices::start(send_event, claim_dbus_servers);
    let requested_port = config.ws_bridge.unwrap_or(0);
    let token = config
        .ws_token
        .clone()
        .unwrap_or_else(ws_bridge::generate_token);
    let bound_port = match ws_bridge::spawn(
        requested_port,
        config.ws_bridge.is_some(),
        token.clone(),
        broadcaster,
        snapshots,
        move |output, req, client_dbus, client_procs, client_send| {
            services.dispatch(output, req, client_dbus, client_procs, client_send);
        },
    ) {
        Ok(port) => port,
        Err(err) => {
            eprintln!("webbar: ws-bridge failed to bind 127.0.0.1:{requested_port}: {err}");
            std::process::exit(1);
        }
    };
    (bound_port, token, initial_theme)
}

fn main() {
    let config = match Config::from_args() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("webbar: {err}\nUsage: webbar (--dir <path> | --url <url>) [--height <px>] [--overlay-height <px>] [--replace] [--devtools]");
            std::process::exit(1);
        }
    };

    if config.source.is_none() {
        let _ = gtk4::init();
        let _ = start_ws_bridge(&config, false);
        let main_loop = glib::MainLoop::new(None, false);
        main_loop.run();
        return;
    }

    let app = Application::builder()
        .application_id("dev.local.webbar")
        .flags(config.application_flags())
        .build();

    app.connect_activate(move |app| activate(app, &config));
    app.run_with_args::<&str>(&[]);
}

fn activate(app: &Application, config: &Config) {
    if let Some(Source::Dir(dir)) = &config.source {
        register_webbar_scheme(dir.clone());
    }

    let (ws_port, ws_token, initial_theme) = start_ws_bridge(config, true);

    let Some(display) = gdk::Display::default() else {
        eprintln!("webbar: no GDK display available");
        return;
    };

    let css = gtk4::CssProvider::new();
    css.load_from_data(
        "window, window.background { background-color: transparent; background-image: none; }",
    );
    gtk4::style_context_add_provider_for_display(
        &display,
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let monitors = display.monitors();
    let windows: Rc<RefCell<HashMap<glib::Object, ApplicationWindow>>> =
        Rc::new(RefCell::new(HashMap::new()));

    let initial_theme_css = theme::format_root_css(&initial_theme);
    let sync_monitors = {
        let app = app.clone();
        let config = config.clone();
        let windows = windows.clone();
        let monitors = monitors.clone();
        move || {
            let mut active_objects = Vec::new();
            for i in 0..monitors.n_items() {
                let Some(obj) = monitors.item(i) else {
                    continue;
                };
                active_objects.push(obj.clone());
                if windows.borrow().contains_key(&obj) {
                    continue;
                }
                if let Ok(monitor) = obj.clone().downcast::<Monitor>() {
                    let win = create_bar_window(
                        &app,
                        &monitor,
                        &config,
                        ws_port,
                        &ws_token,
                        &initial_theme_css,
                    );
                    windows.borrow_mut().insert(obj, win);
                }
            }

            let stale: Vec<glib::Object> = windows
                .borrow()
                .keys()
                .filter(|obj| !active_objects.contains(obj))
                .cloned()
                .collect();
            for obj in stale {
                if let Some(win) = windows.borrow_mut().remove(&obj) {
                    win.close();
                }
            }
            WINDOWS.with(|cell| {
                cell.borrow_mut().retain(|_, h| h.window.upgrade().is_some());
            });
        }
    };

    sync_monitors();
    monitors.connect_items_changed(move |_, _, _, _| {
        sync_monitors();
    });

    let devtools_action = gio::SimpleAction::new("devtools", None);
    devtools_action.connect_activate(|_, _| {
        open_devtools_first();
    });
    app.add_action(&devtools_action);
}

fn open_devtools(web_view: &WebView) {
    if let Some(inspector) = web_view.inspector() {
        inspector.show();
        inspector.detach();
    }
}

fn open_devtools_first() {
    WEB_VIEWS.with(|cell| {
        let mut views = cell.borrow_mut();
        views.retain(|weak| weak.upgrade().is_some());
        if let Some(view) = views.first().and_then(|weak| weak.upgrade()) {
            open_devtools(&view);
        }
    });
}

#[derive(Debug, PartialEq, Eq)]
enum AssetError {
    Forbidden,
    NotFound,
}

fn register_webbar_scheme(dir: PathBuf) {
    let Some(context) = WebContext::default() else {
        return;
    };
    let base_dir = fs::canonicalize(&dir).unwrap_or(dir);
    context.register_uri_scheme("webbar", move |request| {
        let uri = request.uri().map(|u| u.to_string()).unwrap_or_default();
        let (disk_path, mime) = match resolve_asset_path(&base_dir, &uri) {
            Ok(resolved) => resolved,
            Err(AssetError::Forbidden) => {
                let stream = MemoryInputStream::from_bytes(&glib::Bytes::from_static(b"Forbidden"));
                request.finish(&stream, -1, Some("text/plain"));
                return;
            }
            Err(AssetError::NotFound) => {
                let stream = MemoryInputStream::from_bytes(&glib::Bytes::from_static(b"Not Found"));
                request.finish(&stream, -1, Some("text/plain"));
                return;
            }
        };

        match fs::read(&disk_path) {
            Ok(bytes) => {
                let stream = MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
                request.finish(&stream, -1, Some(mime));
            }
            Err(err) => {
                eprintln!("webbar: failed to read {}: {err}", disk_path.display());
                let stream = MemoryInputStream::from_bytes(&glib::Bytes::from_static(b"Not Found"));
                request.finish(&stream, -1, Some("text/plain"));
            }
        }
    });
}

fn resolve_asset_path(
    canonical_base: &Path,
    uri: &str,
) -> Result<(PathBuf, &'static str), AssetError> {
    let path_part = uri
        .strip_prefix("webbar://app/")
        .unwrap_or("index.html")
        .split(['?', '#'])
        .next()
        .unwrap_or("index.html");
    let rel = if path_part.is_empty() {
        "index.html"
    } else {
        path_part
    };

    let rel_path = Path::new(rel);
    if rel_path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(AssetError::Forbidden);
    }

    let joined = canonical_base.join(rel_path);
    let canonical = fs::canonicalize(&joined).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AssetError::NotFound
        } else {
            AssetError::Forbidden
        }
    })?;
    if !canonical.starts_with(canonical_base) {
        return Err(AssetError::Forbidden);
    }
    Ok((canonical, mime_for(rel)))
}

fn mime_for(rel: &str) -> &'static str {
    let ext = Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("html" | "htm") => "text/html",
        Some("js" | "mjs") => "application/javascript",
        Some("css") => "text/css",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    BarOnly,
    Overlay,
    CustomRects(Vec<(i32, i32, i32, i32)>),
}

#[derive(Debug, Clone)]
struct WindowInputState {
    bar_height: i32,
    fixed_overlay_height: Option<i32>,
    monitor_width: i32,
    overlay_height: i32,
    mode: InputMode,
}

impl WindowInputState {
    fn new(
        bar_height: i32,
        fixed_overlay_height: Option<i32>,
        monitor_width: i32,
        monitor_height: i32,
    ) -> Self {
        let monitor_width = monitor_width.max(1);
        let overlay_height = fixed_overlay_height
            .unwrap_or(monitor_height)
            .max(bar_height);
        Self {
            bar_height,
            fixed_overlay_height,
            monitor_width,
            overlay_height,
            mode: InputMode::BarOnly,
        }
    }

    fn update_monitor_geometry(&mut self, monitor_width: i32, monitor_height: i32) -> i32 {
        self.monitor_width = monitor_width.max(1);
        self.overlay_height = self
            .fixed_overlay_height
            .unwrap_or(monitor_height)
            .max(self.bar_height);
        self.overlay_height
    }

    fn set_overlay(&mut self, active: bool) {
        self.mode = if active {
            InputMode::Overlay
        } else {
            InputMode::BarOnly
        };
    }

    fn set_custom_rects(&mut self, rects: Vec<crate::bridge::InputRect>) {
        let filtered: Vec<(i32, i32, i32, i32)> = rects
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
            .collect();
        self.mode = if filtered.is_empty() {
            InputMode::BarOnly
        } else {
            InputMode::CustomRects(filtered)
        };
    }

    fn reset(&mut self) {
        self.mode = InputMode::BarOnly;
    }

    fn keyboard_mode(&self) -> KeyboardMode {
        match self.mode {
            InputMode::BarOnly => KeyboardMode::None,
            InputMode::Overlay => KeyboardMode::Exclusive,
            InputMode::CustomRects(_) => KeyboardMode::OnDemand,
        }
    }

    fn rect_tuples(&self) -> Vec<(i32, i32, i32, i32)> {
        match &self.mode {
            InputMode::BarOnly => vec![(0, 0, self.monitor_width, self.bar_height)],
            InputMode::Overlay => vec![(0, 0, self.monitor_width, self.overlay_height)],
            InputMode::CustomRects(rects) => rects.clone(),
        }
    }

    fn cairo_region(&self) -> cairo::Region {
        let rects: Vec<cairo::RectangleInt> = self
            .rect_tuples()
            .into_iter()
            .map(|(x, y, w, h)| cairo::RectangleInt::new(x, y, w, h))
            .collect();
        cairo::Region::create_rectangles(&rects)
    }
}

fn apply_window_input_state(win: &ApplicationWindow, state: &WindowInputState) {
    let target_kb = state.keyboard_mode();
    if win.keyboard_mode() != target_kb {
        win.set_keyboard_mode(target_kb);
    }
    if let Some(surface) = win.surface() {
        surface.set_input_region(&state.cairo_region());
    }
}

fn format_bar_defaults_css(bar_height: i32) -> String {
    format!(
        ":root {{ --webbar-height: {bar_height}px; }}\nhtml, body {{ background: transparent; }}\n"
    )
}

fn create_bar_window(
    app: &Application,
    monitor: &Monitor,
    config: &Config,
    ws_port: u16,
    ws_token: &str,
    initial_theme_css: &str,
) -> ApplicationWindow {
    let geom = monitor.geometry();
    let input_state = Rc::new(RefCell::new(WindowInputState::new(
        config.height,
        config.overlay_height,
        geom.width(),
        geom.height(),
    )));
    let initial_overlay_height = input_state.borrow().overlay_height;

    let window = ApplicationWindow::builder()
        .application(app)
        .title("webbar")
        .default_height(initial_overlay_height)
        .build();

    window.init_layer_shell();
    window.set_monitor(Some(monitor));
    window.set_layer(Layer::Top);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);
    window.set_exclusive_zone(config.height);
    window.set_keyboard_mode(KeyboardMode::None);

    {
        let input_state = input_state.clone();
        window.connect_realize(move |win| {
            if let Some(surface) = win.surface() {
                surface.set_input_region(&input_state.borrow().cairo_region());
                let input_state = input_state.clone();
                surface.connect_layout(move |surface, _width, _height| {
                    surface.set_input_region(&input_state.borrow().cairo_region());
                });
            }
        });
    }

    {
        let input_state = input_state.clone();
        window.connect_map(move |win| {
            apply_window_input_state(win, &input_state.borrow());
        });
    }

    {
        let input_state = input_state.clone();
        let window_weak = window.downgrade();
        monitor.connect_geometry_notify(move |mon| {
            let g = mon.geometry();
            let next_h = input_state
                .borrow_mut()
                .update_monitor_geometry(g.width(), g.height());
            if let Some(win) = window_weak.upgrade() {
                win.set_default_size(-1, next_h);
                apply_window_input_state(&win, &input_state.borrow());
            }
        });
    }

    let connector = monitor
        .connector()
        .map(|s| s.to_string())
        .unwrap_or_default();

    WINDOWS.with(|cell| {
        cell.borrow_mut().insert(
            connector.clone(),
            WindowHandle {
                window: window.downgrade(),
                input_state: input_state.clone(),
            },
        );
    });

    let ucm = UserContentManager::new();
    let bar_defaults_css = format_bar_defaults_css(config.height);
    ucm.add_style_sheet(&UserStyleSheet::new(
        &bar_defaults_css,
        UserContentInjectedFrames::TopFrame,
        UserStyleLevel::User,
        &[],
        &[],
    ));
    ucm.add_style_sheet(&UserStyleSheet::new(
        initial_theme_css,
        UserContentInjectedFrames::TopFrame,
        UserStyleLevel::Author,
        &[],
        &[],
    ));
    ucm.add_script(&UserScript::new(
        PRELOAD_JS,
        UserContentInjectedFrames::TopFrame,
        UserScriptInjectionTime::Start,
        &[],
        &[],
    ));

    let related_view = WEB_VIEWS.with(|cell| {
        let mut views = cell.borrow_mut();
        views.retain(|weak| weak.upgrade().is_some());
        views.first().and_then(|weak| weak.upgrade())
    });
    let mut web_view_builder = WebView::builder().user_content_manager(&ucm);
    if let Some(related) = &related_view {
        web_view_builder = web_view_builder.related_view(related);
    }
    let web_view = web_view_builder.build();
    if let Some(settings) = WebViewExt::settings(&web_view) {
        settings.set_enable_developer_extras(true);
    }
    if let Some(inspector) = web_view.inspector() {
        inspector.connect_attach(|inspector| {
            let inspector = inspector.clone();
            glib::idle_add_local_once(move || {
                inspector.detach();
            });
            false
        });
    }
    web_view.set_background_color(&RGBA::new(0.0, 0.0, 0.0, 0.0));
    web_view.set_vexpand(true);
    web_view.set_hexpand(true);

    let key_controller = gtk4::EventControllerKey::new();
    let web_view_weak = web_view.downgrade();
    key_controller.connect_key_pressed(move |_, key, _, mods| {
        let is_f12 = key == gdk::Key::F12;
        let is_ctrl_shift_i = (key == gdk::Key::I || key == gdk::Key::i)
            && mods.contains(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK);
        if is_f12 || is_ctrl_shift_i {
            if let Some(view) = web_view_weak.upgrade() {
                open_devtools(&view);
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    web_view.add_controller(key_controller);

    WEB_VIEWS.with(|cell| {
        cell.borrow_mut().push(web_view.downgrade());
    });

    {
        let input_state = input_state.clone();
        let window_weak = window.downgrade();
        web_view.connect_load_changed(move |_view, event| {
            if event == LoadEvent::Committed {
                input_state.borrow_mut().reset();
                if let Some(win) = window_weak.upgrade() {
                    apply_window_input_state(&win, &input_state.borrow());
                }
            }
        });
    }

    let query = format!("output={connector}&wsPort={ws_port}&wsToken={ws_token}");
    let uri = match &config.source {
        Some(Source::Url(url)) => {
            let sep = if url.contains('?') { '&' } else { '?' };
            format!("{url}{sep}{query}")
        }
        Some(Source::Dir(_)) => format!("webbar://app/index.html?{query}"),
        None => unreachable!("headless ws-bridge mode does not create bar windows"),
    };
    web_view.load_uri(&uri);

    window.set_child(Some(&web_view));
    window.present();
    if config.devtools {
        open_devtools(&web_view);
    }
    window
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Config, String> {
        Config::from_iter(args.iter().map(|s| (*s).to_string()))
    }

    #[test]
    fn defaults_allow_replacement_without_replace() {
        let cfg = parse(&["--dir", "/tmp/ui"]).unwrap();
        assert!(!cfg.replace);
        assert!(!cfg.devtools);
        assert_eq!(cfg.height, DEFAULT_BAR_HEIGHT);
        assert_eq!(cfg.overlay_height, None);
        assert_eq!(
            cfg.application_flags(),
            gio::ApplicationFlags::ALLOW_REPLACEMENT
        );
    }

    #[test]
    fn devtools_flag_enables_devtools() {
        let cfg = parse(&["--dir", "/tmp/ui", "--devtools"]).unwrap();
        assert!(cfg.devtools);
    }

    #[test]
    fn replace_flag_sets_replace_application_flag() {
        let cfg = parse(&["--url", "http://localhost:8080", "--height", "86", "--replace"]).unwrap();
        assert!(cfg.replace);
        assert_eq!(cfg.height, 86);
        assert_eq!(
            cfg.application_flags(),
            gio::ApplicationFlags::ALLOW_REPLACEMENT | gio::ApplicationFlags::REPLACE
        );
    }

    #[test]
    fn rejects_unknown_argument() {
        let err = parse(&["--dir", "/tmp/ui", "-b", "bar-0"])
            .err()
            .expect("expected error");
        assert_eq!(err, "unknown argument: -b");
    }

    #[test]
    fn window_input_state_transitions_and_resets_cleanly() {
        let mut state = WindowInputState::new(86, None, 1920, 1080);
        assert_eq!(state.overlay_height, 1080);
        assert_eq!(state.keyboard_mode(), KeyboardMode::None);
        assert_eq!(state.rect_tuples(), vec![(0, 0, 1920, 86)]);

        // Activating overlay expands input to full monitor height with Exclusive keyboard focus
        state.set_overlay(true);
        assert_eq!(state.keyboard_mode(), KeyboardMode::Exclusive);
        assert_eq!(state.rect_tuples(), vec![(0, 0, 1920, 1080)]);

        // Monitor geometry changes update active overlay bounds unless fixed --overlay-height is set
        let next_h = state.update_monitor_geometry(2560, 1440);
        assert_eq!(next_h, 1440);
        assert_eq!(state.rect_tuples(), vec![(0, 0, 2560, 1440)]);

        // Page reload (reset) restores bar-only input and releases keyboard focus
        state.reset();
        assert_eq!(state.keyboard_mode(), KeyboardMode::None);
        assert_eq!(state.rect_tuples(), vec![(0, 0, 2560, 86)]);

        // Custom rects round floats, filter invalid dimensions, and fall back to BarOnly when empty
        state.set_custom_rects(vec![
            crate::bridge::InputRect {
                x: 0.0,
                y: 0.0,
                width: 2560.0,
                height: 86.0,
            },
            crate::bridge::InputRect {
                x: 100.4,
                y: 93.6,
                width: 260.2,
                height: 179.8,
            },
            crate::bridge::InputRect {
                x: 10.0,
                y: 10.0,
                width: 0.0,
                height: 50.0,
            },
        ]);
        assert_eq!(state.keyboard_mode(), KeyboardMode::OnDemand);
        assert_eq!(
            state.rect_tuples(),
            vec![(0, 0, 2560, 86), (100, 94, 260, 180)]
        );

        state.set_custom_rects(Vec::new());
        assert_eq!(state.keyboard_mode(), KeyboardMode::None);
        assert_eq!(state.rect_tuples(), vec![(0, 0, 2560, 86)]);
    }

    #[test]
    fn window_input_state_respects_explicit_overlay_height() {
        let mut state = WindowInputState::new(86, Some(500), 1920, 1080);
        assert_eq!(state.overlay_height, 500);
        state.set_overlay(true);
        assert_eq!(state.rect_tuples(), vec![(0, 0, 1920, 500)]);

        let next_h = state.update_monitor_geometry(3840, 2160);
        assert_eq!(next_h, 500);
        assert_eq!(state.rect_tuples(), vec![(0, 0, 3840, 500)]);
    }

    #[test]
    fn mime_for_maps_common_web_assets_and_safe_default() {
        assert_eq!(mime_for("index.html"), "text/html");
        assert_eq!(mime_for("app.js"), "application/javascript");
        assert_eq!(mime_for("mod.mjs"), "application/javascript");
        assert_eq!(mime_for("styles.css"), "text/css");
        assert_eq!(mime_for("data.json"), "application/json");
        assert_eq!(mime_for("font.woff2"), "font/woff2");
        assert_eq!(mime_for("icon.webp"), "image/webp");
        assert_eq!(mime_for("core.wasm"), "application/wasm");
        assert_eq!(mime_for("unknown.bin"), "application/octet-stream");
    }

    #[test]
    fn resolve_asset_path_blocks_traversal_and_resolves_valid_assets() {
        let dir = std::env::temp_dir().join(format!("webbar-asset-test-{}", std::process::id()));
        let sub = dir.join("assets");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.join("index.html"), b"ok").unwrap();
        fs::write(sub.join("app.js"), b"console.log(1)").unwrap();

        let base = fs::canonicalize(&dir).unwrap();

        let (index_path, index_mime) =
            resolve_asset_path(&base, "webbar://app/?output=DP-1#top").unwrap();
        assert_eq!(index_path, base.join("index.html"));
        assert_eq!(index_mime, "text/html");

        let (js_path, js_mime) =
            resolve_asset_path(&base, "webbar://app/assets/app.js?v=1").unwrap();
        assert_eq!(js_path, base.join("assets/app.js"));
        assert_eq!(js_mime, "application/javascript");

        assert_eq!(
            resolve_asset_path(&base, "webbar://app/../etc/passwd"),
            Err(AssetError::Forbidden)
        );
        assert_eq!(
            resolve_asset_path(&base, "webbar://app/assets/../../etc/passwd"),
            Err(AssetError::Forbidden)
        );
        assert_eq!(
            resolve_asset_path(&base, "webbar://app//etc/passwd"),
            Err(AssetError::Forbidden)
        );
        assert_eq!(
            resolve_asset_path(&base, "webbar://app/favicon.ico"),
            Err(AssetError::NotFound)
        );

        let outside = std::env::temp_dir().join(format!("webbar-outside-{}.txt", std::process::id()));
        fs::write(&outside, b"secret").unwrap();
        let escape_link = dir.join("escape.txt");
        let _ = fs::remove_file(&escape_link);
        std::os::unix::fs::symlink(&outside, &escape_link).unwrap();

        assert_eq!(
            resolve_asset_path(&base, "webbar://app/escape.txt"),
            Err(AssetError::Forbidden)
        );

        let _ = fs::remove_file(&outside);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_bar_defaults_css_sets_height_var_and_transparent_root() {
        let css = format_bar_defaults_css(42);
        assert!(css.contains("--webbar-height: 42px;"));
        assert!(css.contains("html, body { background: transparent; }"));
    }
}

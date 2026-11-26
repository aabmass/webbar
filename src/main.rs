mod audio;
mod backlight;
mod bluetooth;
mod bridge;
mod dbus_bridge;
mod dbus_json;
mod greetd;
mod icons;
mod network;
mod notifications;
mod process_bridge;
mod surface;
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
use std::sync::{Arc, Mutex};

use gtk4::gdk::{self, Monitor, RGBA};
use gtk4::gio::{self, MemoryInputStream};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};
use gtk4_layer_shell::LayerShell;
use webkit6::prelude::*;
use webkit6::{
    LoadEvent, NetworkSession, UserContentInjectedFrames, UserContentManager, UserScript,
    UserScriptInjectionTime, UserStyleLevel, UserStyleSheet, WebContext, WebView,
};

use crate::audio::AudioController;
use crate::backlight::BacklightController;
use crate::bluetooth::BluetoothController;
use crate::bridge::{BridgeEvent, BridgeRequest, SendEvent, SnapshotCache};
use crate::dbus_bridge::DbusBridge;
use crate::greetd::GreetdController;
use crate::notifications::NotificationStore;
use crate::process_bridge::ProcessBridge;
use crate::surface::{
    load_shell_config_from_dir, load_shell_config_from_url, MonitorTarget, ResolvedSurface,
    WindowInputState,
};
use crate::ws_bridge::ClientIdentity;

const PRELOAD_JS: &str = include_str!("../generated/preload.js");
const FOCUSED_WINDOW_SLOT: &str = "__focused__";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WindowKey {
    surface_id: String,
    output: String,
}

struct WindowHandle {
    window: ApplicationWindow,
    input_state: Rc<RefCell<WindowInputState>>,
    monitors_target: MonitorTarget,
}

struct SurfaceRegistry {
    specs: Vec<ResolvedSurface>,
    send_event: Option<SendEvent>,
    focused_output: Option<Arc<Mutex<Option<String>>>>,
}

impl SurfaceRegistry {
    fn new() -> Self {
        Self {
            specs: Vec::new(),
            send_event: None,
            focused_output: None,
        }
    }

    fn emit_state(&self) {
        if let Some(send) = &self.send_event {
            let surfaces = self.specs.iter().map(ResolvedSurface::runtime_state).collect();
            send(BridgeEvent::SurfacesState { surfaces });
        }
    }
}

thread_local! {
    static WEB_VIEWS: RefCell<Vec<glib::WeakRef<WebView>>> = const { RefCell::new(Vec::new()) };
    static WINDOWS: RefCell<HashMap<WindowKey, WindowHandle>> = RefCell::new(HashMap::new());
    static SURFACES: RefCell<SurfaceRegistry> = RefCell::new(SurfaceRegistry::new());
}

#[derive(Clone)]
enum Source {
    Dir(PathBuf),
    Url(String),
}

#[derive(Clone)]
struct Config {
    source: Option<Source>,
    replace: bool,
    devtools: bool,
    notifications: bool,
    ws_bridge: Option<u16>,
    ws_token: Option<String>,
    ws_claim_notifications: bool,
}

impl Config {
    fn from_args() -> Result<Self, String> {
        Self::from_iter(std::env::args().skip(1))
    }

    fn from_iter<I: IntoIterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut dir: Option<PathBuf> = None;
        let mut url: Option<String> = None;
        let mut replace = false;
        let mut devtools = false;
        let mut notifications = true;
        let mut ws_bridge: Option<u16> = None;
        let mut ws_token: Option<String> = None;
        let mut ws_claim_notifications = false;
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
                "--no-notifications" => notifications = false,
                "--ws-claim-notifications" => ws_claim_notifications = true,
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
            replace,
            devtools,
            notifications,
            ws_bridge,
            ws_token,
            ws_claim_notifications,
        })
    }

    fn resolve_surfaces(&self) -> Result<Vec<ResolvedSurface>, String> {
        match &self.source {
            Some(Source::Dir(dir)) => load_shell_config_from_dir(dir),
            Some(Source::Url(url)) => load_shell_config_from_url(url),
            None => Ok(Vec::new()),
        }
    }

    fn application_flags(&self, has_session_dbus: bool) -> gio::ApplicationFlags {
        if !has_session_dbus {
            return gio::ApplicationFlags::NON_UNIQUE;
        }
        let mut flags = gio::ApplicationFlags::ALLOW_REPLACEMENT;
        if self.replace {
            flags |= gio::ApplicationFlags::REPLACE;
        }
        flags
    }
}

fn has_session_dbus() -> bool {
    std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|v| !v.is_empty())
}

struct HostServices {
    notifications: NotificationStore,
    audio: AudioController,
    bluetooth: BluetoothController,
    backlight: BacklightController,
    greetd: GreetdController,
    sway: sway_ipc::SwayCommands,
    tray_menus: tray::TrayMenuWatcher,
    icon_palette: icons::SharedIconPalette,
    focused_output: Arc<Mutex<Option<String>>>,
}

impl HostServices {
    fn focused_output(&self) -> Option<String> {
        self.focused_output.lock().unwrap().clone()
    }

    fn start(
        send_event: SendEvent,
        claim_dbus_servers: bool,
        claim_notifications: bool,
        on_greetd_exit: Arc<dyn Fn() + Send + Sync>,
    ) -> (Arc<Self>, theme::ThemeState) {
        let icon_palette = icons::SharedIconPalette::default();
        let tray_menus = tray::TrayMenuWatcher::new(send_event.clone(), icon_palette.clone());
        let tray_store = if claim_dbus_servers {
            tray::spawn_server(send_event.clone(), icon_palette.clone())
        } else {
            tray::spawn_observer(send_event.clone(), icon_palette.clone())
        };
        let notifications = NotificationStore::new(send_event.clone(), icon_palette.clone());
        let theme_send: SendEvent = Arc::new({
            let send_event = send_event.clone();
            let icon_palette = icon_palette.clone();
            let notifications = notifications.clone();
            move |ev| {
                if let BridgeEvent::Theme(ref next) = ev {
                    if icon_palette.update(next) {
                        let tray_store = tray_store.clone();
                        let notifications = notifications.clone();
                        std::thread::spawn(move || {
                            tray_store.refresh_icons();
                            notifications.refresh_icons();
                        });
                    }
                }
                send_event(ev);
            }
        });
        let initial_theme = theme::start_theme_watcher(theme_send);
        if claim_notifications {
            notifications::spawn_server(notifications.clone());
        } else {
            notifications.set_available(false);
        }
        let focused_output = Arc::new(Mutex::new(None::<String>));
        let sway_send: SendEvent = Arc::new({
            let send_event = send_event.clone();
            let focused_output = Arc::clone(&focused_output);
            let notifications = notifications.clone();
            move |ev| {
                if let BridgeEvent::SwayState(ref state) = ev {
                    let next = state
                        .workspaces
                        .iter()
                        .find(|ws| ws.focused)
                        .map(|ws| ws.output.clone());
                    let changed_from_existing = {
                        let mut guard = focused_output.lock().unwrap();
                        let changed = guard.is_some() && *guard != next;
                        *guard = next;
                        changed
                    };
                    if changed_from_existing {
                        notifications.resume_all_paused();
                    }
                }
                send_event(ev);
            }
        });
        sway_ipc::spawn(sway_send);
        let audio = audio::spawn(send_event.clone());
        let bluetooth = bluetooth::spawn(send_event.clone());
        network::spawn(send_event.clone());
        let backlight = backlight::spawn(send_event.clone());
        let greetd = greetd::spawn(send_event, on_greetd_exit);

        (
            Arc::new(Self {
                notifications,
                audio,
                bluetooth,
                backlight,
                greetd,
                sway: sway_ipc::SwayCommands::spawn(),
                tray_menus,
                icon_palette,
                focused_output,
            }),
            initial_theme,
        )
    }

    fn dispatch(
        &self,
        identity: &ClientIdentity,
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
            BridgeRequest::HideNotificationPopup { id } => {
                self.notifications.hide_popup(id);
            }
            BridgeRequest::InvokeNotificationAction { id, action_key } => {
                self.notifications.invoke_action(id, &action_key);
            }
            BridgeRequest::ActivateNotification { id } => {
                self.notifications.activate(id);
            }
            BridgeRequest::ClearNotifications => {
                self.notifications.clear_all();
            }
            BridgeRequest::SetNotificationsDoNotDisturb { enabled } => {
                self.notifications.set_do_not_disturb(enabled);
            }
            BridgeRequest::MarkNotificationsRead => {
                self.notifications.mark_all_read();
            }
            BridgeRequest::PauseNotificationTimeout { id } => {
                self.notifications.pause_timeout(id);
            }
            BridgeRequest::ResumeNotificationTimeout { id } => {
                self.notifications.resume_timeout(id);
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
                tray::get_menu(id, bus, menu_path, reply_send.clone(), self.icon_palette.clone());
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
            BridgeRequest::SetInteractiveRegions { rects } => {
                update_window_input_state(identity, |s| s.set_interactive_rects(rects));
            }
            BridgeRequest::SetOverlay { active } => {
                if active {
                    self.notifications.resume_all_paused();
                }
                update_window_input_state(identity, move |s| s.set_overlay(active));
            }
            BridgeRequest::SurfaceSetVisible { id, visible } => {
                glib::idle_add_once(move || set_surface_visible(&id, visible));
            }
            BridgeRequest::SurfaceToggle { id } => {
                glib::idle_add_once(move || toggle_surface_visible(&id));
            }
            BridgeRequest::GreetdCreateSession { id, user } => {
                self.greetd.create_session(id, user, reply_send.clone());
            }
            BridgeRequest::GreetdRespond { id, response } => {
                self.greetd
                    .respond(id, response.map(|r| r.0), reply_send.clone());
            }
            BridgeRequest::GreetdCancelSession { id } => {
                self.greetd.cancel_session(id, reply_send.clone());
            }
            BridgeRequest::GreetdLaunch {
                id,
                command,
                env,
                exit,
            } => {
                self.greetd
                    .launch(id, command, env, exit, reply_send.clone());
            }
        }
    }
}

fn update_window_input_state<F>(identity: &ClientIdentity, f: F)
where
    F: FnOnce(&mut WindowInputState) + Send + 'static,
{
    let surface_id = identity.surface.clone();
    let output = identity.output.clone();
    if surface_id.is_none() && output.is_none() {
        return;
    }
    glib::idle_add_once(move || {
        WINDOWS.with(|cell| {
            let map = cell.borrow();
            let handle = match (&surface_id, &output) {
                (Some(sid), Some(out)) => map
                    .get(&WindowKey {
                        surface_id: sid.clone(),
                        output: out.clone(),
                    })
                    .or_else(|| {
                        map.get(&WindowKey {
                            surface_id: sid.clone(),
                            output: FOCUSED_WINDOW_SLOT.to_string(),
                        })
                    }),
                (Some(sid), None) => map.iter().find(|(k, _)| k.surface_id == *sid).map(|(_, h)| h),
                (None, Some(out)) => map.iter().find(|(k, _)| k.output == *out).map(|(_, h)| h),
                (None, None) => None,
            };
            if let Some(handle) = handle {
                f(&mut handle.input_state.borrow_mut());
                apply_window_input_state(&handle.window, &handle.input_state.borrow());
            }
        });
    });
}

fn find_monitor_by_connector(connector: Option<&str>) -> Option<Monitor> {
    let display = gdk::Display::default()?;
    let monitors = display.monitors();
    if let Some(target) = connector {
        for i in 0..monitors.n_items() {
            if let Some(mon) = monitors.item(i).and_then(|o| o.downcast::<Monitor>().ok()) {
                if mon.connector().as_deref() == Some(target) {
                    return Some(mon);
                }
            }
        }
    }
    monitors.item(0).and_then(|o| o.downcast::<Monitor>().ok())
}

fn set_surface_visible(surface_id: &str, visible: bool) {
    let focused_output = SURFACES.with(|cell| {
        let mut reg = cell.borrow_mut();
        let spec = reg.specs.iter_mut().find(|s| s.id == surface_id)?;
        spec.visible = visible;
        reg.emit_state();
        Some(
            reg.focused_output
                .as_ref()
                .and_then(|f| f.lock().unwrap().clone()),
        )
    });
    let Some(focused_output) = focused_output else {
        return;
    };

    WINDOWS.with(|cell| {
        for (key, handle) in cell.borrow().iter() {
            if key.surface_id != surface_id {
                continue;
            }
            let win = &handle.window;
            if visible {
                if handle.monitors_target == MonitorTarget::Focused {
                    if let Some(mon) = find_monitor_by_connector(focused_output.as_deref()) {
                        let g = mon.geometry();
                        let (w, h) = handle
                            .input_state
                            .borrow_mut()
                            .update_monitor_geometry(g.width(), g.height());
                        win.set_monitor(Some(&mon));
                        win.set_default_size(w, h);
                    }
                }
                win.set_visible(true);
                win.present();
                apply_window_input_state(win, &handle.input_state.borrow());
            } else {
                win.set_visible(false);
            }
        }
    });
}

fn toggle_surface_visible(surface_id: &str) {
    let next = SURFACES.with(|cell| {
        cell.borrow()
            .specs
            .iter()
            .find(|s| s.id == surface_id)
            .map(|spec| !spec.visible)
    });
    if let Some(visible) = next {
        set_surface_visible(surface_id, visible);
    }
}

fn start_ws_bridge(
    config: &Config,
    claim_dbus_servers: bool,
    on_greetd_exit: Arc<dyn Fn() + Send + Sync>,
) -> (
    u16,
    String,
    theme::ThemeState,
    Arc<HostServices>,
    ws_bridge::WsBroadcaster,
    SendEvent,
) {
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

    let (services, initial_theme) = HostServices::start(
        send_event.clone(),
        claim_dbus_servers,
        (claim_dbus_servers && config.notifications) || config.ws_claim_notifications,
        on_greetd_exit,
    );
    let requested_port = config.ws_bridge.unwrap_or(0);
    let token = config
        .ws_token
        .clone()
        .unwrap_or_else(ws_bridge::generate_token);
    let dispatch_services = Arc::clone(&services);
    let disconnect_services = Arc::clone(&services);
    let bound_port = match ws_bridge::spawn(
        requested_port,
        config.ws_bridge.is_some(),
        token.clone(),
        broadcaster.clone(),
        snapshots,
        move |identity, req, client_dbus, client_procs, client_send| {
            dispatch_services.dispatch(identity, req, client_dbus, client_procs, client_send);
        },
        move || {
            disconnect_services.notifications.resume_all_paused();
        },
    ) {
        Ok(port) => port,
        Err(err) => {
            eprintln!("webbar: ws-bridge failed to bind 127.0.0.1:{requested_port}: {err}");
            std::process::exit(1);
        }
    };
    (bound_port, token, initial_theme, services, broadcaster, send_event)
}

fn main() {
    let config = match Config::from_args() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("webbar: {err}\nUsage: webbar (--dir <path> | --url <url>) [--replace] [--devtools] [--no-notifications]");
            std::process::exit(1);
        }
    };

    if config.source.is_none() {
        let _ = gtk4::init();
        let main_loop = glib::MainLoop::new(None, false);
        let exit_loop = main_loop.clone();
        let on_greetd_exit: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let exit_loop = exit_loop.clone();
            glib::idle_add_once(move || exit_loop.quit());
        });
        let _ = start_ws_bridge(&config, false, on_greetd_exit);
        main_loop.run();
        return;
    }

    let app = Application::builder()
        .application_id("dev.local.webbar")
        .flags(config.application_flags(has_session_dbus()))
        .build();

    app.connect_activate(move |app| activate(app, &config));
    app.run_with_args::<&str>(&[]);
}

fn activate(app: &Application, config: &Config) {
    let surfaces = match config.resolve_surfaces() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("webbar: {err}");
            std::process::exit(1);
        }
    };

    if let Some(Source::Dir(dir)) = &config.source {
        register_webbar_scheme(dir.clone());
    }

    let on_greetd_exit: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {
        glib::idle_add_once(|| {
            if let Some(app) = gio::Application::default() {
                app.quit();
            } else {
                std::process::exit(0);
            }
        });
    });
    let claim_dbus = has_session_dbus();
    let (ws_port, ws_token, initial_theme, services, broadcaster, send_event) =
        start_ws_bridge(config, claim_dbus, on_greetd_exit);

    SURFACES.with(|cell| {
        let mut reg = cell.borrow_mut();
        reg.specs = surfaces.clone();
        reg.send_event = Some(send_event.clone());
        reg.focused_output = Some(Arc::clone(&services.focused_output));
        reg.emit_state();
    });

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
    // WebKitGTK 2.54 computes WebCore::fontDPI() from gtk-xft-dpi first and
    // falls back to the primary monitor's physical DPI. On headless/virtual
    // Wayland outputs with 0mm physical size and unset gtk-xft-dpi (-1), that
    // fallback evaluates to 0 DPI on secondary WebViews, collapsing pageScale
    // to 0 and window.innerWidth/Height to 0x0.
    let settings = gtk4::Settings::for_display(&display);
    if settings.gtk_xft_dpi() <= 0 {
        let has_zero_mm_monitor = (0..monitors.n_items()).any(|i| {
            monitors
                .item(i)
                .and_then(|o| o.downcast::<Monitor>().ok())
                .is_some_and(|m| m.width_mm() <= 0 || m.height_mm() <= 0)
        });
        if has_zero_mm_monitor {
            settings.set_gtk_xft_dpi(96 * 1024);
        }
    }
    let initial_theme_css = theme::format_root_css(&initial_theme);
    let sync_monitors = {
        let app = app.clone();
        let config = config.clone();
        let surfaces = surfaces.clone();
        let monitors = monitors.clone();
        let services = Arc::clone(&services);
        move || {
            let mut connected: Vec<(String, Monitor)> = Vec::new();
            for i in 0..monitors.n_items() {
                if let Some(monitor) = monitors.item(i).and_then(|o| o.downcast::<Monitor>().ok()) {
                    let connector = monitor
                        .connector()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("monitor-{i}"));
                    connected.push((connector, monitor));
                }
            }

            let mut desired: Vec<(WindowKey, String, Monitor, &ResolvedSurface)> = Vec::new();
            for spec in &surfaces {
                match &spec.monitors {
                    MonitorTarget::Focused => {
                        let focused = services.focused_output();
                        if let Some((connector, monitor)) = focused
                            .as_deref()
                            .and_then(|f| connected.iter().find(|(c, _)| c == f))
                            .or_else(|| connected.first())
                        {
                            desired.push((
                                WindowKey {
                                    surface_id: spec.id.clone(),
                                    output: FOCUSED_WINDOW_SLOT.to_string(),
                                },
                                connector.clone(),
                                monitor.clone(),
                                spec,
                            ));
                        }
                    }
                    MonitorTarget::All | MonitorTarget::Primary | MonitorTarget::Connectors(_) => {
                        for (idx, (connector, monitor)) in connected.iter().enumerate() {
                            let included = match &spec.monitors {
                                MonitorTarget::All => true,
                                MonitorTarget::Primary => idx == 0,
                                MonitorTarget::Connectors(list) => {
                                    list.iter().any(|c| c == connector)
                                }
                                MonitorTarget::Focused => unreachable!(),
                            };
                            if included {
                                desired.push((
                                    WindowKey {
                                        surface_id: spec.id.clone(),
                                        output: connector.clone(),
                                    },
                                    connector.clone(),
                                    monitor.clone(),
                                    spec,
                                ));
                            }
                        }
                    }
                }
            }

            let desired_keys: Vec<WindowKey> = desired.iter().map(|(k, _, _, _)| k.clone()).collect();
            for (key, connector, monitor, spec) in desired {
                let exists = WINDOWS.with(|cell| {
                    let map = cell.borrow();
                    if let Some(handle) = map.get(&key) {
                        if handle.monitors_target == MonitorTarget::Focused {
                            let g = monitor.geometry();
                            let (w, h) = handle
                                .input_state
                                .borrow_mut()
                                .update_monitor_geometry(g.width(), g.height());
                            handle.window.set_monitor(Some(&monitor));
                            handle.window.set_default_size(w, h);
                        }
                        true
                    } else {
                        false
                    }
                });
                if !exists {
                    let visible = SURFACES.with(|cell| {
                        cell.borrow()
                            .specs
                            .iter()
                            .find(|s| s.id == spec.id)
                            .map(|s| s.visible)
                            .unwrap_or(spec.visible)
                    });
                    create_surface_window(
                        &app,
                        &monitor,
                        &connector,
                        &key,
                        spec,
                        visible,
                        &config,
                        ws_port,
                        &ws_token,
                        &initial_theme_css,
                    );
                }
            }

            WINDOWS.with(|cell| {
                let mut map = cell.borrow_mut();
                let stale: Vec<WindowKey> = map
                    .keys()
                    .filter(|k| !desired_keys.contains(k))
                    .cloned()
                    .collect();
                for key in stale {
                    if let Some(handle) = map.remove(&key) {
                        handle.window.close();
                    }
                }
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

    let toggle_center_action = gio::SimpleAction::new("notifications-toggle-center", None);
    {
        let services = Arc::clone(&services);
        toggle_center_action.connect_activate(move |_, _| {
            let ev = BridgeEvent::HostAction {
                action: "notifications_toggle_center".to_string(),
            };
            if let Ok(json) = serde_json::to_string(&ev) {
                broadcaster.send_to_target(
                    Some("bar"),
                    services.focused_output().as_deref(),
                    &json,
                );
            }
        });
    }
    app.add_action(&toggle_center_action);

    let toggle_do_not_disturb_action =
        gio::SimpleAction::new("notifications-toggle-do-not-disturb", None);
    {
        let notifications = services.notifications.clone();
        toggle_do_not_disturb_action.connect_activate(move |_, _| {
            notifications.toggle_do_not_disturb();
        });
    }
    app.add_action(&toggle_do_not_disturb_action);

    let dismiss_latest_action = gio::SimpleAction::new("notifications-dismiss-latest", None);
    {
        let notifications = services.notifications.clone();
        dismiss_latest_action.connect_activate(move |_, _| {
            notifications.dismiss_latest();
        });
    }
    app.add_action(&dismiss_latest_action);

    let clear_all_action = gio::SimpleAction::new("notifications-clear-all", None);
    {
        let notifications = services.notifications.clone();
        clear_all_action.connect_activate(move |_, _| {
            notifications.clear_all();
        });
    }
    app.add_action(&clear_all_action);

    let surface_show_action =
        gio::SimpleAction::new("surface-show", Some(glib::VariantTy::STRING));
    surface_show_action.connect_activate(|_, param| {
        if let Some(id) = param.and_then(|v| v.str()) {
            set_surface_visible(id, true);
        }
    });
    app.add_action(&surface_show_action);

    let surface_hide_action =
        gio::SimpleAction::new("surface-hide", Some(glib::VariantTy::STRING));
    surface_hide_action.connect_activate(|_, param| {
        if let Some(id) = param.and_then(|v| v.str()) {
            set_surface_visible(id, false);
        }
    });
    app.add_action(&surface_hide_action);

    let surface_toggle_action =
        gio::SimpleAction::new("surface-toggle", Some(glib::VariantTy::STRING));
    surface_toggle_action.connect_activate(|_, param| {
        if let Some(id) = param.and_then(|v| v.str()) {
            toggle_surface_visible(id);
        }
    });
    app.add_action(&surface_toggle_action);
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

fn apply_window_input_state(win: &ApplicationWindow, state: &WindowInputState) {
    if win.is_layer_window() {
        let target_kb = state.keyboard_mode();
        if win.keyboard_mode() != target_kb {
            win.set_keyboard_mode(target_kb);
        }
    }
    // Fullscreen surfaces accept pointer input across the entire surface;
    // avoid clipping with a monitor.geometry()-derived input region (which can
    // start at 0x0 under non-layer-shell compositors like cage).
    if !state.is_fullscreen() {
        if let Some(surface) = win.surface() {
            surface.set_input_region(&state.cairo_region());
        }
    }
}

fn allow_devtools(config: &Config, is_fullscreen: bool, has_greetd_socket: bool) -> bool {
    let is_greeter = is_fullscreen || has_greetd_socket;
    config.devtools || !is_greeter
}

fn build_surface_uri(source: &Source, spec: &ResolvedSurface, query: &str) -> String {
    match source {
        Source::Url(url) => {
            let base = if let Some(entry) = &spec.entry {
                let trimmed = url.trim_end_matches('/');
                let rel = entry.trim_start_matches('/');
                format!("{trimmed}/{rel}")
            } else {
                url.clone()
            };
            let sep = if base.contains('?') { '&' } else { '?' };
            format!("{base}{sep}{query}")
        }
        Source::Dir(_) => {
            let entry = spec
                .entry
                .as_deref()
                .map(|s| s.trim_start_matches('/'))
                .filter(|s| !s.is_empty())
                .unwrap_or("index.html");
            format!("webbar://app/{entry}?{query}")
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn create_surface_window(
    app: &Application,
    monitor: &Monitor,
    connector: &str,
    window_key: &WindowKey,
    spec: &ResolvedSurface,
    visible: bool,
    config: &Config,
    ws_port: u16,
    ws_token: &str,
    initial_theme_css: &str,
) {
    let geom = monitor.geometry();
    let input_state = Rc::new(RefCell::new(WindowInputState::from_surface(
        spec,
        geom.width(),
        geom.height(),
    )));
    let (default_w, default_h) = input_state.borrow().default_window_size();

    let window = ApplicationWindow::builder()
        .application(app)
        .title(&spec.namespace)
        .default_width(default_w)
        .default_height(default_h)
        .build();
    if spec.width.is_some() || spec.height.is_some() {
        window.set_size_request(spec.width.unwrap_or(-1), spec.height.unwrap_or(-1));
    }

    if gtk4_layer_shell::is_supported() {
        window.init_layer_shell();
        window.set_namespace(Some(&spec.namespace));
        window.set_monitor(Some(monitor));
        window.set_layer(spec.layer.to_layer());
        for anchor in &spec.anchors {
            window.set_anchor(anchor.to_layer_edge(), true);
        }
        if spec.margins.top != 0 {
            window.set_margin(gtk4_layer_shell::Edge::Top, spec.margins.top);
        }
        if spec.margins.right != 0 {
            window.set_margin(gtk4_layer_shell::Edge::Right, spec.margins.right);
        }
        if spec.margins.bottom != 0 {
            window.set_margin(gtk4_layer_shell::Edge::Bottom, spec.margins.bottom);
        }
        if spec.margins.left != 0 {
            window.set_margin(gtk4_layer_shell::Edge::Left, spec.margins.left);
        }
        spec.exclusive_zone.apply_to_window(&window);
        window.set_keyboard_mode(spec.keyboard.to_keyboard_mode());
    } else if spec.is_fullscreen() {
        window.fullscreen();
    }

    if !spec.is_fullscreen() {
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
            let (next_w, next_h) = input_state
                .borrow_mut()
                .update_monitor_geometry(g.width(), g.height());
            if let Some(win) = window_weak.upgrade() {
                win.set_default_size(next_w, next_h);
                apply_window_input_state(&win, &input_state.borrow());
            }
        });
    }

    WINDOWS.with(|cell| {
        cell.borrow_mut().insert(
            window_key.clone(),
            WindowHandle {
                window: window.clone(),
                input_state: input_state.clone(),
                monitors_target: spec.monitors.clone(),
            },
        );
    });

    let ucm = UserContentManager::new();
    let defaults_css = spec.format_defaults_css();
    ucm.add_style_sheet(&UserStyleSheet::new(
        &defaults_css,
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

    let has_greetd_socket = greetd::socket_path().is_some();
    let devtools_allowed = allow_devtools(config, spec.is_fullscreen(), has_greetd_socket);

    let mut web_view_builder = WebView::builder().user_content_manager(&ucm);
    if spec.is_fullscreen() || has_greetd_socket {
        let ephemeral = NetworkSession::new_ephemeral();
        web_view_builder = web_view_builder.network_session(&ephemeral);
    }
    let web_view = web_view_builder.build();
    if let Some(settings) = WebViewExt::settings(&web_view) {
        settings.set_enable_developer_extras(devtools_allowed);
    }
    if devtools_allowed {
        if let Some(inspector) = web_view.inspector() {
            inspector.connect_attach(|inspector| {
                let inspector = inspector.clone();
                glib::idle_add_local_once(move || {
                    inspector.detach();
                });
                false
            });
        }
    }
    web_view.set_background_color(&RGBA::new(0.0, 0.0, 0.0, 0.0));
    web_view.set_vexpand(true);
    web_view.set_hexpand(true);

    if devtools_allowed {
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
    }

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
    {
        let sid = spec.id.clone();
        web_view.connect_load_failed(move |_view, _event, uri, err| {
            let clean_uri = uri.split('?').next().unwrap_or(uri);
            eprintln!("webbar: surface '{sid}' load failed ({clean_uri}): {err}");
            false
        });
    }

    let query = format!(
        "surface={}&output={connector}&wsPort={ws_port}&wsToken={ws_token}",
        spec.id
    );
    let source = config
        .source
        .as_ref()
        .expect("headless ws-bridge mode does not create surface windows");
    let uri = build_surface_uri(source, spec, &query);

    window.set_child(Some(&web_view));
    web_view.load_uri(&uri);
    if visible {
        window.present();
    } else {
        window.set_visible(false);
    }
    if config.devtools && visible {
        open_devtools(&web_view);
    }
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
        assert!(cfg.notifications);
        assert_eq!(
            cfg.application_flags(true),
            gio::ApplicationFlags::ALLOW_REPLACEMENT
        );
        assert_eq!(
            cfg.application_flags(false),
            gio::ApplicationFlags::NON_UNIQUE
        );
    }

    #[test]
    fn greeter_devtools_policy_disables_inspector_unless_explicit() {
        let cfg = parse(&["--dir", "/tmp/ui"]).unwrap();
        assert!(allow_devtools(&cfg, false, false));
        assert!(!allow_devtools(&cfg, true, false));
        assert!(!allow_devtools(&cfg, false, true));
        assert!(!allow_devtools(&cfg, true, true));

        let with_devtools = parse(&["--dir", "/tmp/ui", "--devtools"]).unwrap();
        assert!(allow_devtools(&with_devtools, true, true));
    }

    #[test]
    fn devtools_and_no_notifications_flags_parse() {
        let cfg = parse(&["--dir", "/tmp/ui", "--devtools", "--no-notifications"]).unwrap();
        assert!(cfg.devtools);
        assert!(!cfg.notifications);
        assert!(!cfg.ws_claim_notifications);

        let ws_cfg = parse(&["--ws-bridge", "9123", "--ws-claim-notifications"]).unwrap();
        assert!(ws_cfg.ws_claim_notifications);
    }

    #[test]
    fn replace_flag_sets_replace_application_flag() {
        let cfg = parse(&["--url", "http://localhost:8080", "--replace"]).unwrap();
        assert!(cfg.replace);
        assert_eq!(
            cfg.application_flags(true),
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
}

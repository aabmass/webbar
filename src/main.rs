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
mod tray;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
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
    UserScriptInjectionTime, WebContext, WebView,
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

thread_local! {
    static WEB_VIEWS: RefCell<Vec<glib::WeakRef<WebView>>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone)]
enum Source {
    Dir(PathBuf),
    Url(String),
}

#[derive(Clone)]
struct Config {
    source: Source,
    height: i32,
    overlay_height: Option<i32>,
    replace: bool,
    devtools: bool,
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
                "--replace" => replace = true,
                "--devtools" => devtools = true,
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        let source = match (dir, url) {
            (Some(d), None) => Source::Dir(d),
            (None, Some(u)) => Source::Url(u),
            _ => return Err("pass either --dir <path> or --url <url>".to_string()),
        };
        Ok(Self {
            source,
            height,
            overlay_height,
            replace,
            devtools,
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
    dbus: Arc<DbusBridge>,
    processes: Arc<ProcessBridge>,
    notifications: NotificationStore,
    audio: AudioController,
    bluetooth: BluetoothController,
    backlight: BacklightController,
    snapshots: SnapshotCache,
    send_event: SendEvent,
}

fn main() {
    let config = match Config::from_args() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("webbar: {err}\nUsage: webbar (--dir <path> | --url <url>) [--height <px>] [--overlay-height <px>] [--replace] [--devtools]");
            std::process::exit(1);
        }
    };

    let app = Application::builder()
        .application_id("dev.local.webbar")
        .flags(config.application_flags())
        .build();

    app.connect_activate(move |app| activate(app, &config));
    app.run_with_args::<&str>(&[]);
}

fn activate(app: &Application, config: &Config) {
    if let Source::Dir(dir) = &config.source {
        register_webbar_scheme(dir.clone());
    }

    let snapshots = SnapshotCache::new();
    let send_event: SendEvent = {
        let snapshots = snapshots.clone();
        Arc::new(move |event: BridgeEvent| {
            snapshots.record(&event);
            let json = match serde_json::to_string(&event) {
                Ok(json) => json,
                Err(err) => {
                    eprintln!("webbar: failed to serialize event: {err}");
                    return;
                }
            };
            glib::idle_add_once(move || {
                WEB_VIEWS.with(|cell| {
                    let mut views = cell.borrow_mut();
                    views.retain(|weak| {
                        if let Some(view) = weak.upgrade() {
                            let script = format!(
                                "window.__webbarReceive && window.__webbarReceive({json});"
                            );
                            view.evaluate_javascript(
                                &script,
                                None,
                                None,
                                None::<&gio::Cancellable>,
                                |_| {},
                            );
                            true
                        } else {
                            false
                        }
                    });
                });
            });
        })
    };

    let notifications = NotificationStore::new(send_event.clone());
    notifications::spawn_server(notifications.clone());
    sway_ipc::spawn(send_event.clone());
    tray::spawn_server(send_event.clone());
    let audio = audio::spawn(send_event.clone());
    let bluetooth = bluetooth::spawn(send_event.clone());
    network::spawn(send_event.clone());
    let backlight = backlight::spawn(send_event.clone());

    let services = Arc::new(HostServices {
        dbus: Arc::new(DbusBridge::new(send_event.clone())),
        processes: Arc::new(ProcessBridge::new(send_event.clone())),
        notifications,
        audio,
        bluetooth,
        backlight,
        snapshots,
        send_event,
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
    let windows: Rc<RefCell<HashMap<glib::Object, ApplicationWindow>>> =
        Rc::new(RefCell::new(HashMap::new()));

    let sync_monitors = {
        let app = app.clone();
        let config = config.clone();
        let services = services.clone();
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
                    let win = create_bar_window(&app, &monitor, &config, &services);
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

fn register_webbar_scheme(dir: PathBuf) {
    let Some(context) = WebContext::default() else {
        return;
    };
    context.register_uri_scheme("webbar", move |request| {
        let uri = request.uri().map(|u| u.to_string()).unwrap_or_default();
        let path_part = uri
            .strip_prefix("webbar://app/")
            .unwrap_or("index.html")
            .split('?')
            .next()
            .unwrap_or("index.html");
        let rel = if path_part.is_empty() {
            "index.html"
        } else {
            path_part
        };

        let disk_path = dir.join(rel);
        match fs::read(&disk_path) {
            Ok(bytes) => {
                let stream = MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
                request.finish(&stream, -1, Some(mime_for(rel)));
            }
            Err(err) => {
                eprintln!("webbar: failed to read {}: {err}", disk_path.display());
                let stream = MemoryInputStream::from_bytes(&glib::Bytes::from_static(b"Not Found"));
                request.finish(&stream, -1, Some("text/plain"));
            }
        }
    });
}

fn mime_for(rel: &str) -> &'static str {
    if rel.ends_with(".js") {
        "application/javascript"
    } else if rel.ends_with(".css") {
        "text/css"
    } else if rel.ends_with(".svg") {
        "image/svg+xml"
    } else if rel.ends_with(".png") {
        "image/png"
    } else {
        "text/html"
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

fn create_bar_window(
    app: &Application,
    monitor: &Monitor,
    config: &Config,
    services: &Arc<HostServices>,
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
    let output_json = serde_json::to_string(&connector).unwrap_or_else(|_| "\"\"".to_string());

    let ucm = UserContentManager::new();
    ucm.register_script_message_handler("bridge", None);
    ucm.add_script(&UserScript::new(
        &format!("window.__webbarOutput = {output_json};"),
        UserContentInjectedFrames::TopFrame,
        UserScriptInjectionTime::Start,
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

    let web_view = WebView::builder().user_content_manager(&ucm).build();
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
        let snapshots = services.snapshots.clone();
        let input_state = input_state.clone();
        let window_weak = window.downgrade();
        web_view.connect_load_changed(move |view, event| match event {
            LoadEvent::Committed => {
                input_state.borrow_mut().reset();
                if let Some(win) = window_weak.upgrade() {
                    apply_window_input_state(&win, &input_state.borrow());
                }
            }
            LoadEvent::Finished => {
                for state in snapshots.snapshot_all() {
                    if let Ok(json) = serde_json::to_string(&state) {
                        let script = format!(
                            "window.__webbarReceive && window.__webbarReceive({json});"
                        );
                        view.evaluate_javascript(
                            &script,
                            None,
                            None,
                            None::<&gio::Cancellable>,
                            |_| {},
                        );
                    }
                }
            }
            _ => {}
        });
    }

    let services = services.clone();
    let window_weak = window.downgrade();
    ucm.connect_script_message_received(Some("bridge"), move |_, value| {
        let raw = value.to_str();
        let req: BridgeRequest = match serde_json::from_str(&raw) {
            Ok(req) => req,
            Err(err) => {
                eprintln!("webbar: invalid bridge request: {err} ({raw})");
                return;
            }
        };
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
            } => services
                .dbus
                .call(id, bus, destination, path, interface, member, signature, body),
            BridgeRequest::DbusMatch {
                id,
                bus,
                sender,
                path,
                interface,
                member,
            } => services
                .dbus
                .match_signals(id, bus, sender, path, interface, member),
            BridgeRequest::DbusUnwatch { id } => services.dbus.unwatch(id),
            BridgeRequest::DismissNotification { id } => {
                services.notifications.dismiss(id);
            }
            BridgeRequest::InvokeNotificationAction { id, action_key } => {
                services.notifications.invoke_action(id, &action_key);
            }
            BridgeRequest::ClearNotifications => {
                services.notifications.clear_all();
            }
            BridgeRequest::ProcessSpawn { id, argv, cwd, env } => {
                services.processes.spawn(id, argv, cwd, env);
            }
            BridgeRequest::ProcessKill { id, signal } => {
                services.processes.kill(id, signal);
            }
            BridgeRequest::AudioSetVolume {
                id,
                device_id,
                volume,
            } => services.audio.set_volume(id, device_id, volume),
            BridgeRequest::AudioSetMuted {
                id,
                device_id,
                muted,
            } => services.audio.set_muted(id, device_id, muted),
            BridgeRequest::AudioSetDefault { id, device_id } => {
                services.audio.set_default(id, device_id);
            }
            BridgeRequest::BluetoothSetPowered { id, path, powered } => {
                services
                    .bluetooth
                    .set_powered(id, path, powered, services.send_event.clone());
            }
            BridgeRequest::BluetoothConnect { id, path } => {
                services
                    .bluetooth
                    .connect(id, path, services.send_event.clone());
            }
            BridgeRequest::BluetoothDisconnect { id, path } => {
                services
                    .bluetooth
                    .disconnect(id, path, services.send_event.clone());
            }
            BridgeRequest::BacklightSet { id, percent } => {
                services.backlight.set_brightness(id, percent);
            }
            BridgeRequest::SwayCommand { command } => {
                if let Err(err) = sway_ipc::run_command(&command) {
                    eprintln!("webbar: sway command failed: {err}");
                }
            }
            BridgeRequest::TrayActivate { bus, path } => {
                tray::activate(bus, path);
            }
            BridgeRequest::TrayGetMenu { id, bus, menu_path } => {
                tray::get_menu(id, bus, menu_path, services.send_event.clone());
            }
            BridgeRequest::TrayMenuEvent {
                bus,
                menu_path,
                item_id,
            } => {
                tray::menu_event(bus, menu_path, item_id);
            }
            BridgeRequest::SetInputRegion { rects } => {
                input_state.borrow_mut().set_custom_rects(rects);
                if let Some(win) = window_weak.upgrade() {
                    apply_window_input_state(&win, &input_state.borrow());
                }
            }
            BridgeRequest::SetOverlay { active } => {
                input_state.borrow_mut().set_overlay(active);
                if let Some(win) = window_weak.upgrade() {
                    apply_window_input_state(&win, &input_state.borrow());
                }
            }
        }
    });

    let uri = match &config.source {
        Source::Url(url) => {
            let sep = if url.contains('?') { '&' } else { '?' };
            format!("{url}{sep}output={connector}")
        }
        Source::Dir(_) => format!("webbar://app/index.html?output={connector}"),
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
}

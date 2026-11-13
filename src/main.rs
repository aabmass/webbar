mod dbus_bridge;
mod dbus_json;
mod file_bridge;
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

use crate::dbus_bridge::{BridgeEvent, BridgeRequest, DbusBridge, SendEvent};
use crate::notifications::NotificationStore;
use crate::process_bridge::ProcessBridge;

const DEFAULT_BAR_HEIGHT: i32 = 34;
const PRELOAD_JS: &str = include_str!("preload.js");

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
}

impl Config {
    fn from_args() -> Result<Self, String> {
        let mut dir: Option<PathBuf> = None;
        let mut url: Option<String> = None;
        let mut height: i32 = DEFAULT_BAR_HEIGHT;
        let mut overlay_height: Option<i32> = None;
        let mut args = std::env::args().skip(1);
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
        })
    }
}

struct HostServices {
    dbus: Arc<DbusBridge>,
    processes: Arc<ProcessBridge>,
    notifications: NotificationStore,
    send_event: SendEvent,
}

fn main() {
    let config = match Config::from_args() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("webbar: {err}\nUsage: webbar (--dir <path> | --url <url>) [--height <px>]");
            std::process::exit(1);
        }
    };

    let app = Application::builder()
        .application_id("dev.local.webbar")
        .build();

    app.connect_activate(move |app| activate(app, &config));
    app.run_with_args::<&str>(&[]);
}

fn activate(app: &Application, config: &Config) {
    if let Source::Dir(dir) = &config.source {
        register_webbar_scheme(dir.clone());
    }

    let send_event: SendEvent = Arc::new(move |event: BridgeEvent| {
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
                            "window.__swayWebbarReceive && window.__swayWebbarReceive({json});"
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
    });

    let notifications = NotificationStore::new(send_event.clone());
    notifications::spawn_server(notifications.clone());
    sway_ipc::spawn(send_event.clone());
    tray::spawn_server(send_event.clone());

    let services = Arc::new(HostServices {
        dbus: Arc::new(DbusBridge::new(send_event.clone())),
        processes: Arc::new(ProcessBridge::new(send_event.clone())),
        notifications,
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

fn create_bar_window(
    app: &Application,
    monitor: &Monitor,
    config: &Config,
    services: &Arc<HostServices>,
) -> ApplicationWindow {
    let bar_height = config.height;
    let overlay_height = config
        .overlay_height
        .unwrap_or_else(|| (monitor.geometry().height() / 2).max(bar_height))
        .max(bar_height);
    let monitor_width = monitor.geometry().width();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("webbar")
        .default_height(bar_height)
        .build();

    window.init_layer_shell();
    window.set_monitor(Some(monitor));
    window.set_layer(Layer::Top);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);
    window.set_exclusive_zone(bar_height);
    window.set_keyboard_mode(KeyboardMode::None);

    let ucm = UserContentManager::new();
    ucm.register_script_message_handler("bridge", None);
    ucm.add_script(&UserScript::new(
        PRELOAD_JS,
        UserContentInjectedFrames::TopFrame,
        UserScriptInjectionTime::Start,
        &[],
        &[],
    ));

    let web_view = WebView::builder().user_content_manager(&ucm).build();
    web_view.set_background_color(&RGBA::new(0.0, 0.0, 0.0, 0.0));
    web_view.set_vexpand(true);
    web_view.set_hexpand(true);

    WEB_VIEWS.with(|cell| {
        cell.borrow_mut().push(web_view.downgrade());
    });

    web_view.connect_load_changed(|view, event| {
        if event == LoadEvent::Finished {
            if let Some(state) = sway_ipc::latest_state() {
                if let Ok(json) = serde_json::to_string(&state) {
                    let script = format!(
                        "window.__swayWebbarReceive && window.__swayWebbarReceive({json});"
                    );
                    view.evaluate_javascript(&script, None, None, None::<&gio::Cancellable>, |_| {});
                }
            }
        }
    });

    let services = services.clone();
    let window_weak = window.downgrade();
    let web_view_weak = web_view.downgrade();
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
            BridgeRequest::ClearNotifications => {
                services.notifications.clear_all();
            }
            BridgeRequest::ProcessSpawn {
                id,
                argv,
                cwd,
                env,
                stdin,
            } => services.processes.spawn(id, argv, cwd, env, stdin),
            BridgeRequest::ProcessWrite { id, data_b64 } => {
                services.processes.write_stdin(id, data_b64);
            }
            BridgeRequest::ProcessCloseStdin { id } => {
                services.processes.close_stdin(id);
            }
            BridgeRequest::ProcessKill { id, signal } => {
                services.processes.kill(id, signal);
            }
            BridgeRequest::FileReadText { id, path } => {
                file_bridge::read_text(id, path, services.send_event.clone());
            }
            BridgeRequest::FileReadDir { id, path } => {
                file_bridge::read_dir(id, path, services.send_event.clone());
            }
            BridgeRequest::FileStat { id, path } => {
                file_bridge::stat(id, path, services.send_event.clone());
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
                if let Some(win) = window_weak.upgrade() {
                    if let Some(surface) = win.surface() {
                        if rects.is_empty() {
                            let rect = cairo::RectangleInt::new(0, 0, monitor_width, bar_height);
                            let region = cairo::Region::create_rectangle(&rect);
                            surface.set_input_region(&region);
                            win.set_keyboard_mode(KeyboardMode::None);
                        } else {
                            let cairo_rects: Vec<cairo::RectangleInt> = rects
                                .into_iter()
                                .filter(|r| r.width > 0 && r.height > 0)
                                .map(|r| cairo::RectangleInt::new(r.x, r.y, r.width, r.height))
                                .collect();
                            let region = cairo::Region::create_rectangles(&cairo_rects);
                            surface.set_input_region(&region);
                            win.set_keyboard_mode(KeyboardMode::OnDemand);
                        }
                    }
                }
            }
            BridgeRequest::SetOverlay { active } => {
                if let (Some(win), Some(view)) = (window_weak.upgrade(), web_view_weak.upgrade()) {
                    let target_h = if active { overlay_height } else { bar_height };
                    view.set_size_request(-1, target_h);
                    win.set_default_size(-1, target_h);
                    win.set_keyboard_mode(if active {
                        KeyboardMode::OnDemand
                    } else {
                        KeyboardMode::None
                    });
                    if let Some(surface) = win.surface() {
                        let rect = cairo::RectangleInt::new(0, 0, monitor_width, target_h);
                        let region = cairo::Region::create_rectangle(&rect);
                        surface.set_input_region(&region);
                    }
                }
            }
        }
    });

    let connector = monitor
        .connector()
        .map(|s| s.to_string())
        .unwrap_or_default();
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
    window
}

//! Minimal `org.freedesktop.ScreenSaver` and `org.gnome.SessionManager` D-Bus
//! endpoints so applications (e.g. Chrome, Firefox, mpv) can acquire session
//! inhibit cookies on standalone Wayland compositors and be tracked by Stasis's
//! D-Bus monitor.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

use zbus::blocking::connection::Builder as ConnectionBuilder;
use zbus::interface;

#[derive(Clone)]
struct ScreenSaverServer {
    next_cookie: Arc<AtomicU32>,
}

#[interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaverServer {
    #[zbus(name = "Inhibit")]
    fn inhibit(&self, _application_name: String, _reason_for_inhibit: String) -> u32 {
        self.next_cookie.fetch_add(1, Ordering::Relaxed)
    }

    #[zbus(name = "UnInhibit")]
    fn un_inhibit(&self, _cookie: u32) {}
}

#[derive(Clone)]
struct SessionManagerServer {
    next_cookie: Arc<AtomicU32>,
}

#[interface(name = "org.gnome.SessionManager")]
impl SessionManagerServer {
    #[zbus(name = "Inhibit")]
    fn inhibit(&self, _app_id: String, _toplevel_xid: u32, _reason: String, _flags: u32) -> u32 {
        self.next_cookie.fetch_add(1, Ordering::Relaxed)
    }

    #[zbus(name = "Uninhibit")]
    fn uninhibit(&self, _inhibit_cookie: u32) {}
}

pub fn spawn_server() {
    thread::spawn(move || {
        let next_cookie = Arc::new(AtomicU32::new(1));
        let screensaver = ScreenSaverServer {
            next_cookie: next_cookie.clone(),
        };
        let session_mgr = SessionManagerServer { next_cookie };

        match ConnectionBuilder::session()
            .and_then(|b| b.name("org.freedesktop.ScreenSaver"))
            .and_then(|b| b.name("org.gnome.SessionManager"))
            .and_then(|b| b.serve_at("/org/freedesktop/ScreenSaver", screensaver.clone()))
            .and_then(|b| b.serve_at("/ScreenSaver", screensaver))
            .and_then(|b| b.serve_at("/org/gnome/SessionManager", session_mgr))
            .and_then(|b| b.build())
        {
            Ok(_conn) => loop {
                thread::park();
            },
            Err(err) => {
                eprintln!(
                    "webbar: could not claim ScreenSaver/SessionManager D-Bus names ({err})"
                );
            }
        }
    });
}

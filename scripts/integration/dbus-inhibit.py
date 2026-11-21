#!/usr/bin/env python3
import sys
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

DBusGMainLoop(set_as_default=True)

class MockScreenSaver(dbus.service.Object):
    @dbus.service.method("org.freedesktop.ScreenSaver", in_signature="ss", out_signature="u")
    def Inhibit(self, _app, _reason):
        return 1

    @dbus.service.method("org.freedesktop.ScreenSaver", in_signature="u", out_signature="")
    def UnInhibit(self, _cookie):
        pass

service_bus = dbus.SessionBus(private=True)
bus_name = dbus.service.BusName("org.freedesktop.ScreenSaver", service_bus)
svc = MockScreenSaver(service_bus, "/org/freedesktop/ScreenSaver")

client_bus = dbus.SessionBus(private=True)
proxy = client_bus.get_object("org.freedesktop.ScreenSaver", "/org/freedesktop/ScreenSaver")
iface = dbus.Interface(proxy, "org.freedesktop.ScreenSaver")

loop = GLib.MainLoop()

def on_error(err):
    print(f"ERROR {err}", file=sys.stderr, flush=True)
    loop.quit()

iface.Inhibit(sys.argv[1], sys.argv[2], reply_handler=lambda _c: None, error_handler=on_error)
loop.run()

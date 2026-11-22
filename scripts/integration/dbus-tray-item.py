#!/usr/bin/env python3
import sys
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

DBusGMainLoop(set_as_default=True)


class MockStatusNotifierWatcher(dbus.service.Object):
    def __init__(self, bus, object_path):
        super().__init__(bus, object_path)
        self.items = []

    @dbus.service.signal("org.kde.StatusNotifierWatcher", signature="s")
    def StatusNotifierItemRegistered(self, service):
        pass

    @dbus.service.method(
        "org.kde.StatusNotifierWatcher",
        in_signature="s",
        out_signature="",
        sender_keyword="sender",
    )
    def RegisterStatusNotifierItem(self, service, sender=None):
        entry = f"{sender}{service}" if service.startswith("/") else f"{service}/StatusNotifierItem"
        if entry not in self.items:
            self.items.append(entry)
        self.StatusNotifierItemRegistered(entry)

    @dbus.service.method(
        "org.freedesktop.DBus.Properties", in_signature="ss", out_signature="v"
    )
    def Get(self, interface_name, prop_name):
        if interface_name == "org.kde.StatusNotifierWatcher" and prop_name == "RegisteredStatusNotifierItems":
            return dbus.Array(self.items, signature="s")
        return dbus.String("")


class MockStatusNotifierItem(dbus.service.Object):
    def __init__(self, bus, object_path, icon_path):
        super().__init__(bus, object_path)
        self.icon_path = icon_path

    @dbus.service.method(
        "org.freedesktop.DBus.Properties", in_signature="s", out_signature="a{sv}"
    )
    def GetAll(self, interface_name):
        if interface_name != "org.kde.StatusNotifierItem":
            return dbus.Dictionary({}, signature="sv")
        return dbus.Dictionary(
            {
                "Id": dbus.String("mock-tray-item"),
                "Title": dbus.String("Mock Tray"),
                "IconName": dbus.String(self.icon_path),
                "IconThemePath": dbus.String(""),
                "IconPixmap": dbus.Array([], signature="(iiay)"),
            },
            signature="sv",
        )


icon_path = sys.argv[1]
bus = dbus.SessionBus(private=True)
watcher_name = dbus.service.BusName("org.kde.StatusNotifierWatcher", bus)
watcher = MockStatusNotifierWatcher(bus, "/StatusNotifierWatcher")
item = MockStatusNotifierItem(bus, "/StatusNotifierItem", icon_path)

service_entry = f"{bus.get_unique_name()}/StatusNotifierItem"
watcher.items.append(service_entry)
watcher.StatusNotifierItemRegistered(service_entry)

loop = GLib.MainLoop()
loop.run()

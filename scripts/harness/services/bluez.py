#!/usr/bin/env python3
"""Minimal org.bluez D-Bus service on an isolated system bus."""

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

BLUEZ_BUS = "org.bluez"
ADAPTER_IFACE = "org.bluez.Adapter1"
DEVICE_IFACE = "org.bluez.Device1"
BATTERY_IFACE = "org.bluez.Battery1"
OBJ_MANAGER_IFACE = "org.freedesktop.DBus.ObjectManager"
PROPS_IFACE = "org.freedesktop.DBus.Properties"

DBusGMainLoop(set_as_default=True)


class MockAdapter(dbus.service.Object):
    def __init__(self, bus, path, name="hci0", powered=True):
        super().__init__(bus, path)
        self.path = path
        self.name = name
        self.powered = powered

    def interfaces(self):
        return dbus.Dictionary(
            {
                ADAPTER_IFACE: dbus.Dictionary(
                    {
                        "Alias": dbus.String(self.name),
                        "Name": dbus.String(self.name),
                        "Powered": dbus.Boolean(self.powered),
                    },
                    signature="sv",
                )
            },
            signature="sa{sv}",
        )

    @dbus.service.signal(PROPS_IFACE, signature="sa{sv}as")
    def PropertiesChanged(self, interface, changed, invalidated):
        pass

    @dbus.service.method(PROPS_IFACE, in_signature="ssv", out_signature="")
    def Set(self, interface, prop, value):
        if interface == ADAPTER_IFACE and prop == "Powered":
            self.powered = bool(value)
            self.PropertiesChanged(
                ADAPTER_IFACE,
                dbus.Dictionary({"Powered": dbus.Boolean(self.powered)}, signature="sv"),
                dbus.Array([], signature="s"),
            )


class MockDevice(dbus.service.Object):
    def __init__(self, bus, path, name, address, paired, connected, battery):
        super().__init__(bus, path)
        self.path = path
        self.name = name
        self.address = address
        self.paired = paired
        self.connected = connected
        self.battery = battery

    def interfaces(self):
        return dbus.Dictionary(
            {
                DEVICE_IFACE: dbus.Dictionary(
                    {
                        "Alias": dbus.String(self.name),
                        "Name": dbus.String(self.name),
                        "Address": dbus.String(self.address),
                        "Paired": dbus.Boolean(self.paired),
                        "Connected": dbus.Boolean(self.connected),
                    },
                    signature="sv",
                ),
                BATTERY_IFACE: dbus.Dictionary(
                    {"Percentage": dbus.Byte(self.battery)},
                    signature="sv",
                ),
            },
            signature="sa{sv}",
        )

    @dbus.service.signal(PROPS_IFACE, signature="sa{sv}as")
    def PropertiesChanged(self, interface, changed, invalidated):
        pass

    @dbus.service.method(DEVICE_IFACE, in_signature="", out_signature="")
    def Connect(self):
        if not self.connected:
            self.connected = True
            self.PropertiesChanged(
                DEVICE_IFACE,
                dbus.Dictionary({"Connected": dbus.Boolean(True)}, signature="sv"),
                dbus.Array([], signature="s"),
            )

    @dbus.service.method(DEVICE_IFACE, in_signature="", out_signature="")
    def Disconnect(self):
        if self.connected:
            self.connected = False
            self.PropertiesChanged(
                DEVICE_IFACE,
                dbus.Dictionary({"Connected": dbus.Boolean(False)}, signature="sv"),
                dbus.Array([], signature="s"),
            )


class MockObjectManager(dbus.service.Object):
    def __init__(self, bus, objects):
        super().__init__(bus, "/")
        self.objects = objects

    @dbus.service.method(OBJ_MANAGER_IFACE, in_signature="", out_signature="a{oa{sa{sv}}}")
    def GetManagedObjects(self):
        return dbus.Dictionary(
            {dbus.ObjectPath(obj.path): obj.interfaces() for obj in self.objects},
            signature="oa{sa{sv}}",
        )


def main():
    bus = dbus.SystemBus(private=True)

    adapter = MockAdapter(bus, "/org/bluez/hci0", name="hci0", powered=True)
    dev_headphones = MockDevice(
        bus,
        "/org/bluez/hci0/dev_00_11_22_33_44_55",
        name="WH-1000XM5",
        address="00:11:22:33:44:55",
        paired=True,
        connected=True,
        battery=85,
    )
    dev_mouse = MockDevice(
        bus,
        "/org/bluez/hci0/dev_00_11_22_33_44_66",
        name="MX Master 3S",
        address="00:11:22:33:44:66",
        paired=True,
        connected=False,
        battery=64,
    )
    _manager = MockObjectManager(bus, [adapter, dev_headphones, dev_mouse])
    _bus_name = dbus.service.BusName(BLUEZ_BUS, bus)

    GLib.MainLoop().run()


if __name__ == "__main__":
    main()

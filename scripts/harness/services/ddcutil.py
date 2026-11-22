#!/usr/bin/env python3
"""Minimal com.ddcutil.DdcutilService D-Bus service on the session bus."""

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

DDC_BUS = "com.ddcutil.DdcutilService"
DDC_PATH = "/com/ddcutil/DdcutilObject"
DDC_IFACE = "com.ddcutil.DdcutilInterface"
VCP_BRIGHTNESS = 0x10

DBusGMainLoop(set_as_default=True)


class MockDdcutilService(dbus.service.Object):
    def __init__(self, bus):
        super().__init__(bus, DDC_PATH)
        self.brightness = 78

    @dbus.service.signal(DDC_IFACE, signature="isyqssu")
    def VcpValueChanged(self, disp_num, edid, vcp_code, new_val, s1, s2, flags):
        pass

    @dbus.service.method(DDC_IFACE, in_signature="u", out_signature="ia(iiisssqsu)is")
    def Detect(self, _flags):
        displays = dbus.Array(
            [
                (
                    dbus.Int32(1),
                    dbus.Int32(-1),
                    dbus.Int32(-1),
                    dbus.String("DEL"),
                    dbus.String("Studio Display"),
                    dbus.String("SN123"),
                    dbus.UInt16(0),
                    dbus.String(""),
                    dbus.UInt32(0),
                )
            ],
            signature="(iiisssqsu)",
        )
        return dbus.Int32(1), displays, dbus.Int32(0), dbus.String("")

    @dbus.service.method(DDC_IFACE, in_signature="isyu", out_signature="qqsis")
    def GetVcp(self, disp_num, _edid, vcp_code, _flags):
        if int(disp_num) != 1 or int(vcp_code) != VCP_BRIGHTNESS:
            return (
                dbus.UInt16(0),
                dbus.UInt16(100),
                dbus.String(""),
                dbus.Int32(-1),
                dbus.String("Unsupported VCP"),
            )
        return (
            dbus.UInt16(self.brightness),
            dbus.UInt16(100),
            dbus.String(str(self.brightness)),
            dbus.Int32(0),
            dbus.String(""),
        )

    @dbus.service.method(DDC_IFACE, in_signature="isyqu", out_signature="is")
    def SetVcp(self, disp_num, _edid, vcp_code, new_value, _flags):
        if int(disp_num) != 1 or int(vcp_code) != VCP_BRIGHTNESS:
            return dbus.Int32(-1), dbus.String("Unsupported VCP")
        self.brightness = max(0, min(100, int(new_value)))
        self.VcpValueChanged(
            dbus.Int32(1),
            dbus.String(""),
            dbus.Byte(VCP_BRIGHTNESS),
            dbus.UInt16(self.brightness),
            dbus.String(""),
            dbus.String(""),
            dbus.UInt32(0),
        )
        return dbus.Int32(0), dbus.String("")


def main():
    bus = dbus.SessionBus(private=True)
    _service = MockDdcutilService(bus)
    _bus_name = dbus.service.BusName(DDC_BUS, bus)

    GLib.MainLoop().run()


if __name__ == "__main__":
    main()

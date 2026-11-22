#!/usr/bin/env python3
import json
import sys

import dbus
import dbus.mainloop.glib
from gi.repository import GLib


def get_iface():
    bus = dbus.SessionBus()
    proxy = bus.get_object("org.freedesktop.Notifications", "/org/freedesktop/Notifications")
    return dbus.Interface(proxy, "org.freedesktop.Notifications")


def cmd_notify(payload_json: str) -> None:
    payload = json.loads(payload_json)
    hints = {}
    for k, v in payload.get("hints", {}).items():
        if isinstance(v, bool):
            hints[k] = dbus.Boolean(v)
        elif isinstance(v, int):
            hints[k] = dbus.Byte(v) if k == "urgency" else dbus.Int32(v)
        else:
            hints[k] = dbus.String(str(v))

    nid = get_iface().Notify(
        payload.get("app_name", "test-app"),
        dbus.UInt32(payload.get("replaces_id", 0)),
        payload.get("app_icon", ""),
        payload.get("summary", ""),
        payload.get("body", ""),
        dbus.Array(payload.get("actions", []), signature="s"),
        dbus.Dictionary(hints, signature="sv"),
        dbus.Int32(payload.get("expire_timeout", -1)),
    )
    print(int(nid), flush=True)


def cmd_close(nid: int) -> None:
    get_iface().CloseNotification(dbus.UInt32(nid))


def cmd_watch(out_path: str) -> None:
    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()

    def write_ev(obj):
        with open(out_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(obj) + "\n")
            f.flush()

    def on_action(nid, key):
        write_ev({"signal": "action_invoked", "id": int(nid), "key": str(key)})

    def on_closed(nid, reason):
        write_ev({"signal": "notification_closed", "id": int(nid), "reason": int(reason)})

    bus.add_signal_receiver(
        on_action,
        signal_name="ActionInvoked",
        dbus_interface="org.freedesktop.Notifications",
        path="/org/freedesktop/Notifications",
    )
    bus.add_signal_receiver(
        on_closed,
        signal_name="NotificationClosed",
        dbus_interface="org.freedesktop.Notifications",
        path="/org/freedesktop/Notifications",
    )
    # Synchronous D-Bus roundtrip ensures the daemon has processed AddMatch
    # before we advertise readiness to the test runner.
    bus.name_has_owner("org.freedesktop.Notifications")
    print("ready", flush=True)
    GLib.MainLoop().run()


def main() -> None:
    if len(sys.argv) < 2:
        raise SystemExit("usage: dbus-notify.py <notify|close|watch> ...")
    cmd = sys.argv[1]
    if cmd == "notify":
        cmd_notify(sys.argv[2])
    elif cmd == "close":
        cmd_close(int(sys.argv[2]))
    elif cmd == "watch":
        cmd_watch(sys.argv[2])
    else:
        raise SystemExit(f"unknown command: {cmd}")


if __name__ == "__main__":
    main()

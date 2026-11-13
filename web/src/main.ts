import type { Audio, AudioDevice } from "@domicile-desktop/system-audio/audio";
import { soundServer } from "@domicile-desktop/system-audio/sound-server";
import {
  type Bluetooth,
  setPowered,
  watchBluetooth,
} from "@domicile-desktop/system-bluetooth/bluetooth";
import {
  Connectivity,
  LinkKind,
  type Network,
  watchNetwork,
} from "@domicile-desktop/system-network/network";
import type {
  NotificationItem,
  SwayState,
  TrayItem,
} from "./global";

const { domicile, sway, tray, notifications } = window;
const { dbus, processes } = domicile;

const targetOutput = new URLSearchParams(window.location.search).get("output");

const workspacesEl = document.getElementById("workspaces")!;
const modeEl = document.getElementById("sway-mode")!;
const focusedTitleEl = document.getElementById("focused-title")!;
const clockEl = document.getElementById("clock")!;
const trayEl = document.getElementById("tray")!;
const audioButton = document.getElementById("audio-status") as HTMLButtonElement;
const networkEl = document.getElementById("network-status")!;
const btButton = document.getElementById("bt-status") as HTMLButtonElement;
const notifEl = document.getElementById("notif-status") as HTMLButtonElement;
const notifClearBtn = document.getElementById("notif-clear") as HTMLButtonElement;

const audio = soundServer({ processes });
let defaultOutput: AudioDevice | undefined;
let latestBluetooth: Bluetooth | undefined;
let currentNotifications: NotificationItem[] = [];

function updateClock(): void {
  const now = new Date();
  clockEl.textContent = now.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function renderSway(state: SwayState): void {
  const visibleWorkspaces = targetOutput
    ? state.workspaces.filter((ws) => ws.output === targetOutput)
    : state.workspaces;

  workspacesEl.replaceChildren(
    ...visibleWorkspaces.map((ws) => {
      const btn = document.createElement("button");
      btn.className = "pill clickable ws-pill";
      btn.textContent = ws.name;
      btn.dataset.focused = String(ws.focused);
      btn.dataset.visible = String(ws.visible);
      btn.dataset.urgent = String(ws.urgent);
      btn.title = `${ws.output}: ${ws.name}`;
      btn.addEventListener("click", () => {
        const escaped = ws.name.replace(/"/g, '\\"');
        sway.command(`workspace "${escaped}"`);
      });
      return btn;
    }),
  );

  if (state.mode && state.mode !== "default") {
    modeEl.textContent = state.mode;
    modeEl.hidden = false;
  } else {
    modeEl.hidden = true;
  }

  focusedTitleEl.textContent = state.focused_title;
  focusedTitleEl.title = state.focused_title;
}

function renderTray(items: TrayItem[]): void {
  trayEl.replaceChildren(
    ...items.map((item) => {
      const btn = document.createElement("button");
      btn.className = "pill clickable tray-pill";
      const label = item.title || item.icon_name || item.bus;
      btn.textContent = label;
      btn.title = `${label} (${item.bus}${item.path})`;
      btn.addEventListener("click", () => {
        tray.activate(item.bus, item.path);
      });
      return btn;
    }),
  );
}

function renderAudio(state: Audio): void {
  defaultOutput = state.outputs.find((o) => o.default) ?? state.outputs[0];
  if (!defaultOutput) {
    audioButton.textContent = "Vol: --";
    audioButton.dataset.state = "unavailable";
    audioButton.title = "No audio output";
    return;
  }
  const pct = Math.round(defaultOutput.volume * 100);
  audioButton.textContent = defaultOutput.muted
    ? `Muted (${pct}%)`
    : `Vol: ${pct}%`;
  audioButton.dataset.state = defaultOutput.muted ? "muted" : "active";
  audioButton.title = `${defaultOutput.description} (click to mute, scroll to adjust)`;
}

function renderNetwork(net: Network): void {
  let text = "Net: Offline";
  let state = "offline";

  switch (net.link.kind) {
    case LinkKind.Wifi: {
      const pct = Math.round(net.link.strength * 100);
      text = `Wi-Fi: ${net.link.ssid} (${pct}%)`;
      state = "connected";
      break;
    }
    case LinkKind.Wired:
      text = `Eth: ${net.link.name}`;
      state = "connected";
      break;
    case LinkKind.Other:
      text = `Net: ${net.link.name}`;
      state = "connected";
      break;
    case LinkKind.None:
      text = "Net: Offline";
      state = "offline";
      break;
  }

  if (net.link.kind !== LinkKind.None) {
    if (net.connectivity === Connectivity.Portal) {
      text += " [Portal]";
      state = "portal";
    } else if (net.connectivity === Connectivity.Limited) {
      text += " [Limited]";
      state = "limited";
    }
  }

  networkEl.textContent = text;
  networkEl.dataset.state = state;
  networkEl.title = text;
}

function renderBluetooth(bt: Bluetooth): void {
  latestBluetooth = bt;
  const adapter = bt.adapters[0];
  if (!adapter) {
    btButton.textContent = "BT: No adapter";
    btButton.dataset.state = "off";
    return;
  }
  if (!adapter.powered) {
    btButton.textContent = "BT: Off";
    btButton.dataset.state = "off";
    return;
  }

  const connected = bt.devices.filter((d) => d.connected);
  btButton.dataset.state = connected.length > 0 ? "connected" : "on";
  if (connected.length === 0) {
    btButton.textContent = "BT: On";
  } else {
    const names = connected.map((d) =>
      d.battery !== undefined ? `${d.name} (${d.battery}%)` : d.name,
    );
    btButton.textContent = `BT: ${names.join(", ")}`;
  }
}

function renderNotifications(items: NotificationItem[]): void {
  if (items.length === 0) {
    notifEl.textContent = "No notifications";
    notifEl.dataset.hasItems = "false";
    notifClearBtn.hidden = true;
    return;
  }
  const latest = items[items.length - 1];
  const label = latest.app_name
    ? `[${latest.app_name}] ${latest.summary}`
    : latest.summary;
  notifEl.textContent = `(${items.length}) ${label}`;
  notifEl.title = latest.body || latest.summary;
  notifEl.dataset.hasItems = "true";
  notifClearBtn.hidden = false;
}

audioButton.addEventListener("click", () => {
  if (!defaultOutput) return;
  void audio.setMuted(defaultOutput.id, !defaultOutput.muted);
});

audioButton.addEventListener("wheel", (event) => {
  if (!defaultOutput) return;
  event.preventDefault();
  const step = event.deltaY < 0 ? 0.05 : -0.05;
  const next = Math.max(0, Math.min(1.5, defaultOutput.volume + step));
  void audio.setVolume(defaultOutput.id, next);
});

btButton.addEventListener("click", () => {
  const adapter = latestBluetooth?.adapters[0];
  if (!adapter) return;
  void setPowered({ dbus }, adapter.path, !adapter.powered);
});

notifEl.addEventListener("click", () => {
  const current = currentNotifications[currentNotifications.length - 1];
  if (current) {
    notifications.dismiss(current.id);
  }
});

notifClearBtn.addEventListener("click", () => {
  notifications.clearAll();
});

sway.onState(renderSway);
tray.onChange(renderTray);
audio.watch(renderAudio);

watchNetwork({ dbus }, (result) => {
  result.match({
    Ok: (net) => renderNetwork(net),
    Err: (err) => {
      networkEl.textContent = `Net: ${err.message}`;
      networkEl.dataset.state = "error";
    },
  });
});

watchBluetooth({ dbus }, (result) => {
  result.match({
    Ok: (bt) => renderBluetooth(bt),
    Err: (err) => {
      btButton.textContent = `BT: ${err.message}`;
      btButton.dataset.state = "error";
    },
  });
});

notifications.onChange((items) => {
  currentNotifications = items;
  renderNotifications(items);
});

updateClock();
setInterval(updateClock, 10_000);

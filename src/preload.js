"use strict";
(() => {
  // src/preload.ts
  function createStore(initial) {
    let current = initial;
    const subscribeListeners = /* @__PURE__ */ new Set();
    const changeListeners = /* @__PURE__ */ new Set();
    return {
      get: () => current,
      subscribe: (listener) => {
        subscribeListeners.add(listener);
        return () => {
          subscribeListeners.delete(listener);
        };
      },
      onChange: (listener) => {
        changeListeners.add(listener);
        listener(current);
        return () => {
          changeListeners.delete(listener);
        };
      },
      _update: (next) => {
        current = next;
        for (const listener of subscribeListeners) {
          listener();
        }
        for (const listener of changeListeners) {
          listener(current);
        }
      }
    };
  }
  function decodeBase64Bytes(b64) {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) {
      bytes[i] = bin.charCodeAt(i);
    }
    return bytes;
  }
  function createWebbarBridge(postMessage) {
    let nextId = 1;
    const actionPending = /* @__PURE__ */ new Map();
    const dbusCallPending = /* @__PURE__ */ new Map();
    const dbusWatches = /* @__PURE__ */ new Map();
    const execPending = /* @__PURE__ */ new Map();
    function send(payload) {
      postMessage(JSON.stringify(payload));
    }
    function invokeAction(type, payload = {}) {
      const id = nextId++;
      return new Promise((resolve, reject) => {
        actionPending.set(id, {
          resolve: () => resolve(),
          reject
        });
        send({ type, id, ...payload });
      });
    }
    const swayStore = createStore({
      workspaces: [],
      focused_title: "",
      mode: "default"
    });
    const trayStore = createStore([]);
    const notificationsStore = createStore([]);
    const audioStore = createStore({
      available: false,
      outputs: [],
      inputs: []
    });
    const bluetoothStore = createStore({
      available: false,
      adapters: [],
      devices: []
    });
    const networkStore = createStore({
      available: false,
      connectivity: "unknown",
      link: { kind: "none" }
    });
    const backlightStore = createStore({
      available: false,
      brightness: null,
      error: null
    });
    function receive2(event) {
      switch (event.type) {
        case "action_reply": {
          const id = event.id;
          const pending = actionPending.get(id);
          if (!pending) return;
          actionPending.delete(id);
          if (event.ok) {
            pending.resolve(void 0);
          } else {
            pending.reject(
              new Error(event.error ?? "Action failed")
            );
          }
          break;
        }
        case "tray_menu_reply": {
          const id = event.id;
          const pending = actionPending.get(id);
          if (!pending) return;
          actionPending.delete(id);
          if (event.ok) {
            pending.resolve(event.items);
          } else {
            pending.reject(
              new Error(
                event.error ?? "Failed to fetch tray menu"
              )
            );
          }
          break;
        }
        case "sway_state": {
          swayStore._update({
            workspaces: event.workspaces ?? [],
            focused_title: event.focused_title ?? "",
            mode: event.mode ?? "default"
          });
          break;
        }
        case "tray": {
          trayStore._update(event.items ?? []);
          break;
        }
        case "notifications": {
          notificationsStore._update(event.items ?? []);
          break;
        }
        case "audio_state": {
          audioStore._update({
            available: Boolean(event.available),
            outputs: event.outputs ?? [],
            inputs: event.inputs ?? []
          });
          break;
        }
        case "bluetooth_state": {
          bluetoothStore._update({
            available: Boolean(event.available),
            adapters: event.adapters ?? [],
            devices: event.devices ?? []
          });
          break;
        }
        case "network_state": {
          networkStore._update({
            available: Boolean(event.available),
            connectivity: event.connectivity ?? "unknown",
            link: event.link ?? { kind: "none" }
          });
          break;
        }
        case "backlight_state": {
          backlightStore._update({
            available: Boolean(event.available),
            brightness: typeof event.brightness === "number" ? event.brightness : null,
            error: typeof event.error === "string" ? event.error : null
          });
          break;
        }
        case "dbus_call_reply": {
          const id = event.id;
          const pending = dbusCallPending.get(id);
          if (!pending) return;
          dbusCallPending.delete(id);
          if (event.ok) {
            pending.resolve({
              signature: event.signature ?? "",
              body: JSON.parse(event.body ?? "[]")
            });
          } else {
            pending.reject(
              new Error(
                event.error ?? "D-Bus call failed"
              )
            );
          }
          break;
        }
        case "dbus_match_started": {
          const id = event.id;
          const watch = dbusWatches.get(id);
          if (!watch) return;
          if (!event.ok) {
            dbusWatches.delete(id);
            watch.rejectStart?.(
              new Error(
                event.error ?? "D-Bus match failed"
              )
            );
            return;
          }
          watch.resolveStart?.({
            stop: () => {
              dbusWatches.delete(id);
              send({ type: "dbus_unwatch", id });
            }
          });
          break;
        }
        case "dbus_signal": {
          const watch = dbusWatches.get(event.id);
          if (!watch) return;
          watch.onSignal({
            sender: event.sender,
            path: event.path,
            interface: event.interface,
            member: event.member,
            signature: event.signature,
            body: JSON.parse(event.body ?? "[]")
          });
          break;
        }
        case "dbus_match_ended": {
          dbusWatches.delete(event.id);
          break;
        }
        case "process_started": {
          const id = event.id;
          const pending = execPending.get(id);
          if (!pending) return;
          if (!event.ok) {
            execPending.delete(id);
            pending.reject(
              new Error(
                event.error ?? "Process spawn failed"
              )
            );
          }
          break;
        }
        case "process_output": {
          const pending = execPending.get(event.id);
          if (!pending) return;
          const chunk = decodeBase64Bytes(event.data_b64);
          if (event.stream === "stdout") {
            pending.stdout.push(chunk);
          } else if (event.stream === "stderr") {
            pending.stderr.push(chunk);
          }
          break;
        }
        case "process_exited": {
          const id = event.id;
          const pending = execPending.get(id);
          if (!pending) return;
          execPending.delete(id);
          const decoder = new TextDecoder();
          const joinText = (chunks) => chunks.map((c) => decoder.decode(c, { stream: true })).join("") + decoder.decode();
          pending.resolve({
            code: typeof event.code === "number" ? event.code : void 0,
            signal: typeof event.signal === "number" ? event.signal : void 0,
            stdout: joinText(pending.stdout),
            stderr: joinText(pending.stderr)
          });
          break;
        }
      }
    }
    const api2 = {
      sway: {
        get: swayStore.get,
        subscribe: swayStore.subscribe,
        onChange: swayStore.onChange,
        command: (command) => send({ type: "sway_command", command })
      },
      tray: {
        get: trayStore.get,
        subscribe: trayStore.subscribe,
        onChange: trayStore.onChange,
        activate: (bus, path) => send({ type: "tray_activate", bus, path }),
        getMenu: (bus, menuPath) => {
          const id = nextId++;
          return new Promise((resolve, reject) => {
            actionPending.set(id, {
              resolve: (val) => resolve(val),
              reject
            });
            send({ type: "tray_get_menu", id, bus, menu_path: menuPath });
          });
        },
        menuEvent: (bus, menuPath, itemId) => send({
          type: "tray_menu_event",
          bus,
          menu_path: menuPath,
          item_id: itemId
        })
      },
      notifications: {
        get: notificationsStore.get,
        subscribe: notificationsStore.subscribe,
        onChange: notificationsStore.onChange,
        dismiss: (id) => send({ type: "dismiss_notification", id }),
        clearAll: () => send({ type: "clear_notifications" })
      },
      audio: {
        get: audioStore.get,
        subscribe: audioStore.subscribe,
        onChange: audioStore.onChange,
        setVolume: (deviceId, volume) => invokeAction("audio_set_volume", { device_id: deviceId, volume }),
        setMuted: (deviceId, muted) => invokeAction("audio_set_muted", { device_id: deviceId, muted }),
        setDefault: (deviceId) => invokeAction("audio_set_default", { device_id: deviceId })
      },
      bluetooth: {
        get: bluetoothStore.get,
        subscribe: bluetoothStore.subscribe,
        onChange: bluetoothStore.onChange,
        setPowered: (path, powered) => invokeAction("bluetooth_set_powered", { path, powered }),
        connect: (path) => invokeAction("bluetooth_connect", { path }),
        disconnect: (path) => invokeAction("bluetooth_disconnect", { path })
      },
      network: {
        get: networkStore.get,
        subscribe: networkStore.subscribe,
        onChange: networkStore.onChange
      },
      backlight: {
        get: backlightStore.get,
        subscribe: backlightStore.subscribe,
        onChange: backlightStore.onChange,
        set: (percent) => invokeAction("backlight_set", {
          percent: Math.max(0, Math.min(100, Math.round(percent)))
        })
      },
      setInputRegion: (rects) => send({
        type: "set_input_region",
        rects: rects.map((r) => ({
          x: Math.round(r.x),
          y: Math.round(r.y),
          width: Math.round(r.width),
          height: Math.round(r.height)
        }))
      }),
      resetInputRegion: () => send({ type: "set_input_region", rects: [] }),
      dbus: {
        call: (opts) => {
          const id = nextId++;
          return new Promise((resolve, reject) => {
            dbusCallPending.set(id, { resolve, reject });
            send({
              type: "dbus_call",
              id,
              bus: opts.bus ?? "session",
              destination: opts.destination,
              path: opts.path,
              interface: opts.interface,
              member: opts.member,
              signature: opts.signature ?? "",
              body: JSON.stringify(opts.body ?? [])
            });
          });
        },
        watch: (opts, onSignal) => {
          const id = nextId++;
          return new Promise((resolve, reject) => {
            dbusWatches.set(id, {
              resolveStart: resolve,
              rejectStart: reject,
              onSignal
            });
            send({
              type: "dbus_match",
              id,
              bus: opts.bus ?? "session",
              sender: opts.sender ?? null,
              path: opts.path ?? null,
              interface: opts.interface ?? null,
              member: opts.member ?? null
            });
          });
        }
      },
      exec: (argv, opts) => {
        const id = nextId++;
        return new Promise((resolve, reject) => {
          execPending.set(id, {
            resolve,
            reject,
            stdout: [],
            stderr: []
          });
          send({
            type: "process_spawn",
            id,
            argv: [...argv],
            cwd: opts?.cwd ?? null,
            env: opts?.env ?? {},
            stdin: false
          });
        });
      }
    };
    return { api: api2, receive: receive2 };
  }

  // src/index.ts
  var { api, receive } = createWebbarBridge((json) => {
    const handler = window.webkit?.messageHandlers?.bridge;
    if (!handler) {
      console.error("webbar: webkit bridge handler is not available");
      return;
    }
    handler.postMessage(json);
  });
  window.webbar = api;
  window.__webbarReceive = receive;
  var overlayActive = false;
  var overlayScheduled = false;
  var OVERLAY_SELECTOR = ':popover-open, [role="dialog"]:not([hidden]):not([aria-hidden="true"]), [aria-modal="true"]:not([hidden]):not([aria-hidden="true"]), [data-webbar-overlay]:not([hidden])';
  function checkOverlay() {
    overlayScheduled = false;
    let active = false;
    try {
      active = document.querySelector(OVERLAY_SELECTOR) !== null;
    } catch {
      active = document.querySelector(
        '[role="dialog"]:not([hidden]):not([aria-hidden="true"]), [aria-modal="true"]:not([hidden]):not([aria-hidden="true"]), [data-webbar-overlay]:not([hidden])'
      ) !== null;
    }
    if (active !== overlayActive) {
      overlayActive = active;
      window.webkit?.messageHandlers?.bridge?.postMessage(
        JSON.stringify({ type: "set_overlay", active })
      );
    }
  }
  function scheduleOverlayCheck() {
    if (!overlayScheduled) {
      overlayScheduled = true;
      requestAnimationFrame(checkOverlay);
    }
  }
  window.addEventListener("DOMContentLoaded", () => {
    document.addEventListener("toggle", scheduleOverlayCheck, true);
    if (document.body) {
      new MutationObserver(scheduleOverlayCheck).observe(document.body, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: [
          "open",
          "hidden",
          "aria-hidden",
          "role",
          "aria-modal",
          "data-webbar-overlay"
        ]
      });
    }
    scheduleOverlayCheck();
  });
})();

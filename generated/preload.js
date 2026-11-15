"use strict";
(() => {
  // src/preload.ts
  function reconcile(prev, next) {
    if (Object.is(prev, next)) {
      return prev;
    }
    if (prev === null || next === null || typeof prev !== "object" || typeof next !== "object") {
      return next;
    }
    if (Array.isArray(prev) && Array.isArray(next)) {
      let changed2 = prev.length !== next.length;
      const result2 = new Array(next.length);
      for (let i = 0; i < next.length; i++) {
        const nextItem = next[i];
        if (i < prev.length) {
          const reconciledItem = reconcile(prev[i], nextItem);
          result2[i] = reconciledItem;
          if (!Object.is(reconciledItem, prev[i])) {
            changed2 = true;
          }
        } else {
          result2[i] = nextItem;
          changed2 = true;
        }
      }
      return changed2 ? result2 : prev;
    }
    if (Array.isArray(prev) || Array.isArray(next)) {
      return next;
    }
    const prevObj = prev;
    const nextObj = next;
    const nextKeys = Object.keys(nextObj);
    let changed = Object.keys(prevObj).length !== nextKeys.length;
    const result = {};
    for (const key of nextKeys) {
      const nextVal = nextObj[key];
      if (Object.prototype.hasOwnProperty.call(prevObj, key)) {
        const reconciledVal = reconcile(prevObj[key], nextVal);
        result[key] = reconciledVal;
        if (!Object.is(reconciledVal, prevObj[key])) {
          changed = true;
        }
      } else {
        result[key] = nextVal;
        changed = true;
      }
    }
    return changed ? result : prev;
  }
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
        const reconciled = reconcile(current, next);
        if (Object.is(reconciled, current)) {
          return;
        }
        current = reconciled;
        for (const listener of subscribeListeners) {
          listener();
        }
        for (const listener of changeListeners) {
          listener(current);
        }
      }
    };
  }
  function storeView(store, extra) {
    return {
      get: store.get,
      subscribe: store.subscribe,
      onChange: store.onChange,
      ...extra
    };
  }
  function createWebbarBridge(postMessage, output = "") {
    let nextId = 1;
    const pending = /* @__PURE__ */ new Map();
    const dbusWatches = /* @__PURE__ */ new Map();
    function send(payload) {
      postMessage(JSON.stringify(payload));
    }
    function rpc(type, payload = {}, abort) {
      if (abort?.signal?.aborted) {
        return Promise.reject(new DOMException("Aborted", "AbortError"));
      }
      const id = nextId++;
      return new Promise((resolve, reject) => {
        const onAbort = () => {
          const entry = pending.get(id);
          entry?.cleanup?.();
          pending.delete(id);
          abort?.onAbort?.(id);
          reject(new DOMException("Aborted", "AbortError"));
        };
        const cleanup = abort?.signal ? () => abort.signal?.removeEventListener("abort", onAbort) : void 0;
        abort?.signal?.addEventListener("abort", onAbort, { once: true });
        pending.set(id, {
          resolve,
          reject,
          cleanup
        });
        send({ type, id, ...payload });
      });
    }
    const stores = {
      sway: createStore({
        workspaces: [],
        focused_title: "",
        mode: "default"
      }),
      tray: createStore([]),
      notifications: createStore([]),
      audio: createStore({
        available: false,
        outputs: [],
        inputs: []
      }),
      bluetooth: createStore({
        available: false,
        adapters: [],
        devices: []
      }),
      network: createStore({
        available: false,
        connectivity: "unknown",
        link: { kind: "none" }
      }),
      backlight: createStore({
        available: false,
        brightness: null,
        error: null
      })
    };
    const stateHandlers = {
      sway_state: (e) => stores.sway._update({
        workspaces: e.workspaces ?? [],
        focused_title: e.focused_title ?? "",
        mode: e.mode ?? "default"
      }),
      tray: (e) => stores.tray._update(e.items ?? []),
      notifications: (e) => stores.notifications._update(e.items ?? []),
      audio_state: (e) => stores.audio._update({
        available: Boolean(e.available),
        outputs: e.outputs ?? [],
        inputs: e.inputs ?? []
      }),
      bluetooth_state: (e) => stores.bluetooth._update({
        available: Boolean(e.available),
        adapters: e.adapters ?? [],
        devices: e.devices ?? []
      }),
      network_state: (e) => stores.network._update({
        available: Boolean(e.available),
        connectivity: e.connectivity ?? "unknown",
        link: e.link ?? { kind: "none" }
      }),
      backlight_state: (e) => stores.backlight._update({
        available: Boolean(e.available),
        brightness: typeof e.brightness === "number" ? e.brightness : null,
        error: typeof e.error === "string" ? e.error : null
      })
    };
    function receive2(event) {
      const type = event.type;
      const stateHandler = stateHandlers[type];
      if (stateHandler) {
        stateHandler(event);
        return;
      }
      switch (type) {
        case "rpc_reply": {
          const id = event.id;
          const entry = pending.get(id);
          if (!entry) return;
          entry.cleanup?.();
          pending.delete(id);
          if (event.ok) {
            entry.resolve(event.data);
          } else {
            entry.reject(
              new Error(event.error ?? "Request failed")
            );
          }
          break;
        }
        case "dbus_match_started": {
          const id = event.id;
          const watch = dbusWatches.get(id);
          if (!watch) return;
          if (!event.ok) {
            watch.cleanup?.();
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
              watch.cleanup?.();
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
            body: event.body ?? []
          });
          break;
        }
        case "dbus_match_ended": {
          const id = event.id;
          const watch = dbusWatches.get(id);
          watch?.cleanup?.();
          dbusWatches.delete(id);
          break;
        }
      }
    }
    const api2 = {
      output,
      sway: storeView(stores.sway, {
        command: (command) => send({ type: "sway_command", command })
      }),
      tray: storeView(stores.tray, {
        activate: (bus, path) => send({ type: "tray_activate", bus, path }),
        getMenu: (bus, menuPath) => rpc("tray_get_menu", { bus, menu_path: menuPath }),
        menuEvent: (bus, menuPath, itemId) => send({
          type: "tray_menu_event",
          bus,
          menu_path: menuPath,
          item_id: itemId
        })
      }),
      notifications: storeView(stores.notifications, {
        dismiss: (id) => send({ type: "dismiss_notification", id }),
        invokeAction: (id, actionKey) => send({
          type: "invoke_notification_action",
          id,
          action_key: actionKey
        }),
        clearAll: () => send({ type: "clear_notifications" })
      }),
      audio: storeView(stores.audio, {
        setVolume: (deviceId, volume) => rpc("audio_set_volume", { device_id: deviceId, volume }),
        setMuted: (deviceId, muted) => rpc("audio_set_muted", { device_id: deviceId, muted }),
        setDefault: (deviceId) => rpc("audio_set_default", { device_id: deviceId })
      }),
      bluetooth: storeView(stores.bluetooth, {
        setPowered: (path, powered) => rpc("bluetooth_set_powered", { path, powered }),
        connect: (path) => rpc("bluetooth_connect", { path }),
        disconnect: (path) => rpc("bluetooth_disconnect", { path })
      }),
      network: storeView(stores.network, {}),
      backlight: storeView(stores.backlight, {
        set: (percent) => rpc("backlight_set", { percent })
      }),
      setInputRegion: (rects) => send({ type: "set_input_region", rects }),
      resetInputRegion: () => send({ type: "set_input_region", rects: [] }),
      dbus: {
        call: (opts) => rpc("dbus_call", {
          bus: opts.bus ?? "session",
          destination: opts.destination,
          path: opts.path,
          interface: opts.interface,
          member: opts.member,
          signature: opts.signature ?? "",
          body: opts.body ?? []
        }),
        watch: (opts, onSignal) => {
          if (opts.signal?.aborted) {
            return Promise.reject(new DOMException("Aborted", "AbortError"));
          }
          const id = nextId++;
          return new Promise((resolve, reject) => {
            const onAbort = () => {
              const watch = dbusWatches.get(id);
              watch?.cleanup?.();
              dbusWatches.delete(id);
              send({ type: "dbus_unwatch", id });
              reject(new DOMException("Aborted", "AbortError"));
            };
            const cleanup = () => {
              opts.signal?.removeEventListener("abort", onAbort);
            };
            opts.signal?.addEventListener("abort", onAbort, { once: true });
            dbusWatches.set(id, {
              resolveStart: resolve,
              rejectStart: reject,
              onSignal,
              cleanup
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
      exec: (argv, opts) => rpc(
        "process_spawn",
        {
          argv: [...argv],
          cwd: opts?.cwd ?? null,
          env: opts?.env ?? {}
        },
        {
          signal: opts?.signal,
          onAbort: (id) => send({ type: "process_kill", id, signal: 15 })
        }
      )
    };
    return { api: api2, receive: receive2 };
  }

  // src/index.ts
  var initialOutput = window.__webbarOutput || new URLSearchParams(window.location.search).get("output") || "";
  var { api, receive } = createWebbarBridge((json) => {
    const handler = window.webkit?.messageHandlers?.bridge;
    if (!handler) {
      console.error("webbar: webkit bridge handler is not available");
      return;
    }
    handler.postMessage(json);
  }, initialOutput);
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

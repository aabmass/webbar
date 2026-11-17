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
  function createWebbarBridge(postMessage, output = "", isNativeOverlayActive = () => false) {
    let nextId = 1;
    let explicitOverlayCount = 0;
    let sentOverlayActive = false;
    let deactivateScheduled = false;
    const pending = /* @__PURE__ */ new Map();
    const dbusWatches = /* @__PURE__ */ new Map();
    const processStreams = /* @__PURE__ */ new Map();
    function send(payload) {
      postMessage(JSON.stringify(payload));
    }
    function computeOverlayActive() {
      return explicitOverlayCount > 0 || isNativeOverlayActive();
    }
    function syncOverlay() {
      if (computeOverlayActive()) {
        deactivateScheduled = false;
        if (!sentOverlayActive) {
          sentOverlayActive = true;
          send({ type: "set_overlay", active: true });
        }
        return;
      }
      if (!sentOverlayActive || deactivateScheduled) {
        return;
      }
      deactivateScheduled = true;
      queueMicrotask(() => {
        if (!deactivateScheduled) return;
        deactivateScheduled = false;
        if (!computeOverlayActive() && sentOverlayActive) {
          sentOverlayActive = false;
          send({ type: "set_overlay", active: false });
        }
      });
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
    function receive(event) {
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
        case "process_stream_started": {
          const id = event.id;
          const entry = processStreams.get(id);
          if (!entry) return;
          if (!event.ok) {
            entry.cleanup?.();
            processStreams.delete(id);
            entry.rejectStart?.(
              new Error(
                event.error ?? "Failed to start process"
              )
            );
            return;
          }
          const resolveStart = entry.resolveStart;
          entry.resolveStart = void 0;
          entry.rejectStart = void 0;
          resolveStart?.({
            stop: () => {
              if (!processStreams.has(id)) return;
              entry.cleanup?.();
              processStreams.delete(id);
              send({ type: "process_kill", id, signal: 15 });
            }
          });
          break;
        }
        case "process_stream_output": {
          const entry = processStreams.get(event.id);
          if (!entry) return;
          const line = event.line ?? "";
          if (event.stream === "stderr") {
            entry.onStderr?.(line);
          } else {
            entry.onStdout?.(line);
          }
          break;
        }
        case "process_stream_ended": {
          const id = event.id;
          const entry = processStreams.get(id);
          if (!entry) return;
          entry.cleanup?.();
          processStreams.delete(id);
          entry.onExit?.({
            code: typeof event.code === "number" ? event.code : null,
            signal: typeof event.signal === "number" ? event.signal : null
          });
          break;
        }
      }
    }
    const api = {
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
      acquireOverlay: () => {
        explicitOverlayCount += 1;
        syncOverlay();
        let released = false;
        return () => {
          if (released) return;
          released = true;
          explicitOverlayCount = Math.max(0, explicitOverlayCount - 1);
          syncOverlay();
        };
      },
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
      ),
      execStream: (argv, opts) => {
        if (opts?.signal?.aborted) {
          return Promise.reject(new DOMException("Aborted", "AbortError"));
        }
        const id = nextId++;
        return new Promise((resolve, reject) => {
          const onAbort = () => {
            const entry = processStreams.get(id);
            if (!entry) return;
            entry.cleanup?.();
            processStreams.delete(id);
            send({ type: "process_kill", id, signal: 15 });
            entry.rejectStart?.(new DOMException("Aborted", "AbortError"));
          };
          const cleanup = opts?.signal ? () => opts.signal?.removeEventListener("abort", onAbort) : void 0;
          opts?.signal?.addEventListener("abort", onAbort, { once: true });
          processStreams.set(id, {
            resolveStart: resolve,
            rejectStart: reject,
            onStdout: opts?.onStdout,
            onStderr: opts?.onStderr,
            onExit: opts?.onExit,
            cleanup
          });
          send({
            type: "process_stream_spawn",
            id,
            argv: [...argv],
            cwd: opts?.cwd ?? null,
            env: opts?.env ?? {}
          });
        });
      }
    };
    return { api, receive, notifyOverlayChange: syncOverlay };
  }

  // src/index.ts
  if (!window.webbar) {
    const params = new URLSearchParams(window.location.search);
    const output = params.get("output") || "";
    const wsPort = params.get("wsPort") || params.get("ws") || "9123";
    const wsToken = params.get("wsToken") || params.get("token") || "";
    const query = [
      wsToken && `token=${encodeURIComponent(wsToken)}`,
      output && `output=${encodeURIComponent(output)}`
    ].filter(Boolean).join("&");
    const url = `ws://127.0.0.1:${wsPort}${query ? `?${query}` : ""}`;
    const pending = [];
    let ws = null;
    let delay = 500;
    const { api, receive, notifyOverlayChange } = createWebbarBridge(
      (json) => {
        if (ws?.readyState === WebSocket.OPEN) {
          ws.send(json);
        } else if (pending.length < 64) {
          pending.push(json);
        }
      },
      output,
      () => {
        try {
          return document.querySelector(":popover-open") !== null;
        } catch {
          return false;
        }
      }
    );
    window.webbar = api;
    document.addEventListener("toggle", notifyOverlayChange, true);
    if (document.readyState === "loading") {
      window.addEventListener("DOMContentLoaded", notifyOverlayChange, {
        once: true
      });
    } else {
      notifyOverlayChange();
    }
    const connect = () => {
      const socket = ws = new WebSocket(url);
      socket.addEventListener("open", () => {
        delay = 500;
        while (pending.length > 0 && socket.readyState === WebSocket.OPEN) {
          socket.send(pending.shift());
        }
      });
      socket.addEventListener("message", (ev) => {
        if (typeof ev.data === "string") {
          receive(JSON.parse(ev.data));
        }
      });
      socket.addEventListener("close", () => {
        if (ws === socket) ws = null;
        pending.length = 0;
        window.setTimeout(connect, delay);
        delay = Math.min(delay * 2, 5e3);
      });
    };
    connect();
  }
})();

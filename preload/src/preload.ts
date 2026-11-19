import type { AudioDevice } from "./generated/AudioDevice";
import type { AudioState } from "./generated/AudioState";
import type { BacklightState } from "./generated/BacklightState";
import type { BluetoothAdapter } from "./generated/BluetoothAdapter";
import type { BluetoothDevice } from "./generated/BluetoothDevice";
import type { BluetoothState } from "./generated/BluetoothState";
import type { Connectivity } from "./generated/Connectivity";
import type { ExecResult } from "./generated/ExecResult";
import type { InputRect } from "./generated/InputRect";
import type { NetworkLink } from "./generated/NetworkLink";
import type { NetworkState } from "./generated/NetworkState";
import type { NotificationAction } from "./generated/NotificationAction";
import type { NotificationItem } from "./generated/NotificationItem";
import type { NotificationUrgency } from "./generated/NotificationUrgency";
import type { SwayCommandError as SwayCommandErrorData } from "./generated/SwayCommandError";
import type { SwayNode, SwayState, SwayWorkspace } from "./generated/sway";
import type { TrayItem } from "./generated/TrayItem";
import type { TrayMenuItem } from "./generated/TrayMenuItem";

export type * from "./generated/sway";

export class SwayCommandError extends Error {
  readonly parseError: boolean;

  constructor(data: SwayCommandErrorData) {
    super(data.message);
    this.name = "SwayCommandError";
    this.parseError = data.parse_error;
  }
}

export type {
  AudioDevice,
  AudioState,
  BacklightState,
  BluetoothAdapter,
  BluetoothDevice,
  BluetoothState,
  Connectivity,
  ExecResult,
  InputRect,
  NetworkLink,
  NetworkState,
  NotificationAction,
  NotificationItem,
  NotificationUrgency,
  TrayItem,
  TrayMenuItem,
};

export type Store<T> = {
  get: () => T;
  subscribe: (listener: () => void) => () => void;
  onChange: (listener: (value: T) => void) => () => void;
};

export type InternalStore<T> = Store<T> & {
  _update: (next: T) => void;
};

export type DbusCallOptions = {
  bus?: "session" | "system";
  destination: string;
  path: string;
  interface: string;
  member: string;
  signature?: string;
  body?: unknown[];
};

export type DbusWatchOptions = {
  bus?: "session" | "system";
  sender?: string;
  path?: string;
  interface?: string;
  member?: string;
  onClose?: (error?: Error) => void;
  signal?: AbortSignal;
};

export type DbusSignalEvent = {
  sender: string;
  path: string;
  interface: string;
  member: string;
  signature: string;
  body: unknown[];
};

export type ExecOptions = {
  cwd?: string;
  env?: Record<string, string>;
  signal?: AbortSignal;
};

export type ExecStreamOptions = ExecOptions & {
  onStdout?: (line: string) => void;
  onStderr?: (line: string) => void;
  onExit?: (result: { code: number | null; signal: number | null }) => void;
};

export type ExecStreamHandle = {
  stop: () => void;
};

export type WebbarApi = {
  readonly output: string;
  sway: Store<SwayState> & {
    command: (cmd: string) => Promise<void>;
  };
  tray: Store<TrayItem[]> & {
    activate: (bus: string, path: string) => void;
    getMenu: (bus: string, menuPath: string) => Promise<TrayMenuItem[]>;
    menu: (bus: string, menuPath: string) => Store<TrayMenuItem[]>;
    menuEvent: (bus: string, menuPath: string, itemId: number) => void;
  };
  notifications: Store<NotificationItem[]> & {
    dismiss: (id: number) => void;
    invokeAction: (id: number, actionKey: string) => void;
    clearAll: () => void;
  };
  audio: Store<AudioState> & {
    setVolume: (deviceId: number, volume: number) => Promise<void>;
    setMuted: (deviceId: number, muted: boolean) => Promise<void>;
    setDefault: (deviceId: number) => Promise<void>;
  };
  bluetooth: Store<BluetoothState> & {
    setPowered: (path: string, powered: boolean) => Promise<void>;
    connect: (path: string) => Promise<void>;
    disconnect: (path: string) => Promise<void>;
  };
  network: Store<NetworkState>;
  backlight: Store<BacklightState> & {
    set: (percent: number) => Promise<void>;
  };
  acquireOverlay: () => () => void;
  setInputRegion: (rects: InputRect[]) => void;
  resetInputRegion: () => void;
  dbus: {
    call: (
      opts: DbusCallOptions,
    ) => Promise<{ signature: string; body: unknown[] }>;
    watch: (
      opts: DbusWatchOptions,
      onSignal: (signal: DbusSignalEvent) => void,
    ) => Promise<{ stop: () => void }>;
  };
  exec: (argv: string[], opts?: ExecOptions) => Promise<ExecResult>;
  execStream: (
    argv: string[],
    opts?: ExecStreamOptions,
  ) => Promise<ExecStreamHandle>;
};

type Id = string | number;

function idOf(item: unknown): Id | undefined {
  if (item === null || typeof item !== "object" || Array.isArray(item)) {
    return undefined;
  }
  const obj = item as { id?: unknown; bus?: unknown; path?: unknown };
  if (typeof obj.id === "number" || typeof obj.id === "string") {
    return obj.id;
  }
  if (typeof obj.path === "string") {
    return typeof obj.bus === "string" ? `${obj.bus}${obj.path}` : obj.path;
  }
  return undefined;
}

/** Map of id -> item if every item is an object with a unique id, else null. */
function indexById(items: unknown[]): Map<Id, unknown> | null {
  if (items.length === 0) return null;
  const map = new Map<Id, unknown>();
  for (const item of items) {
    const id = idOf(item);
    if (id === undefined || map.has(id)) return null;
    map.set(id, item);
  }
  return map;
}

/**
 * Reconcile arrays by item id, so reordering or inserting items keeps the
 * identity of untouched siblings (e.g. Sway tree nodes when a window moves).
 */
function reconcileById(
  prev: unknown[],
  next: unknown[],
  prevById: Map<Id, unknown>,
): unknown[] {
  let changed = prev.length !== next.length;
  const result = next.map((nextItem, i) => {
    const prevItem = prevById.get(idOf(nextItem)!);
    const item =
      prevItem === undefined ? nextItem : reconcile<unknown>(prevItem, nextItem);
    if (!Object.is(item, prev[i])) changed = true;
    return item;
  });
  return changed ? result : prev;
}

function reconcile<T>(prev: T, next: T): T {
  if (Object.is(prev, next)) {
    return prev;
  }
  if (
    prev === null ||
    next === null ||
    typeof prev !== "object" ||
    typeof next !== "object"
  ) {
    return next;
  }

  if (Array.isArray(prev) && Array.isArray(next)) {
    const byId = indexById(prev);
    if (byId && indexById(next)) {
      return reconcileById(prev, next, byId) as T;
    }
    let changed = prev.length !== next.length;
    const result: unknown[] = new Array(next.length);
    for (let i = 0; i < next.length; i++) {
      const nextItem: unknown = next[i];
      if (i < prev.length) {
        const reconciledItem = reconcile<unknown>(prev[i], nextItem);
        result[i] = reconciledItem;
        if (!Object.is(reconciledItem, prev[i])) {
          changed = true;
        }
      } else {
        result[i] = nextItem;
        changed = true;
      }
    }
    return changed ? (result as T) : prev;
  }

  if (Array.isArray(prev) || Array.isArray(next)) {
    return next;
  }

  const prevObj = prev as Record<string, unknown>;
  const nextObj = next as Record<string, unknown>;
  const nextKeys = Object.keys(nextObj);
  let changed = Object.keys(prevObj).length !== nextKeys.length;
  const result: Record<string, unknown> = {};

  for (const key of nextKeys) {
    const nextVal = nextObj[key];
    if (Object.prototype.hasOwnProperty.call(prevObj, key)) {
      const reconciledVal = reconcile<unknown>(prevObj[key], nextVal);
      result[key] = reconciledVal;
      if (!Object.is(reconciledVal, prevObj[key])) {
        changed = true;
      }
    } else {
      result[key] = nextVal;
      changed = true;
    }
  }

  return changed ? (result as T) : prev;
}

export function createStore<T>(initial: T): InternalStore<T> {
  let current = initial;
  const subscribeListeners = new Set<() => void>();
  const changeListeners = new Set<(value: T) => void>();

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
    },
  };
}

function storeView<T, E extends Record<string, unknown>>(
  store: Store<T>,
  extra: E,
): Store<T> & E {
  return {
    get: store.get,
    subscribe: store.subscribe,
    onChange: store.onChange,
    ...extra,
  };
}

export function createWebbarBridge(
  postMessage: (json: string) => void,
  output = "",
  isNativeOverlayActive: () => boolean = () => false,
): {
  api: WebbarApi;
  receive: (event: Record<string, unknown>) => void;
  notifyOverlayChange: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
} {
  let nextId = 1;
  let explicitOverlayCount = 0;
  let sentOverlayActive = false;
  let deactivateScheduled = false;
  const pending = new Map<
    number,
    {
      resolve: (value: never) => void;
      reject: (err: Error) => void;
      cleanup?: () => void;
      mapError?: (message: string, data: unknown) => Error;
    }
  >();
  const dbusWatches = new Map<
    number,
    {
      resolveStart?: (val: { stop: () => void }) => void;
      rejectStart?: (err: Error) => void;
      onSignal: (sig: DbusSignalEvent) => void;
      onClose?: (error?: Error) => void;
      cleanup?: () => void;
    }
  >();
  const processStreams = new Map<
    number,
    {
      resolveStart?: (val: ExecStreamHandle) => void;
      rejectStart?: (err: Error) => void;
      onStdout?: (line: string) => void;
      onStderr?: (line: string) => void;
      onExit?: (result: { code: number | null; signal: number | null }) => void;
      cleanup?: () => void;
    }
  >();

  function send(payload: Record<string, unknown>): void {
    postMessage(JSON.stringify(payload));
  }

  function computeOverlayActive(): boolean {
    return explicitOverlayCount > 0 || isNativeOverlayActive();
  }

  function syncOverlay(): void {
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

  function rpc<T = void>(
    type: string,
    payload: Record<string, unknown> = {},
    abort?: { signal?: AbortSignal; onAbort?: (id: number) => void },
    mapError?: (message: string, data: unknown) => Error,
  ): Promise<T> {
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
      const cleanup = abort?.signal
        ? () => abort.signal?.removeEventListener("abort", onAbort)
        : undefined;
      abort?.signal?.addEventListener("abort", onAbort, { once: true });
      pending.set(id, {
        resolve: resolve as (value: never) => void,
        reject,
        cleanup,
        mapError,
      });
      send({ type, id, ...payload });
    });
  }

  const stores = {
    sway: createStore<SwayState>({
      available: false,
      tree: null,
      workspaces: [],
      mode: "default",
    }),
    tray: createStore<TrayItem[]>([]),
    notifications: createStore<NotificationItem[]>([]),
    audio: createStore<AudioState>({
      available: false,
      outputs: [],
      inputs: [],
    }),
    bluetooth: createStore<BluetoothState>({
      available: false,
      adapters: [],
      devices: [],
    }),
    network: createStore<NetworkState>({
      available: false,
      connectivity: "unknown",
      link: { kind: "none" },
    }),
    backlight: createStore<BacklightState>({
      available: false,
      brightness: null,
      error: null,
    }),
  };

  type MenuStoreEntry = {
    bus: string;
    menuPath: string;
    store: InternalStore<TrayMenuItem[]>;
    view: Store<TrayMenuItem[]>;
    subCount: number;
  };
  const menuStores = new Map<string, MenuStoreEntry>();

  function trayMenuKey(bus: string, menuPath: string): string {
    return `${bus}\0${menuPath}`;
  }

  function trayMenuStore(bus: string, menuPath: string): Store<TrayMenuItem[]> {
    const key = trayMenuKey(bus, menuPath);
    const existing = menuStores.get(key);
    if (existing) {
      return existing.view;
    }

    const store = createStore<TrayMenuItem[]>([]);
    const entry: MenuStoreEntry = {
      bus,
      menuPath,
      store,
      subCount: 0,
      view: null as unknown as Store<TrayMenuItem[]>,
    };

    const retain = (): (() => void) => {
      entry.subCount += 1;
      if (entry.subCount === 1) {
        send({ type: "tray_watch_menu", bus, menu_path: menuPath });
      }
      let released = false;
      return () => {
        if (released) return;
        released = true;
        entry.subCount = Math.max(0, entry.subCount - 1);
        if (entry.subCount === 0) {
          send({ type: "tray_unwatch_menu", bus, menu_path: menuPath });
          entry.store._update([]);
        }
      };
    };

    entry.view = {
      get: store.get,
      subscribe: (listener) => {
        const unsub = store.subscribe(listener);
        const release = retain();
        return () => {
          unsub();
          release();
        };
      },
      onChange: (listener) => {
        const unsub = store.onChange(listener);
        const release = retain();
        return () => {
          unsub();
          release();
        };
      },
    };

    menuStores.set(key, entry);
    return entry.view;
  }

  const stateHandlers: Record<string, (event: Record<string, unknown>) => void> = {
    sway_state: (e) =>
      stores.sway._update({
        available: Boolean(e.available),
        tree: (e.tree as SwayNode | null) ?? null,
        workspaces: (e.workspaces as SwayWorkspace[]) ?? [],
        mode: (e.mode as string) ?? "default",
      }),
    tray: (e) => stores.tray._update((e.items as TrayItem[]) ?? []),
    tray_menu: (e) => {
      const bus = e.bus as string;
      const menuPath = e.menu_path as string;
      const entry = menuStores.get(trayMenuKey(bus, menuPath));
      if (entry && entry.subCount > 0) {
        entry.store._update((e.items as TrayMenuItem[]) ?? []);
      }
    },
    notifications: (e) =>
      stores.notifications._update((e.items as NotificationItem[]) ?? []),
    audio_state: (e) =>
      stores.audio._update({
        available: Boolean(e.available),
        outputs: (e.outputs as AudioDevice[]) ?? [],
        inputs: (e.inputs as AudioDevice[]) ?? [],
      }),
    bluetooth_state: (e) =>
      stores.bluetooth._update({
        available: Boolean(e.available),
        adapters: (e.adapters as BluetoothAdapter[]) ?? [],
        devices: (e.devices as BluetoothDevice[]) ?? [],
      }),
    network_state: (e) =>
      stores.network._update({
        available: Boolean(e.available),
        connectivity: (e.connectivity as Connectivity) ?? "unknown",
        link: (e.link as NetworkLink) ?? { kind: "none" },
      }),
    backlight_state: (e) =>
      stores.backlight._update({
        available: Boolean(e.available),
        brightness: typeof e.brightness === "number" ? e.brightness : null,
        error: typeof e.error === "string" ? e.error : null,
      }),
  };

  function receive(event: Record<string, unknown>): void {
    const type = event.type as string;
    const stateHandler = stateHandlers[type];
    if (stateHandler) {
      stateHandler(event);
      return;
    }

    switch (type) {
      case "rpc_reply": {
        const id = event.id as number;
        const entry = pending.get(id);
        if (!entry) return;
        entry.cleanup?.();
        pending.delete(id);
        if (event.ok) {
          entry.resolve(event.data as never);
        } else {
          const message = (event.error as string | undefined) ?? "Request failed";
          entry.reject(
            entry.mapError
              ? entry.mapError(message, event.data)
              : new Error(message),
          );
        }
        break;
      }
      case "dbus_match_started": {
        const id = event.id as number;
        const watch = dbusWatches.get(id);
        if (!watch) return;
        if (!event.ok) {
          watch.cleanup?.();
          dbusWatches.delete(id);
          watch.rejectStart?.(
            new Error(
              (event.error as string | undefined) ?? "D-Bus match failed",
            ),
          );
          return;
        }
        const resolveStart = watch.resolveStart;
        watch.resolveStart = undefined;
        watch.rejectStart = undefined;
        resolveStart?.({
          stop: () => {
            if (!dbusWatches.has(id)) return;
            watch.cleanup?.();
            dbusWatches.delete(id);
            send({ type: "dbus_unwatch", id });
          },
        });
        break;
      }
      case "dbus_signal": {
        const watch = dbusWatches.get(event.id as number);
        if (!watch) return;
        watch.onSignal({
          sender: event.sender as string,
          path: event.path as string,
          interface: event.interface as string,
          member: event.member as string,
          signature: event.signature as string,
          body: (event.body as unknown[]) ?? [],
        });
        break;
      }
      case "dbus_match_ended": {
        const id = event.id as number;
        const watch = dbusWatches.get(id);
        if (!watch) break;
        watch.cleanup?.();
        dbusWatches.delete(id);
        const errMsg = typeof event.error === "string" ? event.error : undefined;
        if (watch.rejectStart) {
          watch.rejectStart(
            new Error(errMsg ?? "D-Bus match ended before starting"),
          );
        } else {
          watch.onClose?.(errMsg ? new Error(errMsg) : undefined);
        }
        break;
      }
      case "process_stream_started": {
        const id = event.id as number;
        const entry = processStreams.get(id);
        if (!entry) return;
        if (!event.ok) {
          entry.cleanup?.();
          processStreams.delete(id);
          entry.rejectStart?.(
            new Error(
              (event.error as string | undefined) ?? "Failed to start process",
            ),
          );
          return;
        }
        const resolveStart = entry.resolveStart;
        entry.resolveStart = undefined;
        entry.rejectStart = undefined;
        resolveStart?.({
          stop: () => {
            if (!processStreams.has(id)) return;
            entry.cleanup?.();
            processStreams.delete(id);
            send({ type: "process_kill", id, signal: 15 });
          },
        });
        break;
      }
      case "process_stream_output": {
        const entry = processStreams.get(event.id as number);
        if (!entry) return;
        const line = (event.line as string) ?? "";
        if (event.stream === "stderr") {
          entry.onStderr?.(line);
        } else {
          entry.onStdout?.(line);
        }
        break;
      }
      case "process_stream_ended": {
        const id = event.id as number;
        const entry = processStreams.get(id);
        if (!entry) return;
        entry.cleanup?.();
        processStreams.delete(id);
        entry.onExit?.({
          code: typeof event.code === "number" ? event.code : null,
          signal: typeof event.signal === "number" ? event.signal : null,
        });
        break;
      }
    }
  }

  function swayCommand(command: string): Promise<void> {
    return rpc<void>("sway_command", { command }, undefined, (message, data) =>
      new SwayCommandError(
        (data as SwayCommandErrorData | undefined) ?? {
          message,
          parse_error: false,
        },
      ),
    );
  }

  const api: WebbarApi = {
    output,
    sway: storeView(stores.sway, {
      command: swayCommand,
    }),
    tray: storeView(stores.tray, {
      activate: (bus: string, path: string) =>
        send({ type: "tray_activate", bus, path }),
      getMenu: (bus: string, menuPath: string) =>
        rpc<TrayMenuItem[]>("tray_get_menu", { bus, menu_path: menuPath }),
      menu: trayMenuStore,
      menuEvent: (bus: string, menuPath: string, itemId: number) =>
        send({
          type: "tray_menu_event",
          bus,
          menu_path: menuPath,
          item_id: itemId,
        }),
    }),
    notifications: storeView(stores.notifications, {
      dismiss: (id: number) => send({ type: "dismiss_notification", id }),
      invokeAction: (id: number, actionKey: string) =>
        send({
          type: "invoke_notification_action",
          id,
          action_key: actionKey,
        }),
      clearAll: () => send({ type: "clear_notifications" }),
    }),
    audio: storeView(stores.audio, {
      setVolume: (deviceId: number, volume: number) =>
        rpc("audio_set_volume", { device_id: deviceId, volume }),
      setMuted: (deviceId: number, muted: boolean) =>
        rpc("audio_set_muted", { device_id: deviceId, muted }),
      setDefault: (deviceId: number) =>
        rpc("audio_set_default", { device_id: deviceId }),
    }),
    bluetooth: storeView(stores.bluetooth, {
      setPowered: (path: string, powered: boolean) =>
        rpc("bluetooth_set_powered", { path, powered }),
      connect: (path: string) => rpc("bluetooth_connect", { path }),
      disconnect: (path: string) => rpc("bluetooth_disconnect", { path }),
    }),
    network: storeView(stores.network, {}),
    backlight: storeView(stores.backlight, {
      set: (percent: number) => rpc("backlight_set", { percent }),
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
    setInputRegion: (rects: InputRect[]) =>
      send({ type: "set_input_region", rects }),
    resetInputRegion: () => send({ type: "set_input_region", rects: [] }),
    dbus: {
      call: (opts: DbusCallOptions) =>
        rpc("dbus_call", {
          bus: opts.bus ?? "session",
          destination: opts.destination,
          path: opts.path,
          interface: opts.interface,
          member: opts.member,
          signature: opts.signature ?? "",
          body: opts.body ?? [],
        }),
      watch: (
        opts: DbusWatchOptions,
        onSignal: (signal: DbusSignalEvent) => void,
      ) => {
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
            onClose: opts.onClose,
            cleanup,
          });
          send({
            type: "dbus_match",
            id,
            bus: opts.bus ?? "session",
            sender: opts.sender ?? null,
            path: opts.path ?? null,
            interface: opts.interface ?? null,
            member: opts.member ?? null,
          });
        });
      },
    },
    exec: (argv: string[], opts?: ExecOptions) =>
      rpc<ExecResult>(
        "process_spawn",
        {
          argv: [...argv],
          cwd: opts?.cwd ?? null,
          env: opts?.env ?? {},
        },
        {
          signal: opts?.signal,
          onAbort: (id) => send({ type: "process_kill", id, signal: 15 }),
        },
      ),
    execStream: (argv: string[], opts?: ExecStreamOptions) => {
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
        const cleanup = opts?.signal
          ? () => opts.signal?.removeEventListener("abort", onAbort)
          : undefined;
        opts?.signal?.addEventListener("abort", onAbort, { once: true });
        processStreams.set(id, {
          resolveStart: resolve,
          rejectStart: reject,
          onStdout: opts?.onStdout,
          onStderr: opts?.onStderr,
          onExit: opts?.onExit,
          cleanup,
        });
        send({
          type: "process_stream_spawn",
          id,
          argv: [...argv],
          cwd: opts?.cwd ?? null,
          env: opts?.env ?? {},
        });
      });
    },
  };

  function onDisconnect(): void {
    const disconnectErr = () => new Error("Webbar bridge disconnected");
    for (const entry of pending.values()) {
      entry.cleanup?.();
      entry.reject(disconnectErr());
    }
    pending.clear();

    for (const watch of dbusWatches.values()) {
      watch.cleanup?.();
      if (watch.rejectStart) {
        watch.rejectStart(disconnectErr());
      } else {
        watch.onClose?.(disconnectErr());
      }
    }
    dbusWatches.clear();

    for (const stream of processStreams.values()) {
      stream.cleanup?.();
      if (stream.rejectStart) {
        stream.rejectStart(disconnectErr());
      } else {
        stream.onExit?.({ code: null, signal: null });
      }
    }
    processStreams.clear();
  }

  // Always send the current overlay state: the server may still hold a stale
  // value from before the disconnect (or from a previous page load).
  function onConnect(): void {
    deactivateScheduled = false;
    sentOverlayActive = computeOverlayActive();
    send({ type: "set_overlay", active: sentOverlayActive });
    for (const entry of menuStores.values()) {
      if (entry.subCount > 0) {
        send({
          type: "tray_watch_menu",
          bus: entry.bus,
          menu_path: entry.menuPath,
        });
      }
    }
  }

  return {
    api,
    receive,
    notifyOverlayChange: syncOverlay,
    onConnect,
    onDisconnect,
  };
}

declare global {
  interface Window {
    webbar: WebbarApi;
  }
}

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
import type { SwayState } from "./generated/SwayState";
import type { SwayWorkspace } from "./generated/SwayWorkspace";
import type { TrayItem } from "./generated/TrayItem";
import type { TrayMenuItem } from "./generated/TrayMenuItem";

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
  SwayState,
  SwayWorkspace,
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

export type WebbarApi = {
  readonly output: string;
  sway: Store<SwayState> & {
    command: (cmd: string) => void;
  };
  tray: Store<TrayItem[]> & {
    activate: (bus: string, path: string) => void;
    getMenu: (bus: string, menuPath: string) => Promise<TrayMenuItem[]>;
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
};

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
      current = next;
      for (const listener of subscribeListeners) listener();
      for (const listener of changeListeners) listener(current);
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
): {
  api: WebbarApi;
  receive: (event: Record<string, unknown>) => void;
} {
  let nextId = 1;
  const pending = new Map<
    number,
    {
      resolve: (value: never) => void;
      reject: (err: Error) => void;
      cleanup?: () => void;
    }
  >();
  const dbusWatches = new Map<
    number,
    {
      resolveStart?: (val: { stop: () => void }) => void;
      rejectStart?: (err: Error) => void;
      onSignal: (sig: DbusSignalEvent) => void;
      cleanup?: () => void;
    }
  >();

  function send(payload: Record<string, unknown>): void {
    postMessage(JSON.stringify(payload));
  }

  function rpc<T = void>(
    type: string,
    payload: Record<string, unknown> = {},
    abort?: { signal?: AbortSignal; onAbort?: (id: number) => void },
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
      });
      send({ type, id, ...payload });
    });
  }

  const stores = {
    sway: createStore<SwayState>({
      workspaces: [],
      focused_title: "",
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

  const stateHandlers: Record<string, (event: Record<string, unknown>) => void> = {
    sway_state: (e) =>
      stores.sway._update({
        workspaces: (e.workspaces as SwayWorkspace[]) ?? [],
        focused_title: (e.focused_title as string) ?? "",
        mode: (e.mode as string) ?? "default",
      }),
    tray: (e) => stores.tray._update((e.items as TrayItem[]) ?? []),
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
          entry.reject(
            new Error((event.error as string | undefined) ?? "Request failed"),
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
        watch.resolveStart?.({
          stop: () => {
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
        watch?.cleanup?.();
        dbusWatches.delete(id);
        break;
      }
    }
  }

  const api: WebbarApi = {
    output,
    sway: storeView(stores.sway, {
      command: (command: string) => send({ type: "sway_command", command }),
    }),
    tray: storeView(stores.tray, {
      activate: (bus: string, path: string) =>
        send({ type: "tray_activate", bus, path }),
      getMenu: (bus: string, menuPath: string) =>
        rpc<TrayMenuItem[]>("tray_get_menu", { bus, menu_path: menuPath }),
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
  };

  return { api, receive };
}

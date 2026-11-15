export type Store<T> = {
  get: () => T;
  subscribe: (listener: () => void) => () => void;
  onChange: (listener: (value: T) => void) => () => void;
};

export type InternalStore<T> = Store<T> & {
  _update: (next: T) => void;
};

export type InputRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export type SwayWorkspace = {
  id?: number;
  num: number;
  name: string;
  output: string;
  focused: boolean;
  visible: boolean;
  urgent: boolean;
};

export type SwayState = {
  workspaces: SwayWorkspace[];
  focused_title: string;
  mode: string;
};

export type TrayItem = {
  id?: string;
  bus: string;
  path: string;
  title: string;
  icon_name: string;
  icon_url?: string;
  menu_path?: string;
};

export type TrayMenuItem = {
  id: number;
  label: string;
  enabled: boolean;
  visible: boolean;
  item_type: string;
  toggle_type: string;
  toggle_state: number;
  children: TrayMenuItem[];
};

export type NotificationUrgency = "low" | "normal" | "critical";

export type NotificationAction = {
  key: string;
  label: string;
};

export type NotificationItem = {
  id: number;
  app_name: string;
  icon: string;
  summary: string;
  body: string;
  urgency: NotificationUrgency;
  actions: NotificationAction[];
};

export type AudioDevice = {
  id: number;
  name: string;
  description: string;
  volume: number;
  muted: boolean;
  default: boolean;
};

export type AudioState = {
  available: boolean;
  outputs: AudioDevice[];
  inputs: AudioDevice[];
};

export type BluetoothAdapter = {
  path: string;
  name: string;
  powered: boolean;
};

export type BluetoothDevice = {
  path: string;
  name: string;
  address: string;
  paired: boolean;
  connected: boolean;
  battery?: number | null;
};

export type BluetoothState = {
  available: boolean;
  adapters: BluetoothAdapter[];
  devices: BluetoothDevice[];
};

export type Connectivity = "unknown" | "none" | "portal" | "limited" | "full";

export type NetworkLink =
  | { kind: "wifi"; name: string; ssid: string; strength: number }
  | { kind: "wired"; name: string }
  | { kind: "other"; name: string }
  | { kind: "none" };

export type NetworkState = {
  available: boolean;
  connectivity: Connectivity;
  link: NetworkLink;
};

export type BacklightState = {
  available: boolean;
  brightness: number | null;
  error: string | null;
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

export type ExecResult = {
  code?: number;
  signal?: number;
  stdout: string;
  stderr: string;
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
      for (const listener of subscribeListeners) {
        listener();
      }
      for (const listener of changeListeners) {
        listener(current);
      }
    },
  };
}

function decodeBase64Bytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) {
    bytes[i] = bin.charCodeAt(i);
  }
  return bytes;
}

export function createWebbarBridge(
  postMessage: (json: string) => void,
  output = "",
): {
  api: WebbarApi;
  receive: (event: Record<string, unknown>) => void;
} {
  let nextId = 1;
  const actionPending = new Map<
    number,
    { resolve: (value: unknown) => void; reject: (err: Error) => void }
  >();
  const dbusCallPending = new Map<
    number,
    {
      resolve: (value: { signature: string; body: unknown[] }) => void;
      reject: (err: Error) => void;
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
  const execPending = new Map<
    number,
    {
      resolve: (res: ExecResult) => void;
      reject: (err: Error) => void;
      stdout: Uint8Array[];
      stderr: Uint8Array[];
      cleanup?: () => void;
    }
  >();

  function send(payload: Record<string, unknown>): void {
    postMessage(JSON.stringify(payload));
  }

  function invokeAction(
    type: string,
    payload: Record<string, unknown> = {},
  ): Promise<void> {
    const id = nextId++;
    return new Promise((resolve, reject) => {
      actionPending.set(id, {
        resolve: () => resolve(),
        reject,
      });
      send({ type, id, ...payload });
    });
  }

  const swayStore = createStore<SwayState>({
    workspaces: [],
    focused_title: "",
    mode: "default",
  });
  const trayStore = createStore<TrayItem[]>([]);
  const notificationsStore = createStore<NotificationItem[]>([]);
  const audioStore = createStore<AudioState>({
    available: false,
    outputs: [],
    inputs: [],
  });
  const bluetoothStore = createStore<BluetoothState>({
    available: false,
    adapters: [],
    devices: [],
  });
  const networkStore = createStore<NetworkState>({
    available: false,
    connectivity: "unknown",
    link: { kind: "none" },
  });
  const backlightStore = createStore<BacklightState>({
    available: false,
    brightness: null,
    error: null,
  });

  function receive(event: Record<string, unknown>): void {
    switch (event.type) {
      case "action_reply": {
        const id = event.id as number;
        const pending = actionPending.get(id);
        if (!pending) return;
        actionPending.delete(id);
        if (event.ok) {
          pending.resolve(undefined);
        } else {
          pending.reject(
            new Error((event.error as string | undefined) ?? "Action failed"),
          );
        }
        break;
      }
      case "tray_menu_reply": {
        const id = event.id as number;
        const pending = actionPending.get(id);
        if (!pending) return;
        actionPending.delete(id);
        if (event.ok) {
          pending.resolve(event.items);
        } else {
          pending.reject(
            new Error(
              (event.error as string | undefined) ?? "Failed to fetch tray menu",
            ),
          );
        }
        break;
      }
      case "sway_state": {
        swayStore._update({
          workspaces: (event.workspaces as SwayWorkspace[]) ?? [],
          focused_title: (event.focused_title as string) ?? "",
          mode: (event.mode as string) ?? "default",
        });
        break;
      }
      case "tray": {
        trayStore._update((event.items as TrayItem[]) ?? []);
        break;
      }
      case "notifications": {
        notificationsStore._update((event.items as NotificationItem[]) ?? []);
        break;
      }
      case "audio_state": {
        audioStore._update({
          available: Boolean(event.available),
          outputs: (event.outputs as AudioDevice[]) ?? [],
          inputs: (event.inputs as AudioDevice[]) ?? [],
        });
        break;
      }
      case "bluetooth_state": {
        bluetoothStore._update({
          available: Boolean(event.available),
          adapters: (event.adapters as BluetoothAdapter[]) ?? [],
          devices: (event.devices as BluetoothDevice[]) ?? [],
        });
        break;
      }
      case "network_state": {
        networkStore._update({
          available: Boolean(event.available),
          connectivity: (event.connectivity as Connectivity) ?? "unknown",
          link: (event.link as NetworkLink) ?? { kind: "none" },
        });
        break;
      }
      case "backlight_state": {
        backlightStore._update({
          available: Boolean(event.available),
          brightness:
            typeof event.brightness === "number" ? event.brightness : null,
          error: typeof event.error === "string" ? event.error : null,
        });
        break;
      }
      case "dbus_call_reply": {
        const id = event.id as number;
        const pending = dbusCallPending.get(id);
        if (!pending) return;
        dbusCallPending.delete(id);
        if (event.ok) {
          pending.resolve({
            signature: (event.signature as string) ?? "",
            body: JSON.parse((event.body as string) ?? "[]"),
          });
        } else {
          pending.reject(
            new Error(
              (event.error as string | undefined) ?? "D-Bus call failed",
            ),
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
          body: JSON.parse((event.body as string) ?? "[]"),
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
      case "process_started": {
        const id = event.id as number;
        const pending = execPending.get(id);
        if (!pending) return;
        if (!event.ok) {
          pending.cleanup?.();
          execPending.delete(id);
          pending.reject(
            new Error(
              (event.error as string | undefined) ?? "Process spawn failed",
            ),
          );
        }
        break;
      }
      case "process_output": {
        const pending = execPending.get(event.id as number);
        if (!pending) return;
        const chunk = decodeBase64Bytes(event.data_b64 as string);
        if (event.stream === "stdout") {
          pending.stdout.push(chunk);
        } else if (event.stream === "stderr") {
          pending.stderr.push(chunk);
        }
        break;
      }
      case "process_exited": {
        const id = event.id as number;
        const pending = execPending.get(id);
        if (!pending) return;
        pending.cleanup?.();
        execPending.delete(id);
        const decoder = new TextDecoder();
        const joinText = (chunks: Uint8Array[]) =>
          chunks.map((c) => decoder.decode(c, { stream: true })).join("") +
          decoder.decode();
        pending.resolve({
          code: typeof event.code === "number" ? event.code : undefined,
          signal: typeof event.signal === "number" ? event.signal : undefined,
          stdout: joinText(pending.stdout),
          stderr: joinText(pending.stderr),
        });
        break;
      }
    }
  }

  const api: WebbarApi = {
    output,
    sway: {
      get: swayStore.get,
      subscribe: swayStore.subscribe,
      onChange: swayStore.onChange,
      command: (command: string) => send({ type: "sway_command", command }),
    },
    tray: {
      get: trayStore.get,
      subscribe: trayStore.subscribe,
      onChange: trayStore.onChange,
      activate: (bus: string, path: string) =>
        send({ type: "tray_activate", bus, path }),
      getMenu: (bus: string, menuPath: string) => {
        const id = nextId++;
        return new Promise((resolve, reject) => {
          actionPending.set(id, {
            resolve: (val) => resolve(val as TrayMenuItem[]),
            reject,
          });
          send({ type: "tray_get_menu", id, bus, menu_path: menuPath });
        });
      },
      menuEvent: (bus: string, menuPath: string, itemId: number) =>
        send({
          type: "tray_menu_event",
          bus,
          menu_path: menuPath,
          item_id: itemId,
        }),
    },
    notifications: {
      get: notificationsStore.get,
      subscribe: notificationsStore.subscribe,
      onChange: notificationsStore.onChange,
      dismiss: (id: number) => send({ type: "dismiss_notification", id }),
      invokeAction: (id: number, actionKey: string) =>
        send({
          type: "invoke_notification_action",
          id,
          action_key: actionKey,
        }),
      clearAll: () => send({ type: "clear_notifications" }),
    },
    audio: {
      get: audioStore.get,
      subscribe: audioStore.subscribe,
      onChange: audioStore.onChange,
      setVolume: (deviceId: number, volume: number) =>
        invokeAction("audio_set_volume", { device_id: deviceId, volume }),
      setMuted: (deviceId: number, muted: boolean) =>
        invokeAction("audio_set_muted", { device_id: deviceId, muted }),
      setDefault: (deviceId: number) =>
        invokeAction("audio_set_default", { device_id: deviceId }),
    },
    bluetooth: {
      get: bluetoothStore.get,
      subscribe: bluetoothStore.subscribe,
      onChange: bluetoothStore.onChange,
      setPowered: (path: string, powered: boolean) =>
        invokeAction("bluetooth_set_powered", { path, powered }),
      connect: (path: string) => invokeAction("bluetooth_connect", { path }),
      disconnect: (path: string) =>
        invokeAction("bluetooth_disconnect", { path }),
    },
    network: {
      get: networkStore.get,
      subscribe: networkStore.subscribe,
      onChange: networkStore.onChange,
    },
    backlight: {
      get: backlightStore.get,
      subscribe: backlightStore.subscribe,
      onChange: backlightStore.onChange,
      set: (percent: number) =>
        invokeAction("backlight_set", {
          percent: Math.max(0, Math.min(100, Math.round(percent))),
        }),
    },
    setInputRegion: (rects: InputRect[]) =>
      send({
        type: "set_input_region",
        rects: rects.map((r) => ({
          x: Math.round(r.x),
          y: Math.round(r.y),
          width: Math.round(r.width),
          height: Math.round(r.height),
        })),
      }),
    resetInputRegion: () => send({ type: "set_input_region", rects: [] }),
    dbus: {
      call: (opts: DbusCallOptions) => {
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
            body: JSON.stringify(opts.body ?? []),
          });
        });
      },
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
    exec: (argv: string[], opts?: ExecOptions) => {
      if (opts?.signal?.aborted) {
        return Promise.reject(new DOMException("Aborted", "AbortError"));
      }
      const id = nextId++;
      return new Promise((resolve, reject) => {
        const onAbort = () => {
          const pending = execPending.get(id);
          pending?.cleanup?.();
          execPending.delete(id);
          send({ type: "process_kill", id, signal: 15 });
          reject(new DOMException("Aborted", "AbortError"));
        };
        const cleanup = () => {
          opts?.signal?.removeEventListener("abort", onAbort);
        };
        opts?.signal?.addEventListener("abort", onAbort, { once: true });
        execPending.set(id, {
          resolve,
          reject,
          stdout: [],
          stderr: [],
          cleanup,
        });
        send({
          type: "process_spawn",
          id,
          argv: [...argv],
          cwd: opts?.cwd ?? null,
          env: opts?.env ?? {},
          stdin: false,
        });
      });
    },
  };

  return { api, receive };
}

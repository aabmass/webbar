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
export type NotificationItem = {
    id: number;
    app_name: string;
    summary: string;
    body: string;
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
export type NetworkLink = {
    kind: "wifi";
    name: string;
    ssid: string;
    strength: number;
} | {
    kind: "wired";
    name: string;
} | {
    kind: "other";
    name: string;
} | {
    kind: "none";
};
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
};
export type ExecResult = {
    code?: number;
    signal?: number;
    stdout: string;
    stderr: string;
};
export type WebbarApi = {
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
        call: (opts: DbusCallOptions) => Promise<{
            signature: string;
            body: unknown[];
        }>;
        watch: (opts: DbusWatchOptions, onSignal: (signal: DbusSignalEvent) => void) => Promise<{
            stop: () => void;
        }>;
    };
    exec: (argv: string[], opts?: ExecOptions) => Promise<ExecResult>;
};
export declare function createStore<T>(initial: T): InternalStore<T>;
export declare function createWebbarBridge(postMessage: (json: string) => void): {
    api: WebbarApi;
    receive: (event: Record<string, unknown>) => void;
};

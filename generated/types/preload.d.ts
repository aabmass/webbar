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
export type { AudioDevice, AudioState, BacklightState, BluetoothAdapter, BluetoothDevice, BluetoothState, Connectivity, ExecResult, InputRect, NetworkLink, NetworkState, NotificationAction, NotificationItem, NotificationUrgency, SwayState, SwayWorkspace, TrayItem, TrayMenuItem, };
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
    acquireOverlay: () => () => void;
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
export declare function createWebbarBridge(postMessage: (json: string) => void, output?: string, isNativeOverlayActive?: () => boolean): {
    api: WebbarApi;
    receive: (event: Record<string, unknown>) => void;
    notifyOverlayChange: () => void;
};

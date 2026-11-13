import { Err, Ok, type Result } from "@cprussin/option-result";
import type { Dbus } from "@domicile-desktop/sdk/desktop/dbus";
import type { Files } from "@domicile-desktop/sdk/desktop/files";
import type { Processes } from "@domicile-desktop/sdk/desktop/processes";
import {
  Bus,
  type DbusBody,
  type DbusCall,
  type DbusMatch,
  type DbusSignal,
  type DirEntry,
  type Exit,
  Listened,
  type Listening,
  type Ran,
  Signal,
  type SpawnOptions,
  type Stat,
  type Subprocess,
  type SystemError,
  SystemErrorKind,
} from "@domicile-desktop/sdk/system";

export type NotificationItem = {
  id: number;
  app_name: string;
  summary: string;
  body: string;
};

export type SwayWorkspace = {
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
  icon_url: string;
  menu_path: string;
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

type HostEvent =
  | {
      type: "dbus_call_reply";
      id: number;
      ok: boolean;
      signature?: string;
      body?: string;
      error?: string;
    }
  | { type: "dbus_match_started"; id: number; ok: boolean; error?: string }
  | {
      type: "dbus_signal";
      id: number;
      sender: string;
      path: string;
      interface: string;
      member: string;
      signature: string;
      body: string;
    }
  | { type: "dbus_match_ended"; id: number; error?: string }
  | { type: "notifications"; items: NotificationItem[] }
  | { type: "process_started"; id: number; ok: boolean; error?: string | null }
  | {
      type: "process_output";
      id: number;
      stream: string;
      data_b64: string;
    }
  | {
      type: "process_exited";
      id: number;
      code?: number | null;
      signal?: number | null;
    }
  | {
      type: "file_reply";
      id: number;
      ok: boolean;
      data?: {};
      error_kind?: string | null;
      error?: string | null;
    }
  | ({ type: "sway_state" } & SwayState)
  | { type: "tray"; items: TrayItem[] }
  | {
      type: "tray_menu_reply";
      id: number;
      ok: boolean;
      items: TrayMenuItem[];
      error?: string | null;
    };

declare global {
  interface Window {
    webkit?: {
      messageHandlers?: {
        bridge?: { postMessage: (msg: string) => void };
      };
    };
    __swayWebbarReceive?: (event: HostEvent) => void;
  }
}

let nextId = 1;

const pendingCalls = new Map<
  number,
  (result: Result<DbusBody, SystemError>) => void
>();

type MatchEntry = {
  resolveStart: (result: Result<Listening<Listened<DbusSignal>>, SystemError>) => void;
  resolveEnd: (result: Result<"stopped", SystemError>) => void;
  controller?: ReadableStreamDefaultController<Listened<DbusSignal>>;
  ended: Promise<Result<"stopped", SystemError>>;
};

type ProcessEntry = {
  resolveStart: (result: Result<Subprocess, SystemError>) => void;
  resolveExit: (result: Result<Exit, SystemError>) => void;
  stdoutController?: ReadableStreamDefaultController<Uint8Array>;
  stderrController?: ReadableStreamDefaultController<Uint8Array>;
  stdout: ReadableStream<Uint8Array>;
  stderr: ReadableStream<Uint8Array>;
  exited: Promise<Result<Exit, SystemError>>;
};

const activeMatches = new Map<number, MatchEntry>();
const activeProcesses = new Map<number, ProcessEntry>();
const pendingFileReplies = new Map<
  number,
  (result: Result<{}, SystemError>) => void
>();

const notificationListeners = new Set<(items: NotificationItem[]) => void>();
let latestNotifications: NotificationItem[] = [];

const swayListeners = new Set<(state: SwayState) => void>();
let latestSwayState: SwayState | undefined;

const trayListeners = new Set<(items: TrayItem[]) => void>();
let latestTrayItems: TrayItem[] = [];
const pendingTrayMenus = new Map<
  number,
  { resolve: (items: TrayMenuItem[]) => void; reject: (err: Error) => void }
>();

function sysErr(
  message: string,
  kind: SystemErrorKind = SystemErrorKind.Other,
): SystemError {
  return { kind, message };
}

function mapFileErrorKind(kind?: string | null): SystemErrorKind {
  switch (kind) {
    case "NotFound":
      return SystemErrorKind.NotFound;
    case "PermissionDenied":
      return SystemErrorKind.PermissionDenied;
    case "AlreadyExists":
      return SystemErrorKind.AlreadyExists;
    case "NotADirectory":
      return SystemErrorKind.NotADirectory;
    case "IsADirectory":
      return SystemErrorKind.IsADirectory;
    default:
      return SystemErrorKind.Other;
  }
}

function toPosixSignal(signal: Signal = Signal.Term): number {
  switch (signal) {
    case Signal.Hup:
      return 1;
    case Signal.Int:
      return 2;
    case Signal.Term:
      return 15;
    case Signal.Kill:
      return 9;
    case Signal.Usr1:
      return 10;
    case Signal.Usr2:
      return 12;
  }
}

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) {
    bytes[i] = bin.charCodeAt(i);
  }
  return bytes;
}

function toBase64(data: Uint8Array | string): string {
  const bytes =
    typeof data === "string" ? new TextEncoder().encode(data) : data;
  let bin = "";
  for (let i = 0; i < bytes.length; i++) {
    bin += String.fromCharCode(bytes[i]);
  }
  return btoa(bin);
}

async function readStreamText(
  stream: ReadableStream<Uint8Array>,
): Promise<string> {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let text = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    text += decoder.decode(value, { stream: true });
  }
  text += decoder.decode();
  return text;
}

function postToHost(payload: Record<string, unknown>): void {
  const handler = window.webkit?.messageHandlers?.bridge;
  if (!handler) {
    console.error("webbar: webkit bridge handler is not available");
    return;
  }
  handler.postMessage(JSON.stringify(payload));
}

window.__swayWebbarReceive = (event: HostEvent) => {
  switch (event.type) {
    case "dbus_call_reply": {
      const resolve = pendingCalls.get(event.id);
      if (!resolve) return;
      pendingCalls.delete(event.id);
      if (event.ok) {
        resolve(
          Ok({
            signature: event.signature ?? "",
            body: JSON.parse(event.body ?? "[]") as unknown[],
          }),
        );
      } else {
        resolve(Err(sysErr(event.error ?? "D-Bus call failed")));
      }
      break;
    }
    case "dbus_match_started": {
      const entry = activeMatches.get(event.id);
      if (!entry) return;
      if (!event.ok) {
        activeMatches.delete(event.id);
        entry.resolveStart(Err(sysErr(event.error ?? "D-Bus match failed")));
        return;
      }
      const items = new ReadableStream<Listened<DbusSignal>>({
        start(controller) {
          entry.controller = controller;
        },
        cancel() {
          postToHost({ type: "dbus_unwatch", id: event.id });
        },
      });
      entry.resolveStart(
        Ok({
          items,
          stop: () => postToHost({ type: "dbus_unwatch", id: event.id }),
          ended: entry.ended,
        }),
      );
      break;
    }
    case "dbus_signal": {
      const entry = activeMatches.get(event.id);
      if (!entry?.controller) return;
      entry.controller.enqueue(
        Listened.Heard({
          sender: event.sender,
          path: event.path,
          interface: event.interface,
          member: event.member,
          signature: event.signature,
          body: JSON.parse(event.body) as unknown[],
        }),
      );
      break;
    }
    case "dbus_match_ended": {
      const entry = activeMatches.get(event.id);
      if (!entry) return;
      activeMatches.delete(event.id);
      try {
        entry.controller?.close();
      } catch {
        // Stream already closed.
      }
      entry.resolveEnd(
        event.error ? Err(sysErr(event.error)) : Ok("stopped" as const),
      );
      break;
    }
    case "notifications": {
      latestNotifications = event.items;
      for (const listener of notificationListeners) {
        listener(latestNotifications);
      }
      break;
    }
    case "process_started": {
      const entry = activeProcesses.get(event.id);
      if (!entry) return;
      if (!event.ok) {
        activeProcesses.delete(event.id);
        entry.resolveStart(Err(sysErr(event.error ?? "Process spawn failed")));
        return;
      }
      entry.resolveStart(
        Ok({
          stdout: entry.stdout,
          stderr: entry.stderr,
          exited: entry.exited,
          write: (data) =>
            postToHost({
              type: "process_write",
              id: event.id,
              data_b64: toBase64(data),
            }),
          closeStdin: () =>
            postToHost({ type: "process_close_stdin", id: event.id }),
          kill: (signal) =>
            postToHost({
              type: "process_kill",
              id: event.id,
              signal: toPosixSignal(signal),
            }),
        }),
      );
      break;
    }
    case "process_output": {
      const entry = activeProcesses.get(event.id);
      if (!entry) return;
      const bytes = base64ToBytes(event.data_b64);
      if (event.stream === "stdout") {
        entry.stdoutController?.enqueue(bytes);
      } else if (event.stream === "stderr") {
        entry.stderrController?.enqueue(bytes);
      }
      break;
    }
    case "process_exited": {
      const entry = activeProcesses.get(event.id);
      if (!entry) return;
      activeProcesses.delete(event.id);
      try {
        entry.stdoutController?.close();
      } catch {
        // Stream already closed.
      }
      try {
        entry.stderrController?.close();
      } catch {
        // Stream already closed.
      }
      entry.resolveExit(
        Ok({
          code: event.code ?? undefined,
          signal: event.signal ?? undefined,
        }),
      );
      break;
    }
    case "file_reply": {
      const resolve = pendingFileReplies.get(event.id);
      if (!resolve) return;
      pendingFileReplies.delete(event.id);
      if (event.ok) {
        resolve(Ok(event.data ?? {}));
      } else {
        resolve(
          Err(
            sysErr(
              event.error ?? "File operation failed",
              mapFileErrorKind(event.error_kind),
            ),
          ),
        );
      }
      break;
    }
    case "sway_state": {
      latestSwayState = {
        workspaces: event.workspaces,
        focused_title: event.focused_title,
        mode: event.mode,
      };
      for (const listener of swayListeners) {
        listener(latestSwayState);
      }
      break;
    }
    case "tray": {
      latestTrayItems = event.items;
      for (const listener of trayListeners) {
        listener(latestTrayItems);
      }
      break;
    }
    case "tray_menu_reply": {
      const pending = pendingTrayMenus.get(event.id);
      if (!pending) return;
      pendingTrayMenus.delete(event.id);
      if (event.ok) {
        pending.resolve(event.items);
      } else {
        pending.reject(new Error(event.error ?? "Failed to fetch tray menu"));
      }
      break;
    }
  }
};

export const dbus: Pick<Dbus, "call" | "match"> = {
  call: (call: DbusCall) =>
    new Promise((resolve) => {
      const id = nextId++;
      pendingCalls.set(id, resolve);
      postToHost({
        type: "dbus_call",
        id,
        bus: call.bus === Bus.System ? "system" : "session",
        destination: call.destination,
        path: call.path,
        interface: call.interface,
        member: call.member,
        signature: call.signature ?? "",
        body: JSON.stringify(call.body ?? []),
      });
    }),

  match: (match: DbusMatch) =>
    new Promise((resolveStart) => {
      const id = nextId++;
      let resolveEnd!: (result: Result<"stopped", SystemError>) => void;
      const ended = new Promise<Result<"stopped", SystemError>>((res) => {
        resolveEnd = res;
      });
      activeMatches.set(id, { resolveStart, resolveEnd, ended });
      postToHost({
        type: "dbus_match",
        id,
        bus: match.bus === Bus.System ? "system" : "session",
        sender: match.sender ?? null,
        path: match.path ?? null,
        interface: match.interface ?? null,
        member: match.member ?? null,
      });
    }),
};

export const processes: Pick<Processes, "spawn" | "run"> = {
  spawn: (argv: readonly string[], options?: SpawnOptions) =>
    new Promise((resolveStart) => {
      const id = nextId++;
      let resolveExit!: (result: Result<Exit, SystemError>) => void;
      const exited = new Promise<Result<Exit, SystemError>>((res) => {
        resolveExit = res;
      });
      let stdoutController:
        | ReadableStreamDefaultController<Uint8Array>
        | undefined;
      let stderrController:
        | ReadableStreamDefaultController<Uint8Array>
        | undefined;
      const stdout = new ReadableStream<Uint8Array>({
        start(controller) {
          stdoutController = controller;
        },
      });
      const stderr = new ReadableStream<Uint8Array>({
        start(controller) {
          stderrController = controller;
        },
      });
      activeProcesses.set(id, {
        resolveStart,
        resolveExit,
        stdoutController,
        stderrController,
        stdout,
        stderr,
        exited,
      });
      postToHost({
        type: "process_spawn",
        id,
        argv: [...argv],
        cwd: options?.cwd ?? null,
        env: options?.env ?? {},
        stdin: options?.stdin ?? false,
      });
    }),

  run: async (
    argv: readonly string[],
    options?: Omit<SpawnOptions, "stdin">,
  ): Promise<Result<Ran, SystemError>> => {
    const spawned = await processes.spawn(argv, options);
    return spawned.andThenAsync(async (proc) => {
      const [stdout, stderr, exit] = await Promise.all([
        readStreamText(proc.stdout),
        readStreamText(proc.stderr),
        proc.exited,
      ]);
      return exit.map((status) => ({ ...status, stdout, stderr }));
    });
  },
};

function requestFile<T extends {}>(
  type: "file_read_text" | "file_read_dir" | "file_stat",
  path: string,
): Promise<Result<T, SystemError>> {
  return new Promise((resolve) => {
    const id = nextId++;
    pendingFileReplies.set(
      id,
      resolve as (result: Result<{}, SystemError>) => void,
    );
    postToHost({ type, id, path });
  });
}

export const files: Pick<Files, "readText" | "readDir" | "stat"> = {
  readText: (path: string) => requestFile<string>("file_read_text", path),
  readDir: (path: string) => requestFile<DirEntry[]>("file_read_dir", path),
  stat: (path: string) => requestFile<Stat>("file_stat", path),
};

export const sway = {
  onState: (cb: (state: SwayState) => void): (() => void) => {
    swayListeners.add(cb);
    if (latestSwayState) {
      cb(latestSwayState);
    }
    return () => swayListeners.delete(cb);
  },
  command: (cmd: string) => postToHost({ type: "sway_command", command: cmd }),
};

export const tray = {
  onChange: (cb: (items: TrayItem[]) => void): (() => void) => {
    trayListeners.add(cb);
    cb(latestTrayItems);
    return () => trayListeners.delete(cb);
  },
  activate: (bus: string, path: string) =>
    postToHost({ type: "tray_activate", bus, path }),
  getMenu: (bus: string, menuPath: string): Promise<TrayMenuItem[]> =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pendingTrayMenus.set(id, { resolve, reject });
      postToHost({
        type: "tray_get_menu",
        id,
        bus,
        menu_path: menuPath,
      });
    }),
  menuEvent: (bus: string, menuPath: string, itemId: number) =>
    postToHost({
      type: "tray_menu_event",
      bus,
      menu_path: menuPath,
      item_id: itemId,
    }),
};

export const notifications = {
  onChange: (cb: (items: NotificationItem[]) => void): (() => void) => {
    notificationListeners.add(cb);
    cb(latestNotifications);
    return () => notificationListeners.delete(cb);
  },
  dismiss: (id: number) => postToHost({ type: "dismiss_notification", id }),
  clearAll: () => postToHost({ type: "clear_notifications" }),
};

export type InputRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export const webbar = {
  setInputRegion: (rects: InputRect[]) =>
    postToHost({
      type: "set_input_region",
      rects: rects.map((r) => ({
        x: Math.round(r.x),
        y: Math.round(r.y),
        width: Math.round(r.width),
        height: Math.round(r.height),
      })),
    }),
  resetInputRegion: () => postToHost({ type: "set_input_region", rects: [] }),
};

Object.assign(window, {
  domicile: { dbus, processes, files },
  sway,
  tray,
  notifications,
  webbar,
});

// Automatically expand/collapse the Wayland overlay when HTML popovers or dialogs open.
let lastOverlayActive = false;
let overlaySyncPending = false;
const OVERLAY_SELECTOR =
  ':popover-open, [role="dialog"]:not([hidden]):not([aria-hidden="true"]), [aria-modal="true"]:not([hidden]):not([aria-hidden="true"]), [data-webbar-overlay]:not([hidden])';

function syncOverlay(): void {
  overlaySyncPending = false;
  let active = false;
  try {
    active = document.querySelector(OVERLAY_SELECTOR) !== null;
  } catch {
    // Fallback if :popover-open is unsupported in an older WebKitGTK build.
    active =
      document.querySelector(
        '[role="dialog"]:not([hidden]):not([aria-hidden="true"]), [aria-modal="true"]:not([hidden]):not([aria-hidden="true"]), [data-webbar-overlay]:not([hidden])',
      ) !== null;
  }
  if (active !== lastOverlayActive) {
    lastOverlayActive = active;
    postToHost({ type: "set_overlay", active });
  }
}

function scheduleOverlaySync(): void {
  if (overlaySyncPending) return;
  overlaySyncPending = true;
  requestAnimationFrame(syncOverlay);
}

window.addEventListener("DOMContentLoaded", () => {
  document.addEventListener("toggle", scheduleOverlaySync, true);
  if (document.body) {
    new MutationObserver(scheduleOverlaySync).observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: [
        "open",
        "hidden",
        "aria-hidden",
        "role",
        "aria-modal",
        "data-webbar-overlay",
      ],
    });
  }
  scheduleOverlaySync();
});

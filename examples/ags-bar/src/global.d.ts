import type { Dbus } from "@domicile-desktop/sdk/desktop/dbus";
import type { Files } from "@domicile-desktop/sdk/desktop/files";
import type { Processes } from "@domicile-desktop/sdk/desktop/processes";

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
  icon_url?: string;
};

export type NotificationItem = {
  id: number;
  app_name: string;
  summary: string;
  body: string;
};

declare global {
  interface Window {
    domicile: {
      dbus: Pick<Dbus, "call" | "match">;
      processes: Pick<Processes, "spawn" | "run">;
      files: Pick<Files, "readText" | "readDir" | "stat">;
    };
    sway: {
      onState: (cb: (state: SwayState) => void) => () => void;
      command: (cmd: string) => void;
    };
    tray: {
      onChange: (cb: (items: TrayItem[]) => void) => () => void;
      activate: (bus: string, path: string) => void;
    };
    notifications: {
      onChange: (cb: (items: NotificationItem[]) => void) => () => void;
      dismiss: (id: number) => void;
      clearAll: () => void;
    };
    webbar?: {
      setInputRegion: (
        rects: { x: number; y: number; width: number; height: number }[],
      ) => void;
      resetInputRegion: () => void;
    };
  }
}

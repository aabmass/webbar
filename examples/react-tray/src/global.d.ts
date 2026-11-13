export type TrayItem = {
  id: string;
  bus: string;
  path: string;
  title: string;
  icon_name: string;
};

declare global {
  interface Window {
    tray?: {
      onChange: (cb: (items: TrayItem[]) => void) => () => void;
      activate: (bus: string, path: string) => void;
    };
  }
}

import type { TrayItem, TrayMenuItem } from "@webbar/types";

/**
 * Dev-only fixtures for previewing the bar in a plain browser.
 *
 * Headless `--ws-bridge` mode does not claim the StatusNotifierWatcher, so the
 * tray is always empty there. Append `&mockTray` to the dev URL to populate it
 * with a few fake items so tray styling can be inspected.
 */

function svgIcon(body: string, color: string): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="${color}">${body}</svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

const MOCK_TRAY: TrayItem[] = [
  {
    bus: ":1.101",
    path: "/StatusNotifierItem",
    title: "Network",
    icon_name: "network-wireless",
    icon_url: svgIcon(
      '<path d="M12 18.5a1.75 1.75 0 1 0 0 .01zM5.2 11.6a9.6 9.6 0 0 1 13.6 0l-1.5 1.5a7.5 7.5 0 0 0-10.6 0zm-3-3a13.8 13.8 0 0 1 19.6 0l-1.5 1.5a11.7 11.7 0 0 0-16.6 0zm6 6a5.4 5.4 0 0 1 7.6 0l-1.5 1.5a3.3 3.3 0 0 0-4.6 0z"/>',
      "#e8e8ec",
    ),
    menu_path: "/MenuBar",
  },
  {
    bus: ":1.102",
    path: "/StatusNotifierItem",
    title: "Chat",
    icon_name: "chat",
    icon_url: svgIcon(
      '<path d="M4 4h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H9l-5 4v-4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z"/>',
      "#7aa2f7",
    ),
    menu_path: "/MenuBar",
  },
  {
    bus: ":1.103",
    path: "/StatusNotifierItem",
    title: "Sync",
    icon_name: "sync",
    icon_url: svgIcon(
      '<path d="M12 4a8 8 0 0 1 7.4 5H17l3 4 3-4h-1.5A10 10 0 0 0 2.3 10l2 .5A8 8 0 0 1 12 4zm7.7 9.5A8 8 0 0 1 4.6 15H7l-3-4-3 4h1.5a10 10 0 0 0 19.2 1z"/>',
      "#9ece6a",
    ),
    menu_path: "",
  },
];

let nextId = 1;
function entry(label: string, extra: Partial<TrayMenuItem> = {}): TrayMenuItem {
  return {
    id: nextId++,
    label,
    enabled: true,
    visible: true,
    item_type: "standard",
    toggle_type: "",
    toggle_state: 0,
    icon_name: "",
    icon_url: "",
    accessible_desc: "",
    children: [],
    ...extra,
  };
}

const MOCK_MENU: TrayMenuItem[] = [
  entry("Open Window"),
  entry("Do Not Disturb", { toggle_type: "checkmark", toggle_state: 1 }),
  entry("Status", {
    children: [
      entry("Online", { toggle_type: "radio", toggle_state: 1 }),
      entry("Away", { toggle_type: "radio" }),
    ],
  }),
  entry("", { item_type: "separator" }),
  entry("Preferences…", { enabled: false }),
  entry("Quit"),
];

export function installDevMocks(): void {
  const params = new URLSearchParams(window.location.search);
  if (!params.has("mockTray")) return;

  const mockMenuStore = {
    get: () => MOCK_MENU,
    subscribe: () => () => {},
    onChange: (cb: (items: TrayMenuItem[]) => void) => {
      cb(MOCK_MENU);
      return () => {};
    },
  };

  const tray = window.webbar.tray;
  window.webbar.tray = {
    ...tray,
    get: () => MOCK_TRAY,
    subscribe: () => () => {},
    activate: () => {},
    getMenu: async () => MOCK_MENU,
    menu: () => mockMenuStore,
    menuEvent: () => {},
  };
}

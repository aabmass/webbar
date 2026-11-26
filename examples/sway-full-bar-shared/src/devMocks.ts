import type {
  NotificationItem,
  NotificationsState,
  TrayItem,
  TrayMenuItem,
} from "@webbar/types";

/**
 * Dev-only fixtures for previewing the bar in a plain browser.
 *
 * Headless `--ws-bridge` mode does not claim StatusNotifierWatcher or
 * org.freedesktop.Notifications. Append `&mockTray` or `&mockNotifications` to
 * the dev URL to populate them with sample items for styling inspection.
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

function installTrayMocks(): void {
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

function installNotificationMocks(): void {
  const now = Date.now();
  const initialItems: NotificationItem[] = [
    {
      id: 1,
      app_name: "Signal",
      app_icon_name: "signal-desktop",
      app_icon_url: svgIcon(
        '<circle cx="12" cy="12" r="9"/>',
        "#3a76f0",
      ),
      image_url: "",
      summary: "Alice",
      body: "Are we still on for lunch at 12:30?",
      urgency: "normal",
      has_default_action: true,
      actions: [
        { key: "reply", label: "Reply" },
        { key: "mark-read", label: "Mark Read" },
      ],
      timestamp_ms: now - 45_000,
      category: "im.received",
      desktop_entry: "signal-desktop",
      progress: null,
      transient: false,
      resident: false,
      popup: true,
      read: false,
    },
    {
      id: 2,
      app_name: "System",
      app_icon_name: "battery-caution",
      app_icon_url: "",
      image_url: "",
      summary: "Battery Critically Low",
      body: "5% remaining — plug in AC power soon.",
      urgency: "critical",
      has_default_action: false,
      actions: [],
      timestamp_ms: now - 3 * 60_000,
      category: "device",
      desktop_entry: "",
      progress: null,
      transient: false,
      resident: false,
      popup: true,
      read: false,
    },
    {
      id: 3,
      app_name: "Downloads",
      app_icon_name: "folder-download",
      app_icon_url: "",
      image_url: "",
      summary: "nixos-unstable.iso",
      body: "Downloading 68% (42 MB/s)",
      urgency: "low",
      has_default_action: false,
      actions: [],
      timestamp_ms: now - 14 * 60_000,
      category: "transfer",
      desktop_entry: "",
      progress: 68,
      transient: false,
      resident: false,
      popup: false,
      read: true,
    },
  ];

  let state: NotificationsState = {
    available: true,
    do_not_disturb: false,
    unread_count: initialItems.filter((i) => !i.read).length,
    items: initialItems,
  };
  const subs = new Set<() => void>();

  const update = (next: NotificationsState) => {
    state = {
      ...next,
      unread_count: next.items.filter((i) => !i.read).length,
    };
    for (const cb of subs) cb();
  };

  const subscribe = (cb: () => void) => {
    subs.add(cb);
    return () => {
      subs.delete(cb);
    };
  };

  const dismiss = (id: number) =>
    update({ ...state, items: state.items.filter((i) => i.id !== id) });
  const hidePopup = (id: number) =>
    update({
      ...state,
      items: state.items.map((i) =>
        i.id === id ? { ...i, popup: false } : i,
      ),
    });

  window.webbar.notifications = {
    get: () => state,
    subscribe,
    onChange: (cb) => {
      cb(state);
      return subscribe(() => cb(state));
    },
    dismiss,
    hidePopup,
    invokeAction: (id) => dismiss(id),
    activate: (id) => {
      const target = state.items.find((i) => i.id === id);
      if (!target) return;
      if (target.has_default_action) {
        dismiss(id);
      } else if (target.popup) {
        hidePopup(id);
      }
    },
    clearAll: () => update({ ...state, items: [] }),
    setDoNotDisturb: (do_not_disturb) => update({ ...state, do_not_disturb }),
    markAllRead: () =>
      update({
        ...state,
        items: state.items.map((i) => ({ ...i, read: true })),
      }),
    pauseTimeout: () => {},
    resumeTimeout: () => {},
  };
}

export function installDevMocks(): void {
  // Standalone browser preview fallback when webbar hasn't set data-desktop-theme
  if (
    typeof document !== "undefined" &&
    typeof window !== "undefined" &&
    !document.documentElement.dataset.desktopTheme &&
    typeof window.matchMedia === "function"
  ) {
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const sync = () => {
      if (!document.documentElement.dataset.desktopTheme) {
        document.documentElement.dataset.desktopTheme = mq.matches
          ? "light"
          : "dark";
      }
    };
    sync();
  }
  const params = new URLSearchParams(window.location.search);
  if (params.has("mockTray")) installTrayMocks();
  if (params.has("mockNotifications")) installNotificationMocks();
}

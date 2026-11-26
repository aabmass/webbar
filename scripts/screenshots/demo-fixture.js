(() => {
  const params = new URLSearchParams(window.location.search);
  const themeMode = params.get("theme") === "light" ? "light" : "dark";
  const isDark = themeMode === "dark";
  const openTarget = params.get("open") || "";
  const showToasts = params.get("toasts") === "1";

  // Freeze Date to a clean, deterministic timestamp (Fri, Oct 16, 2026 14:35:42 UTC)
  // so regenerating screenshots does not create noisy timestamp diffs.
  const FIXED_MS = Date.UTC(2026, 9, 16, 14, 35, 42);
  const RealDate = window.Date;
  class DemoDate extends RealDate {
    constructor(...args) {
      if (args.length === 0) {
        super(FIXED_MS);
      } else {
        super(...args);
      }
    }
    static now() {
      return FIXED_MS;
    }
  }
  window.Date = DemoDate;

  // Enforce deterministic data-desktop-theme and color-scheme even if the host
  // emits a late GTK theme snapshot in headless mode.
  const applyThemeAttr = () => {
    const root = document.documentElement;
    if (root.getAttribute("data-desktop-theme") !== themeMode) {
      root.setAttribute("data-desktop-theme", themeMode);
    }
    if (root.style.colorScheme !== themeMode) {
      root.style.colorScheme = themeMode;
    }
  };
  applyThemeAttr();
  new MutationObserver(applyThemeAttr).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-desktop-theme", "style"],
  });

  function makeStaticStore(value) {
    return {
      get: () => value,
      subscribe: () => () => {},
      onChange: (cb) => {
        cb(value);
        return () => {};
      },
    };
  }

  function svgIcon(body, color) {
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="${color}">${body}</svg>`;
    return `data:image/svg+xml,${encodeURIComponent(svg)}`;
  }

  const DEMO_AUDIO = {
    available: true,
    outputs: [
      {
        id: 41,
        name: "alsa_output.usb-studio-monitors",
        description: "Studio Monitors",
        volume: 0.72,
        muted: false,
        default: true,
      },
      {
        id: 42,
        name: "alsa_output.usb-dac",
        description: "USB-C Headphone DAC",
        volume: 0.85,
        muted: false,
        default: false,
      },
      {
        id: 43,
        name: "alsa_output.hdmi-display",
        description: "HDMI Display Audio",
        volume: 0.5,
        muted: false,
        default: false,
      },
    ],
    inputs: [
      {
        id: 51,
        name: "alsa_input.usb-condenser",
        description: "USB Condenser Mic",
        volume: 0.8,
        muted: false,
        default: true,
      },
    ],
    streams: [
      {
        id: 101,
        name: "Tycho — Awake",
        app_name: "Spotify",
        device_id: 41,
        volume: 0.84,
        muted: false,
      },
      {
        id: 102,
        name: "Lofi Study Radio",
        app_name: "Firefox",
        device_id: 41,
        volume: 0.68,
        muted: false,
      },
    ],
  };

  const DEMO_BLUETOOTH = {
    available: true,
    adapters: [
      {
        path: "/org/bluez/hci0",
        name: "hci0",
        powered: true,
      },
    ],
    devices: [
      {
        path: "/org/bluez/hci0/dev_00_11_22_33_44_55",
        name: "WH-1000XM5",
        address: "00:11:22:33:44:55",
        paired: true,
        connected: true,
        battery: 85,
      },
      {
        path: "/org/bluez/hci0/dev_00_11_22_33_44_66",
        name: "Keychron Q1 Pro",
        address: "00:11:22:33:44:66",
        paired: true,
        connected: true,
        battery: 92,
      },
      {
        path: "/org/bluez/hci0/dev_00_11_22_33_44_77",
        name: "MX Master 3S",
        address: "00:11:22:33:44:77",
        paired: true,
        connected: false,
        battery: 64,
      },
    ],
  };

  const DEMO_BACKLIGHT = {
    available: true,
    brightness: 78,
    error: null,
    displays: [
      {
        id: "ddcci-1",
        name: "HEADLESS-1",
        kind: "ddcci",
        output: "HEADLESS-1",
        brightness: 78,
        available: true,
        error: null,
      },
    ],
  };

  const DEMO_THEME = {
    dark: isDark,
    high_contrast: false,
    theme_name: "Adwaita",
    icon_theme_name: "Adwaita",
    font_family: '"Inter", sans-serif',
    font_size: "11pt",
    font_weight: 500,
    monospace_font_family: '"JetBrains Mono", monospace',
    enable_animations: false,
    accent_color: isDark ? "#8ab4f8" : "#2b6de0",
    desktop_colors: {},
    gtk_colors: {},
  };

  const trayFg = isDark ? "#e8e8ec" : "#232530";
  const trayAccent = isDark ? "#8ab4f8" : "#2b6de0";
  const trayGreen = isDark ? "#6fcf97" : "#1f8f54";

  const DEMO_NOTIFICATION_ITEMS = [
    {
      id: 1,
      app_name: "Signal",
      app_icon_name: "signal-desktop",
      app_icon_url: svgIcon('<circle cx="12" cy="12" r="9"/>', trayAccent),
      image_url: "",
      summary: "Alice",
      body: "Are we still on for lunch at 12:30?",
      urgency: "normal",
      has_default_action: true,
      actions: [
        { key: "reply", label: "Reply" },
        { key: "mark-read", label: "Mark Read" },
      ],
      timestamp_ms: FIXED_MS - 45_000,
      category: "im.received",
      desktop_entry: "signal-desktop",
      progress: null,
      transient: false,
      resident: false,
      popup: showToasts,
      read: false,
    },
    {
      id: 2,
      app_name: "System",
      app_icon_name: "dialog-warning",
      app_icon_url: svgIcon(
        '<path d="M12 3L2 20h20L12 3zm0 6v5m0 3v1"/>',
        "#f2c94c",
      ),
      image_url: "",
      summary: "System Update Ready",
      body: "Rebuild complete — restart to apply kernel 6.12.",
      urgency: "normal",
      has_default_action: false,
      actions: [{ key: "restart", label: "Restart" }],
      timestamp_ms: FIXED_MS - 3 * 60_000,
      category: "device",
      desktop_entry: "",
      progress: null,
      transient: false,
      resident: false,
      popup: showToasts,
      read: false,
    },
    {
      id: 3,
      app_name: "Downloads",
      app_icon_name: "folder-download",
      app_icon_url: svgIcon(
        '<path d="M12 3a1 1 0 0 1 1 1v7.59l2.3-2.3a1 1 0 1 1 1.4 1.42l-4 4a1 1 0 0 1-1.4 0l-4-4a1 1 0 0 1 1.4-1.42L11 11.59V4a1 1 0 0 1 1-1zm-7 14h14v2H5v-2z"/>',
        trayGreen,
      ),
      image_url: "",
      summary: "nixos-unstable.iso",
      body: "Downloading 68% (42 MB/s)",
      urgency: "low",
      has_default_action: false,
      actions: [],
      timestamp_ms: FIXED_MS - 14 * 60_000,
      category: "transfer",
      desktop_entry: "",
      progress: 68,
      transient: false,
      resident: false,
      popup: false,
      read: true,
    },
  ];

  const DEMO_TRAY = [
    {
      bus: ":1.101",
      path: "/StatusNotifierItem",
      title: "Network",
      icon_name: "network-wireless",
      icon_url: svgIcon(
        '<path d="M12 18.5a1.75 1.75 0 1 0 0 .01zM5.2 11.6a9.6 9.6 0 0 1 13.6 0l-1.5 1.5a7.5 7.5 0 0 0-10.6 0zm-3-3a13.8 13.8 0 0 1 19.6 0l-1.5 1.5a11.7 11.7 0 0 0-16.6 0zm6 6a5.4 5.4 0 0 1 7.6 0l-1.5 1.5a3.3 3.3 0 0 0-4.6 0z"/>',
        trayFg,
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
        trayAccent,
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
        trayGreen,
      ),
      menu_path: "",
    },
  ];

  const DEMO_TRAY_MENU = [
    {
      id: 1,
      label: "Open Window",
      enabled: true,
      visible: true,
      item_type: "standard",
      toggle_type: "",
      toggle_state: 0,
      icon_name: "",
      icon_url: "",
      accessible_desc: "",
      children: [],
    },
    {
      id: 2,
      label: "Do Not Disturb",
      enabled: true,
      visible: true,
      item_type: "standard",
      toggle_type: "checkmark",
      toggle_state: 1,
      icon_name: "",
      icon_url: "",
      accessible_desc: "",
      children: [],
    },
    {
      id: 3,
      label: "",
      enabled: true,
      visible: true,
      item_type: "separator",
      toggle_type: "",
      toggle_state: 0,
      icon_name: "",
      icon_url: "",
      accessible_desc: "",
      children: [],
    },
    {
      id: 4,
      label: "Quit",
      enabled: true,
      visible: true,
      item_type: "standard",
      toggle_type: "",
      toggle_state: 0,
      icon_name: "",
      icon_url: "",
      accessible_desc: "",
      children: [],
    },
  ];

  const wb = window.webbar;
  if (wb) {
    // Keep wb.sway REAL (connected to the headless Sway session).
    wb.audio = {
      ...wb.audio,
      ...makeStaticStore(DEMO_AUDIO),
      setVolume: async () => {},
      setMuted: async () => {},
      setDefault: async () => {},
      setStreamVolume: async () => {},
      setStreamMuted: async () => {},
    };

    wb.bluetooth = {
      ...wb.bluetooth,
      ...makeStaticStore(DEMO_BLUETOOTH),
      setPowered: async () => {},
      connect: async () => {},
      disconnect: async () => {},
    };

    wb.backlight = {
      ...wb.backlight,
      ...makeStaticStore(DEMO_BACKLIGHT),
      set: async () => {},
    };

    wb.theme = {
      ...wb.theme,
      ...makeStaticStore(DEMO_THEME),
    };

    const trayMenuStore = makeStaticStore(DEMO_TRAY_MENU);
    wb.tray = {
      ...wb.tray,
      ...makeStaticStore(DEMO_TRAY),
      activate: () => {},
      getMenu: async () => DEMO_TRAY_MENU,
      menu: () => trayMenuStore,
      menuEvent: () => {},
    };

    let notifState = {
      available: true,
      do_not_disturb: false,
      unread_count: DEMO_NOTIFICATION_ITEMS.filter((i) => !i.read).length,
      items: DEMO_NOTIFICATION_ITEMS,
    };
    const notifSubs = new Set();
    const updateNotifs = (next) => {
      notifState = {
        ...next,
        unread_count: next.items.filter((i) => !i.read).length,
      };
      for (const cb of notifSubs) cb();
    };
    const subscribeNotifs = (cb) => {
      notifSubs.add(cb);
      return () => notifSubs.delete(cb);
    };
    const dismissNotif = (id) =>
      updateNotifs({
        ...notifState,
        items: notifState.items.filter((i) => i.id !== id),
      });
    const hideNotifPopup = (id) =>
      updateNotifs({
        ...notifState,
        items: notifState.items.map((i) =>
          i.id === id ? { ...i, popup: false } : i,
        ),
      });
    wb.notifications = {
      get: () => notifState,
      subscribe: subscribeNotifs,
      onChange: (cb) => {
        cb(notifState);
        return subscribeNotifs(() => cb(notifState));
      },
      dismiss: dismissNotif,
      hidePopup: hideNotifPopup,
      invokeAction: (id) => {
        const target = notifState.items.find((i) => i.id === id);
        if (target && !target.resident) {
          dismissNotif(id);
        }
      },
      activate: (id) => {
        const target = notifState.items.find((i) => i.id === id);
        if (!target) return;
        if (target.has_default_action) {
          if (!target.resident) {
            dismissNotif(id);
          }
        } else if (target.popup) {
          hideNotifPopup(id);
        }
      },
      clearAll: () => updateNotifs({ ...notifState, items: [] }),
      setDoNotDisturb: (do_not_disturb) =>
        updateNotifs({ ...notifState, do_not_disturb }),
      markAllRead: () =>
        updateNotifs({
          ...notifState,
          items: notifState.items.map((i) => ({ ...i, read: true })),
        }),
      pauseTimeout: () => {},
      resumeTimeout: () => {},
    };
    wb.greetd = {
      ...makeStaticStore({
        available: true,
        phase: "inactive",
        user: null,
        prompt: null,
        error: null,
      }),
      createSession: async () => wb.greetd.get(),
      respond: async () => wb.greetd.get(),
      cancelSession: async () => {},
      launch: async () => {},
    };

    const origExec = wb.exec.bind(wb);
    wb.exec = async (argv, opts) => {
      if (Array.isArray(argv) && argv[0] === "stasis") {
        if (argv[1] === "list" && argv[2] === "profiles") {
          return {
            code: 0,
            stdout: "Profiles:\ndefault\nwork\npresentation\n",
            stderr: "",
          };
        }
        if (argv[1] === "blame") {
          return {
            code: 0,
            stdout: JSON.stringify({
              progression_blocked: false,
              manual_pause: false,
              system_pause: false,
              browser_source_capture: false,
              app_inhibitors: { active: false, count: 0, sources: [] },
              media_inhibitors: { active: false, count: 0, sources: [] },
              game_inhibitors: { active: false, count: 0, sources: [] },
              suspend_app_inhibitors: { active: false, count: 0, sources: [] },
              suspend_media_inhibitors: { active: false, count: 0, sources: [] },
              dbus_holds: [],
              login1_idle_holds: [],
            }),
            stderr: "",
          };
        }
        return { code: 0, stdout: "", stderr: "" };
      }
      return origExec(argv, opts);
    };

    const origExecStream = wb.execStream.bind(wb);
    wb.execStream = async (argv, opts = {}) => {
      if (Array.isArray(argv) && argv[0] === "stasis" && argv[1] === "watch") {
        const line = JSON.stringify({
          state: "active",
          paused: false,
          manually_paused: false,
          profile: "default",
        });
        setTimeout(() => {
          opts.onStdout?.(line);
        }, 10);
        return new Promise((resolve) => {
          if (opts.signal) {
            opts.signal.addEventListener("abort", () => resolve({ code: 0 }), {
              once: true,
            });
          }
        });
      }
      return origExecStream(argv, opts);
    };
  }

  // Under headless pixman + GSK_RENDERER=cairo, WebKitGTK only commits the
  // bounding box of DOM regions invalidated after the surface maps. Nudging
  // `.centerbox` once after mount forces a full-width 1600x86 repaint so
  // workspaces, clock, tray, and controls all render cleanly.
  const triggerFullBarRepaint = () => {
    setTimeout(() => {
      const targets = document.querySelectorAll(
        ".centerbox, .notif-toast-stack, .backdrop",
      );
      const elements = Array.from(targets).filter(
        (el) => el instanceof HTMLElement,
      );
      if (elements.length === 0) return;
      for (const el of elements) {
        el.style.opacity = "0.999";
      }
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          for (const el of elements) {
            el.style.opacity = "";
          }
        });
      });
    }, 800);
  };
  if (document.readyState === "loading") {
    window.addEventListener("DOMContentLoaded", triggerFullBarRepaint);
  } else {
    triggerFullBarRepaint();
  }

  // Open a requested popover automatically after React mounts.
  if (openTarget) {
    const selectors = {
      calendar: "button.clock",
      stasis: "button.stasis-btn",
      audio: '.widget-cluster button[title*="Studio Monitors"]',
      display: '.widget-cluster button[title*="Monitor brightness"]',
      bluetooth: '.widget-cluster button[title*="Bluetooth"]',
      notifications: "button.notif-btn",
      tray: '.tray button[title^="Network"]',
    };
    const selector = selectors[openTarget];
    if (selector) {
      let attempts = 0;
      const tryOpen = () => {
        const btn = document.querySelector(selector);
        if (btn instanceof HTMLElement) {
          btn.click();
          return;
        }
        if (attempts++ < 60) {
          setTimeout(tryOpen, 50);
        }
      };
      if (document.readyState === "loading") {
        window.addEventListener("DOMContentLoaded", () => setTimeout(tryOpen, 150));
      } else {
        setTimeout(tryOpen, 150);
      }
    }
  }
})();

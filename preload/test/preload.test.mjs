import assert from "node:assert/strict";
import test from "node:test";
import {
  SwayCommandError,
  applyThemeToDocument,
  createStore,
  createWebbarBridge,
} from "../dist/preload.mjs";

// Minimal Sway tree node; only the fields the helpers and reconcile look at.
const swayNode = (id, type, overrides = {}) => ({
  id,
  name: null,
  type,
  focused: false,
  pid: null,
  nodes: [],
  floating_nodes: [],
  ...overrides,
});

const swayTree = (title) =>
  swayNode(1, "root", {
    name: "root",
    nodes: [
      swayNode(2, "output", {
        name: "DP-1",
        nodes: [
          swayNode(3, "workspace", {
            name: "1",
            nodes: [swayNode(4, "con", { name: title, focused: true, pid: 100 })],
          }),
        ],
      }),
    ],
  });

test("createStore satisfies React useSyncExternalStore contract", () => {
  const store = createStore({ count: 0 });
  const initial = store.get();

  // Repeated get() calls without updates must return the exact same reference
  assert.equal(Object.is(store.get(), initial), true);

  let callsA = 0;
  let callsB = 0;
  const unsubA = store.subscribe(() => {
    callsA += 1;
  });
  const unsubB = store.subscribe(() => {
    callsB += 1;
  });

  // subscribe() must not fire synchronously
  assert.equal(callsA, 0);
  assert.equal(callsB, 0);

  store._update({ count: 1 });
  assert.equal(callsA, 1);
  assert.equal(callsB, 1);
  assert.equal(Object.is(store.get(), initial), false);
  assert.equal(store.get().count, 1);

  // Unsubscribing one listener leaves others intact
  unsubA();
  store._update({ count: 2 });
  assert.equal(callsA, 1);
  assert.equal(callsB, 2);

  unsubB();
  store._update({ count: 3 });
  assert.equal(callsB, 2);
});

test("onChange delivers initial snapshot immediately and streams updates", () => {
  const store = createStore("init");
  const seen = [];
  const unsub = store.onChange((val) => seen.push(val));
  assert.deepEqual(seen, ["init"]);

  store._update("next");
  assert.deepEqual(seen, ["init", "next"]);

  unsub();
  store._update("ignored");
  assert.deepEqual(seen, ["init", "next"]);
});

test("all window.webbar stores follow snapshot + subscribe + onChange contract", () => {
  const { api, receive } = createWebbarBridge(() => {});

  const cases = [
    {
      name: "sway",
      store: api.sway,
      event: {
        type: "sway_state",
        available: true,
        workspaces: [
          {
            id: 1,
            num: 1,
            name: "1",
            output: "DP-1",
            focused: true,
            visible: true,
            urgent: false,
          },
        ],
        tree: swayTree("Alacritty"),
        mode: "default",
      },
      assertState: (s) => {
        assert.equal(s.workspaces.length, 1);
        assert.equal(s.tree.nodes[0].nodes[0].nodes[0].name, "Alacritty");
      },
    },
    {
      name: "tray",
      store: api.tray,
      event: {
        type: "tray",
        items: [
          {
            bus: ":1.42",
            path: "/StatusNotifierItem",
            title: "nm-applet",
            icon_name: "network-wireless",
            icon_url: "",
            menu_path: "/MenuBar",
          },
        ],
      },
      assertState: (s) => {
        assert.equal(s.length, 1);
        assert.equal(s[0].title, "nm-applet");
      },
    },
    {
      name: "notifications",
      store: api.notifications,
      event: {
        type: "notifications",
        items: [
          { id: 7, app_name: "notify-send", summary: "Hi", body: "World" },
        ],
      },
      assertState: (s) => {
        assert.equal(s.length, 1);
        assert.equal(s[0].id, 7);
      },
    },
    {
      name: "audio",
      store: api.audio,
      event: {
        type: "audio_state",
        available: true,
        outputs: [
          {
            id: 42,
            name: "alsa_out",
            description: "Speakers",
            volume: 0.5,
            muted: false,
            default: true,
          },
        ],
        inputs: [],
      },
      assertState: (s) => {
        assert.equal(s.available, true);
        assert.equal(s.outputs[0].volume, 0.5);
      },
    },
    {
      name: "bluetooth",
      store: api.bluetooth,
      event: {
        type: "bluetooth_state",
        available: true,
        adapters: [{ path: "/org/bluez/hci0", name: "hci0", powered: true }],
        devices: [
          {
            path: "/org/bluez/hci0/dev_AA",
            name: "Headphones",
            address: "AA:BB:CC:DD:EE:FF",
            paired: true,
            connected: true,
            battery: 80,
          },
        ],
      },
      assertState: (s) => {
        assert.equal(s.available, true);
        assert.equal(s.devices[0].battery, 80);
      },
    },
    {
      name: "network",
      store: api.network,
      event: {
        type: "network_state",
        available: true,
        connectivity: "full",
        link: { kind: "wifi", name: "Home", ssid: "HomeNet", strength: 0.9 },
      },
      assertState: (s) => {
        assert.equal(s.available, true);
        assert.equal(s.link.kind, "wifi");
      },
    },
    {
      name: "backlight",
      store: api.backlight,
      event: {
        type: "backlight_state",
        available: true,
        brightness: 65,
        error: null,
        displays: [
          {
            id: "ddc:1",
            name: "Dell U2723QE",
            kind: "ddc",
            output: "DP-1",
            brightness: 65,
            available: true,
            error: null,
          },
        ],
      },
      assertState: (s) => {
        assert.equal(s.available, true);
        assert.equal(s.brightness, 65);
        assert.equal(s.displays.length, 1);
        assert.equal(s.displays[0].output, "DP-1");
      },
    },
    {
      name: "theme",
      store: api.theme,
      event: {
        type: "theme",
        dark: false,
        high_contrast: false,
        theme_name: "Adwaita",
        icon_theme_name: "Adwaita",
        font_family: '"Inter", sans-serif',
        font_size: "10pt",
        font_weight: 500,
        monospace_font_family: '"JetBrains Mono", sans-serif',
        enable_animations: true,
        accent_color: "#3584e4",
        desktop_colors: { "window-bg": "#fafafa", "accent-bg": "#3584e4" },
        gtk_colors: { "theme-bg-color": "#fafafa" },
      },
      assertState: (s) => {
        assert.equal(s.dark, false);
        assert.equal(s.font_family, '"Inter", sans-serif');
        assert.equal(s.font_weight, 500);
        assert.equal(s.desktop_colors["window-bg"], "#fafafa");
        assert.equal(s.gtk_colors["theme-bg-color"], "#fafafa");
      },
    },
  ];

  for (const { name, store, event, assertState } of cases) {
    const before = store.get();
    assert.equal(Object.is(store.get(), before), true, `${name}: stable get()`);

    let subCount = 0;
    const unsub = store.subscribe(() => {
      subCount += 1;
    });
    const onChangeSnapshots = [];
    const unsubChange = store.onChange((val) => onChangeSnapshots.push(val));
    assert.equal(subCount, 0, `${name}: subscribe must not fire sync`);
    assert.equal(onChangeSnapshots.length, 1, `${name}: onChange fires sync`);

    receive(event);

    assert.equal(subCount, 1, `${name}: subscribe fires on event`);
    assert.equal(onChangeSnapshots.length, 2, `${name}: onChange fires on event`);
    const after = store.get();
    assert.equal(Object.is(before, after), false, `${name}: new snapshot ref`);
    assert.equal(
      Object.is(store.get(), after),
      true,
      `${name}: stable get() after update`,
    );
    assertState(after);

    unsub();
    unsubChange();
  }
});

test("Promise actions resolve and reject with standard Error instances", async () => {
  const sent = [];
  const { api, receive } = createWebbarBridge((json) => {
    sent.push(JSON.parse(json));
  });

  const p1 = api.audio.setVolume(3, 0.65);
  assert.ok(p1 instanceof Promise);
  assert.equal(sent[0].type, "audio_set_volume");
  receive({ type: "rpc_reply", id: sent[0].id, ok: true });
  await p1;

  const p2 = api.bluetooth.connect("/org/bluez/hci0/dev_1");
  receive({
    type: "rpc_reply",
    id: sent[1].id,
    ok: false,
    error: "Device not ready",
  });
  await assert.rejects(p2, (err) => {
    assert.ok(err instanceof Error);
    assert.match(err.message, /Device not ready/);
    return true;
  });

  const pMenu = api.tray.getMenu(":1.42", "/MenuBar");
  receive({
    type: "rpc_reply",
    id: sent[2].id,
    ok: true,
    data: [{ id: 1, label: "Quit", enabled: true, visible: true, item_type: "standard", toggle_type: "", toggle_state: 0, children: [] }],
  });
  const menu = await pMenu;
  assert.equal(menu[0].label, "Quit");

  const pBlDefault = api.backlight.set(70);
  assert.deepEqual(sent[3], {
    type: "backlight_set",
    id: sent[3].id,
    percent: 70,
  });
  receive({ type: "rpc_reply", id: sent[3].id, ok: true });
  await pBlDefault;

  const pBlTarget = api.backlight.set(45, "ddc:2");
  assert.deepEqual(sent[4], {
    type: "backlight_set",
    id: sent[4].id,
    percent: 45,
    display_id: "ddc:2",
  });
  receive({ type: "rpc_reply", id: sent[4].id, ok: true });
  await pBlTarget;
});

test("dbus and exec escape hatches follow Promise + Error contract", async () => {
  const sent = [];
  const { api, receive } = createWebbarBridge((json) => {
    sent.push(JSON.parse(json));
  });

  // dbus.call
  const callP = api.dbus.call({
    bus: "session",
    destination: "org.example.Test",
    path: "/org/example/Test",
    interface: "org.example.Test",
    member: "Ping",
  });
  receive({
    type: "rpc_reply",
    id: sent[0].id,
    ok: true,
    data: { signature: "s", body: ["pong"] },
  });
  const res = await callP;
  assert.deepEqual(res, { signature: "s", body: ["pong"] });

  // dbus.watch
  const signals = [];
  const watchP = api.dbus.watch(
    { bus: "session", interface: "org.example.Test" },
    (sig) => signals.push(sig),
  );
  receive({ type: "dbus_match_started", id: sent[1].id, ok: true, error: null });
  const watcher = await watchP;
  receive({
    type: "dbus_signal",
    id: sent[1].id,
    sender: ":1.1",
    path: "/org/example/Test",
    interface: "org.example.Test",
    member: "Changed",
    signature: "u",
    body: [42],
  });
  assert.equal(signals.length, 1);
  assert.deepEqual(signals[0].body, [42]);
  watcher.stop();
  assert.equal(sent[2].type, "dbus_unwatch");
  assert.equal(sent[2].id, sent[1].id);

  // exec
  const execP = api.exec(["echo", "hi"]);
  receive({
    type: "rpc_reply",
    id: sent[3].id,
    ok: true,
    data: {
      code: 0,
      signal: null,
      stdout: "hello world\n",
      stderr: "warn\n",
    },
  });
  const execRes = await execP;
  assert.deepEqual(execRes, {
    code: 0,
    signal: null,
    stdout: "hello world\n",
    stderr: "warn\n",
  });
});

test("AbortSignal cancels exec and dbus.watch, and output + notifications.invokeAction work", async () => {
  const sent = [];
  const { api } = createWebbarBridge((json) => {
    sent.push(JSON.parse(json));
  }, "DP-2");

  assert.equal(api.output, "DP-2");

  api.notifications.invokeAction(9, "open");
  assert.deepEqual(sent[0], {
    type: "invoke_notification_action",
    id: 9,
    action_key: "open",
  });

  // Pre-aborted signal rejects immediately without posting
  const preAborted = new AbortController();
  preAborted.abort();
  await assert.rejects(
    api.exec(["sleep", "10"], { signal: preAborted.signal }),
    (err) => err.name === "AbortError",
  );
  assert.equal(sent.length, 1);

  // In-flight exec abort sends process_kill(SIGTERM)
  const execCtrl = new AbortController();
  const execP = api.exec(["sleep", "10"], { signal: execCtrl.signal });
  const spawnId = sent[1].id;
  execCtrl.abort();
  await assert.rejects(execP, (err) => err.name === "AbortError");
  assert.deepEqual(sent[2], { type: "process_kill", id: spawnId, signal: 15 });

  // In-flight dbus.watch abort sends dbus_unwatch
  const watchCtrl = new AbortController();
  const watchP = api.dbus.watch(
    { bus: "session", interface: "org.example.Test", signal: watchCtrl.signal },
    () => {},
  );
  const watchId = sent[3].id;
  watchCtrl.abort();
  await assert.rejects(watchP, (err) => err.name === "AbortError");
  assert.deepEqual(sent[4], { type: "dbus_unwatch", id: watchId });
});

test("stores preserve unchanged slice references and skip identical updates", () => {
  const { api, receive } = createWebbarBridge(() => {});

  const makeWorkspace = (overrides = {}) => ({
    id: 1,
    num: 1,
    name: "1",
    output: "DP-1",
    focused: true,
    visible: true,
    urgent: false,
    ...overrides,
  });

  receive({
    type: "sway_state",
    available: true,
    workspaces: [makeWorkspace()],
    tree: swayTree("Alacritty"),
    mode: "default",
  });

  const firstSnapshot = api.sway.get();
  const firstWorkspaces = firstSnapshot.workspaces;

  let subCalls = 0;
  let changeCalls = 0;
  api.sway.subscribe(() => {
    subCalls += 1;
  });
  api.sway.onChange(() => {
    changeCalls += 1;
  });
  // onChange fires once synchronously on registration
  assert.equal(changeCalls, 1);

  // Identical snapshot must not notify listeners or replace snapshot reference
  receive({
    type: "sway_state",
    available: true,
    workspaces: [makeWorkspace()],
    tree: swayTree("Alacritty"),
    mode: "default",
  });
  assert.equal(subCalls, 0);
  assert.equal(changeCalls, 1);
  assert.equal(Object.is(api.sway.get(), firstSnapshot), true);

  // Changing only a window title must preserve workspaces array reference
  receive({
    type: "sway_state",
    available: true,
    workspaces: [makeWorkspace()],
    tree: swayTree("Firefox"),
    mode: "default",
  });
  assert.equal(subCalls, 1);
  assert.equal(changeCalls, 2);
  assert.equal(api.sway.get().tree.nodes[0].nodes[0].nodes[0].name, "Firefox");
  assert.equal(Object.is(api.sway.get().workspaces, firstWorkspaces), true);
});

test("overlay controller ref-counts acquireOverlay, coalesces switches, and tracks native popovers", async () => {
  const sent = [];
  let nativePopoverOpen = false;
  const { api, notifyOverlayChange } = createWebbarBridge(
    (json) => {
      sent.push(JSON.parse(json));
    },
    "DP-1",
    () => nativePopoverOpen,
  );

  // Initial check when closed sends nothing
  notifyOverlayChange();
  assert.equal(sent.length, 0);

  // Opening first popover activates overlay synchronously
  const releaseA = api.acquireOverlay();
  assert.deepEqual(sent, [{ type: "set_overlay", active: true }]);

  // Synchronous handoff (e.g. StrictMode remount or 1-click popover switch)
  // must not flicker set_overlay(false) -> set_overlay(true)
  releaseA();
  releaseA(); // idempotent double-release
  const releaseB = api.acquireOverlay();
  await Promise.resolve();
  assert.deepEqual(sent, [{ type: "set_overlay", active: true }]);

  // Releasing final explicit lease deactivates on microtask
  releaseB();
  assert.equal(sent.length, 1);
  await Promise.resolve();
  assert.deepEqual(sent, [
    { type: "set_overlay", active: true },
    { type: "set_overlay", active: false },
  ]);

  // Native [popover] toggle events also drive set_overlay without explicit leases
  nativePopoverOpen = true;
  notifyOverlayChange();
  assert.deepEqual(sent[2], { type: "set_overlay", active: true });

  nativePopoverOpen = false;
  notifyOverlayChange();
  await Promise.resolve();
  assert.deepEqual(sent[3], { type: "set_overlay", active: false });
});

test("execStream streams stdout/stderr lines, supports stop(), and handles AbortSignal", async () => {
  const sent = [];
  const { api, receive } = createWebbarBridge((json) => {
    sent.push(JSON.parse(json));
  });

  const stdout = [];
  const stderr = [];
  let exited = null;

  const streamP = api.execStream(["stasis", "watch"], {
    onStdout: (line) => stdout.push(line),
    onStderr: (line) => stderr.push(line),
    onExit: (res) => {
      exited = res;
    },
  });

  assert.equal(sent[0].type, "process_stream_spawn");
  assert.deepEqual(sent[0].argv, ["stasis", "watch"]);

  receive({ type: "process_stream_started", id: sent[0].id, ok: true, error: null });
  const handle = await streamP;

  receive({
    type: "process_stream_output",
    id: sent[0].id,
    stream: "stdout",
    line: '{"state":"waiting"}',
  });
  receive({
    type: "process_stream_output",
    id: sent[0].id,
    stream: "stderr",
    line: "warn",
  });
  assert.deepEqual(stdout, ['{"state":"waiting"}']);
  assert.deepEqual(stderr, ["warn"]);

  handle.stop();
  assert.deepEqual(sent[1], { type: "process_kill", id: sent[0].id, signal: 15 });
  // Idempotent stop
  handle.stop();
  assert.equal(sent.length, 2);
  assert.equal(exited, null);

  // Natural exit
  const ac = new AbortController();
  let exit2 = null;
  const stream2P = api.execStream(["echo", "ok"], {
    signal: ac.signal,
    onExit: (res) => {
      exit2 = res;
    },
  });
  receive({ type: "process_stream_started", id: sent[2].id, ok: true, error: null });
  await stream2P;
  receive({ type: "process_stream_ended", id: sent[2].id, code: 0, signal: null });
  assert.deepEqual(exit2, { code: 0, signal: null });

  // Abort before start resolves
  const ac2 = new AbortController();
  const stream3P = api.execStream(["sleep", "10"], { signal: ac2.signal });
  ac2.abort();
  await assert.rejects(stream3P, { name: "AbortError" });
  assert.deepEqual(sent[4], { type: "process_kill", id: sent[3].id, signal: 15 });
});

test("reconcile matches id'd array items by id, keeping untouched siblings", () => {
  const store = createStore({ items: [] });
  const a = { id: 1, name: "a", nested: { v: 1 } };
  const b = { id: 2, name: "b", nested: { v: 2 } };
  store._update({ items: [a, b] });
  const [prevA, prevB] = store.get().items;

  // Reorder + insert: a and b keep identity even though positions changed.
  store._update({
    items: [
      { id: 3, name: "c", nested: { v: 3 } },
      { id: 2, name: "b", nested: { v: 2 } },
      { id: 1, name: "a", nested: { v: 1 } },
    ],
  });
  const items = store.get().items;
  assert.equal(Object.is(items[1], prevB), true);
  assert.equal(Object.is(items[2], prevA), true);

  // Same ids, same order, same content: array identity preserved.
  const before = store.get().items;
  store._update({ items: before.map((x) => ({ ...x, nested: { ...x.nested } })) });
  assert.equal(Object.is(store.get().items, before), true);

  // Duplicate or missing ids fall back to positional matching.
  store._update({ items: [{ id: 9 }, { id: 9 }] });
  assert.equal(store.get().items.length, 2);
  store._update({ items: [{ name: "no id" }] });
  assert.deepEqual(store.get().items, [{ name: "no id" }]);
});

test("sway tree updates keep identity of unchanged subtrees", () => {
  const { api, receive } = createWebbarBridge(() => {});
  const tree = (ws2Windows) =>
    swayNode(1, "root", {
      nodes: [
        swayNode(2, "output", {
          nodes: [
            swayNode(10, "workspace", {
              name: "1",
              nodes: [swayNode(11, "con", { name: "term", pid: 1 })],
            }),
            swayNode(20, "workspace", { name: "2", nodes: ws2Windows }),
          ],
        }),
      ],
    });
  receive({ type: "sway_state", workspaces: [], mode: "default", tree: tree([]) });
  const ws1 = api.sway.get().tree.nodes[0].nodes[0];

  // A window opens on workspace 2: workspace 1's subtree is untouched.
  receive({
    type: "sway_state",
    workspaces: [],
    mode: "default",
    tree: tree([swayNode(21, "con", { name: "new", pid: 2 })]),
  });
  const next = api.sway.get().tree.nodes[0].nodes;
  assert.equal(Object.is(next[0], ws1), true);
  assert.equal(next[1].nodes.length, 1);
});

test("sway.command resolves on success and rejects with SwayCommandError", async () => {
  const sent = [];
  const { api, receive } = createWebbarBridge((json) => sent.push(JSON.parse(json)));

  const ok = api.sway.command("workspace 1");
  assert.equal(sent[0].type, "sway_command");
  assert.equal(sent[0].command, "workspace 1");
  receive({ type: "rpc_reply", id: sent[0].id, ok: true });
  await ok;

  const bad = api.sway.command("nonsense");
  receive({
    type: "rpc_reply",
    id: sent[1].id,
    ok: false,
    error: "Unknown/invalid command 'nonsense'",
    data: { message: "Unknown/invalid command 'nonsense'", parse_error: true },
  });
  await assert.rejects(bad, (err) => {
    assert.equal(err instanceof SwayCommandError, true);
    assert.equal(err.name, "SwayCommandError");
    assert.equal(err.parseError, true);
    assert.equal(err.message, "Unknown/invalid command 'nonsense'");
    return true;
  });
});

test("reconcile matches Bluetooth paths and Tray bus+path items by stable identity", () => {
  const { api, receive } = createWebbarBridge(() => {});

  // Bluetooth devices (keyed by path)
  const devA = {
    path: "/org/bluez/hci0/dev_AA",
    name: "Headphones",
    address: "AA:AA:AA:AA:AA:AA",
    paired: true,
    connected: true,
    battery: 80,
  };
  const devB = {
    path: "/org/bluez/hci0/dev_BB",
    name: "Keyboard",
    address: "BB:BB:BB:BB:BB:BB",
    paired: true,
    connected: false,
    battery: null,
  };
  receive({
    type: "bluetooth_state",
    available: true,
    adapters: [{ path: "/org/bluez/hci0", name: "hci0", powered: true }],
    devices: [devA, devB],
  });
  const [prevA, prevB] = api.bluetooth.get().devices;

  // Insert new device at start and swap order: prevA and prevB keep identity
  receive({
    type: "bluetooth_state",
    available: true,
    adapters: [{ path: "/org/bluez/hci0", name: "hci0", powered: true }],
    devices: [
      {
        path: "/org/bluez/hci0/dev_CC",
        name: "Mouse",
        address: "CC:CC:CC:CC:CC:CC",
        paired: true,
        connected: true,
        battery: 95,
      },
      { ...devB },
      { ...devA },
    ],
  });
  const devices = api.bluetooth.get().devices;
  assert.equal(Object.is(devices[1], prevB), true);
  assert.equal(Object.is(devices[2], prevA), true);

  // Tray items (keyed by bus + path so two buses sharing /StatusNotifierItem stay distinct)
  const tray1 = {
    bus: ":1.10",
    path: "/StatusNotifierItem",
    title: "App1",
    icon_name: "app1",
    icon_url: "",
    menu_path: "",
  };
  const tray2 = {
    bus: ":1.20",
    path: "/StatusNotifierItem",
    title: "App2",
    icon_name: "app2",
    icon_url: "",
    menu_path: "",
  };
  receive({ type: "tray", items: [tray1, tray2] });
  const [prevTray1, prevTray2] = api.tray.get();

  // Reorder and insert at front
  receive({
    type: "tray",
    items: [{ ...tray2 }, { ...tray1 }],
  });
  const trays = api.tray.get();
  assert.equal(Object.is(trays[0], prevTray2), true);
  assert.equal(Object.is(trays[1], prevTray1), true);
});

test("dbus.watch reports errors via onClose", async () => {
  const sent = [];
  const { api, receive } = createWebbarBridge((json) => {
    sent.push(JSON.parse(json));
  });

  const closedEvents = [];
  const watchP = api.dbus.watch(
    {
      bus: "system",
      sender: "org.freedesktop.login1",
      path: "/org/freedesktop/login1",
      interface: "org.freedesktop.DBus.Properties",
      member: "PropertiesChanged",
      onClose: (err) => closedEvents.push(err),
    },
    () => {},
  );

  assert.equal(sent[0].type, "dbus_match");
  assert.equal(sent[0].path, "/org/freedesktop/login1");

  receive({ type: "dbus_match_started", id: sent[0].id, ok: true, error: null });
  const watcher = await watchP;

  // Match ending with error calls onClose(Error)
  receive({ type: "dbus_match_ended", id: sent[0].id, error: "stream failed" });
  assert.equal(closedEvents.length, 1);
  assert.ok(closedEvents[0] instanceof Error);
  assert.match(closedEvents[0].message, /stream failed/);

  // Explicit stop() must NOT trigger onClose, nor send duplicate unwatch
  const closedEvents2 = [];
  const watch2P = api.dbus.watch(
    { interface: "org.example.Test", onClose: (err) => closedEvents2.push(err) },
    () => {},
  );
  receive({ type: "dbus_match_started", id: sent[1].id, ok: true, error: null });
  const watcher2 = await watch2P;
  watcher2.stop();
  watcher2.stop();
  assert.equal(sent.filter((m) => m.type === "dbus_unwatch").length, 1);
  receive({ type: "dbus_match_ended", id: sent[1].id, error: null });
  assert.equal(closedEvents2.length, 0);

  // Server-initiated clean end (error: null) calls onClose(undefined)
  const closedEvents3 = [];
  const watch3P = api.dbus.watch(
    { interface: "org.example.Test", onClose: (err) => closedEvents3.push(err) },
    () => {},
  );
  receive({ type: "dbus_match_started", id: sent[3].id, ok: true, error: null });
  await watch3P;
  receive({ type: "dbus_match_ended", id: sent[3].id, error: null });
  assert.deepEqual(closedEvents3, [undefined]);
  void watcher;
});

test("onDisconnect cleans up pending RPCs, watches, streams, and onConnect re-syncs overlay", async () => {
  const sent = [];
  const { api, receive, onConnect, onDisconnect } = createWebbarBridge((json) => {
    sent.push(JSON.parse(json));
  });

  // Acquire an overlay lease before disconnect
  const releaseOverlay = api.acquireOverlay();
  assert.deepEqual(sent[0], { type: "set_overlay", active: true });

  // In-flight RPC
  const rpcP = api.exec(["echo", "hi"]);

  // Unstarted dbus.watch
  const unstartedWatchP = api.dbus.watch(
    { interface: "org.example.Unstarted" },
    () => {},
  );

  // Started dbus.watch
  const watchCloseErrs = [];
  const startedWatchP = api.dbus.watch(
    {
      interface: "org.example.Started",
      onClose: (err) => watchCloseErrs.push(err),
    },
    () => {},
  );
  const startedWatchId = sent[3].id;
  receive({ type: "dbus_match_started", id: startedWatchId, ok: true, error: null });
  await startedWatchP;

  // Unstarted execStream
  const unstartedStreamP = api.execStream(["sleep", "5"]);

  // Started execStream
  const exits = [];
  const startedStreamP = api.execStream(["stasis", "watch"], {
    onExit: (res) => exits.push(res),
  });
  const startedStreamId = sent[5].id;
  receive({
    type: "process_stream_started",
    id: startedStreamId,
    ok: true,
    error: null,
  });
  await startedStreamP;

  // Simulate socket close
  onDisconnect();

  await assert.rejects(rpcP, /disconnected/i);
  await assert.rejects(unstartedWatchP, /disconnected/i);
  await assert.rejects(unstartedStreamP, /disconnected/i);
  assert.equal(watchCloseErrs.length, 1);
  assert.ok(watchCloseErrs[0] instanceof Error);
  assert.deepEqual(exits, [{ code: null, signal: null }]);

  // Simulate reconnect: overlay lease is still held so set_overlay(true) re-sends
  sent.length = 0;
  onConnect();
  assert.deepEqual(sent, [{ type: "set_overlay", active: true }]);

  releaseOverlay();
  await Promise.resolve();
  assert.deepEqual(sent[1], { type: "set_overlay", active: false });
});

test("onConnect sends overlay=false if the lease was released while disconnected", async () => {
  const sent = [];
  const { api, onConnect, onDisconnect } = createWebbarBridge((json) => {
    sent.push(JSON.parse(json));
  });

  const releaseOverlay = api.acquireOverlay();
  assert.deepEqual(sent, [{ type: "set_overlay", active: true }]);

  onDisconnect();
  releaseOverlay();
  await Promise.resolve();

  // The server still believes overlay is active; reconnect must correct it.
  sent.length = 0;
  onConnect();
  assert.deepEqual(sent, [{ type: "set_overlay", active: false }]);
});

test("tray.menu exposes a reactive store that watches/unwatches on subscribe", () => {
  const sent = [];
  const { api, receive, onConnect } = createWebbarBridge((json) => {
    sent.push(JSON.parse(json));
  });

  const bus = ":1.7";
  const menuPath = "/org/ayatana/NotificationItem/nm_applet/Menu";
  const storeA = api.tray.menu(bus, menuPath);
  const storeB = api.tray.menu(bus, menuPath);
  assert.equal(storeA, storeB);
  assert.deepEqual(storeA.get(), []);
  assert.equal(sent.length, 0);

  let notifications = 0;
  const unsub1 = storeA.subscribe(() => {
    notifications += 1;
  });
  assert.deepEqual(sent, [
    { type: "tray_watch_menu", bus, menu_path: menuPath },
  ]);

  // Second subscriber reuses the existing watch without sending a duplicate.
  const observed = [];
  const unsub2 = storeB.onChange((items) => observed.push(items));
  assert.equal(sent.length, 1);
  assert.deepEqual(observed, [[]]);

  const initialItems = [
    {
      id: 2513,
      label: "Connection Information",
      enabled: true,
      visible: true,
      item_type: "standard",
      toggle_type: "",
      toggle_state: -1,
      children: [],
    },
  ];
  receive({ type: "tray_menu", bus, menu_path: menuPath, items: initialItems });
  assert.equal(notifications, 1);
  assert.deepEqual(storeA.get(), initialItems);
  assert.deepEqual(observed[observed.length - 1], initialItems);

  // Subsequent LayoutUpdated push updates IDs reactively.
  const refreshedItems = [{ ...initialItems[0], id: 2562 }];
  receive({
    type: "tray_menu",
    bus,
    menu_path: menuPath,
    items: refreshedItems,
  });
  assert.equal(storeA.get()[0].id, 2562);

  // Reconnecting re-sends tray_watch_menu for active stores.
  onConnect();
  assert.deepEqual(sent[sent.length - 1], {
    type: "tray_watch_menu",
    bus,
    menu_path: menuPath,
  });

  // Unsubscribing one of two does not unwatch yet.
  unsub1();
  assert.equal(sent.filter((m) => m.type === "tray_unwatch_menu").length, 0);

  // Unsubscribing the last subscriber unwatches and clears stale items.
  unsub2();
  unsub2(); // idempotent
  assert.deepEqual(
    sent.filter((m) => m.type === "tray_unwatch_menu"),
    [{ type: "tray_unwatch_menu", bus, menu_path: menuPath }],
  );
  assert.deepEqual(storeA.get(), []);
});

test("applyThemeToDocument sets and updates --desktop-* and --gtk-* CSS variables", () => {
  const props = new Map();
  const mockRoot = {
    dataset: {},
    style: {
      colorScheme: "",
      setProperty(k, v) {
        props.set(k, v);
      },
      removeProperty(k) {
        props.delete(k);
      },
    },
  };

  applyThemeToDocument(
    {
      dark: true,
      high_contrast: false,
      theme_name: "Adwaita",
      icon_theme_name: "Adwaita",
      font_family: '"Inter", sans-serif',
      font_size: "11pt",
      font_weight: 500,
      monospace_font_family: '"JetBrains Mono", sans-serif',
      enable_animations: true,
      accent_color: "#89b4fa",
      desktop_colors: { "window-bg": "#1e1e2e", "accent-bg": "#89b4fa" },
      gtk_colors: { "theme-bg-color": "#1e1e2e" },
    },
    mockRoot,
  );

  assert.equal(mockRoot.dataset.desktopTheme, "dark");
  assert.equal(mockRoot.style.colorScheme, "dark");
  assert.equal(props.get("--desktop-font-family"), '"Inter", sans-serif');
  assert.equal(props.get("--desktop-font-size"), "11pt");
  assert.equal(props.get("--desktop-font-weight"), "500");
  assert.equal(
    props.get("--desktop-monospace-font-family"),
    '"JetBrains Mono", sans-serif',
  );
  assert.equal(props.get("--desktop-enable-animations"), "1");
  assert.equal(props.get("--desktop-window-bg"), "#1e1e2e");
  assert.equal(props.get("--desktop-accent-bg"), "#89b4fa");
  assert.equal(props.get("--gtk-theme-bg-color"), "#1e1e2e");

  // Switching to light mode updates properties and removes monospace font when null
  applyThemeToDocument(
    {
      dark: false,
      high_contrast: false,
      theme_name: "Adwaita",
      icon_theme_name: "Adwaita",
      font_family: '"Cantarell", sans-serif',
      font_size: "11pt",
      font_weight: 400,
      monospace_font_family: null,
      enable_animations: false,
      accent_color: null,
      desktop_colors: { "window-bg": "#fafafa" },
      gtk_colors: { "theme-bg-color": "#ffffff" },
    },
    mockRoot,
  );

  assert.equal(mockRoot.dataset.desktopTheme, "light");
  assert.equal(mockRoot.style.colorScheme, "light");
  assert.equal(props.get("--desktop-enable-animations"), "0");
  assert.equal(props.get("--desktop-window-bg"), "#fafafa");
  assert.equal(props.get("--gtk-theme-bg-color"), "#ffffff");
  assert.equal(props.has("--desktop-monospace-font-family"), false);
});

test("bridge theme events sync api.theme store and document.documentElement", () => {
  const props = new Map();
  const mockRoot = {
    dataset: {},
    style: {
      colorScheme: "",
      setProperty(k, v) {
        props.set(k, v);
      },
      removeProperty(k) {
        props.delete(k);
      },
    },
  };

  const prevDocument = globalThis.document;
  globalThis.document = { documentElement: mockRoot };
  try {
    const { api, receive } = createWebbarBridge(() => {});
    const seenDark = [];
    const unsub = api.theme.subscribe(() => {
      seenDark.push(api.theme.get().dark);
    });

    const base = {
      type: "theme",
      high_contrast: false,
      theme_name: "Adwaita",
      icon_theme_name: "Adwaita",
      font_family: '"Inter", sans-serif',
      font_size: "11pt",
      font_weight: 400,
      monospace_font_family: null,
      enable_animations: true,
      accent_color: null,
      desktop_colors: {},
      gtk_colors: {},
    };

    receive({ ...base, dark: false, desktop_colors: { "window-bg": "#fafafa" } });
    assert.equal(api.theme.get().dark, false);
    assert.equal(mockRoot.dataset.desktopTheme, "light");
    assert.equal(mockRoot.style.colorScheme, "light");
    assert.equal(props.get("--desktop-window-bg"), "#fafafa");

    receive({ ...base, dark: true, desktop_colors: { "window-bg": "#1e1e2e" } });
    assert.equal(api.theme.get().dark, true);
    assert.equal(mockRoot.dataset.desktopTheme, "dark");
    assert.equal(mockRoot.style.colorScheme, "dark");
    assert.equal(props.get("--desktop-window-bg"), "#1e1e2e");
    assert.deepEqual(seenDark, [false, true]);

    unsub();
  } finally {
    if (prevDocument === undefined) {
      delete globalThis.document;
    } else {
      globalThis.document = prevDocument;
    }
  }
});

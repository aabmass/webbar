import assert from "node:assert/strict";
import test from "node:test";
import { createStore, createWebbarBridge } from "../dist/preload.mjs";

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
        focused_title: "Alacritty",
        mode: "default",
      },
      assertState: (s) => {
        assert.equal(s.workspaces.length, 1);
        assert.equal(s.focused_title, "Alacritty");
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
      },
      assertState: (s) => {
        assert.equal(s.available, true);
        assert.equal(s.brightness, 65);
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
  receive({ type: "action_reply", id: sent[0].id, ok: true, error: null });
  await p1;

  const p2 = api.bluetooth.connect("/org/bluez/hci0/dev_1");
  receive({
    type: "action_reply",
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
    type: "tray_menu_reply",
    id: sent[2].id,
    ok: true,
    items: [{ id: 1, label: "Quit", enabled: true, visible: true, item_type: "standard", toggle_type: "", toggle_state: 0, children: [] }],
    error: null,
  });
  const menu = await pMenu;
  assert.equal(menu[0].label, "Quit");
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
    type: "dbus_call_reply",
    id: sent[0].id,
    ok: true,
    signature: "s",
    body: JSON.stringify(["pong"]),
    error: null,
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
    body: "[42]",
  });
  assert.equal(signals.length, 1);
  assert.deepEqual(signals[0].body, [42]);
  watcher.stop();
  assert.equal(sent[2].type, "dbus_unwatch");
  assert.equal(sent[2].id, sent[1].id);

  // exec with base64 stdout/stderr
  const execP = api.exec(["echo", "hi"]);
  receive({ type: "process_started", id: sent[3].id, ok: true, error: null });
  receive({
    type: "process_output",
    id: sent[3].id,
    stream: "stdout",
    data_b64: Buffer.from("hello ").toString("base64"),
  });
  receive({
    type: "process_output",
    id: sent[3].id,
    stream: "stdout",
    data_b64: Buffer.from("world\n").toString("base64"),
  });
  receive({
    type: "process_output",
    id: sent[3].id,
    stream: "stderr",
    data_b64: Buffer.from("warn\n").toString("base64"),
  });
  receive({ type: "process_exited", id: sent[3].id, code: 0, signal: null });
  const execRes = await execP;
  assert.deepEqual(execRes, {
    code: 0,
    signal: undefined,
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

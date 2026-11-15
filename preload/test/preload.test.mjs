import assert from "node:assert/strict";
import test from "node:test";
import { createStore, createWebbarBridge } from "../dist/preload.mjs";

test("createStore maintains referential stability for useSyncExternalStore", () => {
  const store = createStore({ count: 0 });
  const initial = store.get();
  assert.equal(store.get(), initial);

  let subCalls = 0;
  const unsub = store.subscribe(() => {
    subCalls += 1;
  });
  // subscribe() must NOT fire synchronously
  assert.equal(subCalls, 0);

  let onChangeValues = [];
  const unsubChange = store.onChange((v) => {
    onChangeValues.push(v.count);
  });
  // onChange() fires immediately with current value
  assert.deepEqual(onChangeValues, [0]);

  store._update({ count: 1 });
  assert.equal(subCalls, 1);
  assert.deepEqual(onChangeValues, [0, 1]);
  assert.equal(store.get().count, 1);

  unsub();
  unsubChange();
  store._update({ count: 2 });
  assert.equal(subCalls, 1);
  assert.deepEqual(onChangeValues, [0, 1]);
});

test("createWebbarBridge resolves and rejects Promise actions", async () => {
  const sent = [];
  const { api, receive } = createWebbarBridge((json) => {
    sent.push(JSON.parse(json));
  });

  const p1 = api.audio.setVolume(3, 0.65);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].type, "audio_set_volume");
  assert.equal(sent[0].device_id, 3);
  assert.equal(sent[0].volume, 0.65);

  receive({ type: "action_reply", id: sent[0].id, ok: true, error: null });
  await p1;

  const p2 = api.bluetooth.connect("/org/bluez/hci0/dev_1");
  assert.equal(sent.length, 2);
  receive({
    type: "action_reply",
    id: sent[1].id,
    ok: false,
    error: "Device not ready",
  });
  await assert.rejects(p2, /Device not ready/);
});

test("createWebbarBridge updates stores on host events", () => {
  const { api, receive } = createWebbarBridge(() => {});
  receive({
    type: "network_state",
    available: true,
    connectivity: "full",
    link: { kind: "wifi", name: "Home", ssid: "HomeNet", strength: 0.9 },
  });
  assert.deepEqual(api.network.get(), {
    available: true,
    connectivity: "full",
    link: { kind: "wifi", name: "Home", ssid: "HomeNet", strength: 0.9 },
  });
});

<script lang="ts">
  import { flip } from "svelte/animate";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { SvelteSet } from "svelte/reactivity";
  import type { BluetoothDevice } from "@webbar/types";
  import { fromExternalStore } from "../lib/webbarStore.svelte";
  import { motionDuration } from "../lib/transitions";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";

  const btStore = fromExternalStore(
    window.webbar.bluetooth.subscribe,
    window.webbar.bluetooth.get,
  );

  let error = $state<string | null>(null);
  let open = $state(false);
  let btnEl = $state<HTMLButtonElement | null>(null);
  const connectingPaths = new SvelteSet<string>();
  let syncing = $state(false);
  let syncStatus = $state<{
    id: number;
    kind: "ok" | "error";
    text: string;
  } | null>(null);

  let bt = $derived(btStore.current);
  let adapter = $derived(bt.adapters[0]);
  let pairedDevices = $derived(bt.devices.filter((d) => d.paired));
  let connectedCount = $derived(
    pairedDevices.filter((d) => d.connected).length,
  );

  function close() {
    open = false;
    syncStatus = null;
  }

  async function syncFromWindows() {
    if (syncing) return;
    syncing = true;
    syncStatus = null;
    error = null;
    try {
      const res = await window.webbar.exec(["bt-win-sync"]);
      if (res.code === 0) {
        const summary =
          res.stdout
            .trim()
            .split("\n")
            .filter(Boolean)
            .at(-1) ?? "Synced from Windows";
        syncStatus = { id: Date.now(), kind: "ok", text: summary };
      } else {
        const msg =
          res.stderr.trim() ||
          res.stdout.trim() ||
          `bt-win-sync exited with ${res.code ?? "error"}`;
        syncStatus = { id: Date.now(), kind: "error", text: msg };
      }
    } catch (err) {
      syncStatus = {
        id: Date.now(),
        kind: "error",
        text: err instanceof Error ? err.message : String(err),
      };
    } finally {
      syncing = false;
    }
  }

  async function toggleDevice(device: BluetoothDevice) {
    connectingPaths.add(device.path);
    error = null;
    try {
      if (device.connected) {
        await window.webbar.bluetooth.disconnect(device.path);
      } else {
        await window.webbar.bluetooth.connect(device.path);
      }
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      connectingPaths.delete(device.path);
    }
  }
</script>

{#snippet batteryGlyph(percent: number)}
  {@const level = percent > 50 ? "high" : percent > 20 ? "mid" : "low"}
  <span class="battery-glyph is-{level}" aria-hidden="true">
    <span style="width: {Math.max(8, Math.min(100, percent))}%"></span>
  </span>
{/snippet}

<button
  bind:this={btnEl}
  type="button"
  class="bar-btn icon-btn"
  class:is-active={connectedCount > 0}
  class:is-dimmed={Boolean(error) || !bt.available}
  class:is-open={open}
  title={error
    ? `Bluetooth error: ${error}`
    : !bt.available
      ? "Bluetooth unavailable"
      : connectedCount > 0
        ? `Bluetooth: ${connectedCount} connected`
        : "Bluetooth"}
  onclick={() => {
    open = !open;
  }}
>
  <Icon name="bluetooth" />
</button>

<Popover {open} onClose={close} anchorEl={btnEl} align="end">
  <div class="qs-header">
    <div class="qs-heading">
      <span class="qs-title">
        <Icon name="bluetooth" /> Bluetooth
      </span>
      <span class="qs-subtitle">
        {!bt.available
          ? "Unavailable"
          : adapter && !adapter.powered
            ? "Off"
            : connectedCount > 0
              ? `${connectedCount} connected`
              : "On"}
      </span>
    </div>
    {#if adapter}
      <button
        type="button"
        role="switch"
        aria-checked={adapter.powered}
        aria-label="Bluetooth power"
        class="qs-switch"
        class:is-on={adapter.powered}
        onclick={() => {
          if (!adapter) return;
          error = null;
          window.webbar.bluetooth
            .setPowered(adapter.path, !adapter.powered)
            .catch((err: unknown) => {
              error = err instanceof Error ? err.message : String(err);
            });
        }}
      ></button>
    {/if}
  </div>

  <div class="bt-popover-list">
    {#if pairedDevices.length === 0}
      <span class="drawer-empty">No paired devices</span>
    {:else}
      {#each pairedDevices as device (device.path)}
        {@const isConnecting = connectingPaths.has(device.path)}
        {@const stateClass = isConnecting
          ? "connecting"
          : device.connected
            ? "connected"
            : "paired"}
        <button
          type="button"
          class="bar-btn bt-device {stateClass}"
          title={`${device.name} (${device.address})`}
          onclick={() => void toggleDevice(device)}
          animate:flip={{ duration: motionDuration(180), easing: cubicOut }}
        >
          <span class="bt-dot" aria-hidden="true"></span>
          <span class="bt-name">{device.name}</span>
          {#if device.battery != null}
            <span class="bt-battery">
              {device.battery}%
              {@render batteryGlyph(device.battery)}
            </span>
          {/if}
        </button>
      {/each}
    {/if}
  </div>

  <div class="bt-popover-footer">
    <button
      type="button"
      class="bar-btn bt-sync-btn"
      disabled={syncing}
      title="Sync Bluetooth pairing keys from Windows (bt-win-sync)"
      onclick={() => void syncFromWindows()}
    >
      {syncing ? "Syncing from Windows…" : "Sync from Windows"}
    </button>
    {#if syncStatus}
      {#key syncStatus.id}
        <div
          class="bt-sync-status is-{syncStatus.kind}"
          title={syncStatus.text}
          transition:slide={{ duration: motionDuration(140), easing: cubicOut }}
          onanimationend={(e) => {
            if (e.animationName === "bt-sync-fade") {
              syncStatus = null;
            }
          }}
        >
          {syncStatus.text}
        </div>
      {/key}
    {/if}
  </div>
</Popover>

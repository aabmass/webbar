<script lang="ts">
  import "../styles/tray.css";
  import { flip } from "svelte/animate";
  import { scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { fromExternalStore } from "../lib/webbarStore.svelte";
  import { motionDuration } from "../lib/transitions";
  import TrayButton from "./TrayButton.svelte";

  const trayStore = fromExternalStore(
    window.webbar.tray.subscribe,
    window.webbar.tray.get,
  );

  let items = $derived(trayStore.current);
</script>

{#if items.length > 0}
  <div
    class="tray"
    role="group"
    aria-label="System tray"
    transition:scale={{ duration: motionDuration(120), start: 0.92, easing: cubicOut }}
  >
    {#each items as item (`${item.bus}${item.path}`)}
      <div
        class="tray-slot"
        animate:flip={{ duration: motionDuration(180), easing: cubicOut }}
        transition:scale={{ duration: motionDuration(120), start: 0.85, easing: cubicOut }}
      >
        <TrayButton {item} />
      </div>
    {/each}
  </div>
{/if}

<style>
  .tray-slot {
    display: inline-flex;
  }
</style>

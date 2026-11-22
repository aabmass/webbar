<script lang="ts">
  import {
    activePopups,
    isFocusedOutput,
  } from "@webbar/sway-full-bar-shared/notifications";
  import { fromExternalStore } from "../lib/webbarStore.svelte";
  import NotificationCard from "./NotificationCard.svelte";

  let { hidden }: { hidden: boolean } = $props();

  const notifStore = fromExternalStore(
    window.webbar.notifications.subscribe,
    window.webbar.notifications.get,
  );
  const swayStore = fromExternalStore(
    window.webbar.sway.subscribe,
    window.webbar.sway.get,
  );

  let nowMs = $state(Date.now());
  let popups = $derived(activePopups(notifStore.current.items));
  let show = $derived(
    !hidden &&
      popups.length > 0 &&
      isFocusedOutput(swayStore.current, window.webbar.output),
  );

  $effect(() => {
    if (!show) return;
    nowMs = Date.now();
    const timer = window.setInterval(() => {
      nowMs = Date.now();
    }, 30_000);
    return () => window.clearInterval(timer);
  });
</script>

{#if show}
  <div
    class="notif-toast-stack"
    role="region"
    aria-label="Notification popups"
  >
    {#each popups as item (item.id)}
      <NotificationCard {item} {nowMs} isToast />
    {/each}
  </div>
{/if}

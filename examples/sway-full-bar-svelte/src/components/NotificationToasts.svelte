<script lang="ts">
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import { fade } from "svelte/transition";
  import {
    activePopups,
    prefersReducedMotion,
    TOAST_EXIT_MS,
  } from "@webbar/sway-full-bar-shared/notifications";
  import { fromExternalStore } from "../lib/webbarStore.svelte";
  import NotificationCard from "./NotificationCard.svelte";

  let { hidden }: { hidden: boolean } = $props();

  const notifStore = fromExternalStore(
    window.webbar.notifications.subscribe,
    window.webbar.notifications.get,
  );
  const focusedStore = fromExternalStore(
    window.webbar.focused.subscribe,
    window.webbar.focused.get,
  );

  let nowMs = $state(Date.now());
  let canShow = $derived(!hidden && focusedStore.current);
  let popups = $derived(activePopups(notifStore.current.items));
  let hasPopups = $derived(canShow && popups.length > 0);

  $effect(() => {
    if (!hasPopups) return;
    nowMs = Date.now();
    const timer = window.setInterval(() => {
      nowMs = Date.now();
    }, 30_000);
    return () => window.clearInterval(timer);
  });
</script>

{#if canShow}
  <div
    class="notif-toast-stack"
    role="region"
    aria-label="Notification popups"
  >
    {#each popups as item (item.id)}
      <div
        class="notif-toast-item"
        animate:flip={{
          duration: prefersReducedMotion() ? 0 : 300,
          easing: cubicOut,
        }}
        out:fade={{
          duration: prefersReducedMotion() ? 0 : TOAST_EXIT_MS,
          easing: cubicOut,
        }}
        onoutrostart={(e) => {
          e.currentTarget.classList.add("is-exiting");
          e.currentTarget
            .querySelector("[data-webbar-interactive]")
            ?.removeAttribute("data-webbar-interactive");
        }}
      >
        <NotificationCard {item} {nowMs} isToast />
      </div>
    {/each}
  </div>
{/if}

<style>
  .notif-toast-item:global(.is-exiting),
  .notif-toast-item:global(.is-exiting) :global(.notif-toast) {
    pointer-events: none;
  }
</style>

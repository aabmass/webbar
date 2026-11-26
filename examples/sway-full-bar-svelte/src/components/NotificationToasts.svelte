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
  let stackEl = $state<HTMLDivElement | null>(null);
  let canShow = $derived(!hidden && focusedStore.current);
  let popups = $derived(activePopups(notifStore.current.items));
  let hasPopups = $derived(canShow && popups.length > 0);

  $effect(() => {
    if (!stackEl) return;
    try {
      if (!stackEl.matches(":popover-open")) {
        stackEl.showPopover();
      }
    } catch {
      // Fallback for environments without Popover API
    }
  });

  $effect(() => {
    if (!hasPopups) return;
    nowMs = Date.now();
    const timer = window.setInterval(() => {
      nowMs = Date.now();
    }, 30_000);
    return () => window.clearInterval(timer);
  });
</script>

{#if hasPopups}
  <div
    bind:this={stackEl}
    class="notif-toast-stack"
    popover="manual"
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
        }}
      >
        <NotificationCard {item} {nowMs} isToast />
      </div>
    {/each}
  </div>
{/if}

<style>
/* Floating toast stack */
.notif-toast-stack {
  position: fixed;
  inset: auto;
  top: calc(var(--webbar-height, 86px) + 10px);
  right: 12px;
  width: 340px;
  margin: 0;
  padding: 0;
  border: none;
  background: transparent;
  color: inherit;
  overflow: visible;
  display: flex;
  flex-direction: column;
  gap: 8px;
  pointer-events: none;
  z-index: 50;
}

.notif-toast-stack[popover]:not(:popover-open) {
  display: none;
}

.notif-toast-item {
  pointer-events: none;
}

.notif-toast-item:global(.is-exiting),
.notif-toast-item:global(.is-exiting) :global(.notif-toast) {
  pointer-events: none;
}
</style>

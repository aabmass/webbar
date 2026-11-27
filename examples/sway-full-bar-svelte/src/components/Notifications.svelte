<script lang="ts">
  import {
    notificationButtonTitle,
    notificationCenterSubtitle,
  } from "@webbar/sway-full-bar-shared/notifications";
  import { fromExternalStore } from "../lib/webbarStore.svelte";
  import Icon from "./Icon.svelte";
  import NotificationCard from "./NotificationCard.svelte";
  import Popover from "./Popover.svelte";

  let {
    open,
    onOpenChange,
  }: {
    open: boolean;
    onOpenChange: (next: boolean) => void;
  } = $props();

  const notifStore = fromExternalStore(
    window.webbar.notifications.subscribe,
    window.webbar.notifications.get,
  );

  let nowMs = $state(Date.now());
  let btnEl = $state<HTMLButtonElement | null>(null);

  let notifState = $derived(notifStore.current);
  let items = $derived(notifState.items);
  let unread = $derived(notifState.unread_count);

  function close() {
    onOpenChange(false);
  }

  $effect(() => {
    return window.webbar.onHostAction((action) => {
      if (action === "notifications_toggle_center") {
        onOpenChange(!open);
      } else if (action === "notifications_close_center") {
        onOpenChange(false);
      }
    });
  });

  $effect(() => {
    if (!open) return;
    nowMs = Date.now();
    const timer = window.setInterval(() => {
      nowMs = Date.now();
    }, 30_000);
    return () => window.clearInterval(timer);
  });

  $effect(() => {
    if (open && unread > 0) {
      window.webbar.notifications.markAllRead();
    }
  });
</script>

<button
  bind:this={btnEl}
  type="button"
  class="bar-btn icon-btn notif-btn"
  class:is-dnd={notifState.do_not_disturb}
  class:is-active={unread > 0 && !notifState.do_not_disturb}
  class:is-dimmed={!notifState.available}
  class:is-open={open}
  aria-label="Notifications"
  title={notificationButtonTitle(notifState)}
  onclick={() => onOpenChange(!open)}
  oncontextmenu={(e) => {
    e.preventDefault();
    window.webbar.notifications.setDoNotDisturb(!notifState.do_not_disturb);
  }}
>
  <Icon name="bell" dnd={notifState.do_not_disturb} />
  {#if unread > 0}
    <span class="notif-badge" aria-hidden="true">
      {unread > 99 ? "99+" : unread}
    </span>
  {/if}
</button>

<Popover {open} onClose={close} anchorEl={btnEl} align="center">
  <div class="notif-popover">
    <div class="notif-header">
      <div class="qs-heading">
        <span class="qs-title">
          <Icon name="bell" dnd={notifState.do_not_disturb} /> Notifications
        </span>
        <span class="qs-subtitle">
          {notificationCenterSubtitle(notifState.do_not_disturb, items.length)}
        </span>
      </div>
      <div class="notif-header-controls">
        <span
          class="notif-dnd-control"
          class:is-on={notifState.do_not_disturb}
          title="Do Not Disturb"
        >
          <Icon name="bell" dnd={true} />
          <button
            type="button"
            role="switch"
            aria-checked={notifState.do_not_disturb}
            aria-label="Do Not Disturb"
            title="Do Not Disturb"
            class="qs-switch"
            class:is-on={notifState.do_not_disturb}
            onclick={() =>
              window.webbar.notifications.setDoNotDisturb(
                !notifState.do_not_disturb,
              )}
          ></button>
        </span>
        <button
          type="button"
          class="notif-clear-btn"
          disabled={items.length === 0}
          onclick={() => window.webbar.notifications.clearAll()}
        >
          Clear
        </button>
      </div>
    </div>

    <div class="notif-list">
      {#if items.length === 0}
        <span class="drawer-empty">No notifications</span>
      {:else}
        {#each items as item (item.id)}
          <NotificationCard {item} {nowMs} />
        {/each}
      {/if}
    </div>
  </div>
</Popover>

<style>
/* Notifications: bell pill, center popover, and floating toast stack */
.notif-btn {
  position: relative;
}

.notif-badge {
  position: absolute;
  top: 2px;
  right: 2px;
  min-width: 14px;
  height: 14px;
  padding: 0 3.5px;
  border-radius: 999px;
  background: var(--accent-color);
  color: var(--accent-fg);
  font-size: 9.5px;
  font-weight: 700;
  line-height: 14px;
  text-align: center;
  font-variant-numeric: tabular-nums;
  box-shadow: 0 0 0 1.5px var(--bar-bg);
}

.notif-btn.is-dnd {
  color: var(--fg-muted);
}

.notif-btn.is-dnd .notif-badge {
  background: var(--fg-faint);
  color: var(--bar-bg);
}

.notif-popover {
  display: flex;
  flex-direction: column;
  width: 340px;
  gap: 8px;
}

.notif-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 0 2px 8px;
  border-bottom: 1px solid var(--panel-border);
}

.notif-header-controls {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.notif-dnd-control {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--fg-muted);
}

.notif-dnd-control.is-on {
  color: var(--fg-color);
}

.notif-dnd-control :global(svg) {
  width: 14px;
  height: 14px;
  flex: none;
}

.notif-clear-btn {
  height: 26px;
  padding: 0 8px;
  border-radius: var(--radius-sm);
  background: var(--surface-color);
  color: var(--fg-muted);
  font-size: 11.5px;
  font-weight: 600;
}

.notif-clear-btn:hover:not(:disabled) {
  background: var(--surface-hover);
  color: var(--fg-color);
}

.notif-clear-btn:disabled {
  opacity: 0.45;
  cursor: default;
}

.notif-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: min(480px, calc(100vh - 180px));
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-width: thin;
  scrollbar-color: color-mix(in srgb, var(--fg-color) 16%, transparent) transparent;
}
</style>

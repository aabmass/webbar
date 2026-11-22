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

<Popover {open} onClose={close} anchorEl={btnEl} align="end">
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

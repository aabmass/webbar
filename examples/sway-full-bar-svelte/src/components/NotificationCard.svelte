<script lang="ts">
  import type { NotificationItem } from "@webbar/types";
  import { formatRelativeTime } from "@webbar/sway-full-bar-shared/notifications";

  function updateSheen(
    el: HTMLElement,
    clientX: number,
    clientY: number,
    isEnter: boolean,
  ) {
    const rect = el.getBoundingClientRect();
    const rawX = clientX - rect.left;
    const maxX = rect.width * 0.25;
    const clampedY = Math.round(
      Math.max(0, Math.min(clientY - rect.top, rect.height)),
    );

    if (isEnter) {
      el.style.setProperty(
        "--toast-x",
        `${Math.round(Math.max(0, Math.min(rawX, maxX)))}px`,
      );
      el.style.setProperty("--toast-y", `${clampedY}px`);
      el.dataset.sheenLocked = rawX >= maxX ? "1" : "";
      return;
    }

    if (rawX <= maxX) {
      el.dataset.sheenLocked = "";
      el.style.setProperty("--toast-x", `${Math.round(Math.max(0, rawX))}px`);
      el.style.setProperty("--toast-y", `${clampedY}px`);
    } else if (el.dataset.sheenLocked !== "1") {
      el.dataset.sheenLocked = "1";
      el.style.setProperty("--toast-x", `${Math.round(maxX)}px`);
    }
  }

  let {
    item,
    nowMs,
    isToast = false,
  }: {
    item: NotificationItem;
    nowMs: number;
    isToast?: boolean;
  } = $props();

  let canClickBody = $derived(isToast || item.has_default_action);
</script>

<article
  class="notif-card"
  class:notif-toast={isToast}
  class:is-critical={item.urgency === "critical"}
  data-webbar-interactive={isToast ? "" : undefined}
  onpointerenter={isToast
    ? (e) => {
        window.webbar.notifications.pauseTimeout(item.id);
        updateSheen(e.currentTarget, e.clientX, e.clientY, true);
      }
    : undefined}
  onpointermove={isToast
    ? (e) => {
        updateSheen(e.currentTarget, e.clientX, e.clientY, false);
      }
    : undefined}
  onpointerleave={isToast
    ? (e) => {
        window.webbar.notifications.resumeTimeout(item.id);
        delete e.currentTarget.dataset.sheenLocked;
      }
    : undefined}
>
  <div class="notif-card-head">
    <div class="notif-card-app">
      {#if item.app_icon_url}
        <img
          class="notif-app-icon"
          src={item.app_icon_url}
          alt=""
          aria-hidden="true"
        />
      {/if}
      <span class="notif-card-app-name">
        {item.app_name || "Notification"}
      </span>
    </div>
    <div class="notif-card-meta">
      <span class="notif-time">
        {formatRelativeTime(item.timestamp_ms, nowMs)}
      </span>
      <button
        type="button"
        class="notif-close-btn"
        title="Dismiss notification"
        aria-label="Dismiss notification"
        onclick={() => window.webbar.notifications.dismiss(item.id)}
      >
        ×
      </button>
    </div>
  </div>

  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="notif-card-body-row"
    class:is-clickable={canClickBody}
    onclick={canClickBody
      ? () => window.webbar.notifications.activate(item.id)
      : undefined}
  >
    {#if item.image_url}
      <img
        class="notif-image"
        src={item.image_url}
        alt=""
        aria-hidden="true"
      />
    {/if}
    <div class="notif-text">
      {#if item.summary}
        <div class="notif-summary">{item.summary}</div>
      {/if}
      {#if item.body}
        <div class="notif-body">{item.body}</div>
      {/if}
    </div>
  </div>

  {#if item.progress !== null}
    <div
      class="notif-progress-track"
      role="progressbar"
      aria-valuenow={item.progress}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <div class="notif-progress-fill" style="width: {item.progress}%"></div>
    </div>
  {/if}

  {#if item.actions.length > 0}
    <div class="notif-actions">
      {#each item.actions as action (action.key)}
        <button
          type="button"
          class="notif-action-btn"
          onclick={() =>
            window.webbar.notifications.invokeAction(item.id, action.key)}
        >
          {action.label}
        </button>
      {/each}
    </div>
  {/if}
</article>

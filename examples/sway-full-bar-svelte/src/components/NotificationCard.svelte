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
</script>

<article
  class="notif-card"
  class:notif-toast={isToast}
  class:is-critical={item.urgency === "critical"}
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
    class="notif-card-body-row is-clickable"
    onclick={() => window.webbar.notifications.activate(item.id)}
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

<style>
.notif-card {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  background: var(--surface-color);
  border: 1px solid var(--hairline);
}

.notif-card.is-critical {
  border-color: color-mix(in srgb, var(--urgent-bg) 55%, transparent);
  background: color-mix(in srgb, var(--surface-color), var(--urgent-bg) 10%);
}

.notif-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.notif-card-app {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  font-size: 11px;
  font-weight: 600;
  color: var(--fg-muted);
}

.notif-card-app-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.notif-app-icon {
  width: 14px;
  height: 14px;
  border-radius: 3px;
  object-fit: contain;
  flex: none;
}

.notif-card-meta {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: none;
}

.notif-time {
  font-size: 11px;
  font-weight: 500;
  color: var(--fg-faint);
  font-variant-numeric: tabular-nums;
}

.notif-close-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--fg-muted);
  font-size: 14px;
  line-height: 1;
}

.notif-close-btn:hover {
  background: var(--surface-hover);
  color: var(--fg-color);
}

.notif-card-body-row {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  min-width: 0;
}

.notif-card-body-row.is-clickable {
  cursor: pointer;
}

.notif-image {
  width: 40px;
  height: 40px;
  border-radius: var(--radius-sm);
  object-fit: cover;
  flex: none;
}

.notif-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}

.notif-summary {
  font-size: 13px;
  font-weight: 600;
  color: var(--fg-color);
  overflow-wrap: break-word;
}

.notif-body {
  font-size: 12px;
  font-weight: 400;
  line-height: 1.35;
  color: var(--fg-muted);
  white-space: pre-line;
  overflow-wrap: break-word;
  display: -webkit-box;
  line-clamp: 4;
  -webkit-line-clamp: 4;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.notif-progress-track {
  width: 100%;
  height: 4px;
  border-radius: 999px;
  background: var(--surface-active);
  overflow: hidden;
}

.notif-progress-fill {
  height: 100%;
  border-radius: inherit;
  background: var(--accent-color);
  transition: width var(--dur-med) var(--ease-out);
}

.notif-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding-top: 2px;
}

.notif-action-btn {
  height: 26px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--hairline);
  background: var(--surface-hover);
  color: var(--fg-color);
  font-size: 11.5px;
  font-weight: 600;
  cursor: pointer;
  transition:
    background-color 140ms var(--ease-out),
    color 140ms var(--ease-out),
    border-color 140ms var(--ease-out);
}

.notif-action-btn:hover {
  background: var(--accent-soft);
  color: var(--accent-color);
  border-color: color-mix(in srgb, var(--accent-color) 30%, transparent);
}

.notif-toast {
  position: relative;
  overflow: hidden;
  pointer-events: auto;
  padding: 12px 14px;
  border-radius: var(--radius-lg);
  background-color: color-mix(in srgb, var(--panel-bg) 68%, transparent);
  background-image: linear-gradient(
    180deg,
    color-mix(in srgb, #ffffff 8%, transparent) 0%,
    color-mix(in srgb, #ffffff 1%, transparent) 55%,
    color-mix(in srgb, #000000 6%, transparent) 100%
  );
  -webkit-backdrop-filter: blur(18px) saturate(1.35);
  backdrop-filter: blur(18px) saturate(1.35);
  border: 1px solid color-mix(in srgb, #ffffff 14%, transparent);
  box-shadow:
    inset 0 1px 0 color-mix(in srgb, #ffffff 18%, transparent),
    inset 0 0 0 1px color-mix(in srgb, #ffffff 4%, transparent),
    0 4px 12px color-mix(in srgb, var(--shadow-color) 28%, transparent),
    0 16px 40px color-mix(in srgb, var(--shadow-color) 46%, transparent);
  transition:
    background-color 480ms cubic-bezier(0.16, 1, 0.3, 1),
    border-color 480ms cubic-bezier(0.16, 1, 0.3, 1),
    box-shadow 480ms cubic-bezier(0.16, 1, 0.3, 1),
    translate 320ms cubic-bezier(0.16, 1, 0.3, 1);
  animation: notif-toast-in 260ms cubic-bezier(0.22, 1, 0.36, 1);
}

.notif-toast::before {
  content: "";
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
  opacity: 0;
  background: radial-gradient(
    320px circle at var(--toast-x, 50%) var(--toast-y, 50%),
    color-mix(in srgb, #ffffff 8%, transparent),
    transparent 75%
  );
  transition: opacity 420ms cubic-bezier(0.16, 1, 0.3, 1);
}

.notif-toast:hover::before {
  opacity: 1;
}

.notif-toast:hover {
  background-color: var(--panel-bg);
  border-color: color-mix(in srgb, #ffffff 22%, transparent);
  box-shadow:
    inset 0 1px 0 color-mix(in srgb, #ffffff 24%, transparent),
    inset 0 0 0 1px color-mix(in srgb, #ffffff 6%, transparent),
    0 6px 16px color-mix(in srgb, var(--shadow-color) 32%, transparent),
    0 20px 44px color-mix(in srgb, var(--shadow-color) 52%, transparent);
  translate: 0 -1px;
}

.notif-toast .notif-close-btn {
  opacity: 0.5;
  transition:
    opacity 180ms cubic-bezier(0.22, 1, 0.36, 1),
    background-color var(--dur-fast) var(--ease-out),
    color var(--dur-fast) var(--ease-out);
}

.notif-toast:hover .notif-close-btn {
  opacity: 1;
}

:global(:root[data-desktop-theme="light"]) .notif-toast {
  background-color: color-mix(in srgb, var(--panel-bg) 72%, transparent);
  background-image: linear-gradient(
    180deg,
    rgba(255, 255, 255, 0.72) 0%,
    rgba(255, 255, 255, 0.18) 60%,
    rgba(255, 255, 255, 0) 100%
  );
  border-color: color-mix(in srgb, var(--fg-color) 11%, transparent);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.95),
    inset 0 0 0 1px rgba(255, 255, 255, 0.45),
    0 4px 12px color-mix(in srgb, var(--shadow-color) 9%, transparent),
    0 16px 36px color-mix(in srgb, var(--shadow-color) 16%, transparent);
}

:global(:root[data-desktop-theme="light"]) .notif-toast::before {
  background: radial-gradient(
    320px circle at var(--toast-x, 50%) var(--toast-y, 50%),
    rgba(255, 255, 255, 0.45),
    transparent 75%
  );
}

:global(:root[data-desktop-theme="light"]) .notif-toast:hover {
  background-color: var(--panel-bg);
  border-color: color-mix(in srgb, var(--fg-color) 17%, transparent);
  box-shadow:
    inset 0 1px 0 #ffffff,
    inset 0 0 0 1px rgba(255, 255, 255, 0.65),
    0 6px 16px color-mix(in srgb, var(--shadow-color) 11%, transparent),
    0 20px 42px color-mix(in srgb, var(--shadow-color) 20%, transparent);
}

.notif-toast.is-critical {
  border-color: color-mix(in srgb, var(--urgent-bg) 44%, transparent);
  border-left: 3px solid var(--urgent-bg);
  background-color: color-mix(
    in srgb,
    color-mix(in srgb, var(--panel-bg) 76%, transparent),
    var(--urgent-bg) 10%
  );
}

.notif-toast.is-critical:hover {
  background-color: color-mix(in srgb, var(--panel-bg), var(--urgent-bg) 11%);
  border-color: color-mix(in srgb, var(--urgent-bg) 62%, transparent);
  border-left-color: var(--urgent-bg);
}

:global(:root[data-desktop-theme="light"]) .notif-toast.is-critical {
  border-color: color-mix(in srgb, var(--urgent-bg) 40%, transparent);
  border-left: 3px solid var(--urgent-bg);
  background-color: color-mix(
    in srgb,
    color-mix(in srgb, var(--panel-bg) 78%, transparent),
    var(--urgent-bg) 8%
  );
}

:global(:root[data-desktop-theme="light"]) .notif-toast.is-critical:hover {
  background-color: color-mix(in srgb, var(--panel-bg), var(--urgent-bg) 9%);
  border-color: color-mix(in srgb, var(--urgent-bg) 56%, transparent);
  border-left-color: var(--urgent-bg);
}

@keyframes notif-toast-in {
  from {
    opacity: 0;
    translate: 14px 0;
    scale: 0.985;
  }
  to {
    translate: 0 0;
    scale: 1;
  }
}
</style>

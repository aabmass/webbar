<script lang="ts">
  import { fly, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import {
    buildWorkspaceItems,
    getWindowCounts,
    switchWorkspace,
  } from "@webbar/sway-full-bar-shared/workspaces";
  import { fromExternalStore } from "../lib/webbarStore.svelte";
  import { motionDuration } from "../lib/transitions";

  const sway = fromExternalStore(
    window.webbar.sway.subscribe,
    window.webbar.sway.get,
  );
  const windowCounts = fromExternalStore(
    window.webbar.sway.subscribe,
    getWindowCounts,
  );

  let items = $derived(
    buildWorkspaceItems(sway.current.workspaces, windowCounts.current),
  );
  let mode = $derived(sway.current.mode);
</script>

<div class="workspaces-wrap">
  <div
    id="workspaces"
    class="workspaces-grid"
    role="group"
    aria-label="Workspaces"
  >
    {#each items as ws (ws.name)}
      <button
        type="button"
        class="ws-btn"
        class:focused={ws.focused}
        class:virtual={ws.virtual}
        class:urgent={ws.urgent}
        class:visible={ws.visible && !ws.focused}
        style="grid-row: {ws.row}; grid-column: {ws.col}"
        title={ws.output
          ? `${ws.name} (${ws.output}) — ${ws.windows} window(s)`
          : `Workspace ${ws.name}`}
        onclick={() => switchWorkspace(ws.name)}
        transition:scale={{ duration: motionDuration(110), start: 0.88, easing: cubicOut }}
      >
        {ws.name}
        {#if ws.windows > 0}
          <span class="ws-dots" aria-hidden="true">
            {#each Array(Math.min(3, ws.windows)) as _, i (i)}
              <i></i>
            {/each}
          </span>
        {/if}
      </button>
    {/each}
  </div>

  {#if mode && mode !== "default"}
    <span
      class="sway-mode"
      title={`Sway mode: ${mode}`}
      transition:fly={{ x: -8, duration: motionDuration(140), easing: cubicOut }}
    >
      {mode}
    </span>
  {/if}
</div>

<style>
/* Workspaces 2-row grid */
.workspaces-wrap {
  display: flex;
  align-items: center;
  gap: 8px;
}

.workspaces-grid {
  display: grid;
  grid-auto-rows: 21px;
  grid-auto-columns: 24px;
  gap: 3px;
  padding: 0;
  contain: paint;
}

.ws-btn {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 6px;
  background: var(--ws-occupied-bg);
  color: var(--fg-color);
  padding: 0 3px 5px;
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
  text-transform: uppercase;
}

.ws-dots {
  position: absolute;
  bottom: 2px;
  left: 0;
  right: 0;
  display: flex;
  justify-content: center;
  gap: 2px;
  pointer-events: none;
}

.ws-dots i {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.78;
  animation: ws-dot-in var(--dur-fast) var(--ease-out) both;
}

@keyframes ws-dot-in {
  from {
    opacity: 0;
    scale: 0.4;
  }
  to {
    opacity: 0.78;
    scale: 1;
  }
}

.ws-btn:hover {
  background: var(--surface-hover);
}

/* Visible on another output */
.ws-btn.visible {
  box-shadow: inset 0 0 0 1.5px var(--accent-color);
  color: var(--accent-color);
}

.ws-btn.focused {
  background: var(--accent-color);
  color: var(--accent-fg);
  box-shadow: none;
}

/* Pinned slot with no workspace: quiet placeholder, label on hover */
.ws-btn.virtual {
  background: var(--ws-empty-bg);
  box-shadow: none;
  color: var(--fg-faint);
}

.ws-btn.virtual:hover {
  background: var(--surface-color);
  color: var(--fg-muted);
}

.ws-btn.urgent {
  background: var(--urgent-bg);
  color: var(--urgent-fg);
}

@keyframes urgent-pulse {
  50% {
    box-shadow:
      0 0 0 3px var(--red-soft),
      0 0 12px var(--urgent-glow);
  }
}

.ws-btn.urgent:not(.focused) {
  animation: urgent-pulse 1.1s ease-in-out infinite;
}

.sway-mode {
  padding: 5px 10px;
  border-radius: var(--radius-sm);
  background: var(--accent-yellow);
  color: var(--accent-yellow-fg);
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  box-shadow:
    0 0 0 2px var(--yellow-soft),
    0 0 14px var(--yellow-glow);
  animation: mode-glow 1.6s ease-in-out infinite;
}

@keyframes mode-glow {
  50% {
    box-shadow:
      0 0 0 4px var(--yellow-soft),
      0 0 18px var(--yellow-soft);
  }
}
</style>

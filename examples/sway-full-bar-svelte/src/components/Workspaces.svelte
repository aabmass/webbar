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

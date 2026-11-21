<script lang="ts">
  import type { Snippet } from "svelte";
  import {
    computePopoverPosition,
    focusedWorkspaceKey,
    popoverPositionToCss,
    type PopoverPosition,
  } from "@webbar/sway-full-bar-shared/popoverPosition";
  import { popoverTransition } from "../lib/transitions";

  interface Props {
    open: boolean;
    onClose: () => void;
    anchorEl: HTMLElement | null;
    align?: "center" | "end";
    children: Snippet;
  }

  let { open, onClose, anchorEl, align = "end", children }: Props = $props();

  let position = $state<PopoverPosition>({
    caret: "none",
    top: 90,
    right: 12,
  });

  function popoverLayer(panel: HTMLDivElement) {
    if (typeof panel.showPopover === "function") {
      try {
        panel.showPopover();
      } catch {
        // Ignore if already open in top layer
      }
    }

    const updatePosition = () => {
      position = computePopoverPosition(anchorEl, panel, align);
    };
    updatePosition();

    const initialSwayKey = focusedWorkspaceKey();

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };

    const onPointerDown = (e: PointerEvent) => {
      const target = e.target;
      if (
        target instanceof Node &&
        (panel.contains(target) || anchorEl?.contains(target))
      ) {
        return;
      }
      onClose();
    };

    const onWindowBlur = () => {
      onClose();
    };

    const unsubSway = window.webbar.sway.subscribe(() => {
      if (focusedWorkspaceKey() !== initialSwayKey) {
        onClose();
      }
    });

    window.addEventListener("resize", updatePosition);
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", onWindowBlur);

    return {
      // Runs only after Svelte's `out` transition completes, keeping the panel
      // in the top layer (so preload's :popover-open overlay tracking stays
      // active) during the close animation.
      destroy() {
        window.removeEventListener("resize", updatePosition);
        window.removeEventListener("keydown", onKeyDown);
        window.removeEventListener("pointerdown", onPointerDown, true);
        window.removeEventListener("blur", onWindowBlur);
        unsubSway();
        try {
          if (panel.matches(":popover-open")) {
            panel.hidePopover();
          }
        } catch {
          // Ignore if already detached or hidden
        }
      },
    };
  }
</script>

{#if open}
  <div
    use:popoverLayer
    transition:popoverTransition={{ duration: 120 }}
    popover="manual"
    class="popover-panel caret-{position.caret}"
    style={popoverPositionToCss(position)}
    role="dialog"
  >
    {@render children()}
  </div>
{/if}

<style>
  .popover-panel {
    animation: none;
  }
</style>

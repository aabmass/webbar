<script lang="ts">
  import type { TrayItem } from "@webbar/types";
  import { trayShortLabel } from "@webbar/sway-full-bar-shared/tray";
  import Popover from "./Popover.svelte";
  import TrayIcon from "./TrayIcon.svelte";
  import TrayMenu from "./TrayMenu.svelte";

  interface Props {
    item: TrayItem;
  }

  let { item }: Props = $props();

  let open = $state(false);
  let btnEl = $state<HTMLButtonElement | null>(null);

  let label = $derived(item.title || item.icon_name || item.bus);
  let hasMenu = $derived(Boolean(item.menu_path));

  function close() {
    open = false;
  }

  function handleClick() {
    if (!hasMenu || !item.menu_path) {
      window.webbar.tray.activate(item.bus, item.path);
      return;
    }
    open = !open;
  }

  function handleSelect(itemId: number) {
    if (item.menu_path) {
      window.webbar.tray.menuEvent(item.bus, item.menu_path, itemId);
    }
    close();
  }
</script>

<button
  bind:this={btnEl}
  type="button"
  class="bar-btn tray-item"
  class:icon-btn={Boolean(item.icon_url)}
  class:is-open={open}
  title={`${label} (${item.bus}${item.path})`}
  onclick={handleClick}
  oncontextmenu={(e) => {
    e.preventDefault();
    window.webbar.tray.activate(item.bus, item.path);
  }}
>
  {#if item.icon_url}
    <TrayIcon
      iconUrl={item.icon_url}
      iconName={item.icon_name}
      class="tray-icon"
    />
  {:else}
    {trayShortLabel(item)}
  {/if}
</button>

{#if hasMenu}
  <Popover {open} onClose={close} anchorEl={btnEl} align="end">
    <TrayMenu {item} {label} onSelect={handleSelect} />
  </Popover>
{/if}

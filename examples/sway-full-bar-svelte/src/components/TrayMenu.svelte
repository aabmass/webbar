<script lang="ts">
  import type { TrayItem } from "@webbar/types";
  import { normalizeMenuItems } from "@webbar/sway-full-bar-shared/tray";
  import { fromExternalStore } from "../lib/webbarStore.svelte";
  import TrayIcon from "./TrayIcon.svelte";
  import TrayMenuList from "./TrayMenuList.svelte";

  interface Props {
    item: TrayItem;
    label: string;
    onSelect: (id: number) => void;
  }

  let { item, label, onSelect }: Props = $props();

  let menuStore = $derived.by(() => {
    const store = window.webbar.tray.menu(item.bus, item.menu_path);
    return fromExternalStore(store.subscribe, store.get);
  });
  let normalized = $derived(normalizeMenuItems(menuStore.current));
</script>

<div class="tray-menu">
  <div class="tray-menu-header">
    {#if item.icon_url}
      <TrayIcon
        iconUrl={item.icon_url}
        iconName={item.icon_name}
        class="tray-menu-header-icon"
      />
    {/if}
    <span class="tray-menu-header-title">{label}</span>
  </div>
  <div class="tray-menu-body">
    {#if normalized.length === 0}
      <div class="drawer-empty">No menu items</div>
    {:else}
      <TrayMenuList items={normalized} {onSelect} />
    {/if}
  </div>
</div>

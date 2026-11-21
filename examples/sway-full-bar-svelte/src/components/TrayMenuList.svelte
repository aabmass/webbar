<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { TrayMenuItem } from "@webbar/types";
  import { classifyMenuItems } from "@webbar/sway-full-bar-shared/tray";
  import { motionDuration } from "../lib/transitions";
  import Icon from "./Icon.svelte";
  import TrayIcon from "./TrayIcon.svelte";
  import TrayMenuList from "./TrayMenuList.svelte";

  interface Props {
    items: TrayMenuItem[];
    onSelect: (id: number) => void;
  }

  let { items, onSelect }: Props = $props();

  // Track expanded submenu by label rather than numeric id because apps like
  // nm-applet destroy and recreate menu items with fresh ids on LayoutUpdated.
  let expandedLabel = $state<string | null>(null);
  let classified = $derived(classifyMenuItems(items));
</script>

<div class="tray-menu-list" role="menu">
  {#each classified as item (item.key)}
    {#if item.kind === "separator"}
      <div class="tray-menu-sep" role="separator"></div>
    {:else if item.kind === "section"}
      <div
        class="tray-menu-section"
        title={item.entry.accessible_desc || undefined}
      >
        {item.entry.label}
      </div>
    {:else if item.kind === "status"}
      <div
        class="tray-menu-status"
        title={item.entry.accessible_desc || undefined}
      >
        {item.entry.label}
      </div>
    {:else}
      {@const entry = item.entry}
      {@const hasChildren = entry.children.length > 0}
      {@const isExpanded = expandedLabel === entry.label}
      {@const isCheckmark = entry.toggle_type === "checkmark"}
      {@const isRadio = entry.toggle_type === "radio"}
      {@const isChecked = entry.toggle_state === 1}
      <div class="tray-menu-entry">
        <button
          type="button"
          role={isCheckmark
            ? "menuitemcheckbox"
            : isRadio
              ? "menuitemradio"
              : "menuitem"}
          aria-checked={isCheckmark || isRadio ? isChecked : undefined}
          aria-expanded={hasChildren ? isExpanded : undefined}
          class="tray-menu-item"
          class:is-disabled={!entry.enabled}
          class:is-expanded={hasChildren && isExpanded}
          class:is-checked={(isCheckmark || isRadio) && isChecked}
          disabled={!entry.enabled}
          title={entry.accessible_desc || undefined}
          onclick={() => {
            if (hasChildren) {
              expandedLabel = isExpanded ? null : entry.label;
            } else {
              onSelect(entry.id);
            }
          }}
        >
          <span class="tray-menu-leading">
            {#if isRadio}
              <span
                class="tray-menu-radio"
                class:is-on={isChecked}
                aria-hidden="true"
              ></span>
            {/if}
            {#if entry.icon_url}
              <TrayIcon
                iconUrl={entry.icon_url}
                iconName={entry.icon_name}
                class="tray-menu-icon"
              />
            {/if}
            <span class="tray-menu-label">{entry.label}</span>
          </span>

          {#if isCheckmark}
            <span
              class="tray-menu-switch"
              class:is-on={isChecked}
              aria-hidden="true"
            ></span>
          {/if}
          {#if hasChildren}
            <Icon name="chevron" expanded={isExpanded} />
          {/if}
        </button>

        {#if hasChildren && isExpanded}
          <div
            class="tray-menu-sub"
            transition:slide={{ duration: motionDuration(140), easing: cubicOut }}
          >
            <TrayMenuList items={entry.children} {onSelect} />
          </div>
        {/if}
      </div>
    {/if}
  {/each}
</div>

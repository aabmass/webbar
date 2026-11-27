<script lang="ts">
  import AudioOutput from "./components/AudioOutput.svelte";
  import Bluetooth from "./components/Bluetooth.svelte";
  import Clock from "./components/Clock.svelte";
  import MonitorBacklight from "./components/MonitorBacklight.svelte";
  import Notifications from "./components/Notifications.svelte";
  import NotificationToasts from "./components/NotificationToasts.svelte";
  import Stasis from "./components/Stasis.svelte";
  import Tray from "./components/Tray.svelte";
  import Workspaces from "./components/Workspaces.svelte";

  let {
    showWorkspaces = true,
    showTray = true,
    showStasis = true,
    showNotifications = true,
  }: {
    showWorkspaces?: boolean;
    showTray?: boolean;
    showStasis?: boolean;
    showNotifications?: boolean;
  } = $props();

  let notifOpen = $state(false);
</script>

<div class="bar-shell">
  <header class="centerbox">
    <div class="bar-start">
      {#if showWorkspaces}
        <Workspaces />
      {/if}
    </div>

    <div class="bar-center">
      <Clock />
    </div>

    <div class="bar-end">
      {#if showTray}
        <Tray />
      {/if}
      <div class="widget-cluster" role="group" aria-label="System controls">
        {#if showStasis}
          <Stasis />
        {/if}
        <AudioOutput />
        <MonitorBacklight />
        <Bluetooth />
        {#if showNotifications}
          <Notifications
            open={notifOpen}
            onOpenChange={(next) => {
              notifOpen = next;
            }}
          />
        {/if}
      </div>
    </div>
  </header>
</div>
{#if showNotifications}
  <NotificationToasts hidden={notifOpen} />
{/if}

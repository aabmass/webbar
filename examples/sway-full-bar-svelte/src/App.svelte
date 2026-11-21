<script lang="ts">
  import AudioOutput from "./components/AudioOutput.svelte";
  import Bluetooth from "./components/Bluetooth.svelte";
  import Clock from "./components/Clock.svelte";
  import MonitorBacklight from "./components/MonitorBacklight.svelte";
  import Stasis from "./components/Stasis.svelte";
  import Tray from "./components/Tray.svelte";
  import Workspaces from "./components/Workspaces.svelte";
</script>

<div class="bar-shell">
  <header class="centerbox">
    <div class="bar-start">
      <Workspaces />
    </div>

    <div class="bar-center">
      <Clock />
    </div>

    <div class="bar-end">
      <Tray />
      <div class="widget-cluster" role="group" aria-label="System controls">
        <Stasis />
        <AudioOutput />
        <MonitorBacklight />
        <Bluetooth />
      </div>
    </div>
  </header>
</div>

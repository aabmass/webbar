<script lang="ts">
  import SwayBar from "webbar-sway-full-bar-svelte/Bar";
  import LoginCard from "./components/LoginCard.svelte";
</script>

<div class="greeter-shell">
  <SwayBar
    showWorkspaces={false}
    showTray={false}
    showStasis={false}
    showNotifications={false}
  />
  <main class="greeter-stage">
    <LoginCard />
  </main>
</div>

<style>
  .greeter-shell {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    background: radial-gradient(
      circle at 50% 42%,
      color-mix(in srgb, var(--bar-bg) 16%, transparent) 0%,
      color-mix(in srgb, var(--shadow-color) 34%, transparent) 100%
    );
  }

  .greeter-stage {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    padding: 24px;
  }
</style>

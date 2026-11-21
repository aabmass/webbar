<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    percent: number | null;
    children: Snippet;
  }

  let { percent, children }: Props = $props();

  let fill = $derived(percent === null ? 0 : Math.max(0, Math.min(100, percent)));
</script>

<span class="level-readout">
  <span class="level-row">
    {@render children()}
    <span class="level-value">
      {percent === null ? "--" : `${Math.round(percent)}%`}
    </span>
  </span>
  <span class="level-track" aria-hidden="true">
    <span class="level-fill" style="width: {fill}%"></span>
  </span>
</span>

<script lang="ts">
  interface Props {
    iconUrl: string;
    iconName?: string;
    class: string;
  }

  let { iconUrl, iconName, class: className }: Props = $props();

  let isSymbolic = $derived(Boolean(iconName) && iconName!.endsWith("-symbolic"));
</script>

{#if isSymbolic}
  <span
    class="{className} is-symbolic"
    style='--icon-src: url("{iconUrl}")'
    aria-hidden="true"
  ></span>
{:else}
  <img class={className} src={iconUrl} alt="" aria-hidden="true" />
{/if}

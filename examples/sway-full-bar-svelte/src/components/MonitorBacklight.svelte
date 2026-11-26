<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { fromExternalStore } from "../lib/webbarStore.svelte";
  import { motionDuration } from "../lib/transitions";
  import Icon from "./Icon.svelte";
  import LevelReadout from "./LevelReadout.svelte";
  import Popover from "./Popover.svelte";

  const backlight = fromExternalStore(
    window.webbar.backlight.subscribe,
    window.webbar.backlight.get,
  );
  const themeStore = fromExternalStore(
    window.webbar.theme.subscribe,
    window.webbar.theme.get,
  );

  let open = $state(false);
  let themeError = $state<string | null>(null);
  let btnEl = $state<HTMLButtonElement | null>(null);

  let bl = $derived(backlight.current);
  let theme = $derived(themeStore.current);

  const currentOutput = window.webbar.output;
  let activeDisplay = $derived(
    (currentOutput
      ? bl.displays.find((d) => d.output === currentOutput)
      : undefined) ?? bl.displays[0],
  );
  let activeBrightness = $derived(activeDisplay?.brightness ?? bl.brightness);
  let activeAvailable = $derived(activeDisplay?.available ?? bl.available);
  let activeError = $derived(activeDisplay?.error ?? bl.error);

  function applyBrightness(percent: number, displayId?: string) {
    void window.webbar.backlight.set(percent, displayId);
  }

  async function setSystemTheme(dark: boolean) {
    if (theme.dark === dark) return;
    themeError = null;
    try {
      const res = await window.webbar.exec([
        "gsettings",
        "set",
        "org.gnome.desktop.interface",
        "color-scheme",
        dark ? "prefer-dark" : "prefer-light",
      ]);
      if (res.code !== 0) {
        themeError =
          res.stderr.trim() ||
          res.stdout.trim() ||
          `gsettings exited with ${res.code ?? "error"}`;
      }
    } catch (err) {
      themeError = err instanceof Error ? err.message : String(err);
    }
  }

  function close() {
    open = false;
  }
</script>

{#snippet sliderRow(opts: {
  label?: string;
  labelTitle?: string;
  ariaLabel: string;
  brightness: number | null;
  disabled: boolean;
  onChange: (percent: number) => void;
})}
  <div class="popover-slider-row">
    <Icon name="display" />
    {#if opts.label}
      <span class="drawer-label" title={opts.labelTitle}>{opts.label}</span>
    {/if}
    <input
      type="range"
      class="bar-slider popover-slider"
      style="--fill: {opts.brightness ?? 50}%"
      min={0}
      max={100}
      step={1}
      disabled={opts.disabled}
      value={opts.brightness ?? 50}
      aria-label={opts.ariaLabel}
      oninput={(e) => opts.onChange(Number(e.currentTarget.value))}
    />
    <span class="drawer-value">
      {opts.brightness !== null ? `${opts.brightness}%` : "--"}
    </span>
  </div>
{/snippet}

<button
  bind:this={btnEl}
  type="button"
  class="bar-btn level-btn"
  class:is-dimmed={Boolean(activeError) || !activeAvailable}
  class:is-open={open}
  title={activeError
    ? `Monitor brightness unavailable: ${activeError} (click for theme)`
    : `Monitor brightness: ${activeBrightness !== null ? `${activeBrightness}%` : "--"} (click for display & theme, scroll to adjust)`}
  onclick={() => {
    themeError = null;
    open = !open;
  }}
  onwheel={(e) => {
    if (activeBrightness === null) return;
    e.preventDefault();
    const step = e.deltaY < 0 ? 5 : -5;
    applyBrightness(activeBrightness + step, activeDisplay?.id);
  }}
>
  <LevelReadout percent={activeBrightness}>
    <Icon name="display" />
  </LevelReadout>
</button>

<Popover {open} onClose={close} anchorEl={btnEl} align="end">
  <div class="display-popover">
    <div class="display-section">
      <span class="display-section-label">Brightness</span>
      {#if bl.displays.length > 1}
        <div class="drawer-section">
          {#each bl.displays as display (display.id)}
            {@render sliderRow({
              label: display.output ?? display.name,
              labelTitle: display.name,
              ariaLabel: `Brightness for ${display.output ?? display.name}`,
              brightness: display.brightness,
              disabled: !display.available,
              onChange: (pct) => applyBrightness(pct, display.id),
            })}
          {/each}
        </div>
      {:else}
        {@render sliderRow({
          ariaLabel: "Monitor brightness",
          brightness: activeBrightness,
          disabled: !activeAvailable,
          onChange: (pct) => applyBrightness(pct, activeDisplay?.id),
        })}
      {/if}
    </div>

    <div class="display-divider"></div>

    <div class="display-section">
      <span class="display-section-label">Appearance</span>
      <div
        class="theme-segmented"
        role="radiogroup"
        aria-label="System theme"
      >
        <button
          type="button"
          role="radio"
          aria-checked={!theme.dark}
          class="theme-option"
          class:is-active={!theme.dark}
          onclick={() => void setSystemTheme(false)}
        >
          <Icon name="sun" />
          <span>Light</span>
        </button>
        <button
          type="button"
          role="radio"
          aria-checked={theme.dark}
          class="theme-option"
          class:is-active={theme.dark}
          onclick={() => void setSystemTheme(true)}
        >
          <Icon name="moon" />
          <span>Dark</span>
        </button>
      </div>
      {#if themeError}
        <div
          class="theme-error"
          transition:slide={{ duration: motionDuration(140), easing: cubicOut }}
        >
          {themeError}
        </div>
      {/if}
    </div>
  </div>
</Popover>

<style>
/* Display & Appearance popover */
.display-popover {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.display-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.display-section-label {
  font-size: 10.5px;
  font-weight: 700;
  color: var(--fg-muted);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  padding: 0 4px;
}

.display-divider {
  height: 1px;
  background: var(--panel-border);
}

.theme-segmented {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 4px;
  padding: 3px;
  border-radius: var(--radius-md);
  background: var(--surface-color);
}

.theme-option {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: 32px;
  padding: 0 12px;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--fg-muted);
  font-size: 12.5px;
  font-weight: 600;
}

.theme-option:hover:not(.is-active) {
  background: var(--surface-hover);
  color: var(--fg-color);
}

.theme-option.is-active {
  background: var(--accent-color);
  color: var(--accent-fg);
  box-shadow: 0 1px 4px var(--shadow-sm);
}

.theme-error {
  font-size: 12px;
  font-weight: 500;
  color: var(--accent-red);
  background: var(--red-soft);
  border-radius: var(--radius-sm);
  padding: 6px 8px;
}
</style>

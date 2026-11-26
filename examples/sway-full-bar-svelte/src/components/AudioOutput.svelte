<script lang="ts">
  import { flip } from "svelte/animate";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { AudioDevice } from "@webbar/types";
  import {
    fillPercentForVolume,
    getActiveMic,
    getActiveSpeaker,
    openAudioMixer,
    sliderMaxForVolume,
  } from "@webbar/sway-full-bar-shared/audio";
  import { fromExternalStore } from "../lib/webbarStore.svelte";
  import { motionDuration } from "../lib/transitions";
  import Icon from "./Icon.svelte";
  import LevelReadout from "./LevelReadout.svelte";
  import Popover from "./Popover.svelte";

  const audio = fromExternalStore(
    window.webbar.audio.subscribe,
    window.webbar.audio.get,
  );

  let open = $state(false);
  let outputsExpanded = $state(false);
  let inputsExpanded = $state(false);
  let btnEl = $state<HTMLButtonElement | null>(null);

  let audioState = $derived(audio.current);
  let speaker = $derived(getActiveSpeaker(audioState));
  let mic = $derived(getActiveMic(audioState));

  let volume = $derived(speaker?.volume ?? 0);
  let muted = $derived(speaker?.muted ?? false);
  let pct = $derived(Math.round(volume * 100));
  let outputSliderMax = $derived(sliderMaxForVolume(volume));
  let outputFillPct = $derived(fillPercentForVolume(volume));

  let micVolume = $derived(mic?.volume ?? 0);
  let micMuted = $derived(mic?.muted ?? false);
  let micPct = $derived(Math.round(micVolume * 100));
  let micSliderMax = $derived(sliderMaxForVolume(micVolume));
  let micFillPct = $derived(fillPercentForVolume(micVolume));

  function close() {
    open = false;
  }

  function handleOpenMixer() {
    close();
    openAudioMixer();
  }

  function selectDevice(device: AudioDevice, kind: "output" | "input") {
    if (!device.default) {
      void window.webbar.audio.setDefault(device.id, kind);
    }
  }
</script>

{#snippet deviceRow(device: AudioDevice, kind: "output" | "input")}
  {@const devicePct = Math.round(device.volume * 100)}
  <button
    type="button"
    role="radio"
    class="audio-device-btn"
    class:is-default={device.default}
    title={device.description}
    aria-checked={device.default}
    onclick={() => selectDevice(device, kind)}
  >
    <span class="audio-device-dot" aria-hidden="true"></span>
    <span class="audio-device-name">{device.description}</span>
    <span class="audio-device-meta" class:is-muted={device.muted}>
      {device.muted ? "Muted" : `${devicePct}%`}
    </span>
  </button>
{/snippet}

<button
  bind:this={btnEl}
  type="button"
  class="bar-btn level-btn"
  class:is-muted={muted}
  class:is-dimmed={!audioState.available}
  class:is-open={open}
  title={speaker
    ? `${speaker.description}: ${muted ? "Muted" : `${pct}%`} (click for mixer, right-click to mute, scroll to adjust)`
    : "No audio output"}
  onclick={() => {
    open = !open;
  }}
  oncontextmenu={(e) => {
    e.preventDefault();
    if (!speaker) return;
    void window.webbar.audio.setMuted(speaker.id, !speaker.muted, "output");
  }}
  onwheel={(e) => {
    if (!speaker) return;
    e.preventDefault();
    const step = e.deltaY < 0 ? 0.05 : -0.05;
    const next = Math.max(0, Math.min(1.5, speaker.volume + step));
    void window.webbar.audio.setVolume(speaker.id, next, "output");
  }}
>
  <LevelReadout percent={speaker ? pct : null}>
    <Icon name="speaker" {volume} {muted} />
  </LevelReadout>
</button>

<Popover {open} onClose={close} anchorEl={btnEl} align="end">
  <div class="audio-popover">
    <div class="qs-header audio-header">
      <div class="qs-heading audio-heading">
        <span class="qs-title">
          <Icon name="speaker" {volume} {muted} /> Audio
        </span>
        <span
          class="qs-subtitle audio-subtitle"
          title={speaker?.description}
        >
          {!audioState.available
            ? "Audio unavailable"
            : speaker
              ? speaker.description
              : "No output device"}
        </span>
      </div>
      <button
        type="button"
        class="audio-mixer-btn"
        title="Open pwvucontrol"
        onclick={handleOpenMixer}
      >
        <Icon name="mixer" />
        <span>Mixer</span>
      </button>
    </div>

    <div class="audio-body">
      <div class="audio-section">
        <div class="audio-section-head">
          <span class="audio-section-label">Output</span>
          {#if audioState.outputs.length > 1}
            <button
              type="button"
              class="audio-collapse-btn"
              aria-expanded={outputsExpanded}
              onclick={() => {
                outputsExpanded = !outputsExpanded;
              }}
            >
              {outputsExpanded
                ? "Hide devices"
                : `Switch (${audioState.outputs.length})`}
            </button>
          {/if}
        </div>

        <div class="audio-slider-card">
          {#if speaker}
            <div class="audio-active-name" title={speaker.description}>
              {speaker.description}
            </div>
          {/if}
          <div class="audio-slider-row">
            <button
              type="button"
              class="audio-mute-btn"
              class:is-muted={muted}
              disabled={!speaker}
              title={muted ? "Unmute output" : "Mute output"}
              aria-label={muted ? "Unmute output" : "Mute output"}
              onclick={() => {
                if (!speaker) return;
                void window.webbar.audio.setMuted(
                  speaker.id,
                  !speaker.muted,
                  "output",
                );
              }}
            >
              <Icon name="speaker" {volume} {muted} />
            </button>
            <input
              type="range"
              class="bar-slider audio-slider"
              class:is-muted={muted}
              style="--fill: {outputFillPct}%"
              min={0}
              max={outputSliderMax}
              step={0.01}
              value={Math.min(outputSliderMax, volume)}
              disabled={!speaker}
              aria-label="Output volume"
              oninput={(e) => {
                if (!speaker) return;
                void window.webbar.audio.setVolume(
                  speaker.id,
                  Number(e.currentTarget.value),
                  "output",
                );
              }}
            />
            <span class="drawer-value">
              {speaker ? `${pct}%` : "--"}
            </span>
          </div>
        </div>

        {#if outputsExpanded && audioState.outputs.length > 1}
          <div
            class="audio-device-list"
            role="radiogroup"
            aria-label="Output devices"
            transition:slide={{ duration: motionDuration(150), easing: cubicOut }}
          >
            {#each audioState.outputs as device (device.id)}
              {@render deviceRow(device, "output")}
            {/each}
          </div>
        {/if}
      </div>

      {#if mic}
        <div class="audio-section">
          <div class="audio-section-head">
            <span class="audio-section-label">Input</span>
            {#if audioState.inputs.length > 1}
              <button
                type="button"
                class="audio-collapse-btn"
                aria-expanded={inputsExpanded}
                onclick={() => {
                  inputsExpanded = !inputsExpanded;
                }}
              >
                {inputsExpanded
                  ? "Hide devices"
                  : `Switch (${audioState.inputs.length})`}
              </button>
            {/if}
          </div>

          <div class="audio-slider-card">
            <div class="audio-active-name" title={mic.description}>
              {mic.description}
            </div>
            <div class="audio-slider-row">
              <button
                type="button"
                class="audio-mute-btn"
                class:is-muted={micMuted}
                title={micMuted ? "Unmute input" : "Mute input"}
                aria-label={micMuted ? "Unmute input" : "Mute input"}
                onclick={() => {
                  if (!mic) return;
                  void window.webbar.audio.setMuted(mic.id, !mic.muted, "input");
                }}
              >
                <Icon name="mic" muted={micMuted} />
              </button>
              <input
                type="range"
                class="bar-slider audio-slider"
                class:is-muted={micMuted}
                style="--fill: {micFillPct}%"
                min={0}
                max={micSliderMax}
                step={0.01}
                value={Math.min(micSliderMax, micVolume)}
                aria-label="Input volume"
                oninput={(e) => {
                  if (!mic) return;
                  void window.webbar.audio.setVolume(
                    mic.id,
                    Number(e.currentTarget.value),
                    "input",
                  );
                }}
              />
              <span class="drawer-value">{micPct}%</span>
            </div>
          </div>

          {#if inputsExpanded && audioState.inputs.length > 1}
            <div
              class="audio-device-list"
              role="radiogroup"
              aria-label="Input devices"
              transition:slide={{ duration: motionDuration(150), easing: cubicOut }}
            >
              {#each audioState.inputs as device (device.id)}
                {@render deviceRow(device, "input")}
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <div class="audio-section">
        <div class="audio-section-label">Applications</div>
        {#if audioState.streams.length === 0}
          <div class="drawer-empty">No apps playing audio</div>
        {:else}
          <div class="audio-stream-list">
            {#each audioState.streams as stream (stream.id)}
              {@const streamPct = Math.round(stream.volume * 100)}
              {@const streamMax = sliderMaxForVolume(stream.volume)}
              {@const streamFill = fillPercentForVolume(stream.volume)}
              <div
                class="audio-stream-card"
                animate:flip={{ duration: motionDuration(180), easing: cubicOut }}
                transition:slide={{ duration: motionDuration(140), easing: cubicOut }}
              >
                <div class="audio-stream-head">
                  <span class="audio-stream-app">{stream.app_name}</span>
                  {#if stream.name}
                    <span class="audio-stream-media" title={stream.name}>
                      {stream.name}
                    </span>
                  {/if}
                </div>
                <div class="audio-slider-row">
                  <button
                    type="button"
                    class="audio-mute-btn"
                    class:is-muted={stream.muted}
                    title={stream.muted
                      ? `Unmute ${stream.app_name}`
                      : `Mute ${stream.app_name}`}
                    aria-label={stream.muted
                      ? `Unmute ${stream.app_name}`
                      : `Mute ${stream.app_name}`}
                    onclick={() => {
                      void window.webbar.audio.setStreamMuted(
                        stream.id,
                        !stream.muted,
                      );
                    }}
                  >
                    <Icon name="speaker" volume={stream.volume} muted={stream.muted} />
                  </button>
                  <input
                    type="range"
                    class="bar-slider audio-slider"
                    class:is-muted={stream.muted}
                    style="--fill: {streamFill}%"
                    min={0}
                    max={streamMax}
                    step={0.01}
                    value={Math.min(streamMax, stream.volume)}
                    aria-label={`${stream.app_name} volume`}
                    oninput={(e) => {
                      void window.webbar.audio.setStreamVolume(
                        stream.id,
                        Number(e.currentTarget.value),
                      );
                    }}
                  />
                  <span class="drawer-value">{streamPct}%</span>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>
</Popover>

<style>
/* Audio popover */
.audio-popover {
  display: flex;
  flex-direction: column;
  width: 320px;
  gap: 8px;
}

.audio-header {
  padding: 0 2px 8px;
  border-bottom: 1px solid var(--panel-border);
}

.audio-heading {
  min-width: 0;
  flex: 1;
}

.audio-subtitle {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.audio-mixer-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 28px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  background: var(--surface-color);
  color: var(--fg-muted);
  font-size: 12px;
  font-weight: 600;
  flex: none;
}

.audio-mixer-btn:hover {
  background: var(--surface-hover);
  color: var(--fg-color);
}

.audio-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-height: min(560px, calc(100vh - 160px));
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-width: thin;
  scrollbar-color: color-mix(in srgb, var(--fg-color) 16%, transparent) transparent;
}

.audio-body::-webkit-scrollbar {
  width: 5px;
}

.audio-body::-webkit-scrollbar-track {
  background: transparent;
}

.audio-body::-webkit-scrollbar-thumb {
  background: color-mix(in srgb, var(--fg-color) 16%, transparent);
  border-radius: 999px;
}

.audio-body::-webkit-scrollbar-button {
  display: none;
}

.audio-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.audio-section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.audio-section-label {
  font-size: 10.5px;
  font-weight: 700;
  color: var(--fg-muted);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  padding: 0 4px;
}

.audio-collapse-btn {
  padding: 2px 6px;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--accent-color);
  font-size: 11px;
  font-weight: 600;
}

.audio-collapse-btn:hover {
  background: var(--accent-soft);
}

.audio-slider-card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  background: var(--surface-color);
}

.audio-active-name {
  font-size: 11.5px;
  font-weight: 600;
  color: var(--fg-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.audio-slider-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.audio-mute-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--fg-muted);
  flex: none;
}

.audio-mute-btn:hover:not(:disabled) {
  background: var(--surface-hover);
  color: var(--fg-color);
}

.audio-mute-btn.is-muted {
  color: var(--accent-red);
  background: var(--red-soft);
}

.audio-slider {
  flex: 1;
  width: auto;
  min-width: 0;
}

.audio-slider.is-muted {
  opacity: 0.55;
  background: linear-gradient(
    to right,
    var(--accent-red) 0 var(--fill),
    var(--surface-active) var(--fill) 100%
  );
}

.audio-device-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.audio-device-btn {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 32px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--fg-color);
  font-size: 12.5px;
  font-weight: 500;
  line-height: 1.3;
  text-align: left;
}

.audio-device-btn:active {
  scale: 0.99;
}

.audio-device-btn:hover {
  background: var(--surface-hover);
}

.audio-device-btn.is-default {
  background: var(--accent-soft);
  color: var(--fg-color);
  font-weight: 600;
}

.audio-device-btn.is-default:hover {
  background: color-mix(in srgb, var(--accent-color) 22%, transparent);
}

.audio-device-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex: none;
  background: var(--fg-faint);
}

.audio-device-btn.is-default .audio-device-dot {
  background: var(--accent-color);
  box-shadow: 0 0 6px var(--accent-glow);
}

.audio-device-name {
  flex: 1;
  min-width: 0;
  overflow-wrap: break-word;
  white-space: normal;
}

.audio-device-meta {
  flex: none;
  font-size: 11px;
  color: var(--fg-muted);
  font-variant-numeric: tabular-nums;
}

.audio-device-meta.is-muted {
  color: var(--accent-red);
}

.audio-stream-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.audio-stream-card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  background: var(--surface-color);
}

.audio-stream-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.audio-stream-app {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--fg-color);
  flex: none;
}

.audio-stream-media {
  font-size: 11px;
  font-weight: 500;
  color: var(--fg-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  min-width: 0;
}
</style>

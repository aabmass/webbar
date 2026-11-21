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

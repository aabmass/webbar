<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import {
    DEFAULT_PROFILE,
    extractBlockers,
    formatProfileLabel,
    parseProfilesOutput,
    type BlameSnapshot,
    type StasisWatchState,
  } from "@webbar/sway-full-bar-shared/stasis";
  import { motionDuration } from "../lib/transitions";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";

  let status = $state<StasisWatchState | null>(null);
  let profiles = $state<string[]>([DEFAULT_PROFILE]);
  let blame = $state<BlameSnapshot | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let open = $state(false);
  let btnEl = $state<HTMLButtonElement | null>(null);

  async function refreshProfiles() {
    try {
      const res = await window.webbar.exec(["stasis", "list", "profiles"]);
      if (res.code === 0) {
        profiles = parseProfilesOutput(res.stdout);
      }
    } catch {
      // Keep existing profiles list if stasis is temporarily unavailable
    }
  }

  async function refreshBlame() {
    try {
      const res = await window.webbar.exec(["stasis", "blame", "--json"]);
      if (res.code === 0) {
        blame = JSON.parse(res.stdout) as BlameSnapshot;
      }
    } catch {
      blame = null;
    }
  }

  $effect(() => {
    let disposed = false;
    let reconnectTimer: number | null = null;
    let controller = new AbortController();

    const startWatch = async () => {
      if (disposed) return;
      controller = new AbortController();

      try {
        await window.webbar.execStream(["stasis", "watch"], {
          signal: controller.signal,
          onStdout: (line) => {
            const trimmed = line.trim();
            if (!trimmed) return;
            try {
              const next = JSON.parse(trimmed) as Partial<StasisWatchState>;
              if (typeof next.state === "string") {
                status = {
                  state: next.state,
                  paused: Boolean(next.paused),
                  manually_paused: Boolean(next.manually_paused),
                  profile:
                    typeof next.profile === "string" && next.profile
                      ? next.profile
                      : DEFAULT_PROFILE,
                };
                error = null;
              }
            } catch {
              // Ignore non-JSON lines
            }
          },
          onStderr: (line) => {
            const trimmed = line.trim();
            if (trimmed) {
              error = trimmed;
            }
          },
          onExit: () => {
            if (disposed) return;
            reconnectTimer = window.setTimeout(() => {
              void startWatch();
            }, 2000);
          },
        });
      } catch (err) {
        if (disposed) return;
        if (err instanceof DOMException && err.name === "AbortError") {
          return;
        }
        error = err instanceof Error ? err.message : String(err);
        reconnectTimer = window.setTimeout(() => {
          void startWatch();
        }, 5000);
      }
    };

    void startWatch();
    void refreshProfiles();

    return () => {
      disposed = true;
      if (reconnectTimer !== null) {
        window.clearTimeout(reconnectTimer);
      }
      controller.abort();
    };
  });

  let hasStatus = $derived(status !== null);
  let statusState = $derived(status?.state ?? null);
  let statusPaused = $derived(status?.paused ?? false);
  let statusManuallyPaused = $derived(status?.manually_paused ?? false);

  $effect(() => {
    if (!hasStatus) {
      blame = null;
      return;
    }
    const timer = window.setInterval(() => {
      void refreshBlame();
    }, 3000);
    return () => {
      window.clearInterval(timer);
    };
  });

  // Refresh blame immediately when daemon status or inhibition state changes
  // (e.g. D-Bus or Wayland idle inhibitors appearing/clearing).
  $effect(() => {
    if (!hasStatus) return;
    void statusState;
    void statusPaused;
    void statusManuallyPaused;
    void refreshBlame();
  });

  $effect(() => {
    if (!open) return;
    void refreshProfiles();
    void refreshBlame();
  });

  let activeProfile = $derived(status?.profile ?? DEFAULT_PROFILE);
  let isManuallyPaused = $derived(Boolean(status?.manually_paused));
  let blockers = $derived(extractBlockers(blame));
  let hasExternalInhibitor = $derived(
    Boolean(status?.paused || blockers.length > 0),
  );
  let isInhibited = $derived(isManuallyPaused || hasExternalInhibitor);

  async function runStasisCommand(argv: string[]) {
    busy = true;
    error = null;
    try {
      const res = await window.webbar.exec(argv);
      if (res.code !== 0) {
        error =
          res.stderr.trim() ||
          res.stdout.trim() ||
          `Command exited with ${res.code ?? "error"}`;
      }
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      busy = false;
    }
  }

  function togglePause() {
    void runStasisCommand(["stasis", isManuallyPaused ? "resume" : "pause"]);
  }

  function close() {
    open = false;
  }

  let statusSuffix = $derived(
    isManuallyPaused
      ? " (paused)"
      : isInhibited
        ? " (inhibited)"
        : "",
  );

  let badgeTooltip = $derived(
    !status
      ? "Stasis idle daemon is not running or unreachable"
      : isManuallyPaused
        ? "Idle timers are manually paused until resumed"
        : isInhibited
          ? blockers.length > 0
            ? `Idle progression blocked by: ${blockers.map((b) => b.name).join(", ")}`
            : "Idle progression is blocked by an active inhibitor"
          : `Idle timers are running (${formatProfileLabel(activeProfile)} profile)`,
  );
</script>

<button
  bind:this={btnEl}
  type="button"
  class="bar-btn stasis-btn"
  class:is-dimmed={!status}
  class:is-inhibited={isInhibited}
  class:is-open={open}
  title={error
    ? `Stasis: ${error}`
    : status
      ? `Idle: ${formatProfileLabel(activeProfile)}${statusSuffix} — click for controls, right-click to ${isManuallyPaused ? "resume" : "pause"}`
      : "Stasis offline"}
  onclick={() => {
    open = !open;
  }}
  oncontextmenu={(e) => {
    e.preventDefault();
    togglePause();
  }}
>
  <Icon name="idle" inhibited={isInhibited} />
  <span class="stasis-label">
    {status ? formatProfileLabel(activeProfile) : "idle"}
  </span>
</button>

<Popover {open} onClose={close} anchorEl={btnEl} align="end">
  <div class="stasis-popover">
    <div class="stasis-header">
      <span class="stasis-title">Idle</span>
      {#if !status}
        <span class="stasis-badge is-offline" title={badgeTooltip}>offline</span>
      {:else if isInhibited}
        <span class="stasis-badge is-inhibited" title={badgeTooltip}>
          {isManuallyPaused ? "paused" : "inhibited"}
        </span>
      {:else}
        <span class="stasis-badge" title={badgeTooltip}>active</span>
      {/if}
    </div>

    <button
      type="button"
      class="bar-btn stasis-action-btn"
      class:is-active={isManuallyPaused}
      disabled={busy || !status}
      onclick={togglePause}
    >
      {isManuallyPaused ? "Resume Idle" : "Pause Idle"}
    </button>

    <div class="stasis-section-label">Profiles</div>
    <div class="stasis-profile-list">
      {#each profiles as profile (profile)}
        {@const isCurrent = activeProfile === profile}
        <button
          type="button"
          class="bar-btn stasis-profile-btn"
          class:is-current={isCurrent}
          disabled={busy || !status}
          onclick={() =>
            void runStasisCommand([
              "stasis",
              "profile",
              profile === DEFAULT_PROFILE ? "none" : profile,
            ])}
        >
          <span>{formatProfileLabel(profile)}</span>
          {#if isCurrent}
            <span class="stasis-check">✓</span>
          {/if}
        </button>
      {/each}
    </div>

    {#if blockers.length > 0}
      <div transition:slide={{ duration: motionDuration(150), easing: cubicOut }}>
        <div class="stasis-section-label">Inhibitors</div>
        <div class="stasis-inhibitor-list">
          {#each blockers as item (item.key)}
            <div class="stasis-inhibitor-item">
              <span class="stasis-inhibitor-name">{item.name}</span>
              {#if item.detail}
                <span class="stasis-inhibitor-detail">{item.detail}</span>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if error}
      <div
        class="stasis-error"
        transition:slide={{ duration: motionDuration(140), easing: cubicOut }}
      >
        {error}
      </div>
    {/if}
  </div>
</Popover>

<style>
/* Stasis idle widget & popover */
.stasis-btn {
  gap: 6px;
  padding: 0 12px;
  font-size: 12px;
}

.stasis-btn :global(svg) {
  width: 16px;
  height: 16px;
}

.stasis-btn.is-inhibited {
  color: var(--accent-yellow);
}

.stasis-btn.is-dimmed {
  opacity: 0.45;
}

.stasis-label {
  white-space: nowrap;
}

.stasis-popover {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 240px;
}

.stasis-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 0 2px 2px;
}

.stasis-title {
  font-size: 14px;
  font-weight: 700;
}

.stasis-badge {
  font-size: 11px;
  font-weight: 700;
  padding: 3px 9px;
  border-radius: 999px;
  background: var(--green-soft);
  color: var(--accent-green);
}

.stasis-badge.is-inhibited {
  background: var(--yellow-soft);
  color: var(--accent-yellow);
}

.stasis-badge.is-offline {
  background: var(--surface-color);
  color: var(--fg-muted);
}

.stasis-error {
  font-size: 12px;
  font-weight: 500;
  color: var(--accent-red);
  background: var(--red-soft);
  border-radius: var(--radius-sm);
  padding: 6px 8px;
}

.stasis-inhibitor-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.stasis-inhibitor-item {
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  background: var(--surface-color);
}

.stasis-inhibitor-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--fg-color);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stasis-inhibitor-detail {
  font-size: 11px;
  font-weight: 500;
  color: var(--fg-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stasis-action-btn {
  width: 100%;
  height: 36px;
  margin: 0;
  padding: 0 10px;
  font-size: 12px;
  border-radius: var(--radius-sm);
}

.stasis-action-btn.is-active {
  background: var(--accent-yellow);
  color: var(--accent-yellow-fg);
}

.stasis-action-btn:disabled,
.stasis-profile-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.stasis-section-label {
  font-size: 11px;
  font-weight: 700;
  color: var(--fg-muted);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  padding: 4px 4px 0;
}

.stasis-profile-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.stasis-profile-btn {
  height: 34px;
  margin: 0;
  padding: 0 12px;
  font-size: 13px;
  justify-content: space-between;
  border-radius: var(--radius-sm);
  background: transparent;
}

.stasis-profile-btn:hover:not(:disabled) {
  background: var(--surface-hover);
}

.stasis-profile-btn.is-current {
  background: var(--accent-soft);
  color: var(--accent-color);
}

.stasis-check {
  font-size: 12px;
}
</style>

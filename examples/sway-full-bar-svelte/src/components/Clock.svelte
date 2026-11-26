<script lang="ts">
  import {
    buildCalendarCells,
    clockParts,
    formatMonthLabel,
    WEEKDAYS,
  } from "@webbar/sway-full-bar-shared/clock";
  import Popover from "./Popover.svelte";

  const initialNow = new Date();
  let now = $state(initialNow);
  let open = $state(false);
  let btnEl = $state<HTMLButtonElement | null>(null);
  let viewDate = $state(
    new Date(initialNow.getFullYear(), initialNow.getMonth(), 1),
  );

  $effect(() => {
    const id = window.setInterval(() => {
      now = new Date();
    }, 1000);
    return () => window.clearInterval(id);
  });

  function toggleOpen() {
    if (!open) {
      viewDate = new Date(now.getFullYear(), now.getMonth(), 1);
    }
    open = !open;
  }

  let parts = $derived(clockParts(now));
  let year = $derived(viewDate.getFullYear());
  let month = $derived(viewDate.getMonth());
  let monthLabel = $derived(formatMonthLabel(viewDate));
  let cells = $derived(
    buildCalendarCells(
      year,
      month,
      now.getFullYear(),
      now.getMonth(),
      now.getDate(),
    ),
  );

  function close() {
    open = false;
  }
</script>

<button
  bind:this={btnEl}
  type="button"
  class="bar-btn clock"
  class:is-open={open}
  title={now.toLocaleString()}
  onclick={toggleOpen}
>
  <span class="clock-time">
    {parts.time}<span class="clock-seconds">{parts.seconds}</span><span class="clock-ampm">{parts.ampm}</span><span class="clock-tz">{parts.tz}</span>
  </span>
  <span class="clock-date">{parts.date}</span>
</button>

<Popover {open} onClose={close} anchorEl={btnEl} align="center">
  <div class="calendar">
    <div class="calendar-header">
      <button
        type="button"
        class="cal-nav"
        aria-label="Previous month"
        onclick={() => {
          viewDate = new Date(year, month - 1, 1);
        }}
      >
        ‹
      </button>
      <span class="cal-title">{monthLabel}</span>
      <button
        type="button"
        class="cal-nav"
        aria-label="Next month"
        onclick={() => {
          viewDate = new Date(year, month + 1, 1);
        }}
      >
        ›
      </button>
    </div>
    <div class="calendar-grid">
      {#each WEEKDAYS as wd (wd)}
        <span class="cal-weekday">{wd}</span>
      {/each}
      {#each cells as cell, idx (idx)}
        <span
          class="cal-day"
          class:is-outside={cell.outside}
          class:is-today={cell.isToday}
          class:is-weekend={cell.isWeekend}
        >
          {cell.day}
        </span>
      {/each}
    </div>
  </div>
</Popover>

<style>
/* Clock: big time over a quiet date line */
.clock {
  flex-direction: column;
  gap: 2px;
  min-width: 200px;
  padding: 0 20px;
  font-variant-numeric: tabular-nums;
  line-height: 1.1;
}

.clock.bar-btn {
  background: transparent;
}

.clock.bar-btn:hover {
  background: var(--surface-color);
}

.clock.bar-btn.is-open {
  background: var(--surface-hover);
}

.clock-time {
  font-size: 20px;
  font-weight: 700;
  letter-spacing: 0.01em;
}

.clock-seconds {
  font-size: 14px;
  font-weight: 600;
  color: var(--fg-muted);
}

.clock-ampm,
.clock-tz {
  font-size: 12px;
  font-weight: 600;
  color: var(--fg-muted);
  margin-left: 4px;
}

.clock-date {
  font-size: 12px;
  font-weight: 500;
  color: var(--fg-muted);
}

/* Calendar */
.calendar {
  width: 260px;
}

.calendar-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}

.cal-title {
  font-size: 14px;
  font-weight: 700;
}

.cal-nav {
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--fg-muted);
  font-size: 18px;
  line-height: 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.cal-nav:hover {
  background: var(--surface-hover);
  color: var(--fg-color);
}

.calendar-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 2px;
  text-align: center;
}

.cal-weekday {
  font-size: 11px;
  font-weight: 600;
  color: var(--fg-faint);
  padding: 4px 0;
}

.cal-day {
  font-size: 12px;
  font-weight: 500;
  padding: 6px 0;
  border-radius: var(--radius-sm);
  font-variant-numeric: tabular-nums;
}

.cal-day.is-weekend {
  color: var(--fg-muted);
}

.cal-day.is-outside {
  color: var(--fg-faint);
  opacity: 0.55;
}

.cal-day.is-today {
  background: var(--accent-color);
  color: var(--accent-fg);
  font-weight: 700;
}
</style>

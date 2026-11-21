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

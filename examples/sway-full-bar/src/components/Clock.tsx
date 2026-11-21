import { memo, useCallback, useEffect, useRef, useState } from "react";
import {
  buildCalendarCells,
  clockParts,
  formatMonthLabel,
  WEEKDAYS,
} from "@webbar/sway-full-bar-shared/clock";
import { Popover } from "./Popover";

type CalendarProps = {
  todayYear: number;
  todayMonth: number;
  todayDay: number;
};

const Calendar = memo(function Calendar({
  todayYear,
  todayMonth,
  todayDay,
}: CalendarProps) {
  const [view, setView] = useState(() => new Date(todayYear, todayMonth, 1));

  const year = view.getFullYear();
  const month = view.getMonth();
  const cells = buildCalendarCells(year, month, todayYear, todayMonth, todayDay);
  const monthLabel = formatMonthLabel(view);

  return (
    <div className="calendar">
      <div className="calendar-header">
        <button
          type="button"
          className="cal-nav"
          aria-label="Previous month"
          onClick={() => setView(new Date(year, month - 1, 1))}
        >
          ‹
        </button>
        <span className="cal-title">{monthLabel}</span>
        <button
          type="button"
          className="cal-nav"
          aria-label="Next month"
          onClick={() => setView(new Date(year, month + 1, 1))}
        >
          ›
        </button>
      </div>
      <div className="calendar-grid">
        {WEEKDAYS.map((wd) => (
          <span key={wd} className="cal-weekday">
            {wd}
          </span>
        ))}
        {cells.map((cell, idx) => {
          const classes = ["cal-day"];
          if (cell.outside) classes.push("is-outside");
          else if (cell.isToday) classes.push("is-today");
          if (cell.isWeekend) classes.push("is-weekend");
          return (
            <span key={idx} className={classes.join(" ")}>
              {cell.day}
            </span>
          );
        })}
      </div>
    </div>
  );
});

export function Clock() {
  const [now, setNow] = useState(() => new Date());
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 1000);
    return () => window.clearInterval(id);
  }, []);

  const parts = clockParts(now);

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={`bar-btn clock${open ? " is-open" : ""}`}
        title={now.toLocaleString()}
        onClick={() => setOpen((prev) => !prev)}
      >
        <span className="clock-time">
          {parts.time}
          <span className="clock-seconds">{parts.seconds}</span>
          <span className="clock-ampm">{parts.ampm}</span>
          <span className="clock-tz">{parts.tz}</span>
        </span>
        <span className="clock-date">{parts.date}</span>
      </button>
      <Popover open={open} onClose={close} anchorRef={btnRef} align="center">
        <Calendar
          todayYear={now.getFullYear()}
          todayMonth={now.getMonth()}
          todayDay={now.getDate()}
        />
      </Popover>
    </>
  );
}

import { useCallback, useEffect, useRef, useState } from "react";
import { Popover } from "./Popover";

function clockParts(date: Date) {
  const parts = new Intl.DateTimeFormat("en-US", {
    hour: "numeric",
    minute: "2-digit",
    second: "2-digit",
    hour12: true,
    timeZoneName: "short",
  }).formatToParts(date);
  const get = (type: Intl.DateTimeFormatPartTypes) =>
    parts.find((p) => p.type === type)?.value ?? "";
  const time = `${get("hour")}:${get("minute")}`;
  const seconds = `:${get("second")}`;
  const date_ = date.toLocaleDateString("en-US", {
    weekday: "long",
    month: "long",
    day: "numeric",
    year: "numeric",
  });
  return {
    time,
    seconds,
    ampm: get("dayPeriod"),
    tz: get("timeZoneName"),
    date: date_,
  };
}

const WEEKDAYS = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

function Calendar({ today }: { today: Date }) {
  const [view, setView] = useState(
    () => new Date(today.getFullYear(), today.getMonth(), 1),
  );

  const year = view.getFullYear();
  const month = view.getMonth();
  const firstDow = new Date(year, month, 1).getDay();
  const daysInMonth = new Date(year, month + 1, 0).getDate();

  const prevMonthDays = new Date(year, month, 0).getDate();
  const cells: { day: number; outside: boolean }[] = [];
  for (let i = firstDow - 1; i >= 0; i--) {
    cells.push({ day: prevMonthDays - i, outside: true });
  }
  for (let d = 1; d <= daysInMonth; d++) cells.push({ day: d, outside: false });
  for (let d = 1; cells.length % 7 !== 0; d++) {
    cells.push({ day: d, outside: true });
  }

  const isToday = (d: number) =>
    d === today.getDate() &&
    month === today.getMonth() &&
    year === today.getFullYear();

  const isWeekend = (cellIdx: number) =>
    cellIdx % 7 === 0 || cellIdx % 7 === 6;

  const monthLabel = view.toLocaleDateString("en-US", {
    month: "long",
    year: "numeric",
  });

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
        {cells.map(({ day, outside }, idx) => {
          const classes = ["cal-day"];
          if (outside) classes.push("is-outside");
          else if (isToday(day)) classes.push("is-today");
          if (isWeekend(idx)) classes.push("is-weekend");
          return (
            <span key={idx} className={classes.join(" ")}>
              {day}
            </span>
          );
        })}
      </div>
    </div>
  );
}

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
        <Calendar today={now} />
      </Popover>
    </>
  );
}

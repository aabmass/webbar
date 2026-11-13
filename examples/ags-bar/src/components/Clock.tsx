import { useCallback, useEffect, useRef, useState } from "react";
import { Popover } from "./Popover";

function formatClock(date: Date): string {
  // Matches GLib "%c" style: "Thu 12 Nov 2026 10:30:52 PM EST"
  const weekday = date.toLocaleDateString("en-US", { weekday: "short" });
  const day = String(date.getDate()).padStart(2, "0");
  const month = date.toLocaleDateString("en-US", { month: "short" });
  const year = date.getFullYear();
  const time = date.toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: true,
    timeZoneName: "short",
  });
  return `${weekday} ${day} ${month} ${year} ${time}`;
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

  const cells: (number | null)[] = [];
  for (let i = 0; i < firstDow; i++) cells.push(null);
  for (let d = 1; d <= daysInMonth; d++) cells.push(d);

  const isToday = (d: number) =>
    d === today.getDate() &&
    month === today.getMonth() &&
    year === today.getFullYear();

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
        {cells.map((day, idx) =>
          day === null ? (
            <span key={`empty-${idx}`} className="cal-empty" />
          ) : (
            <span
              key={day}
              className={`cal-day${isToday(day) ? " is-today" : ""}`}
            >
              {day}
            </span>
          ),
        )}
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

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={`bar-btn clock${open ? " is-open" : ""}`}
        title={now.toLocaleString()}
        onClick={() => setOpen((prev) => !prev)}
      >
        {formatClock(now)}
      </button>
      <Popover open={open} onClose={close} anchorRef={btnRef} align="center">
        <Calendar today={now} />
      </Popover>
    </>
  );
}

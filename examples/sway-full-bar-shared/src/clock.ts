const TIME_FORMATTER = new Intl.DateTimeFormat("en-US", {
  hour: "numeric",
  minute: "2-digit",
  second: "2-digit",
  hour12: true,
  timeZoneName: "short",
});

const DATE_FORMATTER = new Intl.DateTimeFormat("en-US", {
  weekday: "long",
  month: "long",
  day: "numeric",
  year: "numeric",
});

const MONTH_FORMATTER = new Intl.DateTimeFormat("en-US", {
  month: "long",
  year: "numeric",
});

export const WEEKDAYS = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"] as const;

export type ClockParts = {
  time: string;
  seconds: string;
  ampm: string;
  tz: string;
  date: string;
};

export function clockParts(date: Date): ClockParts {
  const parts = TIME_FORMATTER.formatToParts(date);
  const get = (type: Intl.DateTimeFormatPartTypes) =>
    parts.find((p) => p.type === type)?.value ?? "";
  return {
    time: `${get("hour")}:${get("minute")}`,
    seconds: `:${get("second")}`,
    ampm: get("dayPeriod"),
    tz: get("timeZoneName"),
    date: DATE_FORMATTER.format(date),
  };
}

export function formatMonthLabel(date: Date): string {
  return MONTH_FORMATTER.format(date);
}

export type CalendarCell = {
  day: number;
  outside: boolean;
  isToday: boolean;
  isWeekend: boolean;
};

export function buildCalendarCells(
  year: number,
  month: number,
  todayYear: number,
  todayMonth: number,
  todayDay: number,
): CalendarCell[] {
  const firstDow = new Date(year, month, 1).getDay();
  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const prevMonthDays = new Date(year, month, 0).getDate();

  const raw: { day: number; outside: boolean }[] = [];
  for (let i = firstDow - 1; i >= 0; i--) {
    raw.push({ day: prevMonthDays - i, outside: true });
  }
  for (let d = 1; d <= daysInMonth; d++) {
    raw.push({ day: d, outside: false });
  }
  for (let d = 1; raw.length % 7 !== 0; d++) {
    raw.push({ day: d, outside: true });
  }

  return raw.map(({ day, outside }, idx) => ({
    day,
    outside,
    isToday:
      !outside && day === todayDay && month === todayMonth && year === todayYear,
    isWeekend: idx % 7 === 0 || idx % 7 === 6,
  }));
}

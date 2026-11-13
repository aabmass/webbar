import { useEffect, useState } from "react";

function formatClock(date: Date, compact: boolean): string {
  if (compact) {
    return date.toLocaleDateString(undefined, {
      weekday: "short",
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  }

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

export function Clock() {
  const [now, setNow] = useState(() => new Date());
  const [compact, setCompact] = useState(false);

  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 1000);
    return () => window.clearInterval(id);
  }, []);

  return (
    <button
      type="button"
      className="bar-btn clock"
      title={now.toLocaleString()}
      onClick={() => setCompact((prev) => !prev)}
    >
      {formatClock(now, compact)}
    </button>
  );
}

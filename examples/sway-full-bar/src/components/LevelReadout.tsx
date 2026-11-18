import type { ReactNode } from "react";

/** Icon + percentage over a thin level bar, used inside bar buttons. */
export function LevelReadout({
  icon,
  percent,
}: {
  icon: ReactNode;
  percent: number | null;
}) {
  const fill = percent === null ? 0 : Math.max(0, Math.min(100, percent));
  return (
    <span className="level-readout">
      <span className="level-row">
        {icon}
        <span className="level-value">
          {percent === null ? "--" : `${Math.round(percent)}%`}
        </span>
      </span>
      <span className="level-track" aria-hidden="true">
        <span className="level-fill" style={{ width: `${fill}%` }} />
      </span>
    </span>
  );
}

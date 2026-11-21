import { useMemo, useSyncExternalStore } from "react";
import {
  buildWorkspaceItems,
  getWindowCounts,
  switchWorkspace,
} from "@webbar/sway-full-bar-shared/workspaces";

export function Workspaces() {
  const { subscribe, get } = window.webbar.sway;
  const workspaces = useSyncExternalStore(subscribe, () => get().workspaces);
  const mode = useSyncExternalStore(subscribe, () => get().mode);
  const windowCounts = useSyncExternalStore(subscribe, getWindowCounts);

  const items = useMemo(
    () => buildWorkspaceItems(workspaces, windowCounts),
    [workspaces, windowCounts],
  );

  return (
    <div className="workspaces-wrap">
      <div
        id="workspaces"
        className="workspaces-grid"
        role="group"
        aria-label="Workspaces"
      >
        {items.map((ws) => {
          const classes = ["ws-btn"];
          if (ws.focused) classes.push("focused");
          if (ws.virtual) classes.push("virtual");
          if (ws.urgent) classes.push("urgent");
          if (ws.visible && !ws.focused) classes.push("visible");

          return (
            <button
              key={ws.name}
              type="button"
              className={classes.join(" ")}
              style={{ gridRow: ws.row, gridColumn: ws.col }}
              title={
                ws.output
                  ? `${ws.name} (${ws.output}) — ${ws.windows} window(s)`
                  : `Workspace ${ws.name}`
              }
              onClick={() => switchWorkspace(ws.name)}
            >
              {ws.name}
              {ws.windows > 0 && (
                <span className="ws-dots" aria-hidden="true">
                  {Array.from({ length: Math.min(3, ws.windows) }, (_, i) => (
                    <i key={i} />
                  ))}
                </span>
              )}
            </button>
          );
        })}
      </div>
      {mode && mode !== "default" && (
        <span className="sway-mode" title={`Sway mode: ${mode}`}>
          {mode}
        </span>
      )}
    </div>
  );
}

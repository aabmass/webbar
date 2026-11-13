import { useEffect, useMemo, useState } from "react";
import type { SwayState, SwayWorkspace } from "../global";

type WorkspaceSlot = {
  row: 1 | 2;
  col: number;
  alwaysShow?: boolean;
};

const WORKSPACE_CONFIG: Record<string, WorkspaceSlot> = {
  "1": { row: 1, col: 1, alwaysShow: true },
  "2": { row: 1, col: 2, alwaysShow: true },
  "3": { row: 1, col: 3, alwaysShow: true },
  "4": { row: 1, col: 4, alwaysShow: true },
  "5": { row: 1, col: 5 },
  "6": { row: 1, col: 6 },
  "7": { row: 1, col: 7 },
  "8": { row: 1, col: 8 },
  "9": { row: 1, col: 9 },
  "0": { row: 1, col: 10 },
  q: { row: 2, col: 1, alwaysShow: true },
  w: { row: 2, col: 2, alwaysShow: true },
  e: { row: 2, col: 3, alwaysShow: true },
  r: { row: 2, col: 4, alwaysShow: true },
  t: { row: 2, col: 5 },
  y: { row: 2, col: 6 },
  u: { row: 2, col: 7 },
  i: { row: 2, col: 8 },
  o: { row: 2, col: 9 },
  p: { row: 2, col: 10 },
};

type WorkspaceItem = {
  name: string;
  focused: boolean;
  visible: boolean;
  urgent: boolean;
  virtual: boolean;
  output: string;
  row: number;
  col: number;
};

export function Workspaces() {
  const [swayState, setSwayState] = useState<SwayState>({
    workspaces: [],
    focused_title: "",
    mode: "default",
  });

  useEffect(() => {
    if (!window.sway) return;
    return window.sway.onState(setSwayState);
  }, []);

  const items = useMemo<WorkspaceItem[]>(() => {
    const byName = new Map<string, SwayWorkspace>();
    for (const ws of swayState.workspaces) {
      byName.set(ws.name, ws);
    }

    const result: WorkspaceItem[] = [];
    for (const [name, cfg] of Object.entries(WORKSPACE_CONFIG)) {
      const ws = byName.get(name);
      if (ws) {
        result.push({
          name,
          focused: ws.focused,
          visible: ws.visible,
          urgent: ws.urgent,
          virtual: false,
          output: ws.output,
          row: cfg.row,
          col: cfg.col,
        });
      } else if (cfg.alwaysShow) {
        result.push({
          name,
          focused: false,
          visible: false,
          urgent: false,
          virtual: true,
          output: "",
          row: cfg.row,
          col: cfg.col,
        });
      }
    }

    // Any extra workspaces not in WORKSPACE_CONFIG get appended on row 1
    let extraCol = 11;
    for (const ws of swayState.workspaces) {
      if (!WORKSPACE_CONFIG[ws.name]) {
        result.push({
          name: ws.name,
          focused: ws.focused,
          visible: ws.visible,
          urgent: ws.urgent,
          virtual: false,
          output: ws.output,
          row: 1,
          col: extraCol++,
        });
      }
    }

    return result;
  }, [swayState.workspaces]);

  return (
    <div className="workspaces-wrap">
      <div id="workspaces" className="workspaces-grid" role="group" aria-label="Workspaces">
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
              title={ws.output ? `${ws.name} (${ws.output})` : `Workspace ${ws.name}`}
              onClick={() => {
                const escaped = ws.name.replace(/"/g, '\\"');
                window.sway?.command(`workspace "${escaped}"`);
              }}
            >
              {ws.name}
            </button>
          );
        })}
      </div>
      {swayState.mode && swayState.mode !== "default" && (
        <span className="sway-mode" title={`Sway mode: ${swayState.mode}`}>
          {swayState.mode}
        </span>
      )}
    </div>
  );
}

import type { SwayNode, SwayWorkspace } from "@webbar/types";

export type WorkspaceSlot = {
  row: 1 | 2;
  col: number;
  alwaysShow?: boolean;
};

export const WORKSPACE_CONFIG: Record<string, WorkspaceSlot> = {
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

export type WorkspaceItem = {
  name: string;
  focused: boolean;
  visible: boolean;
  urgent: boolean;
  virtual: boolean;
  output: string;
  windows: number;
  row: number;
  col: number;
};

export function countWorkspaceWindows(tree: SwayNode): Map<string, number> {
  const counts = new Map<string, number>();

  const countWindows = (node: SwayNode): number => {
    const children = [...node.nodes, ...node.floating_nodes];
    if (children.length === 0) {
      const isCon = node.type === "con" || node.type === "floating_con";
      return isCon && node.pid !== null ? 1 : 0;
    }
    return children.reduce((sum, child) => sum + countWindows(child), 0);
  };

  const walk = (node: SwayNode): void => {
    if (node.type === "workspace") {
      if (node.name !== null && node.name !== "__i3_scratch") {
        counts.set(node.name, countWindows(node));
      }
      return;
    }
    node.nodes.forEach(walk);
  };

  walk(tree);
  return counts;
}

let prevTree: SwayNode | null = null;
let prevCounts = new Map<string, number>();

export function getWindowCounts(): Map<string, number> {
  const tree = window.webbar.sway.get().tree;
  if (tree === prevTree) return prevCounts;
  prevTree = tree;
  const next = tree ? countWorkspaceWindows(tree) : new Map<string, number>();
  if (
    next.size !== prevCounts.size ||
    [...next].some(([k, v]) => prevCounts.get(k) !== v)
  ) {
    prevCounts = next;
  }
  return prevCounts;
}

export function buildWorkspaceItems(
  workspaces: SwayWorkspace[],
  windowCounts: Map<string, number>,
): WorkspaceItem[] {
  const byName = new Map<string, SwayWorkspace>();
  for (const ws of workspaces) {
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
        windows: windowCounts.get(name) ?? 0,
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
        windows: 0,
        row: cfg.row,
        col: cfg.col,
      });
    }
  }

  // Any extra workspaces not in WORKSPACE_CONFIG get appended on row 1
  let extraCol = 11;
  for (const ws of workspaces) {
    if (!WORKSPACE_CONFIG[ws.name]) {
      result.push({
        name: ws.name,
        focused: ws.focused,
        visible: ws.visible,
        urgent: ws.urgent,
        virtual: false,
        output: ws.output,
        windows: windowCounts.get(ws.name) ?? 0,
        row: 1,
        col: extraCol++,
      });
    }
  }

  return result;
}

export function switchWorkspace(name: string): void {
  const escaped = name.replace(/[\\"]/g, "\\$&");
  window.webbar.sway
    .command(`workspace "${escaped}"`)
    .catch((err) => console.warn("workspace switch failed:", err));
}

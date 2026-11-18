// Pure helpers over Sway's own IPC types (generated into ./generated/sway).

import type { SwayCommandError as SwayCommandErrorData } from "./generated/SwayCommandError";
import type { SwayNode } from "./generated/sway";

/** Rejection reason of `webbar.sway.command()` and friends. */
export class SwayCommandError extends Error {
  /** True when Sway could not parse the command (vs. it failing to run). */
  readonly parseError: boolean;

  constructor(data: SwayCommandErrorData) {
    super(data.message);
    this.name = "SwayCommandError";
    this.parseError = data.parse_error;
  }
}

/**
 * Quote a value as a single Sway command argument, e.g. a workspace name.
 * `swayQuote('a "b"')` -> `"a \"b\""`.
 */
export function swayQuote(value: string): string {
  return `"${value.replace(/[\\"]/g, (c) => `\\${c}`)}"`;
}

/** Title of the focused container (or name of the focused empty workspace). */
export function swayFocusedTitle(tree: SwayNode): string {
  const stack: SwayNode[] = [tree];
  while (stack.length > 0) {
    const node = stack.pop()!;
    if (node.focused) return node.name ?? "";
    stack.push(...node.nodes, ...node.floating_nodes);
  }
  return "";
}

/** Number of application windows (tiled + floating) per workspace name. */
export function swayWindowCounts(tree: SwayNode): Map<string, number> {
  const counts = new Map<string, number>();

  const countWindows = (node: SwayNode): number => {
    const children = [...node.nodes, ...node.floating_nodes];
    if (children.length === 0) {
      const isCon = node.type === "con" || node.type === "floating_con";
      return isCon && node.pid !== null && node.pid !== undefined ? 1 : 0;
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

/** Placeholder tree used until the first snapshot arrives. */
export const EMPTY_SWAY_TREE: SwayNode = {
  id: 0,
  name: "root",
  type: "root",
  border: "none",
  current_border_width: 0,
  layout: "none",
  orientation: "none",
  percent: null,
  rect: { x: 0, y: 0, width: 0, height: 0 },
  window_rect: { x: 0, y: 0, width: 0, height: 0 },
  deco_rect: { x: 0, y: 0, width: 0, height: 0 },
  geometry: { x: 0, y: 0, width: 0, height: 0 },
  urgent: false,
  focused: false,
  focus: [],
  floating: null,
  nodes: [],
  floating_nodes: [],
  sticky: false,
  representation: null,
  fullscreen_mode: null,
  scratchpad_state: null,
  app_id: null,
  pid: null,
  window: null,
  num: null,
  window_properties: null,
  marks: [],
  inhibit_idle: null,
  idle_inhibitors: null,
  sandbox_engine: null,
  sandbox_app_id: null,
  sandbox_instance_id: null,
  tag: null,
  shell: null,
  foreign_toplevel_identifier: null,
  visible: null,
  output: null,
};

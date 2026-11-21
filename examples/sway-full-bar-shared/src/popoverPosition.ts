export type PopoverCaret = "center" | "end" | "none";

export type PopoverPosition = {
  caret: PopoverCaret;
  top: number;
  left?: number;
  right?: number;
  transform?: string;
  caretRight?: string;
};

const VIEWPORT_MARGIN = 8;

export function focusedWorkspaceKey(): string {
  const s = window.webbar.sway.get();
  const focused = s.workspaces.find((w) => w.focused);
  return `${focused?.id ?? ""}:${s.mode}`;
}

export function computePopoverPosition(
  anchorEl: HTMLElement | null,
  panelEl: HTMLElement | null,
  align: "center" | "end",
): PopoverPosition {
  const rect = anchorEl?.getBoundingClientRect();
  // Open just below the bar (not the button) so the caret has room to show.
  const barBottom =
    anchorEl?.closest(".centerbox")?.getBoundingClientRect().bottom ??
    rect?.bottom ??
    80;
  const top = Math.round(barBottom + 10);
  const vw = window.innerWidth;
  const panelWidth = panelEl?.getBoundingClientRect().width ?? 0;

  if (align === "center" && rect) {
    const anchorCenter = rect.left + rect.width / 2;
    if (panelWidth > 0) {
      const idealLeft = anchorCenter - panelWidth / 2;
      const maxLeft = Math.max(
        VIEWPORT_MARGIN,
        vw - panelWidth - VIEWPORT_MARGIN,
      );
      const clampedLeft = Math.round(
        Math.min(Math.max(VIEWPORT_MARGIN, idealLeft), maxLeft),
      );
      return {
        caret: "center",
        top,
        left: clampedLeft,
      };
    }
    return {
      caret: "center",
      top,
      left: Math.round(anchorCenter),
      transform: "translateX(-50%)",
    };
  }

  if (anchorEl && rect) {
    // Derive the right edge from offsetWidth around the center so CSS :active
    // scale() on the anchor button doesn't shift the popover between clicks.
    const anchorCenter = rect.left + rect.width / 2;
    const unscaledRight = anchorCenter + anchorEl.offsetWidth / 2;
    let right = Math.max(VIEWPORT_MARGIN, Math.round(vw - unscaledRight));
    if (panelWidth > 0) {
      const maxRight = Math.max(
        VIEWPORT_MARGIN,
        vw - panelWidth - VIEWPORT_MARGIN,
      );
      right = Math.min(right, Math.round(maxRight));
    }
    const caretRight = vw - right - anchorCenter;
    return {
      caret: "end",
      top,
      right,
      caretRight: `${Math.max(12, Math.round(caretRight))}px`,
    };
  }

  return { caret: "none", top, right: 12 };
}

export function popoverPositionToCss(pos: PopoverPosition): string {
  const parts: string[] = [`top: ${pos.top}px`];
  if (pos.left !== undefined) parts.push(`left: ${pos.left}px`);
  if (pos.right !== undefined) parts.push(`right: ${pos.right}px`);
  if (pos.transform) parts.push(`transform: ${pos.transform}`);
  if (pos.caretRight) parts.push(`--caret-right: ${pos.caretRight}`);
  return parts.join("; ");
}

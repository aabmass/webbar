import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react";

type PopoverProps = {
  open: boolean;
  onClose: () => void;
  anchorRef: RefObject<HTMLElement | null>;
  align?: "center" | "end";
  children: ReactNode;
};

type PositionState = {
  style: CSSProperties;
  caret: "center" | "end" | "none";
};

const VIEWPORT_MARGIN = 8;

function focusedWorkspaceKey(): string {
  const s = window.webbar.sway.get();
  const focused = s.workspaces.find((w) => w.focused);
  return `${focused?.id ?? ""}:${s.mode}`;
}

function computePosition(
  anchorEl: HTMLElement | null,
  panelEl: HTMLElement | null,
  align: "center" | "end",
): PositionState {
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
      const maxLeft = Math.max(VIEWPORT_MARGIN, vw - panelWidth - VIEWPORT_MARGIN);
      const clampedLeft = Math.round(
        Math.min(Math.max(VIEWPORT_MARGIN, idealLeft), maxLeft),
      );
      return {
        caret: "center",
        style: { top, left: clampedLeft },
      };
    }
    return {
      caret: "center",
      style: {
        top,
        left: Math.round(anchorCenter),
        transform: "translateX(-50%)",
      },
    };
  }

  if (anchorEl && rect) {
    // Derive the right edge from offsetWidth around the center so CSS :active
    // scale() on the anchor button doesn't shift the popover between clicks.
    const anchorCenter = rect.left + rect.width / 2;
    const unscaledRight = anchorCenter + anchorEl.offsetWidth / 2;
    let right = Math.max(VIEWPORT_MARGIN, Math.round(vw - unscaledRight));
    if (panelWidth > 0) {
      const maxRight = Math.max(VIEWPORT_MARGIN, vw - panelWidth - VIEWPORT_MARGIN);
      right = Math.min(right, Math.round(maxRight));
    }
    const caretRight = vw - right - anchorCenter;
    return {
      caret: "end",
      style: {
        top,
        right,
        ["--caret-right" as string]: `${Math.max(12, Math.round(caretRight))}px`,
      },
    };
  }

  return { caret: "none", style: { top, right: 12 } };
}

export function Popover({
  open,
  onClose,
  anchorRef,
  align = "end",
  children,
}: PopoverProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<PositionState>({
    caret: "none",
    style: { top: 90, right: 12 },
  });

  useLayoutEffect(() => {
    if (!open) return;
    const panel = panelRef.current;
    if (!panel || typeof panel.showPopover !== "function") return;
    try {
      panel.showPopover();
    } catch {
      // Ignore if already open in top layer
    }

    const updatePosition = () => {
      setPosition(computePosition(anchorRef.current, panelRef.current, align));
    };
    updatePosition();
    window.addEventListener("resize", updatePosition);

    return () => {
      window.removeEventListener("resize", updatePosition);
      try {
        if (panel.matches(":popover-open")) {
          panel.hidePopover();
        }
      } catch {
        // Ignore if already detached or hidden
      }
    };
  }, [open, anchorRef, align]);

  useEffect(() => {
    if (!open) return;

    const releaseOverlay = window.webbar.acquireOverlay();
    const initialSwayKey = focusedWorkspaceKey();

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };

    const onPointerDown = (e: PointerEvent) => {
      const target = e.target;
      if (
        target instanceof Node &&
        (panelRef.current?.contains(target) ||
          anchorRef.current?.contains(target))
      ) {
        return;
      }
      onClose();
    };

    const onWindowBlur = () => {
      onClose();
    };

    const unsubSway = window.webbar.sway.subscribe(() => {
      if (focusedWorkspaceKey() !== initialSwayKey) {
        onClose();
      }
    });

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", onWindowBlur);

    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("blur", onWindowBlur);
      unsubSway();
      releaseOverlay();
    };
  }, [open, onClose, anchorRef]);

  if (!open) return null;

  return (
    <div
      ref={panelRef}
      popover="manual"
      className={`popover-panel caret-${position.caret}`}
      style={position.style}
      role="dialog"
    >
      {children}
    </div>
  );
}

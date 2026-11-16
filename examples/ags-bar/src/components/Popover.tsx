import {
  useEffect,
  useLayoutEffect,
  useRef,
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

function focusedWorkspaceKey(): string {
  const s = window.webbar.sway.get();
  const focused = s.workspaces.find((w) => w.focused);
  return `${focused?.id ?? ""}:${s.mode}`;
}

export function Popover({
  open,
  onClose,
  anchorRef,
  align = "end",
  children,
}: PopoverProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!open) return;
    const panel = panelRef.current;
    if (!panel || typeof panel.showPopover !== "function") return;
    try {
      panel.showPopover();
    } catch {
      // Ignore if already open in top layer
    }
    return () => {
      try {
        if (panel.matches(":popover-open")) {
          panel.hidePopover();
        }
      } catch {
        // Ignore if already detached or hidden
      }
    };
  }, [open]);

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

  const rect = anchorRef.current?.getBoundingClientRect();
  const top = rect ? Math.round(rect.bottom + 8) : 88;
  const style: React.CSSProperties =
    align === "center" && rect
      ? {
          top,
          left: Math.round(rect.left + rect.width / 2),
          transform: "translateX(-50%)",
        }
      : rect
        ? {
            top,
            right: Math.max(8, Math.round(window.innerWidth - rect.right)),
          }
        : { top, right: 12 };

  return (
    <div
      ref={panelRef}
      popover="manual"
      className="popover-panel"
      style={style}
      role="dialog"
    >
      {children}
    </div>
  );
}

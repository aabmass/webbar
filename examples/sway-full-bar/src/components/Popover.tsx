import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react";
import {
  computePopoverPosition,
  focusedWorkspaceKey,
  type PopoverPosition,
} from "@webbar/sway-full-bar-shared/popoverPosition";

type PopoverProps = {
  open: boolean;
  onClose: () => void;
  anchorRef: RefObject<HTMLElement | null>;
  align?: "center" | "end";
  children: ReactNode;
};

function toReactStyle(pos: PopoverPosition): CSSProperties {
  const style: CSSProperties = { top: pos.top };
  if (pos.left !== undefined) style.left = pos.left;
  if (pos.right !== undefined) style.right = pos.right;
  if (pos.transform) style.transform = pos.transform;
  if (pos.caretRight) {
    (style as Record<string, string>)["--caret-right"] = pos.caretRight;
  }
  return style;
}

export function Popover({
  open,
  onClose,
  anchorRef,
  align = "end",
  children,
}: PopoverProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<PopoverPosition>({
    caret: "none",
    top: 90,
    right: 12,
  });

  useLayoutEffect(() => {
    if (!open) return;
    const panel = panelRef.current;
    if (!panel || typeof panel.showPopover !== "function") return;
    const releaseOverlay = window.webbar.acquireOverlay();
    try {
      panel.showPopover();
    } catch {
      // Ignore if already open in top layer
    }

    const updatePosition = () => {
      setPosition(
        computePopoverPosition(anchorRef.current, panelRef.current, align),
      );
    };
    updatePosition();
    window.addEventListener("resize", updatePosition);

    return () => {
      releaseOverlay();
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
    };
  }, [open, onClose, anchorRef]);

  if (!open) return null;

  return (
    <div
      ref={panelRef}
      popover="manual"
      className={`popover-panel caret-${position.caret}`}
      style={toReactStyle(position)}
      role="dialog"
    >
      {children}
    </div>
  );
}

import { useEffect, type ReactNode, type RefObject } from "react";

type PopoverProps = {
  open: boolean;
  onClose: () => void;
  anchorRef: RefObject<HTMLElement | null>;
  align?: "center" | "end";
  children: ReactNode;
};

export function Popover({
  open,
  onClose,
  anchorRef,
  align = "end",
  children,
}: PopoverProps) {
  useEffect(() => {
    if (!open) return;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

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
      className="popover-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) {
          onClose();
        }
      }}
    >
      <div className="popover-panel" style={style} role="dialog">
        {children}
      </div>
    </div>
  );
}

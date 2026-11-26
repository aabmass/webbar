import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  activePopups,
  prefersReducedMotion,
  reconcileToastEntries,
  TOAST_EXIT_FALLBACK_MS,
  type ToastEntry,
} from "@webbar/sway-full-bar-shared/notifications";
import { NotificationCard } from "./NotificationCard";

export function NotificationToasts({ hidden }: { hidden: boolean }) {
  const notifState = useSyncExternalStore(
    window.webbar.notifications.subscribe,
    window.webbar.notifications.get,
  );
  const focused = useSyncExternalStore(
    window.webbar.focused.subscribe,
    window.webbar.focused.get,
  );
  const [nowMs, setNowMs] = useState(() => Date.now());
  const canShow = !hidden && focused;
  const [entries, setEntries] = useState<ToastEntry[]>(() =>
    canShow
      ? activePopups(notifState.items).map((item) => ({ item, exiting: false }))
      : [],
  );
  const timersRef = useRef(new Map<number, number>());
  const stackRef = useRef<HTMLDivElement | null>(null);
  const slotRefs = useRef(new Map<number, HTMLDivElement>());
  const prevTopsRef = useRef(new Map<number, number>());

  const removeEntry = useCallback((id: number) => {
    const timer = timersRef.current.get(id);
    if (timer !== undefined) {
      window.clearTimeout(timer);
      timersRef.current.delete(id);
    }
    setEntries((prev) =>
      prev.filter((e) => !(e.item.id === id && e.exiting)),
    );
  }, []);

  useEffect(() => {
    if (!canShow) {
      for (const timer of timersRef.current.values()) {
        window.clearTimeout(timer);
      }
      timersRef.current.clear();
      setEntries([]);
      return;
    }
    const popups = activePopups(notifState.items);
    setEntries((prev) => reconcileToastEntries(prev, popups));
  }, [canShow, notifState.items]);

  useEffect(() => {
    const exitingIds = new Set(
      entries.filter((e) => e.exiting).map((e) => e.item.id),
    );
    for (const [id, timer] of timersRef.current) {
      if (!exitingIds.has(id)) {
        window.clearTimeout(timer);
        timersRef.current.delete(id);
      }
    }
    const delay = prefersReducedMotion() ? 50 : TOAST_EXIT_FALLBACK_MS;
    for (const id of exitingIds) {
      if (!timersRef.current.has(id)) {
        const timer = window.setTimeout(() => removeEntry(id), delay);
        timersRef.current.set(id, timer);
      }
    }
  }, [entries, removeEntry]);

  useEffect(() => {
    const timers = timersRef.current;
    return () => {
      for (const timer of timers.values()) {
        window.clearTimeout(timer);
      }
      timers.clear();
    };
  }, []);

  const show = canShow && entries.length > 0;

  useLayoutEffect(() => {
    if (!show) {
      prevTopsRef.current.clear();
      return;
    }
    const nextTops = new Map<number, number>();
    const reduced = prefersReducedMotion();

    for (const entry of entries) {
      const id = entry.item.id;
      const el = slotRefs.current.get(id);
      if (!el) continue;
      const nextTop = el.offsetTop;
      nextTops.set(id, nextTop);

      if (!entry.exiting && !reduced) {
        const prevTop = prevTopsRef.current.get(id);
        if (prevTop !== undefined) {
          const dy = prevTop - nextTop;
          if (Math.abs(dy) >= 1) {
            el.animate(
              [{ transform: `translateY(${dy}px)` }, { transform: "translateY(0)" }],
              { duration: 300, easing: "cubic-bezier(0.33, 1, 0.68, 1)" },
            );
          }
        }
      }
    }
    prevTopsRef.current = nextTops;
  }, [entries, show]);

  useEffect(() => {
    if (!show) return;
    setNowMs(Date.now());
    const timer = window.setInterval(() => setNowMs(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, [show]);

  useLayoutEffect(() => {
    const stack = stackRef.current;
    if (!show || !stack) return;
    try {
      if (!stack.matches(":popover-open")) {
        stack.showPopover();
      }
    } catch {
      // Fallback for environments without Popover API
    }
  }, [show]);

  if (!show) return null;

  return (
    <div
      ref={stackRef}
      className="notif-toast-stack"
      popover="manual"
      role="region"
      aria-label="Notification popups"
    >
      {entries.map((entry) => (
        <div
          key={entry.item.id}
          ref={(el) => {
            if (el) {
              slotRefs.current.set(entry.item.id, el);
            } else {
              slotRefs.current.delete(entry.item.id);
            }
          }}
          className={`notif-toast-slot${entry.exiting ? " is-exiting" : ""}`}
          onAnimationEnd={(e) => {
            if (
              entry.exiting &&
              e.target === e.currentTarget &&
              e.animationName === "notif-toast-out"
            ) {
              removeEntry(entry.item.id);
            }
          }}
        >
          <NotificationCard
            item={entry.item}
            nowMs={nowMs}
            isToast
            exiting={entry.exiting}
          />
        </div>
      ))}
    </div>
  );
}

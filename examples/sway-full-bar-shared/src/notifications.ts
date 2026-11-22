import type { NotificationItem, NotificationsState } from "@webbar/types";

export function formatRelativeTime(timestampMs: number, nowMs: number): string {
  const diffSec = Math.max(0, Math.floor((nowMs - timestampMs) / 1000));
  if (diffSec < 60) return "now";
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h`;
  return `${Math.floor(diffHr / 24)}d`;
}

export const TOAST_EXIT_MS = 1500;
export const TOAST_EXIT_FALLBACK_MS = 1650;

export type ToastEntry = {
  item: NotificationItem;
  exiting: boolean;
};

export function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false)
  );
}

export function reconcileToastEntries(
  prev: ToastEntry[],
  popups: NotificationItem[],
): ToastEntry[] {
  const popupById = new Map(popups.map((p) => [p.id, p]));
  const prevIds = new Set(prev.map((e) => e.item.id));
  const next: ToastEntry[] = [];

  for (const popup of popups) {
    if (!prevIds.has(popup.id)) {
      next.push({ item: popup, exiting: false });
    }
  }

  for (const entry of prev) {
    const active = popupById.get(entry.item.id);
    if (active) {
      next.push({ item: active, exiting: false });
    } else {
      next.push({ item: entry.item, exiting: true });
    }
  }

  return next;
}

export function activePopups(
  items: NotificationItem[],
  limit = 4,
): NotificationItem[] {
  return items.filter((item) => item.popup).slice(0, limit);
}

export function notificationButtonTitle(state: NotificationsState): string {
  if (!state.available) {
    return "Notifications unavailable (another daemon is running) — click to open";
  }
  if (state.do_not_disturb) {
    return state.unread_count > 0
      ? `Notifications: Do Not Disturb (${state.unread_count} unread) — click for center, right-click to enable`
      : "Notifications: Do Not Disturb — click for center, right-click to enable";
  }
  if (state.unread_count > 0) {
    return `Notifications: ${state.unread_count} unread — click for center, right-click for Do Not Disturb`;
  }
  return "Notifications — click for center, right-click for Do Not Disturb";
}

export function notificationCenterSubtitle(
  doNotDisturb: boolean,
  count: number,
): string {
  if (doNotDisturb) return "Do Not Disturb";
  if (count === 0) return "All caught up";
  return `${count} ${count === 1 ? "notification" : "notifications"}`;
}

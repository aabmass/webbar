import type {
  NotificationAction,
  NotificationItem,
  NotificationsState,
  SwayState,
} from "@webbar/types";

export function isFocusedOutput(sway: SwayState, output: string): boolean {
  if (!output) return true;
  const focusedOutput = sway.workspaces.find((ws) => ws.focused)?.output;
  return !focusedOutput || focusedOutput === output;
}

export function formatRelativeTime(timestampMs: number, nowMs: number): string {
  const diffSec = Math.max(0, Math.floor((nowMs - timestampMs) / 1000));
  if (diffSec < 60) return "now";
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h`;
  return `${Math.floor(diffHr / 24)}d`;
}

export function activePopups(
  items: NotificationItem[],
  limit = 4,
): NotificationItem[] {
  const popups: NotificationItem[] = [];
  for (let i = items.length - 1; i >= 0 && popups.length < limit; i--) {
    if (items[i].popup) {
      popups.push(items[i]);
    }
  }
  return popups;
}

export function historyItems(items: NotificationItem[]): NotificationItem[] {
  return [...items].reverse();
}

export function hasDefaultAction(item: NotificationItem): boolean {
  return item.actions.some((a) => a.key === "default");
}

export function visibleActions(item: NotificationItem): NotificationAction[] {
  return item.actions.filter((a) => a.key !== "default");
}

export function notificationButtonTitle(state: NotificationsState): string {
  if (!state.available) {
    return "Notifications unavailable (another daemon is running) — click to open";
  }
  if (state.dnd) {
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
  dnd: boolean,
  count: number,
): string {
  if (dnd) return "Do Not Disturb";
  if (count === 0) return "All caught up";
  return `${count} ${count === 1 ? "notification" : "notifications"}`;
}

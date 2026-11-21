import type { TrayItem, TrayMenuItem } from "@webbar/types";

export function trayShortLabel(item: TrayItem): string {
  const raw = item.title || item.icon_name || item.bus;
  return raw.length > 14 ? `${raw.slice(0, 12)}…` : raw;
}

export function normalizeMenuItems(items: TrayMenuItem[]): TrayMenuItem[] {
  const result: TrayMenuItem[] = [];
  for (const item of items) {
    if (item.item_type === "separator") {
      if (
        result.length === 0 ||
        result[result.length - 1].item_type === "separator"
      ) {
        continue;
      }
    }
    result.push(item);
  }
  while (
    result.length > 0 &&
    result[result.length - 1].item_type === "separator"
  ) {
    result.pop();
  }
  return result;
}

export function endsWithEllipsis(label: string): boolean {
  const trimmed = label.trimEnd();
  return trimmed.endsWith("...") || trimmed.endsWith("…");
}

export function isPlainDisabled(entry: TrayMenuItem): boolean {
  return (
    !entry.enabled &&
    entry.children.length === 0 &&
    !entry.toggle_type &&
    !entry.icon_url
  );
}

export type ClassifiedMenuEntry =
  | { kind: "separator"; key: string; entry: TrayMenuItem }
  | { kind: "section"; key: string; entry: TrayMenuItem }
  | { kind: "status"; key: string; entry: TrayMenuItem }
  | { kind: "item"; key: string; entry: TrayMenuItem };

export function classifyMenuItems(
  items: TrayMenuItem[],
): ClassifiedMenuEntry[] {
  const normalized = normalizeMenuItems(items);
  let prevWasSectionHeader = false;
  const result: ClassifiedMenuEntry[] = [];

  for (let idx = 0; idx < normalized.length; idx++) {
    const entry = normalized[idx];
    if (entry.item_type === "separator") {
      prevWasSectionHeader = false;
      result.push({
        kind: "separator",
        key: `sep-${entry.id}-${idx}`,
        entry,
      });
      continue;
    }

    const prev = idx > 0 ? normalized[idx - 1] : null;
    const next = idx + 1 < normalized.length ? normalized[idx + 1] : null;
    const isGroupStart = !prev || prev.item_type === "separator";
    const hasNextInGroup = Boolean(next && next.item_type !== "separator");

    if (isPlainDisabled(entry)) {
      const isSectionHeader =
        isGroupStart && hasNextInGroup && !endsWithEllipsis(entry.label);
      if (isSectionHeader) {
        prevWasSectionHeader = true;
        result.push({ kind: "section", key: String(entry.id), entry });
        continue;
      }

      if (prevWasSectionHeader || normalized.length === 1) {
        prevWasSectionHeader = false;
        result.push({ kind: "status", key: String(entry.id), entry });
        continue;
      }
    }

    prevWasSectionHeader = false;
    result.push({ kind: "item", key: String(entry.id), entry });
  }

  return result;
}

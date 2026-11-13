import { useCallback, useEffect, useRef, useState } from "react";
import type { TrayItem, TrayMenuItem } from "../global";
import { Popover } from "./Popover";

function trayShortLabel(item: TrayItem): string {
  const raw = item.title || item.icon_name || item.bus;
  return raw.length > 14 ? `${raw.slice(0, 12)}…` : raw;
}

function MenuList({
  items,
  onSelect,
}: {
  items: TrayMenuItem[];
  onSelect: (id: number) => void;
}) {
  const [expandedId, setExpandedId] = useState<number | null>(null);

  return (
    <div className="tray-menu-list">
      {items.map((entry) => {
        if (entry.item_type === "separator") {
          return <div key={entry.id} className="tray-menu-sep" />;
        }

        const hasChildren = entry.children.length > 0;
        const isExpanded = expandedId === entry.id;
        const checkPrefix =
          entry.toggle_type === "checkmark"
            ? entry.toggle_state === 1
              ? "✓ "
              : "  "
            : entry.toggle_type === "radio"
              ? entry.toggle_state === 1
                ? "● "
                : "○ "
              : "";

        return (
          <div key={entry.id} className="tray-menu-entry">
            <button
              type="button"
              className={`tray-menu-item${!entry.enabled ? " is-disabled" : ""}`}
              disabled={!entry.enabled}
              onClick={() => {
                if (hasChildren) {
                  setExpandedId(isExpanded ? null : entry.id);
                } else {
                  onSelect(entry.id);
                }
              }}
            >
              <span className="tray-menu-label">
                {checkPrefix}
                {entry.label}
              </span>
              {hasChildren && (
                <span className="tray-menu-arrow">{isExpanded ? "▾" : "▸"}</span>
              )}
            </button>
            {hasChildren && isExpanded && (
              <div className="tray-menu-sub">
                <MenuList items={entry.children} onSelect={onSelect} />
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

function TrayButton({ item }: { item: TrayItem }) {
  const [open, setOpen] = useState(false);
  const [menuItems, setMenuItems] = useState<TrayMenuItem[]>([]);
  const [loading, setLoading] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

  const label = item.title || item.icon_name || item.bus;
  const hasMenu = Boolean(item.menu_path && window.tray?.getMenu);

  async function handleClick() {
    if (!hasMenu || !item.menu_path || !window.tray?.getMenu) {
      window.tray?.activate(item.bus, item.path);
      return;
    }

    if (open) {
      setOpen(false);
      return;
    }

    setOpen(true);
    setLoading(true);
    try {
      const items = await window.tray.getMenu(item.bus, item.menu_path);
      setMenuItems(items);
    } catch {
      setMenuItems([]);
      setOpen(false);
      window.tray?.activate(item.bus, item.path);
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={`bar-btn tray-item${item.icon_url ? " icon-btn" : ""}${open ? " is-open" : ""}`}
        title={`${label} (${item.bus}${item.path})`}
        onClick={() => void handleClick()}
        onContextMenu={(e) => {
          e.preventDefault();
          window.tray?.activate(item.bus, item.path);
        }}
      >
        {item.icon_url ? (
          <img className="tray-icon" src={item.icon_url} alt={label} />
        ) : (
          trayShortLabel(item)
        )}
      </button>

      {hasMenu && (
        <Popover open={open} onClose={close} anchorRef={btnRef} align="end">
          <div className="tray-menu">
            {loading && menuItems.length === 0 ? (
              <span className="drawer-empty">Loading…</span>
            ) : (
              <MenuList
                items={menuItems}
                onSelect={(itemId) => {
                  if (item.menu_path) {
                    window.tray?.menuEvent?.(item.bus, item.menu_path, itemId);
                  }
                  close();
                }}
              />
            )}
          </div>
        </Popover>
      )}
    </>
  );
}

export function Tray() {
  const [items, setItems] = useState<TrayItem[]>([]);

  useEffect(() => {
    if (!window.tray) return;
    return window.tray.onChange(setItems);
  }, []);

  if (items.length === 0) {
    return null;
  }

  return (
    <div className="tray" role="list" aria-label="System tray">
      {items.map((item) => {
        const key = item.id || `${item.bus}${item.path}`;
        return <TrayButton key={key} item={item} />;
      })}
    </div>
  );
}

import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import type { TrayItem, TrayMenuItem } from "@webbar/types";
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
  // Track expanded submenu by label rather than numeric id because apps like
  // nm-applet destroy and recreate menu items with fresh ids on LayoutUpdated.
  const [expandedLabel, setExpandedLabel] = useState<string | null>(null);

  return (
    <div className="tray-menu-list">
      {items.map((entry) => {
        if (entry.item_type === "separator") {
          return <div key={entry.id} className="tray-menu-sep" />;
        }

        const hasChildren = entry.children.length > 0;
        const isExpanded = expandedLabel === entry.label;
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
                  setExpandedLabel(isExpanded ? null : entry.label);
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

function TrayMenu({
  bus,
  menuPath,
  onSelect,
}: {
  bus: string;
  menuPath: string;
  onSelect: (id: number) => void;
}) {
  const menuStore = window.webbar.tray.menu(bus, menuPath);
  const items = useSyncExternalStore(menuStore.subscribe, menuStore.get);

  return (
    <div className="tray-menu">
      <MenuList items={items} onSelect={onSelect} />
    </div>
  );
}

function TrayButton({ item }: { item: TrayItem }) {
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);

  const label = item.title || item.icon_name || item.bus;
  const hasMenu = Boolean(item.menu_path);

  function handleClick() {
    if (!hasMenu || !item.menu_path) {
      window.webbar.tray.activate(item.bus, item.path);
      return;
    }
    setOpen((prev) => !prev);
  }

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={`bar-btn tray-item${item.icon_url ? " icon-btn" : ""}${open ? " is-open" : ""}`}
        title={`${label} (${item.bus}${item.path})`}
        onClick={handleClick}
        onContextMenu={(e) => {
          e.preventDefault();
          window.webbar.tray.activate(item.bus, item.path);
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
          {open && (
            <TrayMenu
              bus={item.bus}
              menuPath={item.menu_path}
              onSelect={(itemId) => {
                if (item.menu_path) {
                  window.webbar.tray.menuEvent(item.bus, item.menu_path, itemId);
                }
                close();
              }}
            />
          )}
        </Popover>
      )}
    </>
  );
}

export function Tray() {
  const items = useSyncExternalStore(
    window.webbar.tray.subscribe,
    window.webbar.tray.get,
  );

  if (items.length === 0) {
    return null;
  }

  return (
    <div className="tray" role="list" aria-label="System tray">
      {items.map((item) => {
        const key = `${item.bus}${item.path}`;
        return <TrayButton key={key} item={item} />;
      })}
    </div>
  );
}

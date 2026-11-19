import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import type { TrayItem, TrayMenuItem } from "@webbar/types";
import { Popover } from "./Popover";

function trayShortLabel(item: TrayItem): string {
  const raw = item.title || item.icon_name || item.bus;
  return raw.length > 14 ? `${raw.slice(0, 12)}…` : raw;
}

function normalizeMenuItems(items: TrayMenuItem[]): TrayMenuItem[] {
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

function ChevronIcon({ expanded }: { expanded: boolean }) {
  return (
    <svg
      className={`tray-menu-chevron${expanded ? " is-expanded" : ""}`}
      width="14"
      height="14"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M6 4l4 4-4 4" />
    </svg>
  );
}

function isPlainDisabled(entry: TrayMenuItem): boolean {
  return (
    !entry.enabled &&
    entry.children.length === 0 &&
    !entry.toggle_type &&
    !entry.icon_url
  );
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
  const normalized = normalizeMenuItems(items);

  return (
    <div className="tray-menu-list" role="menu">
      {normalized.map((entry, idx) => {
        if (entry.item_type === "separator") {
          return (
            <div
              key={`sep-${entry.id}-${idx}`}
              className="tray-menu-sep"
              role="separator"
            />
          );
        }

        const prev = idx > 0 ? normalized[idx - 1] : null;
        const isGroupStart = !prev || prev.item_type === "separator";

        if (isPlainDisabled(entry)) {
          if (isGroupStart) {
            return (
              <div
                key={entry.id}
                className="tray-menu-section"
                title={entry.accessible_desc || undefined}
              >
                {entry.label}
              </div>
            );
          }
          return (
            <div
              key={entry.id}
              className="tray-menu-status"
              title={entry.accessible_desc || undefined}
            >
              {entry.label}
            </div>
          );
        }

        const hasChildren = entry.children.length > 0;
        const isExpanded = expandedLabel === entry.label;
        const isCheckmark = entry.toggle_type === "checkmark";
        const isRadio = entry.toggle_type === "radio";
        const isChecked = entry.toggle_state === 1;
        const isSymbolicIcon =
          Boolean(entry.icon_name) && entry.icon_name.endsWith("-symbolic");

        return (
          <div key={entry.id} className="tray-menu-entry">
            <button
              type="button"
              role={
                isCheckmark
                  ? "menuitemcheckbox"
                  : isRadio
                    ? "menuitemradio"
                    : "menuitem"
              }
              aria-checked={isCheckmark || isRadio ? isChecked : undefined}
              aria-expanded={hasChildren ? isExpanded : undefined}
              className={[
                "tray-menu-item",
                !entry.enabled ? "is-disabled" : "",
                hasChildren && isExpanded ? "is-expanded" : "",
                (isCheckmark || isRadio) && isChecked ? "is-checked" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              disabled={!entry.enabled}
              title={entry.accessible_desc || undefined}
              onClick={() => {
                if (hasChildren) {
                  setExpandedLabel(isExpanded ? null : entry.label);
                } else {
                  onSelect(entry.id);
                }
              }}
            >
              <span className="tray-menu-leading">
                {isRadio && (
                  <span
                    className={`tray-menu-radio${isChecked ? " is-on" : ""}`}
                    aria-hidden="true"
                  />
                )}
                {entry.icon_url &&
                  (isSymbolicIcon ? (
                    <span
                      className="tray-menu-icon is-symbolic"
                      style={{
                        ["--icon-src" as string]: `url("${entry.icon_url}")`,
                      }}
                      aria-hidden="true"
                    />
                  ) : (
                    <img
                      className="tray-menu-icon"
                      src={entry.icon_url}
                      alt=""
                      aria-hidden="true"
                    />
                  ))}
                <span className="tray-menu-label">{entry.label}</span>
              </span>

              {isCheckmark && (
                <span
                  className={`tray-menu-switch${isChecked ? " is-on" : ""}`}
                  aria-hidden="true"
                />
              )}
              {hasChildren && <ChevronIcon expanded={isExpanded} />}
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
  item,
  label,
  onSelect,
}: {
  item: TrayItem;
  label: string;
  onSelect: (id: number) => void;
}) {
  const menuStore = window.webbar.tray.menu(item.bus, item.menu_path);
  const items = useSyncExternalStore(menuStore.subscribe, menuStore.get);
  const normalized = normalizeMenuItems(items);

  return (
    <div className="tray-menu">
      <div className="tray-menu-header">
        {item.icon_url && (
          <img
            className="tray-menu-header-icon"
            src={item.icon_url}
            alt=""
            aria-hidden="true"
          />
        )}
        <span className="tray-menu-header-title">{label}</span>
      </div>
      <div className="tray-menu-body">
        {normalized.length === 0 ? (
          <div className="drawer-empty">No menu items</div>
        ) : (
          <MenuList items={normalized} onSelect={onSelect} />
        )}
      </div>
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
              item={item}
              label={label}
              onSelect={(itemId) => {
                if (item.menu_path) {
                  window.webbar.tray.menuEvent(
                    item.bus,
                    item.menu_path,
                    itemId,
                  );
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

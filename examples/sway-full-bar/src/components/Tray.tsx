import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import type { TrayItem, TrayMenuItem } from "@webbar/types";
import {
  classifyMenuItems,
  normalizeMenuItems,
  trayShortLabel,
} from "@webbar/sway-full-bar-shared/tray";
import { Popover } from "./Popover";

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

function MenuIcon({
  iconUrl,
  iconName,
  className,
}: {
  iconUrl: string;
  iconName?: string;
  className: string;
}) {
  const isSymbolic = Boolean(iconName) && iconName!.endsWith("-symbolic");
  if (isSymbolic) {
    return (
      <span
        className={`${className} is-symbolic`}
        style={{ ["--icon-src" as string]: `url("${iconUrl}")` }}
        aria-hidden="true"
      />
    );
  }
  return <img className={className} src={iconUrl} alt="" aria-hidden="true" />;
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
  const classified = classifyMenuItems(items);

  return (
    <div className="tray-menu-list" role="menu">
      {classified.map(({ kind, key, entry }) => {
        if (kind === "separator") {
          return (
            <div key={key} className="tray-menu-sep" role="separator" />
          );
        }

        if (kind === "section") {
          return (
            <div
              key={key}
              className="tray-menu-section"
              title={entry.accessible_desc || undefined}
            >
              {entry.label}
            </div>
          );
        }

        if (kind === "status") {
          return (
            <div
              key={key}
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
                {entry.icon_url && (
                  <MenuIcon
                    iconUrl={entry.icon_url}
                    iconName={entry.icon_name}
                    className="tray-menu-icon"
                  />
                )}
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
          <MenuIcon
            iconUrl={item.icon_url}
            iconName={item.icon_name}
            className="tray-menu-header-icon"
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
          <MenuIcon
            iconUrl={item.icon_url}
            iconName={item.icon_name}
            className="tray-icon"
          />
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
    <div className="tray" role="group" aria-label="System tray">
      {items.map((item) => {
        const key = `${item.bus}${item.path}`;
        return <TrayButton key={key} item={item} />;
      })}
    </div>
  );
}

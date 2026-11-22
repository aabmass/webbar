import { useEffect, useRef } from "react";
import type { NotificationItem } from "@webbar/types";
import {
  formatRelativeTime,
  hasDefaultAction,
  visibleActions,
} from "@webbar/sway-full-bar-shared/notifications";

export function NotificationCard({
  item,
  nowMs,
  isToast = false,
}: {
  item: NotificationItem;
  nowMs: number;
  isToast?: boolean;
}) {
  const hoveredRef = useRef(false);
  const hasDefault = hasDefaultAction(item);
  const actions = visibleActions(item);
  const canClickBody = isToast || hasDefault;

  useEffect(() => {
    if (!isToast) return;
    return () => {
      if (hoveredRef.current) {
        hoveredRef.current = false;
        window.webbar.notifications.resumeTimeout(item.id);
      }
    };
  }, [isToast, item.id]);

  function onBodyClick() {
    if (hasDefault) {
      window.webbar.notifications.invokeAction(item.id, "default");
    } else if (isToast) {
      window.webbar.notifications.hidePopup(item.id);
    }
  }

  return (
    <article
      className={`notif-card${isToast ? " notif-toast" : ""}${item.urgency === "critical" ? " is-critical" : ""}`}
      data-webbar-interactive={isToast ? "" : undefined}
      onPointerEnter={
        isToast
          ? () => {
              hoveredRef.current = true;
              window.webbar.notifications.pauseTimeout(item.id);
            }
          : undefined
      }
      onPointerLeave={
        isToast
          ? () => {
              hoveredRef.current = false;
              window.webbar.notifications.resumeTimeout(item.id);
            }
          : undefined
      }
    >
      <div className="notif-card-head">
        <div className="notif-card-app">
          {item.app_icon_url && (
            <img
              className="notif-app-icon"
              src={item.app_icon_url}
              alt=""
              aria-hidden="true"
            />
          )}
          <span className="notif-card-app-name">
            {item.app_name || "Notification"}
          </span>
        </div>
        <div className="notif-card-meta">
          <span className="notif-time">
            {formatRelativeTime(item.timestamp_ms, nowMs)}
          </span>
          <button
            type="button"
            className="notif-close-btn"
            title="Dismiss notification"
            aria-label="Dismiss notification"
            onClick={() => window.webbar.notifications.dismiss(item.id)}
          >
            ×
          </button>
        </div>
      </div>

      <div
        className={`notif-card-body-row${canClickBody ? " is-clickable" : ""}`}
        onClick={canClickBody ? onBodyClick : undefined}
      >
        {item.image_url && (
          <img
            className="notif-image"
            src={item.image_url}
            alt=""
            aria-hidden="true"
          />
        )}
        <div className="notif-text">
          {item.summary && <div className="notif-summary">{item.summary}</div>}
          {item.body && <div className="notif-body">{item.body}</div>}
        </div>
      </div>

      {item.progress !== null && (
        <div
          className="notif-progress-track"
          role="progressbar"
          aria-valuenow={item.progress}
          aria-valuemin={0}
          aria-valuemax={100}
        >
          <div
            className="notif-progress-fill"
            style={{ width: `${item.progress}%` }}
          />
        </div>
      )}

      {actions.length > 0 && (
        <div className="notif-actions">
          {actions.map((action) => (
            <button
              key={action.key}
              type="button"
              className="notif-action-btn"
              onClick={() =>
                window.webbar.notifications.invokeAction(item.id, action.key)
              }
            >
              {action.label}
            </button>
          ))}
        </div>
      )}
    </article>
  );
}

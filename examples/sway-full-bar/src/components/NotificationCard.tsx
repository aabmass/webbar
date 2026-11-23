import type { NotificationItem } from "@webbar/types";
import { formatRelativeTime } from "@webbar/sway-full-bar-shared/notifications";

function updateSheen(
  el: HTMLElement,
  clientX: number,
  clientY: number,
  isEnter: boolean,
) {
  const rect = el.getBoundingClientRect();
  const rawX = clientX - rect.left;
  const maxX = rect.width * 0.25;
  const clampedY = Math.round(
    Math.max(0, Math.min(clientY - rect.top, rect.height)),
  );

  if (isEnter) {
    el.style.setProperty(
      "--toast-x",
      `${Math.round(Math.max(0, Math.min(rawX, maxX)))}px`,
    );
    el.style.setProperty("--toast-y", `${clampedY}px`);
    el.dataset.sheenLocked = rawX >= maxX ? "1" : "";
    return;
  }

  if (rawX <= maxX) {
    el.dataset.sheenLocked = "";
    el.style.setProperty("--toast-x", `${Math.round(Math.max(0, rawX))}px`);
    el.style.setProperty("--toast-y", `${clampedY}px`);
  } else if (el.dataset.sheenLocked !== "1") {
    el.dataset.sheenLocked = "1";
    el.style.setProperty("--toast-x", `${Math.round(maxX)}px`);
  }
}

export function NotificationCard({
  item,
  nowMs,
  isToast = false,
  exiting = false,
}: {
  item: NotificationItem;
  nowMs: number;
  isToast?: boolean;
  exiting?: boolean;
}) {
  const interactiveToast = isToast && !exiting;
  const canClickBody = !exiting;

  return (
    <article
      className={`notif-card${isToast ? " notif-toast" : ""}${item.urgency === "critical" ? " is-critical" : ""}`}
      data-webbar-interactive={interactiveToast ? "" : undefined}
      onPointerEnter={
        interactiveToast
          ? (e) => {
              window.webbar.notifications.pauseTimeout(item.id);
              updateSheen(e.currentTarget, e.clientX, e.clientY, true);
            }
          : undefined
      }
      onPointerMove={
        interactiveToast
          ? (e) => {
              updateSheen(e.currentTarget, e.clientX, e.clientY, false);
            }
          : undefined
      }
      onPointerLeave={
        interactiveToast
          ? (e) => {
              window.webbar.notifications.resumeTimeout(item.id);
              delete e.currentTarget.dataset.sheenLocked;
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
        onClick={
          canClickBody
            ? () => window.webbar.notifications.activate(item.id)
            : undefined
        }
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

      {item.actions.length > 0 && (
        <div className="notif-actions">
          {item.actions.map((action) => (
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

import { createWebbarBridge, type WebbarApi } from "./preload";

declare global {
  interface Window {
    webbar: WebbarApi;
    __webbarOutput?: string;
    __webbarReceive?: (event: Record<string, unknown>) => void;
    webkit?: {
      messageHandlers?: {
        bridge?: {
          postMessage: (msg: string) => void;
        };
      };
    };
  }
}

const initialOutput =
  window.__webbarOutput ||
  new URLSearchParams(window.location.search).get("output") ||
  "";

const { api, receive } = createWebbarBridge((json) => {
  const handler = window.webkit?.messageHandlers?.bridge;
  if (!handler) {
    console.error("webbar: webkit bridge handler is not available");
    return;
  }
  handler.postMessage(json);
}, initialOutput);

window.webbar = api;
window.__webbarReceive = receive;

let overlayActive = false;
let overlayScheduled = false;
const OVERLAY_SELECTOR =
  ':popover-open, [role="dialog"]:not([hidden]):not([aria-hidden="true"]), [aria-modal="true"]:not([hidden]):not([aria-hidden="true"]), [data-webbar-overlay]:not([hidden])';

function checkOverlay(): void {
  overlayScheduled = false;
  let active = false;
  try {
    active = document.querySelector(OVERLAY_SELECTOR) !== null;
  } catch {
    active =
      document.querySelector(
        '[role="dialog"]:not([hidden]):not([aria-hidden="true"]), [aria-modal="true"]:not([hidden]):not([aria-hidden="true"]), [data-webbar-overlay]:not([hidden])',
      ) !== null;
  }
  if (active !== overlayActive) {
    overlayActive = active;
    window.webkit?.messageHandlers?.bridge?.postMessage(
      JSON.stringify({ type: "set_overlay", active }),
    );
  }
}

function scheduleOverlayCheck(): void {
  if (!overlayScheduled) {
    overlayScheduled = true;
    requestAnimationFrame(checkOverlay);
  }
}

window.addEventListener("DOMContentLoaded", () => {
  document.addEventListener("toggle", scheduleOverlayCheck, true);
  if (document.body) {
    new MutationObserver(scheduleOverlayCheck).observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: [
        "open",
        "hidden",
        "aria-hidden",
        "role",
        "aria-modal",
        "data-webbar-overlay",
      ],
    });
  }
  scheduleOverlayCheck();
});

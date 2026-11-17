import { createWebbarBridge } from "./preload";
export * from "./preload";

const initialOutput =
  window.__webbarOutput ||
  new URLSearchParams(window.location.search).get("output") ||
  "";

function hasOpenNativePopover(): boolean {
  try {
    return document.querySelector(":popover-open") !== null;
  } catch {
    return false;
  }
}

const { api, receive, notifyOverlayChange } = createWebbarBridge(
  (json) => {
    const handler = window.webkit?.messageHandlers?.bridge;
    if (!handler) {
      console.error("webbar: webkit bridge handler is not available");
      return;
    }
    handler.postMessage(json);
  },
  initialOutput,
  hasOpenNativePopover,
);

window.webbar = api;
window.__webbarReceive = receive;

document.addEventListener("toggle", notifyOverlayChange, true);
if (document.readyState === "loading") {
  window.addEventListener("DOMContentLoaded", notifyOverlayChange, {
    once: true,
  });
} else {
  notifyOverlayChange();
}

import { createWebbarBridge } from "./preload";
export * from "./preload";

if (!window.webbar) {
  const params = new URLSearchParams(window.location.search);
  const output = params.get("output") || "";
  const wsPort = params.get("wsPort") || params.get("ws") || "9123";
  const wsToken = params.get("wsToken") || params.get("token") || "";
  const query = [
    wsToken && `token=${encodeURIComponent(wsToken)}`,
    output && `output=${encodeURIComponent(output)}`,
  ]
    .filter(Boolean)
    .join("&");
  const url = `ws://127.0.0.1:${wsPort}${query ? `?${query}` : ""}`;

  const pending: string[] = [];
  let ws: WebSocket | null = null;
  let delay = 500;

  const { api, receive, notifyOverlayChange, onConnect, onDisconnect } =
    createWebbarBridge(
    (json) => {
      if (ws?.readyState === WebSocket.OPEN) {
        ws.send(json);
      } else if (pending.length < 64) {
        pending.push(json);
      }
    },
    output,
    () => {
      try {
        return document.querySelector(":popover-open") !== null;
      } catch {
        return false;
      }
    },
  );

  window.webbar = api;
  document.addEventListener("toggle", notifyOverlayChange, true);
  if (document.readyState === "loading") {
    window.addEventListener("DOMContentLoaded", notifyOverlayChange, {
      once: true,
    });
  } else {
    notifyOverlayChange();
  }

  const connect = () => {
    const socket = (ws = new WebSocket(url));
    socket.addEventListener("open", () => {
      delay = 500;
      while (pending.length > 0 && socket.readyState === WebSocket.OPEN) {
        socket.send(pending.shift()!);
      }
      onConnect();
    });
    socket.addEventListener("message", (ev) => {
      if (typeof ev.data === "string") {
        receive(JSON.parse(ev.data) as Record<string, unknown>);
      }
    });
    socket.addEventListener("close", () => {
      if (ws === socket) ws = null;
      pending.length = 0;
      onDisconnect();
      window.setTimeout(connect, delay);
      delay = Math.min(delay * 2, 5000);
    });
  };
  connect();
}

import { createWebbarBridge } from "@webbar/types";

const DEFAULT_WS_PORT = "9123";
const MAX_PENDING_OUTBOUND = 64;

export async function initDevWsBridge(): Promise<void> {
  if (window.webbar) return;

  const params = new URLSearchParams(window.location.search);
  const wsPort = params.get("wsPort") || params.get("ws") || DEFAULT_WS_PORT;
  const wsToken = params.get("wsToken") || params.get("token") || "";
  const output = params.get("output") || "";
  const query = wsToken ? `?token=${encodeURIComponent(wsToken)}` : "";
  const url = `ws://127.0.0.1:${wsPort}${query}`;

  const pendingOutbound: string[] = [];
  let ws: WebSocket | null = null;
  let reconnectDelayMs = 500;
  let reconnectTimer: number | null = null;

  const { api, receive } = createWebbarBridge((json) => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(json);
    } else if (pendingOutbound.length < MAX_PENDING_OUTBOUND) {
      pendingOutbound.push(json);
    }
  }, output);

  window.webbar = api;
  window.__webbarReceive = receive;

  await new Promise<void>((resolve) => {
    let initialSettled = false;
    const settleInitial = () => {
      if (!initialSettled) {
        initialSettled = true;
        resolve();
      }
    };

    const connect = () => {
      if (reconnectTimer !== null) {
        window.clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }

      const socket = new WebSocket(url);
      ws = socket;

      socket.addEventListener("open", () => {
        reconnectDelayMs = 500;
        while (pendingOutbound.length > 0 && socket.readyState === WebSocket.OPEN) {
          const msg = pendingOutbound.shift();
          if (msg) socket.send(msg);
        }
        settleInitial();
      });

      socket.addEventListener("message", (ev) => {
        if (typeof ev.data !== "string") return;
        try {
          receive(JSON.parse(ev.data) as Record<string, unknown>);
        } catch (err) {
          console.error("webbar dev ws: invalid message", err);
        }
      });

      socket.addEventListener("close", () => {
        if (ws === socket) {
          ws = null;
        }
        pendingOutbound.length = 0;
        settleInitial();
        const delay = reconnectDelayMs;
        reconnectDelayMs = Math.min(reconnectDelayMs * 2, 5000);
        reconnectTimer = window.setTimeout(connect, delay);
      });

      socket.addEventListener("error", () => {
        console.warn(`webbar dev ws: connection error on ${url}`);
      });
    };

    connect();
    window.setTimeout(settleInitial, 1500);
  });
}

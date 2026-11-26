import {
  INTERACTIVE_SELECTOR,
  collectInteractiveRegions,
  createWebbarBridge,
  hasOpenModalPopover,
  hasRunningInteractiveAnimation,
} from "./preload";
export * from "./preload";

if (!window.webbar) {
  const params = new URLSearchParams(window.location.search);
  const output = params.get("output") || "";
  const surface = params.get("surface") || "bar";
  const wsPort = params.get("wsPort") || params.get("ws") || "9123";
  const wsToken = params.get("wsToken") || params.get("token") || "";
  const query = [
    wsToken && `token=${encodeURIComponent(wsToken)}`,
    surface && `surface=${encodeURIComponent(surface)}`,
    output && `output=${encodeURIComponent(output)}`,
  ]
    .filter(Boolean)
    .join("&");
  const url = `ws://127.0.0.1:${wsPort}${query ? `?${query}` : ""}`;

  const pending: string[] = [];
  let ws: WebSocket | null = null;
  let delay = 500;

  const hasOpenPopover = (): boolean => hasOpenModalPopover(document);

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
      hasOpenPopover,
      surface,
    );

  // Recover if an open [popover] is removed from the DOM without hidePopover()
  // (which skips firing a capturing `toggle` event on `document`).
  let popoverRemovalObserver: MutationObserver | null = null;
  const syncNativePopover = () => {
    if (hasOpenPopover()) {
      if (!popoverRemovalObserver && document.documentElement) {
        popoverRemovalObserver = new MutationObserver((records) => {
          if (records.some((r) => r.removedNodes.length > 0)) {
            syncNativePopover();
          }
        });
        popoverRemovalObserver.observe(document.documentElement, {
          childList: true,
          subtree: true,
        });
      }
    } else if (popoverRemovalObserver) {
      popoverRemovalObserver.disconnect();
      popoverRemovalObserver = null;
    }
    scheduleInteractiveSync();
    notifyOverlayChange();
  };

  // Non-modal floating elements (`:popover-open[popover="manual"]` or legacy
  // `[data-webbar-interactive]`) stay clickable under KeyboardMode::None
  // without stealing Sway focus.
  let lastInteractiveKey = "";
  let interactiveRaf = 0;
  const observedInteractive = new Set<Element>();
  const interactiveResizeObserver =
    typeof ResizeObserver !== "undefined"
      ? new ResizeObserver(() => scheduleInteractiveSync())
      : null;

  const hasAnyInteractiveNode = (): boolean => {
    try {
      return document.querySelector(INTERACTIVE_SELECTOR) !== null;
    } catch {
      return document.querySelector("[data-webbar-interactive]") !== null;
    }
  };

  const syncInteractiveRegions = () => {
    interactiveRaf = 0;
    const { rects, tracked } = collectInteractiveRegions(document, (el) =>
      window.getComputedStyle(el as Element),
    );
    const currentSet = new Set(tracked as Element[]);
    if (interactiveResizeObserver) {
      for (const el of observedInteractive) {
        if (!currentSet.has(el)) {
          interactiveResizeObserver.unobserve(el);
          observedInteractive.delete(el);
        }
      }
      for (const el of currentSet) {
        if (!observedInteractive.has(el)) {
          interactiveResizeObserver.observe(el);
          observedInteractive.add(el);
        }
      }
    }

    const key = rects.map((r) => `${r.x},${r.y},${r.width},${r.height}`).join(";");
    if (key !== lastInteractiveKey) {
      lastInteractiveKey = key;
      api.setInteractiveRegions(rects);
    }

    if (hasRunningInteractiveAnimation(document, tracked)) {
      scheduleInteractiveSync();
    }
  };

  function scheduleInteractiveSync(): void {
    if (interactiveRaf !== 0) return;
    interactiveRaf = window.requestAnimationFrame(syncInteractiveRegions);
  }

  const startInteractiveObserver = () => {
    if (!document.documentElement) return;
    const observer = new MutationObserver(() => {
      if (
        observedInteractive.size === 0 &&
        lastInteractiveKey === "" &&
        !hasAnyInteractiveNode()
      ) {
        return;
      }
      scheduleInteractiveSync();
    });
    observer.observe(document.documentElement, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: [
        "popover",
        "data-webbar-interactive",
        "style",
        "class",
        "hidden",
      ],
    });
    const onMotion = () => {
      if (observedInteractive.size > 0 || hasAnyInteractiveNode()) {
        scheduleInteractiveSync();
      }
    };
    window.addEventListener("resize", scheduleInteractiveSync);
    window.addEventListener("transitionrun", onMotion, true);
    window.addEventListener("transitionend", onMotion, true);
    window.addEventListener("transitioncancel", onMotion, true);
    window.addEventListener("animationstart", onMotion, true);
    window.addEventListener("animationend", onMotion, true);
    window.addEventListener("animationcancel", onMotion, true);
    syncInteractiveRegions();
  };

  window.webbar = api;
  document.addEventListener("toggle", syncNativePopover, true);
  if (document.readyState === "loading") {
    window.addEventListener(
      "DOMContentLoaded",
      () => {
        syncNativePopover();
        startInteractiveObserver();
      },
      { once: true },
    );
  } else {
    syncNativePopover();
    startInteractiveObserver();
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

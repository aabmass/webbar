import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./App.css";

async function boot() {
  if (import.meta.env.DEV && !window.webbar) {
    const { initDevWsBridge } = await import("./devWsBridge");
    await initDevWsBridge();
  }

  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void boot();

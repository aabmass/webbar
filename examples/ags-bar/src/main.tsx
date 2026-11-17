/// <reference types="vite/client" />
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { installDevMocks } from "./devMocks";
import "./App.css";

if (import.meta.env.DEV) {
  installDevMocks();
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

/// <reference types="vite/client" />
import { mount } from "svelte";
import { installDevMocks } from "@webbar/sway-full-bar-shared/devMocks";
import "@webbar/sway-full-bar-shared/styles/theme.css";
import "./styles/base.css";
import App from "./App.svelte";

if (import.meta.env.DEV) {
  installDevMocks();
}

mount(App, {
  target: document.getElementById("root")!,
});

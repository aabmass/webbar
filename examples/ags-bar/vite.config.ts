import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [
    react(),
    {
      name: "webbar-preload",
      apply: "serve",
      transformIndexHtml: () => [
        {
          tag: "script",
          children: readFileSync("../../generated/preload.js", "utf-8"),
          injectTo: "head-prepend",
        },
      ],
    },
  ],
  base: "./",
});

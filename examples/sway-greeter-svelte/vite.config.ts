import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [
    svelte(),
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

import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: /greeter\.spec\.mjs$/,
  outputDir: "./test-results",
  workers: 1,
  timeout: 30_000,
  expect: {
    timeout: 10_000,
  },
  reporter: "list",
  use: {
    headless: true,
    viewport: { width: 1280, height: 800 },
    reducedMotion: "reduce",
    launchOptions: {
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    },
  },
});

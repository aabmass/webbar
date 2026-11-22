import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: /parity\.spec\.mjs$/,
  outputDir: "./test-results",
  workers: 1,
  timeout: 30_000,
  expect: {
    timeout: 10_000,
  },
  reporter: "list",
  use: {
    headless: true,
    viewport: { width: 1600, height: 600 },
    reducedMotion: "reduce",
    launchOptions: {
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    },
  },
});

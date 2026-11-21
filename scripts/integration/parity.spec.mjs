import { test, expect } from "@playwright/test";
import { execFileSync, spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const INHIBIT_SCRIPT = path.join(__dirname, "dbus-inhibit.py");
const PRELOAD_JS = process.env.WEBBAR_PRELOAD_JS;

if (!PRELOAD_JS) {
  throw new Error("WEBBAR_PRELOAD_JS must be set");
}

const TARGETS = [
  { name: "react", url: process.env.WEBBAR_REACT_URL },
  { name: "svelte", url: process.env.WEBBAR_SVELTE_URL },
].filter((t) => Boolean(t.url));

if (TARGETS.length === 0) {
  throw new Error("WEBBAR_REACT_URL and/or WEBBAR_SVELTE_URL must be set");
}

for (const target of TARGETS) {
  test.describe(`${target.name} bar integration`, () => {
    test("sway IPC, stasis watch/pause/profiles, dbus inhibit, and popovers", async ({
      page,
    }) => {
      const errors = [];
      page.on("pageerror", (err) => errors.push(`pageerror: ${err.message}`));
      page.on("console", (msg) => {
        if (msg.type() === "error") {
          errors.push(`console.error: ${msg.text()}`);
        }
      });

      await page.addInitScript({ path: PRELOAD_JS });
      await page.goto(target.url, { waitUntil: "domcontentloaded" });

      // 1. Wait for live Sway workspaces + Stasis watch connection
      const focusedWs = page.locator(".workspaces-grid .ws-btn.focused");
      await expect(focusedWs).toHaveText(/1/);

      const stasisBtn = page.locator(".stasis-btn");
      await expect(stasisBtn).toBeVisible();
      await expect(stasisBtn).not.toHaveClass(/is-dimmed/);
      await expect(stasisBtn.locator(".stasis-label")).toHaveText("default");

      // 2. Real Sway IPC workspace push
      execFileSync("swaymsg", ["workspace", "2"]);
      await expect(page.locator(".workspaces-grid .ws-btn.focused")).toHaveText(
        /2/,
      );
      execFileSync("swaymsg", ["workspace", "1"]);
      await expect(page.locator(".workspaces-grid .ws-btn.focused")).toHaveText(
        /1/,
      );

      // 3. Real Stasis pause/resume & profile switching
      await stasisBtn.click();
      const stasisPopover = page.locator(".stasis-popover");
      await expect(stasisPopover).toBeVisible();
      await expect(stasisPopover.locator(".stasis-badge")).toHaveText("active");

      await stasisPopover
        .getByRole("button", { name: "Pause Idle" })
        .click();
      await expect(stasisBtn).toHaveClass(/is-inhibited/);
      await expect(stasisPopover.locator(".stasis-badge")).toHaveText("paused");
      await expect(
        stasisPopover.getByRole("button", { name: "Resume Idle" }),
      ).toBeVisible();

      await stasisPopover
        .getByRole("button", { name: "Resume Idle" })
        .click();
      await expect(stasisBtn).not.toHaveClass(/is-inhibited/);
      await expect(stasisPopover.locator(".stasis-badge")).toHaveText("active");

      await stasisPopover
        .locator(".stasis-profile-btn", { hasText: "gaming" })
        .click();
      await expect(stasisBtn.locator(".stasis-label")).toHaveText("gaming");

      await stasisPopover
        .locator(".stasis-profile-btn", { hasText: "default" })
        .click();
      await expect(stasisBtn.locator(".stasis-label")).toHaveText("default");

      await page.keyboard.press("Escape");
      await expect(stasisPopover).toBeHidden();

      // 4. Real D-Bus org.freedesktop.ScreenSaver.Inhibit -> stasis watch + blame push
      const inhibitor = spawn(
        "python3",
        [INHIBIT_SCRIPT, "webbar-test", "Playing video"],
        { stdio: "inherit" },
      );
      try {
        await expect(stasisBtn).toHaveClass(/is-inhibited/);
        await stasisBtn.click();
        await expect(stasisPopover).toBeVisible();
        await expect(stasisPopover.locator(".stasis-badge")).toHaveText(
          "inhibited",
        );
        await expect(
          stasisPopover.locator(".stasis-inhibitor-list"),
        ).toContainText("webbar-test");
        await expect(
          stasisPopover.locator(".stasis-inhibitor-list"),
        ).toContainText("Playing video");
      } finally {
        inhibitor.kill();
      }

      await expect(stasisBtn).not.toHaveClass(/is-inhibited/);
      await expect(stasisPopover.locator(".stasis-badge")).toHaveText("active");
      await page.keyboard.press("Escape");
      await expect(stasisPopover).toBeHidden();

      // 5. Clock popover parity
      await page.locator(".clock").click();
      const calendarPopover = page.locator(".calendar");
      await expect(calendarPopover).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(calendarPopover).toBeHidden();

      expect(errors).toEqual([]);
    });
  });
}

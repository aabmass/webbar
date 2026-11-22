import { test, expect } from "@playwright/test";
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const NOTIFY_SCRIPT = path.join(__dirname, "dbus-notify.py");
const PRELOAD_JS = process.env.WEBBAR_PRELOAD_JS;
const TARGET_NAME = process.env.WEBBAR_TARGET || "bar";
const TARGET_URL = process.env.WEBBAR_URL;

if (!PRELOAD_JS || !TARGET_URL) {
  throw new Error("WEBBAR_PRELOAD_JS and WEBBAR_URL must be set");
}

function sendNotify(payload) {
  const out = execFileSync(
    "python3",
    [NOTIFY_SCRIPT, "notify", JSON.stringify(payload)],
    { encoding: "utf8" },
  );
  return Number(out.trim());
}

function closeNotify(id) {
  execFileSync("python3", [NOTIFY_SCRIPT, "close", String(id)]);
}

function readSignals(logPath) {
  if (!fs.existsSync(logPath)) return [];
  return fs
    .readFileSync(logPath, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

function waitForWatcherReady(watcher) {
  return new Promise((resolve, reject) => {
    let buffer = "";
    const cleanup = () => {
      watcher.stdout.off("data", onData);
      watcher.off("error", onError);
      watcher.off("exit", onExit);
    };
    const onData = (chunk) => {
      buffer += chunk.toString("utf8");
      if (buffer.split("\n").includes("ready")) {
        cleanup();
        resolve();
      }
    };
    const onError = (err) => {
      cleanup();
      reject(err);
    };
    const onExit = (code, signal) => {
      cleanup();
      reject(
        new Error(`dbus-notify watch exited early (code=${code}, signal=${signal})`),
      );
    };
    watcher.stdout.on("data", onData);
    watcher.once("error", onError);
    watcher.once("exit", onExit);
  });
}

test.describe(`${TARGET_NAME} notifications integration`, () => {
  test("D-Bus notifications, toast interactive regions, actions, DND, and CloseNotification", async ({
    page,
  }) => {
    const errors = [];
    page.on("pageerror", (err) => errors.push(`pageerror: ${err.message}`));
    page.on("console", (msg) => {
      if (msg.type() === "error") {
        errors.push(`console.error: ${msg.text()}`);
      }
    });

    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "webbar-notif-e2e-"));
    const signalsPath = path.join(tmpDir, "signals.jsonl");
    const watcher = spawn("python3", [NOTIFY_SCRIPT, "watch", signalsPath], {
      stdio: ["ignore", "pipe", "inherit"],
    });

    try {
      await waitForWatcherReady(watcher);
      await page.addInitScript(() => {
        window.__webbarSent = [];
        const origSend = WebSocket.prototype.send;
        WebSocket.prototype.send = function (data) {
          if (typeof data === "string") {
            try {
              const parsed = JSON.parse(data);
              if (
                parsed.type === "set_overlay" ||
                parsed.type === "set_interactive_regions"
              ) {
                window.__webbarSent.push(parsed);
              }
            } catch {
              // ignore non-JSON
            }
          }
          return origSend.call(this, data);
        };
      });
      await page.addInitScript({ path: PRELOAD_JS });
      await page.goto(TARGET_URL, { waitUntil: "domcontentloaded" });

      const bellBtn = page.locator("button.notif-btn");
      await expect(bellBtn).toBeVisible();
      await expect(bellBtn).not.toHaveClass(/is-dimmed/);

      // 1. Send notification with markup, progress, and actions -> toast renders
      const id1 = sendNotify({
        app_name: "Chat",
        summary: "New message",
        body: "<b>Hello</b> &amp; <i>world</i>",
        actions: ["default", "Open", "reply", "Reply"],
        hints: { value: 65 },
        expire_timeout: 0,
      });

      const toast = page.locator(".notif-toast-stack .notif-toast", {
        hasText: "New message",
      });
      await expect(toast).toBeVisible();
      await expect(toast).toHaveAttribute("data-webbar-interactive", "");
      await expect(toast.locator(".notif-body")).toHaveText("Hello & world");
      await expect(toast.getByRole("progressbar")).toHaveAttribute(
        "aria-valuenow",
        "65",
      );
      await expect(toast.getByRole("button", { name: "Open" })).toBeHidden();
      await expect(toast.getByRole("button", { name: "Reply" })).toBeVisible();
      await expect(bellBtn.locator(".notif-badge")).toHaveText("1");

      // Verify toast sends interactive rects without triggering modal overlay
      await expect
        .poll(() =>
          page.evaluate(() => {
            const sent = window.__webbarSent;
            const overlays = sent.filter((m) => m.type === "set_overlay");
            const regions = sent.filter(
              (m) => m.type === "set_interactive_regions",
            );
            return {
              lastOverlay: overlays[overlays.length - 1]?.active ?? null,
              lastRectsCount: regions[regions.length - 1]?.rects?.length ?? 0,
            };
          }),
        )
        .toEqual({ lastOverlay: false, lastRectsCount: 1 });

      // Opening a non-modal [popover="manual"][data-webbar-interactive] must NOT trigger set_overlay(true)
      await page.evaluate(() => {
        const el = document.createElement("div");
        el.id = "test-non-modal-popover";
        el.setAttribute("popover", "manual");
        el.setAttribute("data-webbar-interactive", "");
        el.style.cssText =
          "position:fixed;top:120px;left:20px;width:120px;height:40px;";
        document.body.appendChild(el);
        el.showPopover();
      });
      await expect
        .poll(() =>
          page.evaluate(() => {
            const sent = window.__webbarSent;
            const overlays = sent.filter((m) => m.type === "set_overlay");
            const regions = sent.filter(
              (m) => m.type === "set_interactive_regions",
            );
            return {
              lastOverlay: overlays[overlays.length - 1]?.active ?? null,
              lastRectsCount: regions[regions.length - 1]?.rects?.length ?? 0,
            };
          }),
        )
        .toEqual({ lastOverlay: false, lastRectsCount: 2 });
      await page.evaluate(() => {
        document.getElementById("test-non-modal-popover")?.remove();
      });

      // 2. Click action button -> emits ActionInvoked + NotificationClosed(2)
      await toast.getByRole("button", { name: "Reply" }).click();
      await expect(toast).toBeHidden();
      await expect(bellBtn.locator(".notif-badge")).toBeHidden();

      await expect
        .poll(() => readSignals(signalsPath))
        .toEqual(
          expect.arrayContaining([
            { signal: "action_invoked", id: id1, key: "reply" },
            { signal: "notification_closed", id: id1, reason: 2 },
          ]),
        );

      // 3. Open Notification Center -> marks read & toggles DND; clicking body without default hides popup only
      const id2 = sendNotify({
        app_name: "Mail",
        summary: "Inbox",
        body: "First email",
        expire_timeout: 0,
      });
      await expect(bellBtn.locator(".notif-badge")).toHaveText("1");

      const mailToast = page.locator(".notif-toast-stack .notif-toast", {
        hasText: "Inbox",
      });
      await expect(mailToast).toBeVisible();
      await mailToast.locator(".notif-card-body-row").click();
      await expect(mailToast).toBeHidden();

      await bellBtn.click();
      const center = page.locator(".notif-popover");
      await expect(center).toBeVisible();
      await expect
        .poll(() =>
          page.evaluate(() => {
            const overlays = window.__webbarSent.filter(
              (m) => m.type === "set_overlay",
            );
            return overlays[overlays.length - 1]?.active ?? null;
          }),
        )
        .toBe(true);
      await expect(bellBtn.locator(".notif-badge")).toBeHidden();
      await expect(center).toContainText("Inbox");

      const dndSwitch = center.getByRole("switch", { name: "Do Not Disturb" });
      await dndSwitch.click();
      await expect(dndSwitch).toHaveAttribute("aria-checked", "true");
      await page.keyboard.press("Escape");
      await expect(center).toBeHidden();
      await expect
        .poll(() =>
          page.evaluate(() => {
            const overlays = window.__webbarSent.filter(
              (m) => m.type === "set_overlay",
            );
            return overlays[overlays.length - 1]?.active ?? null;
          }),
        )
        .toBe(false);

      // 4. Under DND: normal notification stays out of toasts, critical still pops up
      const id3 = sendNotify({
        app_name: "Updates",
        summary: "Suppressed Toast",
        body: "Should only be in history",
        expire_timeout: 0,
      });
      const id4 = sendNotify({
        app_name: "Power",
        summary: "Battery Critical",
        body: "5% remaining",
        hints: { urgency: 2 },
        expire_timeout: 0,
      });

      await expect(
        page.locator(".notif-toast-stack .notif-toast", {
          hasText: "Battery Critical",
        }),
      ).toBeVisible();
      await expect(
        page.locator(".notif-toast-stack .notif-toast", {
          hasText: "Suppressed Toast",
        }),
      ).toBeHidden();

      // 5. CloseNotification over D-Bus removes item & emits reason=3
      closeNotify(id2);
      await expect
        .poll(() => readSignals(signalsPath))
        .toEqual(
          expect.arrayContaining([
            { signal: "notification_closed", id: id2, reason: 3 },
          ]),
        );

      // 6. Open center -> both suppressed & critical in history, id2 gone, Clear works
      await bellBtn.click();
      await expect(center).toContainText("Suppressed Toast");
      await expect(center).toContainText("Battery Critical");
      await expect(center).not.toContainText("Inbox");

      await center.getByRole("button", { name: "Clear" }).click();
      await expect(center.locator(".drawer-empty")).toHaveText(
        "No notifications",
      );
      await expect
        .poll(() => readSignals(signalsPath))
        .toEqual(
          expect.arrayContaining([
            { signal: "notification_closed", id: id3, reason: 2 },
            { signal: "notification_closed", id: id4, reason: 2 },
          ]),
        );

      // Restore DND to off so repeated runs start from a clean state
      await dndSwitch.click();
      await expect(dndSwitch).toHaveAttribute("aria-checked", "false");
      await page.keyboard.press("Escape");
      await expect(center).toBeHidden();

      expect(errors).toEqual([]);
    } finally {
      watcher.kill();
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

import { test, expect, chromium } from "@playwright/test";
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
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
      await expect(page.locator(".notif-toast-stack")).toHaveAttribute(
        "popover",
        "manual",
      );
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

      // Opening a standard HTML [popover="manual"] (zero custom attributes) must NOT trigger set_overlay(true)
      await page.evaluate(() => {
        const el = document.createElement("div");
        el.id = "test-non-modal-popover";
        el.setAttribute("popover", "manual");
        el.style.cssText =
          "position:fixed;inset:auto;margin:0;top:120px;left:20px;width:120px;height:40px;";
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

      // CSS pointer-events: none must immediately drop the rect from interactive regions
      await page.evaluate(() => {
        const el = document.getElementById("test-non-modal-popover");
        el.style.pointerEvents = "none";
      });
      await expect
        .poll(() =>
          page.evaluate(() => {
            const regions = window.__webbarSent.filter(
              (m) => m.type === "set_interactive_regions",
            );
            return regions[regions.length - 1]?.rects?.length ?? 0;
          }),
        )
        .toBe(1);

      // Restore pointer-events first and wait for rect to reappear at y=120
      await page.evaluate(() => {
        const el = document.getElementById("test-non-modal-popover");
        el.style.pointerEvents = "auto";
      });
      await expect
        .poll(() =>
          page.evaluate(() => {
            const regions = window.__webbarSent.filter(
              (m) => m.type === "set_interactive_regions",
            );
            const last = regions[regions.length - 1]?.rects ?? [];
            return last.some((r) => r.x === 20 && r.y === 120);
          }),
        )
        .toBe(true);

      // Now trigger a CSS keyframe animation independently (animationstart -> rAF loop)
      await page.evaluate(() => {
        const style = document.createElement("style");
        style.id = "test-anim-style";
        style.textContent =
          "@keyframes wbSlideDown { from { transform: translateY(0px); } to { transform: translateY(40px); } } .wb-slide-down { animation: wbSlideDown 150ms linear forwards; }";
        document.head.appendChild(style);
        document
          .getElementById("test-non-modal-popover")
          .classList.add("wb-slide-down");
      });
      await expect
        .poll(() =>
          page.evaluate(() => {
            const regions = window.__webbarSent.filter(
              (m) => m.type === "set_interactive_regions",
            );
            const last = regions[regions.length - 1]?.rects ?? [];
            return last.some((r) => r.x === 20 && r.y >= 155);
          }),
        )
        .toBe(true);
      await page.evaluate(() => {
        document.getElementById("test-non-modal-popover")?.remove();
        document.getElementById("test-anim-style")?.remove();
      });

      // 2. Click action button -> emits ActivationToken + ActionInvoked + NotificationClosed(2)
      await toast.getByRole("button", { name: "Reply" }).click();
      await expect(toast).toBeHidden();
      await expect(bellBtn.locator(".notif-badge")).toBeHidden();

      await expect
        .poll(() => readSignals(signalsPath))
        .toEqual(
          expect.arrayContaining([
            {
              signal: "activation_token",
              id: id1,
              token: expect.stringMatching(/\S+/),
            },
            { signal: "action_invoked", id: id1, key: "reply" },
            { signal: "notification_closed", id: id1, reason: 2 },
          ]),
        );
      const step2Signals = readSignals(signalsPath);
      expect(
        step2Signals.findIndex(
          (s) => s.signal === "activation_token" && s.id === id1,
        ),
      ).toBeLessThan(
        step2Signals.findIndex(
          (s) => s.signal === "action_invoked" && s.id === id1,
        ),
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

      // 7. Real 2-tab Chromium Web Notification: clicking in webbar focuses workspace 2 AND switches from Tab Two -> Tab One
      const appServer = http.createServer((req, res) => {
        res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
        if (req.url === "/tab1") {
          res.end("<!doctype html><title>Tab One</title><h1>Tab One</h1>");
        } else {
          res.end("<!doctype html><title>Tab Two</title><h1>Tab Two</h1>");
        }
      });
      await new Promise((resolve) => appServer.listen(0, "127.0.0.1", resolve));
      const { port: appPort } = appServer.address();
      const appOrigin = `http://127.0.0.1:${appPort}`;

      execFileSync("swaymsg", ["workspace", "2"]);
      const profileDir = path.join(tmpDir, "chromium-profile");
      const appContext = await chromium.launchPersistentContext(profileDir, {
        headless: false,
        args: [
          "--ozone-platform=wayland",
          "--no-sandbox",
          "--enable-features=NativeNotifications",
        ],
      });
      try {
        await appContext.grantPermissions(["notifications"], {
          origin: appOrigin,
        });
        const tab1 = appContext.pages()[0] || (await appContext.newPage());
        await tab1.goto(`${appOrigin}/tab1`, { waitUntil: "domcontentloaded" });
        const tab2 = await appContext.newPage();
        await tab2.goto(`${appOrigin}/tab2`, { waitUntil: "domcontentloaded" });
        await tab2.bringToFront();

        const getChromiumWindowInfo = () => {
          const tree = JSON.parse(
            execFileSync("swaymsg", ["-t", "get_tree"], { encoding: "utf8" }),
          );
          const wins = [];
          const walk = (node) => {
            const children = [
              ...(node.nodes || []),
              ...(node.floating_nodes || []),
            ];
            if (
              children.length === 0 &&
              node.app_id &&
              node.app_id.toLowerCase().includes("chromium")
            ) {
              wins.push({
                name: node.name ?? "",
                focused: Boolean(node.focused),
              });
            }
            for (const child of children) walk(child);
          };
          walk(tree);
          return wins;
        };

        await expect
          .poll(() => {
            const wins = getChromiumWindowInfo();
            return (
              wins.length === 1 &&
              wins[0].name.includes("Tab Two") &&
              !wins[0].name.includes("Tab One")
            );
          })
          .toBe(true);

        execFileSync("swaymsg", ["workspace", "1"]);
        await expect(
          page.locator(".workspaces-grid .ws-btn.focused"),
        ).toHaveText(/1/);

        await tab1.evaluate(() => {
          window.__notifClicked = false;
          const n = new Notification("Chrome Tab 1 Ping", {
            body: "Switch back to Tab One",
          });
          n.onclick = () => {
            window.__notifClicked = true;
            window.focus();
            n.close();
          };
          window.__activeNotif = n;
        });

        await expect(bellBtn.locator(".notif-badge")).toHaveText("1");
        await bellBtn.click();
        await expect(center).toBeVisible();
        const card = center.locator(".notif-card", {
          hasText: "Chrome Tab 1 Ping",
        });
        await expect(card).toBeVisible();
        await card.locator(".notif-card-body-row").click();

        await expect(center).toBeHidden();
        await expect(
          page.locator(".workspaces-grid .ws-btn.focused"),
        ).toHaveText(/2/);
        await expect
          .poll(() => tab1.evaluate(() => window.__notifClicked))
          .toBe(true);
        await expect
          .poll(() => {
            const wins = getChromiumWindowInfo();
            return (
              wins.length === 1 &&
              wins[0].focused &&
              wins[0].name.includes("Tab One")
            );
          })
          .toBe(true);
      } finally {
        try {
          await appContext.close();
        } catch {}
        try {
          await new Promise((resolve) => appServer.close(resolve));
        } catch {}
        try {
          await page.evaluate(() => window.webbar.notifications.clearAll());
        } catch {}
        try {
          execFileSync("swaymsg", ["workspace", "1"]);
        } catch {}
      }

      expect(errors).toEqual([]);
    } finally {
      watcher.kill();
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

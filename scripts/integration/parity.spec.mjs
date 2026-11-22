import { test, expect } from "@playwright/test";
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const INHIBIT_SCRIPT = path.join(__dirname, "dbus-inhibit.py");
const TRAY_ITEM_SCRIPT = path.join(__dirname, "dbus-tray-item.py");
const PRELOAD_JS = process.env.WEBBAR_PRELOAD_JS;

if (!PRELOAD_JS) {
  throw new Error("WEBBAR_PRELOAD_JS must be set");
}

const TARGET_NAME = process.env.WEBBAR_TARGET || "bar";
const TARGET_URL = process.env.WEBBAR_URL;

if (!TARGET_URL) {
  throw new Error("WEBBAR_URL must be set");
}

test.describe(`${TARGET_NAME} bar integration`, () => {
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
      await page.goto(TARGET_URL, { waitUntil: "domcontentloaded" });

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

      // 6. Real StatusNotifierItem SVG current-color-scheme recoloring
      const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "webbar-tray-e2e-"));
      const svgPath = path.join(tmpDir, "panel-test.svg");
      fs.writeFileSync(
        svgPath,
        `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><style id="current-color-scheme">.ColorScheme-Text { color:#dfdfdf; }</style><path class="ColorScheme-Text" fill="currentColor" d="M0 0h24v24H0z"/></svg>`,
      );
      const trayProc = spawn("python3", [TRAY_ITEM_SCRIPT, svgPath], {
        stdio: "inherit",
      });
      try {
        const trayIcon = page.locator(".tray .tray-item img.tray-icon");
        await expect(trayIcon).toBeVisible();
        const src = await trayIcon.getAttribute("src");
        expect(src).toMatch(/^data:image\/svg\+xml;base64,/);
        const decodedSvg = Buffer.from(
          src.slice("data:image/svg+xml;base64,".length),
          "base64",
        ).toString("utf8");
        expect(decodedSvg).toContain(".ColorScheme-Text { color:");
        expect(decodedSvg).not.toContain("#dfdfdf");
      } finally {
        trayProc.kill();
        fs.rmSync(tmpDir, { recursive: true, force: true });
      }
      await expect(page.locator(".tray")).toBeHidden();

      // 7. Real BlueZ Bluetooth (org.bluez on isolated system bus)
      const btBtn = page.locator(
        '.widget-cluster button.icon-btn[title^="Bluetooth"]',
      );
      await expect(btBtn).toBeVisible();
      await expect(btBtn).not.toHaveClass(/is-dimmed/);
      await btBtn.click();

      const btPopover = page.locator(".popover-panel", {
        has: page.locator(".bt-popover-list"),
      });
      await expect(btPopover).toBeVisible();
      await expect(btPopover.locator(".qs-subtitle")).toHaveText("1 connected");

      const headphones = btPopover.locator(".bt-device", {
        hasText: "WH-1000XM5",
      });
      await expect(headphones).toHaveClass(/connected/);
      await expect(headphones).toContainText("85%");

      const mouse = btPopover.locator(".bt-device", { hasText: "MX Master 3S" });
      await expect(mouse).toHaveClass(/paired/);
      await expect(mouse).toContainText("64%");

      await mouse.click();
      await expect(mouse).toHaveClass(/connected/);
      await expect(btPopover.locator(".qs-subtitle")).toHaveText("2 connected");

      await mouse.click();
      await expect(mouse).toHaveClass(/paired/);
      await expect(btPopover.locator(".qs-subtitle")).toHaveText("1 connected");

      const btPowerSwitch = btPopover.getByRole("switch", {
        name: "Bluetooth power",
      });
      await expect(btPowerSwitch).toHaveAttribute("aria-checked", "true");
      await btPowerSwitch.click();
      await expect(btPowerSwitch).toHaveAttribute("aria-checked", "false");
      await expect(btPopover.locator(".qs-subtitle")).toHaveText("Off");

      await btPowerSwitch.click();
      await expect(btPowerSwitch).toHaveAttribute("aria-checked", "true");
      await expect(btPopover.locator(".qs-subtitle")).toHaveText("1 connected");

      await page.keyboard.press("Escape");
      await expect(btPopover).toBeHidden();

      // 8. Real DDC Backlight + gsettings Theme Switching
      const displayBtn = page.locator(
        '.widget-cluster button.level-btn[title*="Monitor brightness"]',
      );
      await expect(displayBtn).toBeVisible();
      await expect(displayBtn).not.toHaveClass(/is-dimmed/);
      await expect(displayBtn).toContainText("78%");
      await displayBtn.click();

      const displayPopover = page.locator(".display-popover");
      await expect(displayPopover).toBeVisible();

      const brightnessSlider = displayPopover.locator(
        'input[aria-label="Monitor brightness"]',
      );
      await expect(brightnessSlider).toHaveValue("78");
      await expect(displayPopover.locator(".drawer-value")).toHaveText("78%");

      await brightnessSlider.fill("55");
      await expect(displayPopover.locator(".drawer-value")).toHaveText("55%");
      await expect(displayBtn).toContainText("55%");

      await brightnessSlider.fill("78");
      await expect(displayPopover.locator(".drawer-value")).toHaveText("78%");
      await expect(displayBtn).toContainText("78%");

      const html = page.locator("html");
      await expect(html).toHaveAttribute("data-desktop-theme", "dark");
      const lightRadio = displayPopover.getByRole("radio", { name: "Light" });
      const darkRadio = displayPopover.getByRole("radio", { name: "Dark" });
      await expect(darkRadio).toHaveAttribute("aria-checked", "true");

      await lightRadio.click();
      await expect(lightRadio).toHaveAttribute("aria-checked", "true");
      await expect(html).toHaveAttribute("data-desktop-theme", "light");

      await darkRadio.click();
      await expect(darkRadio).toHaveAttribute("aria-checked", "true");
      await expect(html).toHaveAttribute("data-desktop-theme", "dark");

      await page.keyboard.press("Escape");
      await expect(displayPopover).toBeHidden();

      // 9. Real PulseAudio Audio (sinks, input, stream)
      const audioBtn = page.locator(
        '.widget-cluster button.level-btn[title*="Studio Monitors"]',
      );
      await expect(audioBtn).toBeVisible();
      await expect(audioBtn).not.toHaveClass(/is-dimmed/);
      await expect(audioBtn).toContainText("72%");
      await audioBtn.click();

      const audioPopover = page.locator(".audio-popover");
      await expect(audioPopover).toBeVisible();
      await expect(audioPopover.locator(".audio-subtitle")).toHaveText(
        "Studio Monitors",
      );
      await expect(audioPopover).toContainText("USB Condenser Mic");
      await expect(audioPopover).toContainText("80%");

      const streamCard = audioPopover.locator(".audio-stream-card", {
        hasText: "Spotify",
      });
      await expect(streamCard).toContainText("Tycho — Awake");
      await expect(streamCard).toContainText("84%");

      await audioPopover.getByRole("button", { name: "Mute output" }).click();
      await expect(audioBtn).toHaveClass(/is-muted/);
      await expect(
        audioPopover.getByRole("button", { name: "Unmute output" }),
      ).toBeVisible();

      await audioPopover.getByRole("button", { name: "Unmute output" }).click();
      await expect(audioBtn).not.toHaveClass(/is-muted/);

      await audioPopover
        .locator("button.audio-collapse-btn", { hasText: "Switch (2)" })
        .click();
      const studioRadio = audioPopover.getByRole("radio", {
        name: /Studio Monitors/,
      });
      const dacRadio = audioPopover.getByRole("radio", {
        name: /USB-C Headphone DAC/,
      });
      await expect(studioRadio).toHaveAttribute("aria-checked", "true");
      await expect(dacRadio).toHaveAttribute("aria-checked", "false");

      await dacRadio.click();
      await expect(audioPopover.locator(".audio-subtitle")).toHaveText(
        "USB-C Headphone DAC",
      );
      const dacBtn = page.locator(
        '.widget-cluster button.level-btn[title*="USB-C Headphone DAC"]',
      );
      await expect(dacBtn).toContainText("85%");

      await studioRadio.click();
      await expect(audioPopover.locator(".audio-subtitle")).toHaveText(
        "Studio Monitors",
      );
      await expect(audioBtn).toContainText("72%");

      await audioPopover.getByRole("button", { name: "Mute Spotify" }).click();
      await expect(
        audioPopover.getByRole("button", { name: "Unmute Spotify" }),
      ).toBeVisible();
      await audioPopover
        .getByRole("button", { name: "Unmute Spotify" })
        .click();
      await expect(
        audioPopover.getByRole("button", { name: "Mute Spotify" }),
      ).toBeVisible();

      await page.keyboard.press("Escape");
      await expect(audioPopover).toBeHidden();

      expect(errors).toEqual([]);
    });
  });

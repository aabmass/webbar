import { expect, test } from "@playwright/test";
import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";

const isBE = os.endianness() === "BE";

function encodeGreetdFrame(payload) {
  const body = Buffer.from(JSON.stringify(payload), "utf8");
  const header = Buffer.alloc(4);
  if (isBE) {
    header.writeUInt32BE(body.length, 0);
  } else {
    header.writeUInt32LE(body.length, 0);
  }
  return Buffer.concat([header, body]);
}

function startMockGreetd(sockPath) {
  const recorded = [];
  const server = net.createServer((socket) => {
    let buf = Buffer.alloc(0);
    socket.on("data", (chunk) => {
      buf = Buffer.concat([buf, chunk]);
      while (buf.length >= 4) {
        const len = isBE ? buf.readUInt32BE(0) : buf.readUInt32LE(0);
        if (buf.length < 4 + len) return;
        const msg = JSON.parse(buf.subarray(4, 4 + len).toString("utf8"));
        buf = buf.subarray(4 + len);
        recorded.push(msg);

        let reply;
        switch (msg.type) {
          case "create_session":
            reply = {
              type: "auth_message",
              auth_message_type: "secret",
              auth_message: "Password:",
            };
            break;
          case "post_auth_message_response":
            if (msg.response === "s3cr3t") {
              reply = { type: "success" };
            } else {
              reply = {
                type: "error",
                error_type: "auth_error",
                description: "Invalid password",
              };
            }
            break;
          case "cancel_session":
          case "start_session":
            reply = { type: "success" };
            break;
          default:
            reply = {
              type: "error",
              error_type: "error",
              description: `Unknown request: ${msg.type}`,
            };
        }
        socket.write(encodeGreetdFrame(reply));
      }
    });
  });

  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(sockPath, () => resolve({ server, recorded }));
  });
}

function mimeType(filePath) {
  if (filePath.endsWith(".html")) return "text/html; charset=utf-8";
  if (filePath.endsWith(".js")) return "application/javascript; charset=utf-8";
  if (filePath.endsWith(".css")) return "text/css; charset=utf-8";
  if (filePath.endsWith(".map")) return "application/json; charset=utf-8";
  return "application/octet-stream";
}

function startUiServer(uiDir, preloadPath) {
  const preloadJs = fs.readFileSync(preloadPath, "utf8");
  const server = http.createServer((req, res) => {
    const reqUrl = new URL(req.url ?? "/", "http://127.0.0.1");
    if (reqUrl.pathname === "/__webbar_preload.js") {
      res.writeHead(200, { "content-type": "application/javascript; charset=utf-8" });
      res.end(preloadJs);
      return;
    }
    const rel = reqUrl.pathname === "/" ? "index.html" : reqUrl.pathname.replace(/^\/+/, "");
    const target = path.resolve(uiDir, rel);
    if (!target.startsWith(path.resolve(uiDir))) {
      res.writeHead(403).end("Forbidden");
      return;
    }
    if (!fs.existsSync(target) || !fs.statSync(target).isFile()) {
      res.writeHead(404).end("Not Found");
      return;
    }
    if (rel === "index.html") {
      const html = fs
        .readFileSync(target, "utf8")
        .replace("<head>", '<head><script src="/__webbar_preload.js"></script>');
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end(html);
      return;
    }
    res.writeHead(200, { "content-type": mimeType(target) });
    fs.createReadStream(target).pipe(res);
  });

  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const addr = server.address();
      resolve({ server, port: addr.port });
    });
  });
}

function pickFreePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.once("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

function startWebbarBridge(port, sockPath, token, xdgDataDir) {
  const env = {
    ...process.env,
    GREETD_SOCK: sockPath,
    XDG_DATA_DIRS: xdgDataDir,
  };
  delete env.DBUS_SESSION_BUS_ADDRESS;
  const proc = spawn(
    process.env.WEBBAR_BIN || "webbar",
    ["--ws-bridge", String(port), "--ws-token", token],
    { env, stdio: ["ignore", "ignore", "pipe"] },
  );

  let stderr = "";
  const exitPromise = new Promise((resolve) => {
    proc.once("exit", (code, signal) => resolve({ code, signal }));
  });

  const readyPromise = new Promise((resolve, reject) => {
    const onData = (chunk) => {
      stderr += chunk.toString("utf8");
      const m = stderr.match(/ws-bridge listening on ws:\/\/127\.0\.0\.1:(\d+)/);
      if (m) {
        proc.stderr.off("data", onData);
        resolve(Number(m[1]));
      }
    };
    proc.stderr.on("data", onData);
    proc.once("exit", (code) => {
      reject(new Error(`webbar exited early (${code}): ${stderr}`));
    });
  });

  return { proc, readyPromise, exitPromise };
}

const GREETER_TARGET = process.env.WEBBAR_GREETER_TARGET || "greeter";

test(`examples/${GREETER_TARGET} completes auth error, cancel, and login launch with clean exit`, async ({
  page,
}) => {
  const uiDir = process.env.WEBBAR_UI_DIR;
  const preloadPath = process.env.WEBBAR_PRELOAD_JS;
  expect(uiDir, "WEBBAR_UI_DIR must be set").toBeTruthy();
  expect(preloadPath, "WEBBAR_PRELOAD_JS must be set").toBeTruthy();

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "webbar-greeter-e2e-"));
  const sockPath = path.join(tmpDir, "greetd.sock");
  const xdgDataDir = path.join(tmpDir, "share");
  const waylandSessionsDir = path.join(xdgDataDir, "wayland-sessions");
  fs.mkdirSync(waylandSessionsDir, { recursive: true });
  fs.writeFileSync(
    path.join(waylandSessionsDir, "sway.desktop"),
    "[Desktop Entry]\nName=Sway\nExec=sway\nType=Application\n",
  );
  fs.writeFileSync(
    path.join(waylandSessionsDir, "sway-custom.desktop"),
    "[Desktop Entry]\nName=Sway (Custom)\nExec=sway --unsupported-gpu\nType=Application\n",
  );
  const wsToken = "greeter-e2e-token";

  const { server: greetdServer, recorded } = await startMockGreetd(sockPath);
  const { server: uiServer, port: uiPort } = await startUiServer(uiDir, preloadPath);
  const wsPortReq = await pickFreePort();
  const { proc: webbarProc, readyPromise, exitPromise } = startWebbarBridge(
    wsPortReq,
    sockPath,
    wsToken,
    xdgDataDir,
  );

  try {
    const wsPort = await readyPromise;
    await page.goto(
      `http://127.0.0.1:${uiPort}/?wsPort=${wsPort}&wsToken=${encodeURIComponent(wsToken)}`,
    );

    const userInput = page.locator("#username");
    const responseInput = page.locator("#response");
    const sessionInput = page.locator("#session");
    const submitBtn = page.locator("#submit-btn");
    const cancelBtn = page.locator("#cancel-btn");
    const errorEl = page.locator("#error");

    await expect(submitBtn).toBeEnabled();
    await expect(userInput).toBeVisible();
    await expect(responseInput).toBeHidden();
    await expect(cancelBtn).toBeHidden();

    // 1. Bad password -> displays error banner and resets to username entry
    await userInput.fill("alice");
    await submitBtn.click();
    await expect(userInput).toBeHidden();
    await expect(responseInput).toBeVisible();
    await expect(cancelBtn).toBeVisible();
    await expect(page.locator("#prompt-label")).toHaveText("Password:");
    await responseInput.fill("wrong");
    await submitBtn.click();
    await expect(errorEl).toBeVisible();
    await expect(errorEl).toContainText("Invalid password");
    await expect(userInput).toBeVisible();
    await expect(responseInput).toBeHidden();
    await expect(cancelBtn).toBeHidden();

    // 2. Cancel mid-authentication -> sends cancel_session and resets
    await userInput.fill("alice");
    await submitBtn.click();
    await expect(userInput).toBeHidden();
    await expect(responseInput).toBeVisible();
    await expect(cancelBtn).toBeVisible();
    await cancelBtn.click();
    await expect(userInput).toBeVisible();
    await expect(responseInput).toBeHidden();
    await expect(cancelBtn).toBeHidden();
    await expect(errorEl).toBeHidden();

    // 3. Valid credentials + discovered desktop session -> launches session & webbar exits 0
    await userInput.fill("alice");
    await sessionInput.selectOption("wayland:sway-custom");
    await submitBtn.click();
    await expect(userInput).toBeHidden();
    await expect(responseInput).toBeVisible();
    await responseInput.fill("s3cr3t");
    await submitBtn.click();

    const { code, signal } = await exitPromise;
    expect(signal).toBeNull();
    expect(code).toBe(0);

    expect(recorded).toEqual([
      { type: "create_session", username: "alice" },
      { type: "post_auth_message_response", response: "wrong" },
      { type: "create_session", username: "alice" },
      { type: "cancel_session" },
      { type: "create_session", username: "alice" },
      { type: "post_auth_message_response", response: "s3cr3t" },
      { type: "start_session", cmd: ["sway", "--unsupported-gpu"], env: [] },
    ]);
  } finally {
    if (webbarProc.exitCode === null) {
      webbarProc.kill("SIGTERM");
    }
    uiServer.close();
    greetdServer.close();
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("examples/greeter responds to light and dark --desktop-* theme tokens with accessible contrast", async ({
  page,
}) => {
  test.skip(GREETER_TARGET !== "greeter", "theme token contrast check is for examples/greeter");
  const uiDir = process.env.WEBBAR_UI_DIR;
  const preloadPath = process.env.WEBBAR_PRELOAD_JS;
  expect(uiDir, "WEBBAR_UI_DIR must be set").toBeTruthy();
  expect(preloadPath, "WEBBAR_PRELOAD_JS must be set").toBeTruthy();

  const { server: uiServer, port: uiPort } = await startUiServer(uiDir, preloadPath);

  try {
    await page.goto(`http://127.0.0.1:${uiPort}/`);

    const themes = [
      {
        name: "light",
        expectLightSurface: true,
        vars: {
          "--desktop-window-bg": "#fafafa",
          "--desktop-card-bg": "#ffffff",
          "--desktop-window-fg": "#2e3436",
          "--desktop-accent-bg": "#3584e4",
        },
        expectedFgRgb: [46, 52, 54],
      },
      {
        name: "dark",
        expectLightSurface: false,
        vars: {
          "--desktop-window-bg": "#242424",
          "--desktop-card-bg": "#303030",
          "--desktop-window-fg": "#ffffff",
          "--desktop-accent-bg": "#3584e4",
        },
        expectedFgRgb: [255, 255, 255],
      },
    ];

    for (const theme of themes) {
      const metrics = await page.evaluate((vars) => {
        const root = document.documentElement;
        for (const [k, v] of Object.entries(vars)) {
          root.style.setProperty(k, v);
        }

        const parseRgba = (css) => {
          const canvas = document.createElement("canvas");
          canvas.width = 1;
          canvas.height = 1;
          const ctx = canvas.getContext("2d");
          ctx.fillStyle = css;
          ctx.fillRect(0, 0, 1, 1);
          const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data;
          return { r, g, b, a: a / 255 };
        };

        const composite = (fg, bg) => ({
          r: Math.round(fg.r * fg.a + bg.r * (1 - fg.a)),
          g: Math.round(fg.g * fg.a + bg.g * (1 - fg.a)),
          b: Math.round(fg.b * fg.a + bg.b * (1 - fg.a)),
          a: 1,
        });

        const luminance = ({ r, g, b }) => {
          const lin = [r, g, b].map((c) => {
            const s = c / 255;
            return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
          });
          return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
        };

        const contrast = (a, b) => {
          const l1 = Math.max(luminance(a), luminance(b));
          const l2 = Math.min(luminance(a), luminance(b));
          return (l1 + 0.05) / (l2 + 0.05);
        };

        const card = document.querySelector(".card");
        const input = document.querySelector("#username");
        const bodyStyle = getComputedStyle(document.body);
        const cardStyle = getComputedStyle(card);
        const inputStyle = getComputedStyle(input);

        // Sample the backdrop gradient's base stop from computed background-image
        const bgStops =
          bodyStyle.backgroundImage.match(/(?:rgba?|color)\([^)]+\)/g) ?? [];
        const backdropBase =
          bgStops.length > 0
            ? parseRgba(bgStops[bgStops.length - 1])
            : parseRgba(bodyStyle.backgroundColor);

        const cardBg = composite(parseRgba(cardStyle.backgroundColor), backdropBase);
        const cardFg = parseRgba(cardStyle.color);
        const inputBg = composite(parseRgba(inputStyle.backgroundColor), cardBg);
        const inputFg = parseRgba(inputStyle.color);

        return {
          backdropLum: luminance(backdropBase),
          cardLum: luminance(cardBg),
          cardFg: [cardFg.r, cardFg.g, cardFg.b],
          cardContrast: contrast(cardFg, cardBg),
          inputContrast: contrast(inputFg, inputBg),
        };
      }, theme.vars);

      // Verify CSS actually consumes --desktop-window-fg (catches dead variable names)
      expect(metrics.cardFg, `${theme.name} foreground token applied`).toEqual(
        theme.expectedFgRgb,
      );
      // Verify backdrop and card match theme polarity
      if (theme.expectLightSurface) {
        expect(metrics.backdropLum, "light theme backdrop should be light").toBeGreaterThan(0.5);
        expect(metrics.cardLum, "light theme card should be light").toBeGreaterThan(0.7);
      } else {
        expect(metrics.backdropLum, "dark theme backdrop should be dark").toBeLessThan(0.2);
        expect(metrics.cardLum, "dark theme card should be dark").toBeLessThan(0.2);
      }
      // Verify WCAG AA text contrast on both card and input
      expect(metrics.cardContrast, `${theme.name} card text contrast`).toBeGreaterThanOrEqual(4.5);
      expect(metrics.inputContrast, `${theme.name} input text contrast`).toBeGreaterThanOrEqual(4.5);
    }
  } finally {
    uiServer.close();
  }
});

test("examples/sway-greeter-svelte reuses the Svelte bar and hides workspaces, tray, stasis, and notifications", async ({
  page,
}) => {
  test.skip(
    GREETER_TARGET !== "sway-greeter-svelte",
    "bar composition check is for examples/sway-greeter-svelte",
  );
  const uiDir = process.env.WEBBAR_UI_DIR;
  const preloadPath = process.env.WEBBAR_PRELOAD_JS;
  expect(uiDir, "WEBBAR_UI_DIR must be set").toBeTruthy();
  expect(preloadPath, "WEBBAR_PRELOAD_JS must be set").toBeTruthy();

  const { server: uiServer, port: uiPort } = await startUiServer(uiDir, preloadPath);

  try {
    await page.goto(`http://127.0.0.1:${uiPort}/`);

    await expect(page.locator("header.centerbox")).toBeVisible();
    await expect(page.locator(".bar-center .clock")).toBeVisible();
    await expect(page.locator(".widget-cluster > .bar-btn")).toHaveCount(3);
    await expect(page.locator(".login-card")).toBeVisible();

    // Hidden widgets on login screen
    await expect(page.locator(".workspaces")).toHaveCount(0);
    await expect(page.locator(".tray")).toHaveCount(0);
    await expect(page.locator(".stasis-btn")).toHaveCount(0);
    await expect(page.locator(".notif-btn")).toHaveCount(0);

    // Bar popover opens cleanly above the greeter and closes with Escape
    await page.locator(".bar-center .clock").click();
    await expect(page.locator(".calendar")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator(".calendar")).toHaveCount(0);
  } finally {
    uiServer.close();
  }
});

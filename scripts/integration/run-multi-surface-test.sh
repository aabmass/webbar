#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$PWD"
WEBBAR_MULTI_SURFACE_DIR="${WEBBAR_MULTI_SURFACE_DIR:?WEBBAR_MULTI_SURFACE_DIR must be set}"

# shellcheck source=../harness/env.sh
source "$REPO_ROOT/scripts/harness/env.sh"
harness_init "webbar-multi-surface-test"

# 1. Start isolated session D-Bus
harness_start_session_dbus

# 2. Start headless Sway with exact 1280x720 geometry and zero gaps/borders
cat > "$WORK_DIR/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1280x720 position 0 0
gaps inner 0
gaps outer 0
default_border none
default_floating_border none
EOF

harness_start_sway "$WORK_DIR/sway.conf"

# 3. Spawn a tiled terminal window to observe compositor exclusive zones
foot --app-id "multi-surface-probe" -- sleep 300 >"$WORK_DIR/foot.log" 2>&1 &
harness_register_pid "$!"
harness_register_log "$WORK_DIR/foot.log"

probe_rect() {
  swaymsg -t get_tree | jq -r '
    .. | objects | select(.app_id? == "multi-surface-probe") | "\(.rect.y) \(.rect.height)"
  ' 2>/dev/null | head -n1
}

wait_probe_rect() {
  local want_y="$1"
  local want_h="$2"
  local label="$3"
  for _ in $(seq 1 120); do
    local got
    got="$(probe_rect || true)"
    if [ "$got" = "$want_y $want_h" ]; then
      echo "multi-surface-test: OK [$label] tiled rect y=$want_y height=$want_h"
      return 0
    fi
    sleep 0.05
  done
  echo "multi-surface-test: FAIL [$label] expected '$want_y $want_h', got '$(probe_rect || true)'" >&2
  return 1
}

wait_probe_rect 0 720 "baseline without webbar"

# 4. Launch real webbar with examples/multi-surface/webbar.json + --ws-bridge
WS_PORT="$(harness_pick_port)"
WS_TOKEN="multi-surface-test-token"
harness_register_log "$WORK_DIR/webbar.log"

WEBKIT_DISABLE_DMABUF_RENDERER=1 \
WEBKIT_DISABLE_COMPOSITING_MODE=1 \
GSK_RENDERER=cairo \
webbar \
  --dir "$WEBBAR_MULTI_SURFACE_DIR" \
  --ws-bridge "$WS_PORT" \
  --ws-token "$WS_TOKEN" \
  --no-notifications \
  --replace >"$WORK_DIR/webbar.log" 2>&1 &
WEBBAR_PID="$!"
harness_register_pid "$WEBBAR_PID"

# Verify BOTH top (38px) and bottom (52px) exclusive zones shrink the tiled window
wait_probe_rect 38 630 "top bar (38px) + bottom dock (52px) exclusive zones"

# 5. Verify SurfacesState snapshot, gapplication toggles, and WebSocket surface_set_visible
node - "$WS_PORT" "$WS_TOKEN" "$WORK_DIR/multi-probe.ppm" <<'NODE'
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";

const [port, token, probePpmPath] = process.argv.slice(2);
const ws = new WebSocket(`ws://127.0.0.1:${port}/?token=${token}`);

let latestSurfaces = null;
const waiters = [];

ws.addEventListener("message", (ev) => {
  const msg = JSON.parse(String(ev.data));
  if (msg.type === "surfaces_state") {
    latestSurfaces = msg.surfaces;
    for (let i = waiters.length - 1; i >= 0; i--) {
      if (waiters[i].pred(latestSurfaces)) {
        const { resolve, timer } = waiters.splice(i, 1)[0];
        clearTimeout(timer);
        resolve(latestSurfaces);
      }
    }
  }
});

function waitForSurfaces(label, pred, timeoutMs = 5000) {
  if (latestSurfaces && pred(latestSurfaces)) {
    return Promise.resolve(latestSurfaces);
  }
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(
        new Error(
          `Timed out waiting for surfaces (${label}); latest=${JSON.stringify(latestSurfaces)}`,
        ),
      );
    }, timeoutMs);
    waiters.push({ pred, resolve, timer });
  });
}

function getProbeRect() {
  const tree = JSON.parse(
    execFileSync("swaymsg", ["-t", "get_tree"], { encoding: "utf8" }),
  );
  const stack = [tree];
  while (stack.length) {
    const node = stack.pop();
    if (node.app_id === "multi-surface-probe") {
      return node.rect;
    }
    for (const child of node.nodes ?? []) stack.push(child);
    for (const child of node.floating_nodes ?? []) stack.push(child);
  }
  return null;
}

async function waitForProbeRect(wantY, wantH, label) {
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    const rect = getProbeRect();
    if (rect && rect.y === wantY && rect.height === wantH) {
      console.log(`multi-surface-test: OK [${label}] tiled rect y=${wantY} height=${wantH}`);
      return;
    }
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error(
    `Timed out waiting for tiled rect (${label}) want y=${wantY} h=${wantH}, got ${JSON.stringify(getProbeRect())}`,
  );
}

await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve, { once: true });
  ws.addEventListener("error", reject, { once: true });
});

// Initial SurfacesState snapshot
const initial = await waitForSurfaces("initial snapshot", (s) => s.length === 3);
const byId = (arr) => Object.fromEntries(arr.map((s) => [s.id, s]));
const initMap = byId(initial);
assert.equal(initMap.bar.visible, true);
assert.equal(initMap.bar.layer, "top");
assert.deepEqual(initMap.bar.anchors, ["top", "left", "right"]);
assert.equal(initMap.dock.visible, true);
assert.equal(initMap.dock.layer, "top");
assert.deepEqual(initMap.dock.anchors, ["bottom", "left", "right"]);
assert.equal(initMap.launcher.visible, false);
assert.equal(initMap.launcher.layer, "overlay");
assert.deepEqual(initMap.launcher.anchors, []);
console.log("multi-surface-test: OK [initial SurfacesState snapshot]");

// Toggle dock OFF via gapplication -> bottom exclusive zone must be released (height 630 -> 682)
execFileSync("gapplication", ["action", "dev.local.webbar", "surface-toggle", "'dock'"]);
await waitForSurfaces("dock hidden", (s) => byId(s).dock.visible === false);
await waitForProbeRect(38, 682, "dock hidden releases bottom 52px exclusive zone");

// Toggle launcher ON via gapplication -> overlay becomes visible without changing tiled geometry
execFileSync("gapplication", ["action", "dev.local.webbar", "surface-toggle", "'launcher'"]);
await waitForSurfaces("launcher shown", (s) => byId(s).launcher.visible === true);
await waitForProbeRect(38, 682, "overlay launcher ignores exclusive zones");

// Restore dock ON and hide launcher via WebSocket BridgeRequest (window.webbar.surfaces API)
ws.send(JSON.stringify({ type: "surface_set_visible", id: "dock", visible: true }));
await waitForSurfaces(
  "all three surfaces visible",
  (s) => byId(s).dock.visible === true && byId(s).launcher.visible === true,
);
await waitForProbeRect(38, 630, "dock restored via WS reclaims bottom 52px exclusive zone");

// Poll grim until all 3 surfaces (bar, dock, launcher) paint visible UI pixels
function sampleMaxRgb() {
  execFileSync("grim", ["-t", "ppm", probePpmPath]);
  const ppm = fs.readFileSync(probePpmPath);
  let idx = 0;
  let tokens = 0;
  while (tokens < 4 && idx < ppm.length) {
    while (idx < ppm.length && ppm[idx] <= 32) idx++;
    if (ppm[idx] === 35) { while (idx < ppm.length && ppm[idx] !== 10) idx++; continue; }
    while (idx < ppm.length && ppm[idx] > 32) idx++;
    tokens++;
  }
  const header = ppm.subarray(0, idx).toString("ascii").trim().split(/\s+/);
  const width = Number(header[1]);
  const dataStart = idx + 1;
  const maxPixel = (x0, x1, y0, y1) => {
    let best = [0, 0, 0];
    for (let y = y0; y <= y1; y += 4) {
      for (let x = x0; x <= x1; x += 4) {
        const o = dataStart + (y * width + x) * 3;
        const r = ppm[o], g = ppm[o + 1], b = ppm[o + 2];
        if (r + g + b > best[0] + best[1] + best[2]) best = [r, g, b];
      }
    }
    return best;
  };
  return {
    barMax: maxPixel(0, 1279, 4, 34),
    dockMax: maxPixel(0, 1279, 672, 716),
    launcherMax: maxPixel(380, 900, 210, 510),
  };
}
const sum = (rgb) => rgb[0] + rgb[1] + rgb[2];
let painted = sampleMaxRgb();
const paintDeadline = Date.now() + 5000;
while (
  Date.now() < paintDeadline &&
  !(sum(painted.barMax) > 200 && sum(painted.dockMax) > 200 && sum(painted.launcherMax) > 200)
) {
  await new Promise((r) => setTimeout(r, 100));
  painted = sampleMaxRgb();
}
console.log("multi-surface-test: max RGB", painted);
assert.ok(sum(painted.barMax) > 200, `top bar must paint visible UI pixels (got ${painted.barMax})`);
assert.ok(sum(painted.dockMax) > 200, `bottom dock must paint visible UI pixels (got ${painted.dockMax})`);
assert.ok(sum(painted.launcherMax) > 200, `overlay launcher must paint visible UI pixels (got ${painted.launcherMax})`);
console.log("multi-surface-test: OK [bar, dock, and overlay launcher all painted visible pixels]");

ws.send(JSON.stringify({ type: "surface_set_visible", id: "launcher", visible: false }));
await waitForSurfaces(
  "launcher hidden via WS",
  (s) => byId(s).launcher.visible === false,
);

ws.close();
console.log("multi-surface-test: ALL ASSERTIONS PASSED");
NODE

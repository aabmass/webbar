#!/usr/bin/env bash
set -euo pipefail

if [ ! -f scripts/integration/parity.spec.mjs ] || [ ! -f scripts/harness/env.sh ]; then
  echo "run-parity-test: run from the webbar repository root" >&2
  exit 1
fi

REPO_ROOT="$PWD"
WEBBAR_UI_DIR="${WEBBAR_UI_DIR:?WEBBAR_UI_DIR must be set}"
WEBBAR_TARGET="${WEBBAR_TARGET:-bar}"

# shellcheck source=../harness/env.sh
source "$REPO_ROOT/scripts/harness/env.sh"
harness_init "webbar-integration"

mkdir -p \
  "$HOME/.config/stasis" \
  "$HOME/.config/glib-2.0/settings"

export GSETTINGS_BACKEND="keyfile"
export ADW_DISABLE_PORTAL="1"

gsettings set org.gnome.desktop.interface color-scheme prefer-dark

# 1. Start isolated session & system D-Bus
harness_start_session_dbus
harness_start_system_dbus

# 2. Start isolated headless Sway
cat > "$WORK_DIR/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1600x900 position 0 0
EOF
harness_start_sway "$WORK_DIR/sway.conf"

# 3. Start D-Bus fixture services before webbar (bluetooth.rs & backlight.rs query once at startup)
harness_register_log "$WORK_DIR/bluez.log"
python3 "$REPO_ROOT/scripts/harness/services/bluez.py" >"$WORK_DIR/bluez.log" 2>&1 &
harness_register_pid "$!"
harness_wait_dbus_name system org.bluez

harness_register_log "$WORK_DIR/ddcutil.log"
python3 "$REPO_ROOT/scripts/harness/services/ddcutil.py" >"$WORK_DIR/ddcutil.log" 2>&1 &
harness_register_pid "$!"
harness_wait_dbus_name session com.ddcutil.DdcutilService

# 4. Start isolated PulseAudio with deterministic sinks, source, and playback stream
harness_start_pulseaudio

# 5. Start real Stasis daemon on the isolated bus & Sway session
cp "$REPO_ROOT/scripts/integration/stasis.rune" "$HOME/.config/stasis/stasis.rune"
harness_register_log "$WORK_DIR/stasis.log"
stasis -c "$HOME/.config/stasis/stasis.rune" >"$WORK_DIR/stasis.log" 2>&1 &
STASIS_PID="$!"
harness_register_pid "$STASIS_PID"

STASIS_READY=0
for _ in $(seq 1 50); do
  if ! kill -0 "$STASIS_PID" 2>/dev/null; then
    echo "run-parity-test: stasis exited early" >&2
    exit 1
  fi
  if stasis info >/dev/null 2>&1; then
    STASIS_READY=1
    break
  fi
  sleep 0.1
done
if [ "$STASIS_READY" -ne 1 ]; then
  echo "run-parity-test: timed out waiting for stasis" >&2
  exit 1
fi

# 6. Start webbar --ws-bridge after all backend daemons are ready
WS_PORT="$(harness_pick_port)"
WS_TOKEN="integration-test-token"
harness_register_log "$WORK_DIR/webbar.log"
webbar --ws-bridge "$WS_PORT" --ws-token "$WS_TOKEN" >"$WORK_DIR/webbar.log" 2>&1 &
WEBBAR_PID="$!"
harness_register_pid "$WEBBAR_PID"

WS_READY=0
for _ in $(seq 1 50); do
  if ! kill -0 "$WEBBAR_PID" 2>/dev/null; then
    echo "run-parity-test: webbar exited early" >&2
    exit 1
  fi
  if grep -q "ws-bridge listening" "$WORK_DIR/webbar.log" 2>/dev/null; then
    WS_READY=1
    break
  fi
  sleep 0.1
done
if [ "$WS_READY" -ne 1 ]; then
  echo "run-parity-test: timed out waiting for webbar ws-bridge" >&2
  exit 1
fi

# 7. Serve built UI
UI_PORT="$(harness_pick_port)"
harness_register_log "$WORK_DIR/http-ui.log"
python3 -m http.server "$UI_PORT" --bind 127.0.0.1 --directory "$WEBBAR_UI_DIR" >"$WORK_DIR/http-ui.log" 2>&1 &
harness_register_pid "$!"
harness_wait_http "http://127.0.0.1:$UI_PORT/index.html"

# 8. Run Playwright parity & real-backend integration tests
TEST_DIR="$WORK_DIR/test-dir"
mkdir -p "$TEST_DIR"
cp "$REPO_ROOT/scripts/integration/"* "$TEST_DIR/"
if [ -n "${PLAYWRIGHT_NODE_MODULES:-}" ]; then
  ln -s "$PLAYWRIGHT_NODE_MODULES" "$TEST_DIR/node_modules"
fi

export WEBBAR_TARGET
export WEBBAR_URL="http://127.0.0.1:$UI_PORT/?wsPort=$WS_PORT&wsToken=$WS_TOKEN"

cd "$TEST_DIR"
playwright test --config "$TEST_DIR/playwright.config.mjs"

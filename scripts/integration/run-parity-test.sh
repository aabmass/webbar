#!/usr/bin/env bash
set -euo pipefail

if [ ! -f scripts/integration/parity.spec.mjs ]; then
  echo "run-parity-test: run from the webbar repository root" >&2
  exit 1
fi

REPO_ROOT="$PWD"
WEBBAR_BIN="${WEBBAR_BIN:?WEBBAR_BIN must be set}"
WEBBAR_REACT_UI_DIR="${WEBBAR_REACT_UI_DIR:?WEBBAR_REACT_UI_DIR must be set}"
WEBBAR_SVELTE_UI_DIR="${WEBBAR_SVELTE_UI_DIR:?WEBBAR_SVELTE_UI_DIR must be set}"

unset SWAYSOCK I3SOCK WAYLAND_DISPLAY DISPLAY

WORK_DIR="$(mktemp -d -t webbar-integration.XXXXXX)"
export HOME="$WORK_DIR/home"
export XDG_RUNTIME_DIR="$WORK_DIR/runtime"
export XDG_CONFIG_HOME="$WORK_DIR/config"
export XDG_CACHE_HOME="$WORK_DIR/cache"
export XDG_DATA_HOME="$WORK_DIR/data"
export TZ="UTC"
export LANG="C.UTF-8"
export LC_ALL="C.UTF-8"
mkdir -p "$HOME" "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME/stasis" "$XDG_CACHE_HOME" "$XDG_DATA_HOME"
chmod 700 "$XDG_RUNTIME_DIR"

PIDS=()
cleanup() {
  local rc=$?
  if [ "$rc" -ne 0 ]; then
    echo "=== stasis.log ===" >&2
    cat "$WORK_DIR/stasis.log" >&2 || true
    echo "=== webbar.log ===" >&2
    cat "$WORK_DIR/webbar.log" >&2 || true
  fi
  for pid in "${PIDS[@]:-}"; do
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
    fi
  done
  wait 2>/dev/null || true
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT INT TERM

# 1. Start isolated session D-Bus
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
cat > "$WORK_DIR/dbus-session.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=$XDG_RUNTIME_DIR/bus</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
    <allow user="*"/>
  </policy>
</busconfig>
EOF
dbus-daemon --config-file="$WORK_DIR/dbus-session.conf" --nofork --nopidfile >"$WORK_DIR/dbus.log" 2>&1 &
PIDS+=("$!")
for _ in $(seq 1 50); do
  [ -S "$XDG_RUNTIME_DIR/bus" ] && break
  sleep 0.05
done
if [ ! -S "$XDG_RUNTIME_DIR/bus" ]; then
  echo "run-parity-test: dbus-daemon failed to start" >&2
  cat "$WORK_DIR/dbus.log" >&2
  exit 1
fi

# 2. Start isolated headless Sway
cat > "$WORK_DIR/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1600x900 position 0 0
EOF

WLR_BACKENDS=headless \
WLR_RENDERER=pixman \
WLR_LIBINPUT_NO_DEVICES=1 \
XDG_CURRENT_DESKTOP=sway \
sway --config "$WORK_DIR/sway.conf" --unsupported-gpu >"$WORK_DIR/sway.log" 2>&1 &
SWAY_PID="$!"
PIDS+=("$SWAY_PID")

SWAYSOCK=""
WAYLAND_DISPLAY=""
for _ in $(seq 1 100); do
  if ! kill -0 "$SWAY_PID" 2>/dev/null; then
    echo "run-parity-test: headless sway exited early:" >&2
    cat "$WORK_DIR/sway.log" >&2
    exit 1
  fi
  sock="$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock 2>/dev/null | head -n1 || true)"
  wl="$(ls "$XDG_RUNTIME_DIR"/wayland-* 2>/dev/null | grep -v '\.lock$' | head -n1 || true)"
  if [ -n "$sock" ] && [ -n "$wl" ]; then
    SWAYSOCK="$sock"
    I3SOCK="$sock"
    WAYLAND_DISPLAY="$(basename "$wl")"
    export SWAYSOCK I3SOCK WAYLAND_DISPLAY
    if swaymsg -t get_outputs >/dev/null 2>&1; then
      break
    fi
  fi
  sleep 0.1
done
if [ -z "$SWAYSOCK" ] || [ -z "$WAYLAND_DISPLAY" ]; then
  echo "run-parity-test: timed out waiting for headless sway" >&2
  cat "$WORK_DIR/sway.log" >&2
  exit 1
fi

# 3. Start real Stasis daemon on the isolated bus & Sway session
cp "$REPO_ROOT/scripts/integration/stasis.rune" "$XDG_CONFIG_HOME/stasis/stasis.rune"
stasis -c "$XDG_CONFIG_HOME/stasis/stasis.rune" >"$WORK_DIR/stasis.log" 2>&1 &
STASIS_PID="$!"
PIDS+=("$STASIS_PID")

STASIS_READY=0
for _ in $(seq 1 50); do
  if ! kill -0 "$STASIS_PID" 2>/dev/null; then
    echo "run-parity-test: stasis exited early:" >&2
    cat "$WORK_DIR/stasis.log" >&2
    exit 1
  fi
  if stasis info >/dev/null 2>&1; then
    STASIS_READY=1
    break
  fi
  sleep 0.1
done
if [ "$STASIS_READY" -ne 1 ]; then
  echo "run-parity-test: timed out waiting for stasis:" >&2
  cat "$WORK_DIR/stasis.log" >&2
  exit 1
fi

# 4. Start webbar --ws-bridge
pick_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()'
}

WS_PORT="$(pick_port)"
WS_TOKEN="integration-test-token"
"$WEBBAR_BIN" --ws-bridge "$WS_PORT" --ws-token "$WS_TOKEN" >"$WORK_DIR/webbar.log" 2>&1 &
WEBBAR_PID="$!"
PIDS+=("$WEBBAR_PID")

for _ in $(seq 1 50); do
  if ! kill -0 "$WEBBAR_PID" 2>/dev/null; then
    echo "run-parity-test: webbar exited early:" >&2
    cat "$WORK_DIR/webbar.log" >&2
    exit 1
  fi
  if grep -q "ws-bridge listening" "$WORK_DIR/webbar.log" 2>/dev/null; then
    break
  fi
  sleep 0.1
done

# 5. Serve built React and Svelte UIs
REACT_PORT="$(pick_port)"
SVELTE_PORT="$(pick_port)"

python3 -m http.server "$REACT_PORT" --bind 127.0.0.1 --directory "$WEBBAR_REACT_UI_DIR" >"$WORK_DIR/http-react.log" 2>&1 &
PIDS+=("$!")
python3 -m http.server "$SVELTE_PORT" --bind 127.0.0.1 --directory "$WEBBAR_SVELTE_UI_DIR" >"$WORK_DIR/http-svelte.log" 2>&1 &
PIDS+=("$!")

for port in "$REACT_PORT" "$SVELTE_PORT"; do
  for _ in $(seq 1 50); do
    if curl -fsS "http://127.0.0.1:$port/index.html" >/dev/null 2>&1; then
      break
    fi
    sleep 0.05
  done
done

# 6. Run Playwright parity & real-backend integration tests
TEST_DIR="$WORK_DIR/test-dir"
mkdir -p "$TEST_DIR"
cp "$REPO_ROOT/scripts/integration/"* "$TEST_DIR/"
if [ -n "${PLAYWRIGHT_NODE_MODULES:-}" ]; then
  ln -s "$PLAYWRIGHT_NODE_MODULES" "$TEST_DIR/node_modules"
fi

export PLAYWRIGHT_OUTPUT_DIR="$WORK_DIR/test-results"
export WEBBAR_REACT_URL="http://127.0.0.1:$REACT_PORT/?wsPort=$WS_PORT&wsToken=$WS_TOKEN"
export WEBBAR_SVELTE_URL="http://127.0.0.1:$SVELTE_PORT/?wsPort=$WS_PORT&wsToken=$WS_TOKEN"

cd "$TEST_DIR"
playwright test --config "$TEST_DIR/playwright.config.mjs"

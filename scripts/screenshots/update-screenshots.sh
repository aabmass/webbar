#!/usr/bin/env bash
set -euo pipefail

if [ ! -f examples/sway-full-bar/package.json ] || [ ! -f scripts/screenshots/demo-fixture.js ]; then
  echo "update-screenshots: run from the webbar repository root" >&2
  exit 1
fi

REPO_ROOT="$PWD"
OUT_DIR="${OUT_DIR:-$REPO_ROOT/docs/screenshots}"
mkdir -p "$OUT_DIR"

WEBBAR_UI_DIR="${WEBBAR_UI_DIR:?WEBBAR_UI_DIR must be set}"
GREETER_UI_DIR="${GREETER_UI_DIR:-}"
FONT_DIRS="${FONT_DIRS:-}"

# shellcheck source=../harness/env.sh
source "$REPO_ROOT/scripts/harness/env.sh"
harness_init "webbar-screenshots"

# Isolated fontconfig so Inter + JetBrains Mono render identically everywhere
if [ -n "$FONT_DIRS" ]; then
  mkdir -p "$HOME/.cache"
  FONTCONFIG_FILE="$WORK_DIR/fonts.conf"
  {
    echo '<?xml version="1.0"?>'
    echo '<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">'
    echo '<fontconfig>'
    IFS=':' read -ra dirs <<< "$FONT_DIRS"
    for d in "${dirs[@]}"; do
      [ -n "$d" ] && printf '  <dir>%s</dir>\n' "$d"
    done
    printf '  <cachedir>%s/.cache/fontconfig</cachedir>\n' "$HOME"
    echo '  <match target="font">'
    echo '    <edit name="antialias" mode="assign"><bool>true</bool></edit>'
    echo '    <edit name="hinting" mode="assign"><bool>true</bool></edit>'
    echo '    <edit name="hintstyle" mode="assign"><const>hintslight</const></edit>'
    echo '    <edit name="rgba" mode="assign"><const>none</const></edit>'
    echo '  </match>'
    echo '</fontconfig>'
  } > "$FONTCONFIG_FILE"
  export FONTCONFIG_FILE
fi

# 1. Prepare temporary UI directory with injected demo-fixture.js
UI_DIR="$WORK_DIR/ui"
cp -r "$WEBBAR_UI_DIR" "$UI_DIR"
chmod -R u+w "$UI_DIR"
cp "$REPO_ROOT/scripts/screenshots/demo-fixture.js" "$UI_DIR/demo-fixture.js"
python3 - "$UI_DIR/index.html" <<'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1])
html = p.read_text()
tag = '<script src="/demo-fixture.js"></script>'
if "<head>" in html:
    html = html.replace("<head>", "<head>\n    " + tag, 1)
else:
    html = tag + "\n" + html
p.write_text(html)
PY

# Pick an ephemeral free TCP port and serve the patched UI
PORT="$(harness_pick_port)"
harness_register_log "$WORK_DIR/http.log"
python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$UI_DIR" >"$WORK_DIR/http.log" 2>&1 &
harness_register_pid "$!"
harness_wait_http "http://127.0.0.1:$PORT/index.html"

# 2. Render vector wallpapers to 1600x900 PNGs
rsvg-convert -w 1600 -h 900 "$REPO_ROOT/scripts/screenshots/wallpapers/dark.svg" -o "$WORK_DIR/dark.png"
rsvg-convert -w 1600 -h 900 "$REPO_ROOT/scripts/screenshots/wallpapers/light.svg" -o "$WORK_DIR/light.png"

# 3. Foot terminal configs (dark & light)
cat > "$WORK_DIR/foot-dark.ini" <<'EOF'
[main]
font=JetBrains Mono:size=10.5
pad=16x12
dpi-aware=no
initial-color-theme=dark

[cursor]
style=beam
blink=no

[colors-dark]
alpha=0.92
background=18181c
foreground=ececf1
regular0=26262c
regular1=f2726f
regular2=6fcf97
regular3=f2c94c
regular4=8ab4f8
regular5=bb9af7
regular6=7dcfff
regular7=c0caf5
EOF

cat > "$WORK_DIR/foot-light.ini" <<'EOF'
[main]
font=JetBrains Mono:size=10.5
pad=16x12
dpi-aware=no
initial-color-theme=light

[cursor]
style=beam
blink=no

[colors-light]
alpha=0.94
background=fafafc
foreground=18181f
regular0=e7e7ee
regular1=d63638
regular2=1f8f54
regular3=c5820a
regular4=2b6de0
regular5=7c5ce0
regular6=0e7490
regular7=5c5c6a
EOF

# 4. Start isolated session D-Bus
harness_start_session_dbus

# 5. Start isolated headless Sway
cat > "$WORK_DIR/sway.conf" <<EOF
output HEADLESS-1 resolution 1600x900 position 0 0
gaps inner 14
gaps outer 8
default_border pixel 2
default_floating_border pixel 2
client.focused          #8ab4f8 #18181c #ececf1 #8ab4f8 #8ab4f8
client.focused_inactive #323442 #18181c #9a9aa6 #323442 #323442
client.unfocused        #2a2c38 #18181c #9a9aa6 #2a2c38 #2a2c38
assign [app_id="^demo-hero-"] workspace 1
assign [app_id="demo-ws-2"] workspace 2
assign [app_id="demo-ws-3"] workspace 3
assign [app_id="demo-ws-q"] workspace q
assign [app_id="demo-ws-w"] workspace w
EOF

harness_start_sway "$WORK_DIR/sway.conf"

set_wallpaper() {
  local img="$1"
  if [ -n "${SWAYBG_PID:-}" ] && kill -0 "$SWAYBG_PID" 2>/dev/null; then
    kill "$SWAYBG_PID" 2>/dev/null || true
    wait "$SWAYBG_PID" 2>/dev/null || true
  fi
  swaybg -i "$img" -m fill >"$WORK_DIR/swaybg.log" 2>&1 &
  SWAYBG_PID="$!"
  harness_register_pid "$SWAYBG_PID"
  sleep 0.3
}

ws_window_count() {
  local ws="$1"
  swaymsg -t get_tree | jq -r --arg ws "$ws" '
    [.. | objects | select(.type? == "workspace" and .name? == $ws)
     | .. | objects | select((.type? == "con" or .type? == "floating_con") and .pid? != null)] | length
  ' 2>/dev/null || echo 0
}

wait_ws_count() {
  local ws="$1"
  local want="$2"
  for _ in $(seq 1 100); do
    local got
    got="$(ws_window_count "$ws")"
    if [ "${got:-0}" -eq "$want" ]; then
      return 0
    fi
    sleep 0.05
  done
  echo "update-screenshots: timed out waiting for workspace '$ws' to have $want windows (got $(ws_window_count "$ws"))" >&2
  return 1
}

spawn_pane() {
  local app_id="$1"
  local foot_cfg="$2"
  local mode="$3"
  local theme="$4"
  foot --app-id "$app_id" --config "$foot_cfg" -- bash "$REPO_ROOT/scripts/screenshots/demo-panes.sh" "$mode" "$theme" "$REPO_ROOT" >"$WORK_DIR/foot-${app_id}-${theme}.log" 2>&1 &
  harness_register_pid "$!"
}

populate_bg_workspaces() {
  local foot_cfg="$1"
  # Give workspaces 2, 3, q, w real windows so the 2-row workspace grid shows dots
  for spec in "2:2" "3:1" "q:3" "w:1"; do
    local ws="${spec%%:*}"
    local count="${spec##*:}"
    for _ in $(seq 1 "$count"); do
      spawn_pane "demo-ws-$ws" "$foot_cfg" idle dark
    done
    wait_ws_count "$ws" "$count"
  done
}

populate_hero_workspace() {
  local theme="$1"
  local foot_cfg="$WORK_DIR/foot-${theme}.ini"

  swaymsg "workspace 1" >/dev/null
  # Close any existing hero windows on workspace 1 and wait until empty
  swaymsg '[app_id="^demo-hero-"] kill' >/dev/null 2>&1 || true
  wait_ws_count "1" 0
  swaymsg "workspace 1" >/dev/null

  # Left pane: syntax-highlighted App.tsx
  swaymsg "splith" >/dev/null
  spawn_pane "demo-hero-code" "$foot_cfg" code "$theme"
  wait_ws_count "1" 1

  # Top-right pane: zero-PII webbar fetch card
  spawn_pane "demo-hero-fetch" "$foot_cfg" fetch "$theme"
  wait_ws_count "1" 2

  # Split vertically on the right pane for bottom-right status terminal
  swaymsg '[app_id="demo-hero-fetch"] focus' >/dev/null
  swaymsg "splitv" >/dev/null
  spawn_pane "demo-hero-status" "$foot_cfg" status "$theme"
  wait_ws_count "1" 3

  # Balance left pane wider (~56%) and focus the left code pane
  swaymsg '[app_id="demo-hero-code"] focus' >/dev/null
  swaymsg "resize set width 56 ppt" >/dev/null
  sleep 0.4
}

WEBBAR_PID=""
harness_register_log "$WORK_DIR/webbar.log"
launch_bar() {
  local query="$1"
  local wait_secs="${2:-2.2}"
  if [ -n "$WEBBAR_PID" ] && kill -0 "$WEBBAR_PID" 2>/dev/null; then
    kill "$WEBBAR_PID" 2>/dev/null || true
    wait "$WEBBAR_PID" 2>/dev/null || true
  fi

  WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  WEBKIT_DISABLE_COMPOSITING_MODE=1 \
  GSK_RENDERER=cairo \
  webbar \
    --url "http://127.0.0.1:$PORT/index.html?$query" \
    --replace >"$WORK_DIR/webbar.log" 2>&1 &
  WEBBAR_PID="$!"
  harness_register_pid "$WEBBAR_PID"

  sleep "$wait_secs"
  if ! kill -0 "$WEBBAR_PID" 2>/dev/null; then
    echo "update-screenshots: webbar exited early for query '$query'" >&2
    exit 1
  fi
}

echo "update-screenshots: setting up headless Sway workspaces..."
set_wallpaper "$WORK_DIR/dark.png"
populate_bg_workspaces "$WORK_DIR/foot-dark.ini"
populate_hero_workspace "dark"

echo "update-screenshots: capturing hero-dark.png..."
launch_bar "theme=dark" 2.5
grim "$OUT_DIR/hero-dark.png"

echo "update-screenshots: capturing popover-audio.png..."
launch_bar "theme=dark&open=audio" 2.0
grim -g "960,0 640x680" "$OUT_DIR/popover-audio.png"

echo "update-screenshots: capturing popover-calendar.png..."
launch_bar "theme=dark&open=calendar" 2.0
grim -g "480,0 640x480" "$OUT_DIR/popover-calendar.png"

echo "update-screenshots: capturing popover-bluetooth.png..."
launch_bar "theme=dark&open=bluetooth" 2.0
grim -g "1000,0 600x480" "$OUT_DIR/popover-bluetooth.png"

echo "update-screenshots: capturing popover-display.png..."
launch_bar "theme=dark&open=display" 2.0
grim -g "1000,0 600x400" "$OUT_DIR/popover-display.png"

echo "update-screenshots: capturing popover-notifications.png..."
launch_bar "theme=dark&open=notifications" 2.0
grim -g "480,0 640x520" "$OUT_DIR/popover-notifications.png"

echo "update-screenshots: capturing notifications-toasts.png..."
launch_bar "theme=dark&toasts=1" 2.0
grim -g "1000,0 600x480" "$OUT_DIR/notifications-toasts.png"

if [ -n "${WEBBAR_MULTI_SURFACE_DIR:-}" ] && [ -d "$WEBBAR_MULTI_SURFACE_DIR" ]; then
  echo "update-screenshots: capturing multi-surface.png..."
  if [ -n "$WEBBAR_PID" ] && kill -0 "$WEBBAR_PID" 2>/dev/null; then
    kill "$WEBBAR_PID" 2>/dev/null || true
    wait "$WEBBAR_PID" 2>/dev/null || true
  fi
  harness_register_log "$WORK_DIR/webbar-multi.log"
  WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  WEBKIT_DISABLE_COMPOSITING_MODE=1 \
  GSK_RENDERER=cairo \
  webbar \
    --dir "$WEBBAR_MULTI_SURFACE_DIR" \
    --no-notifications \
    --replace >"$WORK_DIR/webbar-multi.log" 2>&1 &
  WEBBAR_PID="$!"
  harness_register_pid "$WEBBAR_PID"
  sleep 2.0
  if ! kill -0 "$WEBBAR_PID" 2>/dev/null; then
    echo "update-screenshots: webbar exited early in multi-surface mode" >&2
    exit 1
  fi
  gapplication action dev.local.webbar surface-show "'launcher'"
  sleep 0.5
  grim "$OUT_DIR/multi-surface.png"
fi

echo "update-screenshots: switching to light theme and capturing hero-light.png..."
set_wallpaper "$WORK_DIR/light.png"
swaymsg "client.focused #2b6de0 #fafafc #18181f #2b6de0 #2b6de0" >/dev/null
swaymsg "client.focused_inactive #cfd4e2 #fafafc #5c5c6a #cfd4e2 #cfd4e2" >/dev/null
swaymsg "client.unfocused #d8dce8 #fafafc #5c5c6a #d8dce8 #d8dce8" >/dev/null
populate_hero_workspace "light"
launch_bar "theme=light" 2.5
grim "$OUT_DIR/hero-light.png"

PNGS=(
  "$OUT_DIR/hero-dark.png"
  "$OUT_DIR/hero-light.png"
  "$OUT_DIR/popover-audio.png"
  "$OUT_DIR/popover-calendar.png"
  "$OUT_DIR/popover-bluetooth.png"
  "$OUT_DIR/popover-display.png"
  "$OUT_DIR/popover-notifications.png"
  "$OUT_DIR/notifications-toasts.png"
)

if [ -n "$GREETER_UI_DIR" ]; then
  echo "update-screenshots: capturing greeter.png..."
  set_wallpaper "$WORK_DIR/dark.png"
  swaymsg '[app_id="^demo-hero-"] kill' >/dev/null 2>&1 || true
  wait_ws_count "1" 0
  GREETER_TMP="$WORK_DIR/greeter-ui"
  cp -r "$GREETER_UI_DIR" "$GREETER_TMP"
  chmod -R u+w "$GREETER_TMP"
  cp "$REPO_ROOT/scripts/screenshots/demo-fixture.js" "$GREETER_TMP/demo-fixture.js"
  python3 - "$GREETER_TMP/index.html" <<'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1])
html = p.read_text()
tag = '<script src="/demo-fixture.js"></script>'
html = html.replace("<head>", "<head>\n    " + tag, 1) if "<head>" in html else tag + "\n" + html
p.write_text(html)
PY
  GREETER_PORT="$(harness_pick_port)"
  harness_register_log "$WORK_DIR/http-greeter.log"
  python3 -m http.server "$GREETER_PORT" --bind 127.0.0.1 --directory "$GREETER_TMP" >"$WORK_DIR/http-greeter.log" 2>&1 &
  harness_register_pid "$!"
  harness_wait_http "http://127.0.0.1:$GREETER_PORT/index.html"

  if [ -n "$WEBBAR_PID" ] && kill -0 "$WEBBAR_PID" 2>/dev/null; then
    kill "$WEBBAR_PID" 2>/dev/null || true
    wait "$WEBBAR_PID" 2>/dev/null || true
  fi
  WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  WEBKIT_DISABLE_COMPOSITING_MODE=1 \
  GSK_RENDERER=cairo \
  webbar \
    --url "http://127.0.0.1:$GREETER_PORT/index.html?theme=dark" \
    --replace >"$WORK_DIR/webbar.log" 2>&1 &
  WEBBAR_PID="$!"
  harness_register_pid "$WEBBAR_PID"
  sleep 2.2
  if ! kill -0 "$WEBBAR_PID" 2>/dev/null; then
    echo "update-screenshots: webbar exited early for greeter" >&2
    exit 1
  fi
  grim "$OUT_DIR/greeter.png"
  PNGS+=("$OUT_DIR/greeter.png")
fi

if [ -f "$OUT_DIR/multi-surface.png" ]; then
  PNGS+=("$OUT_DIR/multi-surface.png")
fi

echo "update-screenshots: optimizing PNGs with oxipng..."
oxipng -o 3 --strip safe --quiet "${PNGS[@]}"

echo "update-screenshots: done! Wrote screenshots to $OUT_DIR"
ls -lh "$OUT_DIR"

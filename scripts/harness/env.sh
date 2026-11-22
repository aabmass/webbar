#!/usr/bin/env bash
# Shared isolated desktop/D-Bus/Wayland harness helpers for webbar scripts.

harness_init() {
  local prefix="${1:-webbar-harness}"

  if [ -z "${NIX_BUILD_TOP:-}" ]; then
    echo "harness: run via Nix sandbox (nix flake check / nix run .#update-screenshots)" >&2
    exit 1
  fi

  WORK_DIR="$(mktemp -d -t "${prefix}.XXXXXX")"
  export WORK_DIR
  export HOME="$WORK_DIR/home"
  export XDG_RUNTIME_DIR="$WORK_DIR/runtime"
  export TZ="UTC"
  export LC_ALL="C.UTF-8"
  mkdir -p "$HOME" "$XDG_RUNTIME_DIR"
  chmod 700 "$XDG_RUNTIME_DIR"

  HARNESS_PIDS=()
  HARNESS_DUMP_LOGS=()
  trap harness_cleanup EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
}

harness_register_pid() {
  local pid="$1"
  if [ -n "$pid" ]; then
    HARNESS_PIDS+=("$pid")
  fi
}

harness_register_log() {
  local path="$1"
  if [ -n "$path" ]; then
    HARNESS_DUMP_LOGS+=("$path")
  fi
}

harness_cleanup() {
  local rc=$?
  trap - EXIT INT TERM
  if [ "$rc" -ne 0 ]; then
    local log_path
    for log_path in "${HARNESS_DUMP_LOGS[@]:-}"; do
      if [ -n "$log_path" ] && [ -f "$log_path" ]; then
        echo "=== $(basename "$log_path") ===" >&2
        cat "$log_path" >&2 || true
      fi
    done
  fi

  local pid
  for pid in "${HARNESS_PIDS[@]:-}"; do
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
    fi
  done
  wait 2>/dev/null || true
  if [ -n "${WORK_DIR:-}" ]; then
    rm -rf "$WORK_DIR" || true
  fi
}

harness_write_dbus_conf() {
  local conf_path="$1"
  local socket_path="$2"
  cat > "$conf_path" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=$socket_path</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
    <allow user="*"/>
  </policy>
</busconfig>
EOF
}

_harness_start_dbus() {
  local label="$1"
  local conf_path="$2"
  local socket_path="$3"
  local log_path="$4"

  harness_write_dbus_conf "$conf_path" "$socket_path"
  harness_register_log "$log_path"
  dbus-daemon --config-file="$conf_path" --nofork --nopidfile >"$log_path" 2>&1 &
  local pid="$!"
  harness_register_pid "$pid"

  for _ in $(seq 1 50); do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "harness: $label dbus-daemon exited early" >&2
      return 1
    fi
    if [ -S "$socket_path" ]; then
      return 0
    fi
    sleep 0.05
  done

  echo "harness: timed out waiting for $label dbus-daemon socket at $socket_path" >&2
  return 1
}

harness_start_session_dbus() {
  local socket_path="$XDG_RUNTIME_DIR/bus"
  _harness_start_dbus \
    "session" \
    "$WORK_DIR/dbus-session.conf" \
    "$socket_path" \
    "$WORK_DIR/dbus-session.log"
  export DBUS_SESSION_BUS_ADDRESS="unix:path=$socket_path"
}

harness_start_system_dbus() {
  local socket_path="$XDG_RUNTIME_DIR/system-bus"
  _harness_start_dbus \
    "system" \
    "$WORK_DIR/dbus-system.conf" \
    "$socket_path" \
    "$WORK_DIR/dbus-system.log"
  export DBUS_SYSTEM_BUS_ADDRESS="unix:path=$socket_path"
}

harness_wait_dbus_name() {
  local bus_kind="$1"
  local bus_name="$2"
  for _ in $(seq 1 50); do
    if python3 - "$bus_kind" "$bus_name" <<'PY'
import sys
import dbus

kind, name = sys.argv[1], sys.argv[2]
bus = dbus.SessionBus() if kind == "session" else dbus.SystemBus()
sys.exit(0 if bus.name_has_owner(name) else 1)
PY
    then
      return 0
    fi
    sleep 0.1
  done
  echo "harness: timed out waiting for $bus_kind D-Bus name '$bus_name'" >&2
  return 1
}

harness_start_sway() {
  local sway_conf_path="$1"
  local log_path="$WORK_DIR/sway.log"
  harness_register_log "$log_path"

  WLR_BACKENDS=headless \
  WLR_RENDERER=pixman \
  WLR_LIBINPUT_NO_DEVICES=1 \
  XDG_CURRENT_DESKTOP=sway \
  sway --config "$sway_conf_path" --unsupported-gpu >"$log_path" 2>&1 &
  SWAY_PID="$!"
  harness_register_pid "$SWAY_PID"

  SWAYSOCK=""
  WAYLAND_DISPLAY=""
  for _ in $(seq 1 100); do
    if ! kill -0 "$SWAY_PID" 2>/dev/null; then
      echo "harness: headless sway exited early" >&2
      return 1
    fi
    local sock wl
    sock="$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock 2>/dev/null | head -n1 || true)"
    wl="$(ls "$XDG_RUNTIME_DIR"/wayland-* 2>/dev/null | grep -v '\.lock$' | head -n1 || true)"
    if [ -n "$sock" ] && [ -n "$wl" ]; then
      SWAYSOCK="$sock"
      WAYLAND_DISPLAY="$(basename "$wl")"
      export SWAYSOCK WAYLAND_DISPLAY
      if swaymsg -t get_outputs >/dev/null 2>&1; then
        return 0
      fi
    fi
    sleep 0.1
  done

  echo "harness: timed out waiting for headless sway" >&2
  return 1
}

harness_pick_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()'
}

harness_wait_http() {
  local url="$1"
  for _ in $(seq 1 50); do
    if curl -fsS "$url" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.1
  done
  echo "harness: timed out waiting for HTTP server at $url" >&2
  return 1
}

harness_start_pulseaudio() {
  local pulse_runtime="$XDG_RUNTIME_DIR/pulse"
  local pulse_config="$HOME/.config/pulse"
  local socket_path="$pulse_runtime/native"
  local pa_log="$WORK_DIR/pulseaudio.log"
  local pacat_log="$WORK_DIR/pacat.log"

  mkdir -p "$pulse_runtime" "$pulse_config"

  cat > "$pulse_config/client.conf" <<EOF
autospawn = no
enable-shm = no
default-server = unix:$socket_path
EOF

  cat > "$WORK_DIR/pulse.pa" <<EOF
load-module module-null-sink sink_name=studio_monitors
update-sink-proplist studio_monitors device.description="Studio Monitors"
load-module module-null-sink sink_name=usb_dac
update-sink-proplist usb_dac device.description="USB-C Headphone DAC"
load-module module-null-source source_name=usb_mic
update-source-proplist usb_mic device.description="USB Condenser Mic"
set-default-sink studio_monitors
set-default-source usb_mic
load-module module-native-protocol-unix socket=$socket_path auth-anonymous=1
EOF

  export PULSE_SERVER="unix:$socket_path"

  harness_register_log "$pa_log"
  pulseaudio \
    -n \
    --daemonize=no \
    --use-pid-file=no \
    --exit-idle-time=-1 \
    --disallow-exit \
    --disable-shm=yes \
    --enable-memfd=no \
    --log-target=stderr \
    --file="$WORK_DIR/pulse.pa" >"$pa_log" 2>&1 &
  local pa_pid="$!"
  harness_register_pid "$pa_pid"

  local pa_ready=0
  for _ in $(seq 1 50); do
    if ! kill -0 "$pa_pid" 2>/dev/null; then
      echo "harness: pulseaudio exited early" >&2
      return 1
    fi
    if pactl info >/dev/null 2>&1; then
      pa_ready=1
      break
    fi
    sleep 0.1
  done
  if [ "$pa_ready" -ne 1 ]; then
    echo "harness: timed out waiting for pulseaudio" >&2
    return 1
  fi

  pactl set-sink-volume studio_monitors 72%
  pactl set-sink-volume usb_dac 85%
  pactl set-source-volume usb_mic 80%

  harness_register_log "$pacat_log"
  pacat \
    --playback \
    --raw \
    --rate=8000 \
    --channels=1 \
    --format=u8 \
    --device=studio_monitors \
    --client-name=Spotify \
    --stream-name="Tycho — Awake" \
    --property=application.name=Spotify \
    --property="media.name=Tycho — Awake" \
    /dev/zero >"$pacat_log" 2>&1 &
  local pacat_pid="$!"
  harness_register_pid "$pacat_pid"

  local stream_id=""
  for _ in $(seq 1 50); do
    if ! kill -0 "$pacat_pid" 2>/dev/null; then
      echo "harness: pacat exited early" >&2
      return 1
    fi
    stream_id="$(pactl list short sink-inputs 2>/dev/null | awk 'NR==1 { print $1 }' || true)"
    if [ -n "$stream_id" ]; then
      break
    fi
    sleep 0.1
  done
  if [ -z "$stream_id" ]; then
    echo "harness: timed out waiting for pacat sink-input" >&2
    return 1
  fi

  pactl set-sink-input-volume "$stream_id" 84%
}

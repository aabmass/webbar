#!/usr/bin/env bash
set -euo pipefail

mode="${1:-fetch}"
theme="${2:-dark}"
repo_root="${3:-$PWD}"

cd "$repo_root"

# Hide terminal cursor for crisp screenshots
printf '\033[?25l'

if [ "$theme" = "light" ]; then
  c_accent='\033[38;2;43;109;224m'
  c_green='\033[38;2;31;143;84m'
  c_violet='\033[38;2;124;92;224m'
  c_fg='\033[38;2;24;24;31m'
  c_muted='\033[38;2;92;92;106m'
  c_faint='\033[38;2;142;142;156m'
  bat_theme="GitHub"
else
  c_accent='\033[38;2;138;180;248m'
  c_green='\033[38;2;111;207;151m'
  c_violet='\033[38;2;187;154;247m'
  c_fg='\033[38;2;236;236;241m'
  c_muted='\033[38;2;154;154;166m'
  c_faint='\033[38;2;92;92;102m'
  bat_theme="OneHalfDark"
fi
c_reset='\033[0m'
c_bold='\033[1m'

case "$mode" in
  code)
    printf '\n'
    bat \
      --color=always \
      --style=numbers,header-filename \
      --paging=never \
      --theme="$bat_theme" \
      --terminal-width=88 \
      examples/sway-full-bar/src/App.tsx
    ;;

  fetch)
    printf '\n'
    printf "  ${c_accent}${c_bold}  ╭──────────╮${c_reset}   ${c_bold}${c_fg}webbar${c_reset}${c_muted}@wayland${c_reset}\n"
    printf "  ${c_accent}${c_bold}  │ ▓▓ ▓▓ ▓▓ │${c_reset}   ${c_faint}─────────────────────────────${c_reset}\n"
    printf "  ${c_accent}${c_bold}  ├──────────┤${c_reset}   ${c_accent}${c_bold}Compositor${c_reset}  ${c_fg}Sway (Wayland)${c_reset}\n"
    printf "  ${c_violet}${c_bold}  │  ╭────╮  │${c_reset}   ${c_accent}${c_bold}Host${c_reset}        ${c_fg}GTK4 + gtk4-layer-shell${c_reset}\n"
    printf "  ${c_violet}${c_bold}  │  │ <> │  │${c_reset}   ${c_accent}${c_bold}Engine${c_reset}      ${c_fg}WebKitGTK 6.0${c_reset}\n"
    printf "  ${c_green}${c_bold}  │  ╰────╯  │${c_reset}   ${c_accent}${c_bold}Frontend${c_reset}    ${c_fg}React 19 + TypeScript${c_reset}\n"
    printf "  ${c_green}${c_bold}  ╰──────────╯${c_reset}   ${c_accent}${c_bold}Theme${c_reset}       ${c_fg}Adaptive %s${c_reset}\n" "$theme"
    printf "                   ${c_accent}${c_bold}Stores${c_reset}      ${c_muted}sway · tray · audio · bt · backlight${c_reset}\n"
    printf '\n'
    printf "                   \033[48;2;38;38;44m   \033[48;2;138;180;248m   \033[48;2;111;207;151m   \033[48;2;242;201;76m   \033[48;2;242;114;111m   \033[48;2;187;154;247m   ${c_reset}\n"
    ;;

  status)
    printf '\n'
    printf "  ${c_green}❯${c_reset} ${c_bold}${c_fg}nix run .#sway-full-bar${c_reset}\n"
    printf "  ${c_muted}info${c_reset}  docked on ${c_accent}DP-1${c_reset} (1600×900 · height 60px)\n"
    printf "  ${c_muted}info${c_reset}  injected ${c_violet}window.webbar${c_reset} + ${c_accent}--desktop-*${c_reset} tokens\n"
    printf '\n'
    printf "  ${c_green}❯${c_reset} ${c_bold}${c_fg}nix flake check${c_reset}\n"
    printf "  ${c_green}✓${c_reset} ${c_fg}rust-test${c_reset}        ${c_muted}unit & bridge tests${c_reset}\n"
    printf "  ${c_green}✓${c_reset} ${c_fg}rust-clippy${c_reset}      ${c_muted}zero warnings${c_reset}\n"
    printf "  ${c_green}✓${c_reset} ${c_fg}rust-ts-bindings${c_reset} ${c_muted}ts-rs types in sync${c_reset}\n"
    printf "  ${c_green}✓${c_reset} ${c_fg}preload-test${c_reset}     ${c_muted}@webbar/types contract${c_reset}\n"
    printf "  ${c_green}✓${c_reset} ${c_fg}sway-full-bar-ui${c_reset} ${c_muted}typecheck & vite build${c_reset}\n"
    printf '\n'
    printf "  ${c_green}❯${c_reset} ${c_faint}█${c_reset}\n"
    ;;

  idle)
    # Background workspace filler window
    ;;
esac

exec sleep infinity

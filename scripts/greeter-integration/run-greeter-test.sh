#!/usr/bin/env bash
set -euo pipefail

if [ ! -f scripts/greeter-integration/greeter.spec.mjs ]; then
  echo "run-greeter-test: run from the webbar repository root" >&2
  exit 1
fi

REPO_ROOT="$PWD"
WEBBAR_UI_DIR="${WEBBAR_UI_DIR:?WEBBAR_UI_DIR must be set}"
WEBBAR_PRELOAD_JS="${WEBBAR_PRELOAD_JS:?WEBBAR_PRELOAD_JS must be set}"

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/webbar-greeter-test.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

TEST_DIR="$WORK_DIR/test-dir"
mkdir -p "$TEST_DIR"
cp "$REPO_ROOT/scripts/greeter-integration/"* "$TEST_DIR/"
if [ -n "${PLAYWRIGHT_NODE_MODULES:-}" ]; then
  ln -s "$PLAYWRIGHT_NODE_MODULES" "$TEST_DIR/node_modules"
fi

export WEBBAR_UI_DIR
export WEBBAR_PRELOAD_JS

cd "$TEST_DIR"
playwright test --config "$TEST_DIR/playwright.config.mjs"

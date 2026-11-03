#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build -p termix-cli
if command -v dx >/dev/null 2>&1; then
  (cd app && dx bundle --desktop)
else
  echo "dioxus-cli (dx) is not installed; the CLI was built, the UI bundle was skipped" >&2
  exit 0
fi

#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build -p termix-cli
if command -v cargo-tauri >/dev/null 2>&1 || cargo tauri --version >/dev/null 2>&1; then
  (cd app && cargo tauri build)
else
  echo "tauri-cli is not installed; the CLI was built, the UI bundle was skipped" >&2
  exit 0
fi
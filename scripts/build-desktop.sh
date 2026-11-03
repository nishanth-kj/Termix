#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build -p termix-cli
cd app
npm ci
cargo tauri build

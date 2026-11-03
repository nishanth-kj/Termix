#!/bin/sh
set -eu
cd "$(dirname "$0")/../app"
npm ci
[ -d src-tauri/gen/apple ] || cargo tauri ios init --ci
cargo tauri ios build

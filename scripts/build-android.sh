#!/bin/sh
set -eu
cd "$(dirname "$0")/../app"
npm ci
cargo tauri android build --debug --apk

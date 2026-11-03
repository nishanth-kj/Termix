#!/bin/sh
set -eu
cd "$(dirname "$0")/../app"
cargo tauri ios build

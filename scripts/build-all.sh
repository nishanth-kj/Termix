#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo test
echo "UI bundles are produced on a machine with the matching SDK. See docs/BUILDING.md"

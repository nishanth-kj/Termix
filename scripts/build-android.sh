#!/bin/sh
set -eu
cd "$(dirname "$0")/../app"
dx bundle --android

#!/bin/bash
# The Swift app moved to legacy/macos/. This keeps `./build.sh [--install|--dmg|--dev]` working from the root;
# the output still lands in ./build.
exec "$(dirname "$0")/legacy/macos/build.sh" "$@"

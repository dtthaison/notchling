#!/bin/bash
# Copies the shared viz bundle (../viz) into assets/viz, where the Visualize tab loads it from.
# Run before `flutter build` or `flutter run`, and again whenever ../viz changes. assets/viz is git-ignored.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ -f ../viz/index.html ]] || { echo "sync_viz: ../viz/index.html not found" >&2; exit 1; }
rm -rf assets/viz
mkdir -p assets/viz
cp -R ../viz/ assets/viz/
echo "viz bundle copied to assets/viz ($(du -sh assets/viz | cut -f1))"

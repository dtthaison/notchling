#!/bin/bash
set -euo pipefail

usage() {
  echo "usage: tools/build-node.sh [--mac-intel] [--dry-run]" >&2
}

MAC_INTEL=0
DRY=0
for arg in "$@"; do
  case "$arg" in
    --mac-intel) MAC_INTEL=1 ;;
    --dry-run) DRY=1 ;;
    *) usage; exit 2 ;;
  esac
done

export PATH="$HOME/.cargo/bin:$PATH"

MISSING=()
command -v zig >/dev/null 2>&1 || MISSING+=("zig")
command -v cargo-zigbuild >/dev/null 2>&1 || MISSING+=("cargo-zigbuild")
if [[ ${#MISSING[@]} -gt 0 ]]; then
  echo "missing: ${MISSING[*]}" >&2
  echo "install zig with: brew install zig" >&2
  echo "install cargo-zigbuild with: cargo install --locked cargo-zigbuild" >&2
  exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."

NOTCHLING_BUILD="$(git rev-parse --short=12 HEAD)$(git diff --quiet HEAD || echo -dirty)"
export NOTCHLING_BUILD

if [[ -n "${NOTCHLING_DATA_ROOT:-}" ]]; then
  DIST="$NOTCHLING_DATA_ROOT/notchling/cluster/dist"
else
  DIST="$HOME/Library/Application Support/Notchling/cluster/dist"
fi

TARGETS=("x86_64-unknown-linux-musl:notchling-linux-x86_64" "aarch64-unknown-linux-musl:notchling-linux-aarch64")
if [[ $MAC_INTEL -eq 1 ]]; then
  TARGETS+=("x86_64-apple-darwin:notchling-macos-x86_64")
fi

hash_of() {
  shasum -a 256 "$1" | cut -d ' ' -f 1
}

if [[ $DRY -eq 1 ]]; then
  echo "NOTCHLING_BUILD=$NOTCHLING_BUILD"
  echo "dist: $DIST"
  for entry in "${TARGETS[@]}"; do
    echo "build ${entry%%:*}: cargo zigbuild --release --locked -p notchling-notchlingd --bin notchling --target ${entry%%:*}"
  done
  for entry in "${TARGETS[@]}"; do
    echo "copy ${entry%%:*} to ${entry##*:}"
  done
  echo "write manifest.json"
  exit 0
fi

for entry in "${TARGETS[@]}"; do
  if command -v rustup >/dev/null 2>&1; then
    rustup target add "${entry%%:*}"
  fi
  cargo zigbuild --release --locked -p notchling-notchlingd --bin notchling --target "${entry%%:*}"
done

TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')"

mkdir -p "$DIST"
rm -f "$DIST/manifest.json"
ENTRIES=""
for entry in "${TARGETS[@]}"; do
  triple="${entry%%:*}"
  name="${entry##*:}"
  cp "$TARGET_DIR/$triple/release/notchling" "$DIST/$name.new"
  chmod 755 "$DIST/$name.new"
  mv -f "$DIST/$name.new" "$DIST/$name"
  size="$(wc -c < "$DIST/$name" | tr -d ' ')"
  piece="\"$name\": {\"sha256\": \"$(hash_of "$DIST/$name")\", \"size\": $size}"
  if [[ -n "$ENTRIES" ]]; then ENTRIES="$ENTRIES, "; fi
  ENTRIES="$ENTRIES$piece"
  echo "wrote $DIST/$name"
done

printf '{"build": "%s", "files": {%s}}\n' "$NOTCHLING_BUILD" "$ENTRIES" > "$DIST/manifest.json.new"
mv -f "$DIST/manifest.json.new" "$DIST/manifest.json"
echo "wrote $DIST/manifest.json (build $NOTCHLING_BUILD)"

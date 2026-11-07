#!/bin/bash
# Fetches the Whisper model the voice input bundles (ggml-small-q5_1, ~181 MiB, from ggerganov/whisper.cpp on
# Hugging Face) into assets/models/, and checks it against the SHA-256 Hugging Face publishes for the file.
# The model is git-ignored; a build without it fails on the missing asset, so run this once after cloning.
#   tool/fetch_model.sh          download if missing, then verify
#   tool/fetch_model.sh --check  only verify; exit 1 with a clear message when missing or wrong
set -euo pipefail
cd "$(dirname "$0")/.."

NAME="ggml-small-q5_1.bin"
DEST="assets/models/$NAME"
BASE="https://huggingface.co/ggerganov/whisper.cpp"

sha_of() { shasum -a 256 "$1" | awk '{print $1}'; }

if [[ "${1:-}" == "--check" ]]; then
  if [[ ! -f "$DEST" ]]; then
    echo "MISSING: $DEST. Run apps/flutter/tool/fetch_model.sh before building (the voice model is bundled, not downloaded by the app)." >&2
    exit 1
  fi
fi

# The LFS pointer of the file carries the published SHA-256 ("oid sha256:<hex>").
EXPECTED="$(curl -fsSL "$BASE/raw/main/$NAME" | awk '/^oid sha256:/ {sub("oid sha256:",""); print $1}')"
if [[ ! "$EXPECTED" =~ ^[0-9a-f]{64}$ ]]; then
  echo "Could not read the published SHA-256 for $NAME from Hugging Face." >&2
  exit 1
fi

if [[ ! -f "$DEST" ]]; then
  mkdir -p "$(dirname "$DEST")"
  echo "Downloading $NAME (~181 MiB)..."
  curl -fL --progress-bar -o "$DEST.part" "$BASE/resolve/main/$NAME"
  mv "$DEST.part" "$DEST"
fi

ACTUAL="$(sha_of "$DEST")"
if [[ "$ACTUAL" != "$EXPECTED" ]]; then
  echo "SHA-256 MISMATCH for $DEST" >&2
  echo "  expected $EXPECTED" >&2
  echo "  actual   $ACTUAL" >&2
  exit 1
fi
echo "OK $DEST sha256=$ACTUAL"

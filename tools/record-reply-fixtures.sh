#!/bin/bash
set -euo pipefail

sentinel=".notchling-record-scratch"

if [ "${1:-}" = "--prepare" ]; then
  dir="${2:-${TMPDIR:-/tmp}/notchling-record-$(date +%s)}"
  case "$dir" in
    /*) ;;
    *) echo "refused: $dir is not an absolute path" >&2; exit 2 ;;
  esac
  case "$(basename "$dir")" in
    notchling-record | notchling-record-*) ;;
    *) echo "refused: the folder's own name must be notchling-record or start with notchling-record-" >&2; exit 2 ;;
  esac
  if [ -L "$dir" ]; then
    echo "refused: $dir is a link" >&2; exit 2
  fi
  if [ -d "$dir" ] && [ ! -f "$dir/$sentinel" ] && [ -n "$(ls -A "$dir")" ]; then
    echo "refused: $dir exists, is not empty and has no $sentinel; use a new folder" >&2; exit 2
  fi
  mkdir -p "$dir"
  if [ ! -d "$dir/.git" ]; then git -C "$dir" init -q; fi
  : > "$dir/$sentinel"
  echo "scratch folder ready; start the jobs to record inside it (cd there, then claude --bg ...)" >&2
  echo "$dir"
  exit 0
fi

install=0
args=()
for a in "$@"; do
  if [ "$a" = "--install" ]; then install=1; else args+=("$a"); fi
done
if [ "${#args[@]}" -lt 1 ]; then
  sed -n '2,38p' "$0"
  exit 2
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
fixtures="$root/legacy/macos/Tests/NotchlingTests/Fixtures/terminal"
out="$(mktemp -d "${TMPDIR:-/tmp}/notchling-record-out.XXXXXX")"

export NOTCHLING_RECORD=1
export NOTCHLING_RECORD_JOB="${args[0]}"
export NOTCHLING_RECORD_OUT="$out"
if [ "${#args[@]}" -ge 2 ]; then export NOTCHLING_RECORD_ASK_JOB="${args[1]}"; fi

cd "$root"
swift test --filter RecordReplyFixturesTests/testRecordReplyFixtures 2>&1 | grep -E "RECORD|error:|failed|passed" || true

echo
echo "Recorded into $out:"
ls -l "$out"
echo
if grep -a -l '@' "$out"/*.bin "$out"/*.json 2>/dev/null; then
  echo "WARNING: the files above contain an @ (an account email?). Remove or redact before committing."
fi

if [ "$install" -eq 1 ]; then
  cp -n "$out"/*.bin "$out"/*.json "$fixtures"/
  echo "Copied (without overwriting) into $fixtures"
else
  echo "To keep them: cp -n \"$out\"/*.bin \"$out\"/*.json \"$fixtures\"/"
fi

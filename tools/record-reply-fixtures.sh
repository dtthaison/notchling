#!/bin/bash
# Records the terminal byte fixtures for the reply behaviors (docs/recording.md) from scratch Claude Code
# background jobs, by running the opt-in Swift test RecordReplyFixturesTests. TRIED: NOT YET. The test
# compiles and its scratch-folder rule is tested, but nobody has run a recording; read the output, check
# the first recording by eye before trusting the rest, and fix this header with what you learn.
#
# Run by the maintainer, by hand. It never starts Claude Code itself and never touches Keychain or a login.
# The test types real replies into the jobs you name, each costing a model turn; use haiku.
#
# Prepare (once). A job counts as scratch only if its folder's own name is "notchling-record" or starts
# with "notchling-record-" AND the folder holds a .notchling-record-scratch file; the test refuses any other
# (a folder merely containing the words does not pass). Start every job you record in such a folder.
# `--prepare` makes one: it creates the folder, runs git init and puts the sentinel file in it, prints the
# path on stdout and refuses a path whose name is not as above or an existing non-empty folder without the
# sentinel.
#
#   SCRATCH=$(tools/record-reply-fixtures.sh --prepare)        # /tmp/notchling-record-<time>, or give a path
#   cd "$SCRATCH"
#   claude --bg -n record --model haiku "Reply with just OK."       # prints the job id; the main job
#   claude --bg -n askprobe --model haiku "Call the AskUserQuestion tool exactly once with two questions: \
#     'Favourite color?' (options Red, Blue) and 'Favourite fruit?' (options Apple, Pear). Then stop."
#
# The question is consumed by answering it, so for each mode you want (option, text, multi) start a fresh
# ask job whose questions fit (a multi-select question for "multi"). Optional extras: a job blocked on a
# permission dialog (NOTCHLING_RECORD_PERMISSION_JOB), a never-trusted scratch folder made with --prepare
# (NOTCHLING_RECORD_TRUST_DIR; the test starts `claude` there and trusts it), the main job's transcript
# (NOTCHLING_RECORD_TRANSCRIPT, to read popAll back). Let the main job finish its first turn before starting.
#
# Run:
#
#   tools/record-reply-fixtures.sh <main-job-id> [<ask-job-id>]
#   NOTCHLING_RECORD_ONLY=attach-idle,clear-input-draft tools/record-reply-fixtures.sh <main-job-id>
#   NOTCHLING_RECORD_ANSWER=1 NOTCHLING_RECORD_ANSWER_MODE=text tools/record-reply-fixtures.sh <main> <ask>
#
# It writes to a new folder under $TMPDIR, lists what it made, warns about files that contain an "@" (the
# Claude Code banner can show an account email: look before committing), and prints the cp command that
# puts them in legacy/macos/Tests/NotchlingTests/Fixtures/terminal/. Pass --install as the last argument to
# run that cp. New files there are new fixtures; replacing an existing one is a test change for the maintainer.
set -euo pipefail

sentinel=".notchling-record-scratch"

# --prepare [dir]: a scratch folder the recorder will accept.
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

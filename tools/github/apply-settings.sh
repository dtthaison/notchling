#!/usr/bin/env bash
# Applies the repository settings that live in this repo: the rulesets in .github/rulesets/,
# secret scanning, the Actions allowlist, auto-merge and the agent run caps.
#
# It changes live repository settings, so it prints every request first and asks before sending
# any of them. Run with --dry-run to only print: that mode never calls gh at all.
#
#   tools/github/apply-settings.sh [--dry-run] [--repo owner/name]
set -euo pipefail

DRY=0
REPO=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --repo) REPO="${2:?--repo needs owner/name}"; shift ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RULESETS="$ROOT/.github/rulesets"
ALLOWLIST="$ROOT/tools/guard/actions-allowlist.json"

if [ -z "$REPO" ]; then
  if [ "$DRY" = 1 ]; then
    echo "need --repo owner/name with --dry-run (dry run never calls gh)" >&2
    exit 2
  fi
  REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
fi

# Plan: parallel arrays of method, path and JSON body ("" for none), printed then sent in order.
METHODS=()
PATHS=()
BODIES=()
NOTES=()
add() { METHODS+=("$1"); PATHS+=("$2"); BODIES+=("${3:-}"); NOTES+=("${4:-}"); }

compact() { python3 -c 'import json,sys; print(json.dumps(json.load(sys.stdin), separators=(",", ":")))'; }

# Sends one planned request (method, path, JSON body or ""). A 404 on the DELETE of the classic
# branch protection means there is none to remove, which is what this script wants, so it counts as
# done: every re-run after the first meets it, and failing there would skip the steps after it.
send() {
  local method="$1" path="$2" body="${3:-}" out rc=0
  if [ -n "$body" ]; then
    out="$(printf '%s' "$body" | gh api -X "$method" "${path#/}" --input - 2>&1)" || rc=$?
  else
    out="$(gh api -X "$method" "${path#/}" 2>&1)" || rc=$?
  fi
  [ -z "$out" ] || printf '%s\n' "$out"
  if [ "$rc" != 0 ] && [ "$method" = DELETE ]; then
    case "$path" in
      */branches/*/protection)
        case "$out" in
          *"HTTP 404"*) echo "  no classic branch protection (404): nothing to remove"; return 0 ;;
        esac ;;
    esac
  fi
  return "$rc"
}

# Id of the ruleset with this name (empty when none), from the "id<TAB>name" lines in $EXISTING.
EXISTING=""
if [ "$DRY" = 1 ]; then
  echo "would look up existing rulesets and variables (dry run: no gh calls)"
else
  EXISTING="$(gh api "repos/$REPO/rulesets" --paginate -q '.[] | "\(.id)\t\(.name)"')"
fi
ruleset_id() { printf '%s\n' "$EXISTING" | awk -F'\t' -v n="$1" '$2 == n { print $1; exit }'; }

# 1. Rulesets, created or updated by name.
for f in "$RULESETS"/*.json; do
  name="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["name"])' "$f")"
  body="$(compact < "$f")"
  if [ "$DRY" = 1 ]; then
    add POST "/repos/$REPO/rulesets" "$body" "(becomes PUT /repos/$REPO/rulesets/<id> when a ruleset named \"$name\" exists)"
  else
    id="$(ruleset_id "$name")"
    if [ -n "$id" ]; then add PUT "/repos/$REPO/rulesets/$id" "$body"; else add POST "/repos/$REPO/rulesets" "$body"; fi
  fi
done

# 2. The classic branch protection on main is replaced by the main ruleset.
add DELETE "/repos/$REPO/branches/main/protection"

# 3. Secret scanning, push protection, Dependabot security updates, private vulnerability reporting.
add PATCH "/repos/$REPO" '{"security_and_analysis":{"secret_scanning":{"status":"enabled"},"secret_scanning_push_protection":{"status":"enabled"},"dependabot_security_updates":{"status":"enabled"}}}'
add PUT "/repos/$REPO/private-vulnerability-reporting"

# 4. Actions: only GitHub's own plus the allowlist, every action pinned to a SHA.
add PUT "/repos/$REPO/actions/permissions" '{"enabled":true,"allowed_actions":"selected","sha_pinning_required":true}'
patterns="$(python3 -c 'import json,sys
allow = json.load(open(sys.argv[1]))["allow"]
print(json.dumps({"github_owned_allowed": True, "verified_allowed": False,
                  "patterns_allowed": [a + "@*" for a in allow]}, separators=(",", ":")))' "$ALLOWLIST")"
add PUT "/repos/$REPO/actions/permissions/selected-actions" "$patterns"

# 5. Auto-merge, so a pull request that passes every check can merge itself.
add PATCH "/repos/$REPO" '{"allow_auto_merge":true}'

# 6. Daily caps the agent worker and the review agent read.
for pair in AGENT_DAILY_RUN_CAP=20 REVIEW_DAILY_CAP=40; do
  vname="${pair%%=*}"; vvalue="${pair#*=}"
  body="{\"name\":\"$vname\",\"value\":\"$vvalue\"}"
  if [ "$DRY" = 1 ]; then
    add POST "/repos/$REPO/actions/variables" "$body" "(becomes PATCH /repos/$REPO/actions/variables/$vname when it exists)"
  elif gh api "repos/$REPO/actions/variables/$vname" >/dev/null 2>&1; then
    add PATCH "/repos/$REPO/actions/variables/$vname" "$body"
  else
    add POST "/repos/$REPO/actions/variables" "$body"
  fi
done

echo
echo "Requests for $REPO:"
for i in "${!METHODS[@]}"; do
  echo "${METHODS[$i]} ${PATHS[$i]}"
  [ -n "${BODIES[$i]}" ] && echo "  ${BODIES[$i]}"
  [ -n "${NOTES[$i]}" ] && echo "  ${NOTES[$i]}"
done

# Who may skip a ruleset, read from the files that are about to be sent.
echo
echo "Bypass actors these rulesets set:"
for f in "$RULESETS"/*.json; do
  python3 -c 'import json,sys
d = json.load(open(sys.argv[1]))
a = ", ".join("%s %s (%s)" % (x["actor_type"], x["actor_id"], x["bypass_mode"]) for x in d.get("bypass_actors") or [])
print("  %s: %s" % (d["name"], a or "nobody"))' "$f"
done
echo "  main: repository admins may merge a pull request without the review (pull request mode only,"
echo "    never a push), because the maintainer is the only code owner and can't approve his own."
echo "  release-tags: the GitHub Actions app (15368) may create v* tags, because release.yml's"
echo "    gh release create makes the tag with GITHUB_TOKEN. Only a workflow holding contents: write"
echo "    can use it, and release.yml is the only one (a guard test pins that)."

# 7. Not something a script can do.
echo
echo "Manual step: set a hard monthly spend limit at the model provider's console (a spend limit"
echo "caps the cost even if the run caps above are bypassed)."

[ "$DRY" = 1 ] && exit 0

echo
read -r -p "Apply? [y/N] " answer
case "$answer" in
  y|Y|yes|YES) ;;
  *) echo "Nothing changed."; exit 0 ;;
esac

for i in "${!METHODS[@]}"; do
  echo "${METHODS[$i]} ${PATHS[$i]}"
  send "${METHODS[$i]}" "${PATHS[$i]}" "${BODIES[$i]}"
done
echo "Done."

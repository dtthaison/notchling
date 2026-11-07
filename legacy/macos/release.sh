#!/bin/bash
# Starts a release: ./release.sh 1.2.0
# Makes branch release/<version> from main with the version bumped in Info.plist and the
# [Unreleased] changelog entries moved under the new version, pushes it and opens a pull request.
# Merging that pull request tags v<version> and publishes the GitHub release (.github/workflows/release.yml).
set -euo pipefail
cd "$(dirname "$0")"

VERSION=${1:-}
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Usage: ./release.sh <major.minor.patch>" >&2
    exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
    echo "Commit or stash your changes first." >&2
    exit 1
fi

git fetch -q origin main --tags
if git rev-parse -q --verify "refs/tags/v$VERSION" >/dev/null; then
    echo "v$VERSION is already tagged." >&2
    exit 1
fi

git switch -q -c "release/$VERSION" origin/main

CURRENT=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" Info.plist)
BUILD=$(/usr/libexec/PlistBuddy -c "Print :CFBundleVersion" Info.plist)
if [[ "$(printf '%s\n%s\n' "$CURRENT" "$VERSION" | sort -V | tail -1)" != "$VERSION" || "$CURRENT" == "$VERSION" ]]; then
    echo "$VERSION isn't newer than $CURRENT." >&2
    git switch -q - && git branch -q -D "release/$VERSION"
    exit 1
fi
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" Info.plist
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $((BUILD + 1))" Info.plist

# Keep an empty [Unreleased] on top, and point the compare links at the new tag.
REPO=https://github.com/dtthaison/notchling
TODAY=$(date +%Y-%m-%d)
awk -v v="$VERSION" -v cur="$CURRENT" -v today="$TODAY" -v repo="$REPO" '
    /^## \[Unreleased\]/ { print; print ""; print "## [" v "] - " today; next }
    /^\[Unreleased\]:/ {
        print "[Unreleased]: " repo "/compare/v" v "...HEAD"
        print "[" v "]: " repo "/compare/v" cur "...v" v
        next
    }
    { print }
' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md

if ! awk -v v="$VERSION" '$0 ~ "^## \\[" v "\\]" { on = 1; next } on && /^## \[/ { exit } on && /^- / { found = 1 } END { exit !found }' CHANGELOG.md; then
    echo "Note: nothing was under [Unreleased]; add the changes to CHANGELOG.md on this branch before merging."
fi

git commit -q -am "Notchling $VERSION"
git push -q -u origin "release/$VERSION"
gh pr create --base main --title "Notchling $VERSION" \
    --body "Bumps the version to $VERSION and moves the changelog entries under it. Merging tags v$VERSION and publishes the release."

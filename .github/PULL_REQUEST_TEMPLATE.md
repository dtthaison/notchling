## What this changes

<!-- One change per pull request. Link the issue: Fixes #123 -->

## How I checked it

<!-- What did you run and look at? For a bug fix, name the test that fails on main and passes here. Screenshots help for anything visual. -->

- [ ] Tests added for the change (a new test file; an edit to an existing test waits for the maintainer)
- [ ] `swift test` passes
- [ ] Guard passes: `PYTHONPATH=tools/guard python3 -m guard lint` (and `pins`, `codeowners --check`, `rulesets`, `rules`, plus the guard tests if `tools/guard/` changed)
- [ ] `./build.sh` builds without warnings I added
- [ ] If `apps/flutter/` changed: `flutter analyze` and `flutter test` pass
- [ ] If users would see it: tried in the running app (macOS version: )
- [ ] Changelog: a line under [Unreleased] in CHANGELOG.md, if users would notice (per-PR fragments replace this in Phase 6)
- [ ] No secrets in the diff or screenshots: tokens, cookies, `~/.claude.json` contents, vault values
- [ ] Every commit is signed off (`git commit -s`)

## Protected areas

<!-- tools/guard/zones.json is the auto-merge zone. To see what would wait:
     PYTHONPATH=tools/guard python3 -m guard gate --base main --head HEAD -->

- [ ] Touches only the auto-merge zone
- [ ] Touches protected areas or trips the capability check: list the paths and why. `maintainer-gate` waits for the maintainer.

## What this changes

<!-- One change per pull request. Link the issue: Fixes #123 -->

## How I checked it

<!-- What did you run and look at? For a bug fix, name the test that fails on main and passes here. Screenshots help for anything visual. -->

- [ ] Tests added for the change (a new test file; an edit to an existing test waits for the maintainer)
- [ ] `cargo test --workspace --locked` passes (and clippy and fmt)
- [ ] Guard passes: `PYTHONPATH=tools/guard python3 -m guard lint` (and `pins`, `codeowners --check`, `rulesets`, `rules`, plus the guard tests if `tools/guard/` changed)
- [ ] `tools/bundle-next.sh` builds without warnings I added
- [ ] If `apps/flutter/` changed: `flutter analyze` and `flutter test` pass
- [ ] If users would see it: tried in the running app (macOS version: )
- [ ] A new feature has its `notchling` command and agent-tier op with `--json` (docs/goals.md, goal 6)
- [ ] If `protocol/` changed: additive only, types regenerated, not edited by hand
- [ ] Changelog: a fragment in `changes/` if this changes `crates/**/src/**` or `apps/flutter/lib/**` ([changes/README.md](../changes/README.md); `internal` when users won't notice)
- [ ] No secrets in the diff or screenshots: tokens, cookies, `~/.claude.json` contents, vault values
- [ ] Every commit is signed off (`git commit -s`)

## Protected areas

<!-- tools/guard/zones.json is the auto-merge zone. To see what would wait:
     PYTHONPATH=tools/guard python3 -m guard gate --base main --head HEAD -->

- [ ] Touches only the auto-merge zone
- [ ] Touches protected areas or trips the capability check: list the paths and why. `maintainer-gate` waits for the maintainer.

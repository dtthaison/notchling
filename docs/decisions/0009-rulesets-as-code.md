# 0009: Rulesets as code, applied by the maintainer

## Context

What may merge into `main` is decided in GitHub's settings, not in the repository. Classic branch protection and rulesets can both be on and disagree. One anti-goal of the design is agents loosening their own guard rails by any path, rulesets and the App's permissions included.

From the design: [GitHub setup](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md#github-setup-review).

## Decision

- **Rulesets only**, kept as code in `.github/rulesets/` and applied by a script the maintainer runs. The old classic branch protection is removed, so the two can't disagree.
- `main`: no direct pushes; required checks pinned to the GitHub Actions source; `maintainer-gate` required; approval after the last push; maintainer bypass only through a PR.
- `agent/**`: only the App may push. Tags `v*` and releases: only the release workflow.
- The agents' identity is a GitHub App *(decided)*: contents write (limited by the rulesets), pull requests write, issues write, checks read. Never `workflows`, never admin. Installed on this repository only.
- TrietKun keeps write access *(decided)*. The rulesets and `maintainer-gate` apply to that account as to any other. Only `@dtthaison` can satisfy `maintainer-gate`.
- Also turned on: private vulnerability reporting, secret scanning, push protection. Allowed actions: GitHub's own and SHA-pinned allowlisted ones.

## Consequences

- A settings change is a PR to a protected path, then a run of the script by the maintainer. CI never applies settings.
- The script is `tools/github/apply-settings.sh`. It prints every request and asks before sending any; `--dry-run` never calls `gh`.
- `python3 -m guard rulesets` checks that the rulesets require only checks the workflows provide. `python3 -m guard pins` checks that every action is pinned by SHA and allowlisted.
- A user-owned repository has no merge queue, so "require up to date" stays off. Binding every check to the PR's head commit does that job.
- The rulesets, the script and both checks exist (Phase 2). The GitHub App comes in Phase 5.

## Status

Accepted. The rulesets as code came from the reviews and are part of revision 2 of the design, which the maintainer approved on 2026-11-07. The GitHub App and TrietKun's access are the maintainer's decisions *(decided)*.

## Date

2026-11-07

# Governance

Who decides what in Notchling, and how. [docs/goals.md](docs/goals.md) says what the project is for.
[docs/policies/automation.md](docs/policies/automation.md) says what agents may do, and
[docs/policies/privacy.md](docs/policies/privacy.md) what data leaves a computer. Changing this file
is a protected change: it waits for the maintainer.

## People

| Who | Role | What they can do |
| --- | --- | --- |
| `@dtthaison` | Maintainer | Owns every file except the auto-merge zone and the test folders (`.github/CODEOWNERS` starts with `* @dtthaison`). The only account whose approval satisfies `maintainer-gate`. Queues work, approves protected changes, applies the repository settings, and makes and changes decisions. |
| TrietKun | Write access | Works under the same rules as everyone else. The rulesets and `maintainer-gate` apply to this account. Its approval does not satisfy `maintainer-gate`. |
| Contributors | Anyone, and the agents they run | Open issues and pull requests. Sign off every commit. |
| The agent App | A GitHub App, from Phase 5 | Pushes `agent/**` branches and opens pull requests for queued issues. It has no say in any decision. |

The list of maintainers is in code: `MAINTAINERS` in `tools/guard/guard/gate.py`. A test checks that
another account's approval does not pass the gate (`tools/guard/tests/test_gate.py`). Both files are
protected.

Write access is given sparingly. Someone who can push a branch to this repository could add a workflow
on it that posts a false `maintainer-gate` status. Today only `@dtthaison` and TrietKun can. Adding a
writer is the maintainer's decision, and this is looked at again before more writers are added.

## Who may queue work

An issue is work for an agent only when it has the `notchling:queued` label. **Only the maintainer
adds it.** Whoever filed the issue and whatever it says, an issue without the label is not queued.

- From Phase 5 the worker checks, through the GitHub API, that the account that added the label is a
  maintainer and that the issue text wasn't edited after the label was added.
- Issue text is data for the agent, never instructions to it.
- The worker reports back with `notchling:ready`, `notchling:needs-you` and `notchling:stuck`.
- Outside pull requests get a model review only after the maintainer adds `notchling:review`.

The `notchling:*` labels and the worker that reads them come in Phase 5.

## Who approves protected changes

A change is protected when it touches anything outside the auto-merge zone, or trips a capability or
test-weakening check, wherever the file is. The zone is listed in `tools/guard/zones.json`; the checks
are in [docs/rules.md](docs/rules.md). The manual itself is protected: `docs/goals.md`,
`docs/rules.md`, `ARCHITECTURE.md`, `docs/decisions/`, this file, `docs/policies/`, every `AGENTS.md`,
`CLAUDE.md` and `.claude/`.

- **The maintainer approves.** `maintainer-gate` passes a protected pull request only when the
  maintainer's latest review approves its current head commit. A later push needs a new approval.
- **The maintainer's own pull requests.** GitHub doesn't let an author approve his own pull request.
  The `main` ruleset lets the repository's admin, the maintainer, merge a pull request without the
  review. It allows that only when merging a pull request, never for a direct push.
- **Labels decide nothing.** Anyone with pull-request write access can remove a label.

The rulesets are code in `.github/rulesets/`. They are **not applied yet**. The maintainer applies them
with `tools/github/apply-settings.sh`, which prints every request and asks before sending any.
Until then, the repository's current settings stay as they are.

## How decisions are made and changed

- The design the maintainer approved,
  [revision 2](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md),
  is where today's decisions come from.
- Each decision is a record in [docs/decisions/](docs/decisions/README.md). A record is Proposed until
  the maintainer accepts it.
- A decision isn't edited in place. To change one, write a new record that says which one it replaces
  and why. The old record is then marked "Superseded by" the new one.
- A rule enters [docs/rules.md](docs/rules.md) together with the check that enforces it. Something
  that can't be checked belongs in the goals or the architecture.
- Goals, governance and the policies change through a pull request the maintainer approves.
- When the maintainer finds a bad change while sampling merged work, the fix is a new check, test,
  lint rule or skill, never just a review comment.

## Appeals

Agents give verdicts: triage labels an issue, and the review agent recommends what to do with a pull
request. Every verdict can be appealed. Reply on the issue or pull request, say what's wrong, and ask
for the maintainer. The maintainer decides.

Security problems don't go in public issues. See [SECURITY.md](SECURITY.md).

## Sign-off (DCO)

Every commit in a pull request is signed off: `git commit -s` adds a `Signed-off-by:` line. It says you
wrote the change or have the right to submit it under the project's MIT license, as the
[Developer Certificate of Origin](https://developercertificate.org/) sets out. When an agent wrote the
commit, the person who runs the agent signs off. No check enforces it yet.
[CONTRIBUTING.md](CONTRIBUTING.md) has the rest of the flow.

## Conduct

Everyone follows the [Code of Conduct](CODE_OF_CONDUCT.md).

# Governance

Who decides what in Notchling, and how. [docs/goals.md](docs/goals.md) says what the project is for.
[docs/policies/automation.md](docs/policies/automation.md) says what agents may do, and
[docs/policies/privacy.md](docs/policies/privacy.md) what data leaves a computer. Changing this file
is a protected change: it waits for the maintainer.

## People

| Who | Role | What they can do |
| --- | --- | --- |
| `@dtthaison` | Maintainer | Owns every file except the auto-merge zone and the test folders (`.github/CODEOWNERS` starts with `* @dtthaison`). The only account whose approval satisfies `maintainer-gate`. Queues work, approves protected changes, applies the repository settings, promotes Betas to Stable, holds the release keys, and makes and changes decisions. |
| TrietKun | Write access | Works under the same rules as everyone else. The rulesets and `maintainer-gate` apply to this account. Its approval does not satisfy `maintainer-gate`. |
| Contributors | Anyone, and the agents they run | Open issues and pull requests. Sign off every commit. |
| The agent App | A GitHub App, from Phase 5. Not created yet. | Pushes `agent/**` branches and opens pull requests for queued issues, through the push wrapper on the workers. It has no say in any decision. |

The list of maintainers is in code: `MAINTAINERS` in `tools/guard/guard/gate.py`. A test checks that
another account's approval does not pass the gate (`tools/guard/tests/test_gate.py`). The Promote
workflow reads the same list. Both files are protected.

Write access is given sparingly. Someone who can push a branch to this repository could add a workflow
on it that posts a false `maintainer-gate` status. Today only `@dtthaison` and TrietKun can. Adding a
writer is the maintainer's decision, and this is looked at again before more writers are added.

## Who may queue work

An issue is work for an agent only when it has the `notchling:queued` label. **Only the maintainer
adds it.** Whoever filed the issue and whatever it says, an issue without the label is not queued.

- Before it starts, the worker checks through the GitHub API that the latest `notchling:queued`
  label event was a maintainer's, that the label is still on and that the issue wasn't edited since
  (`tools/agents/preflight.py`). The check is built; the worker doesn't run it yet.
- Issue text is data for the agent, never instructions to it.
- The worker reports back with `notchling:ready`, `notchling:needs-you` and `notchling:stuck`.
- Outside pull requests get a model review only after the maintainer adds `notchling:review`. The
  review agent checks that a maintainer added it.

sd's worker still uses its own `sd:*` labels. It moves to the `notchling:*` labels once sd has its
`labels.prefix` setting, which belongs in sd's own repository.

## Agents in Phase 5

The parts that need no credential exist: the triage Action, the worker's pre-start check, the push
wrapper and the review agent, which only recommends. Each stays off, or fails closed, until what it
needs exists:

- the GitHub App and the agent OS user on each worker, which only the maintainer can create;
- the `ANTHROPIC_API_KEY` repository secret, after a spend cap at the provider, and the
  `AGENT_APP_LOGIN` variable;
- the rulesets, applied;
- sd's label prefix, and the Morning Actions and Machines changes in the Swift app, which is
  protected code the maintainer uses every day.

Then a week of sampling comes before anything widens.
[docs/policies/automation.md](docs/policies/automation.md#phase-5-so-far) says what turns each part
on, in order; [docs/agents/worker-setup.md](docs/agents/worker-setup.md) has the steps on each worker.

## Releases in Phase 6

The Beta and Promote workflows are built and off. Every job stops at its first step until the
repository variable `NOTCHLING_RELEASES` is `on`. Until then releases are cut by hand with
`legacy/macos/release.sh` ([CONTRIBUTING.md](CONTRIBUTING.md#releasing)).

- **A Beta** is cut by the daily schedule, or by a click from anyone with write access. Only a
  maintainer may allow a breaking change in one, and only to start a new major line: the workflow checks
  the account against `MAINTAINERS`. Later Betas on that line need no approval.
- **Promoting a Beta to Stable** is for a maintainer only. `promote.yml` refuses any account not in
  `MAINTAINERS`, and any run not started from `main`.
- **The keys and the switch are the maintainer's:** the kill switch, the `release` environment (only
  the `sign` job uses it), its signing secrets and the offline backup of the update signing key. In a personal account's
  repository only the owner can set Actions variables, secrets and environments.
- **Agents never touch the release path.** The build job runs repository code with no secrets, and
  the signing job runs none.
- Anyone with write access can replace the update feed (an asset of the `appcast` release); Sparkle's
  signature check is what protects what installs ([automation.md](docs/policies/automation.md#ruling-the-feed-stays-on-the-appcast-release)).
- Anyone with write access can delete a release; the `release-tags` ruleset, once applied, protects
  only the `v*` tags. Never delete the `appcast` release or a promoted Beta's release: installed copies would lose
  the feed or the download.

[docs/policies/automation.md](docs/policies/automation.md#releases-so-far) says what turns each part
on, in order, and what isn't built yet.

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
- **The review agent approves nothing.** Its `review-agent` status is a recommendation, not a review
  and not a required check. Only the maintainer's approval counts for `maintainer-gate`.

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

# Phase 5: Agents On (the parts that need no credentials yet)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** Everything Phase 5 needs that can be built and tested before the maintainer creates the GitHub App, the agent OS user on each worker and the model API key: the triage Action, the worker's pre-start checks, the push wrapper, and the recommend-only review agent. Each part is complete and tested against fakes. Each one stays off, or fails closed, until its credential exists.

**Spec:** `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md`, "The flow" items 1–7, "GitHub setup", "Phases" item 5.

**Blocked on the maintainer:**
- Creating the GitHub App and its key.
- The agent OS user on each worker.
- The `ANTHROPIC_API_KEY` repository secret and the spend caps.
- sd's `labels.prefix` (sd's own repo).
- Morning Actions' label change and Machines' "agent user can't push" step (protected Swift code that the maintainer uses daily).
- Applying the rulesets.
- The week of sampling.

## Global Constraints

- Python 3.9+ standard library only. Code goes under `tools/agents/`, tests under `tools/agents/tests/`, and they run with `python3 -m unittest discover -s tools/agents/tests -t tools/agents`.
- Workflows:
  - Use only the already-allowlisted, SHA-pinned `actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7` with `persist-credentials: false`. No other actions.
  - Top-level `permissions: {}`. Each job grants only what it needs.
  - Event text (titles, bodies, branch names) reaches scripts only through `env:`, never `${{ }}` inside `run:`.
  - `guard pins` and tools/guard/tests/test_workflows.py must pass.
- Issue and PR text is data. It is never put into a prompt as instructions (it is quoted and fenced), and it is never echoed back into a comment.
- Tests never touch GitHub, `gh`, ssh, Keychain or a real model. Fakes only: a local HTTP server on 127.0.0.1, local bare git repos, and recorded JSON.
- Fail closed: a missing secret, a model error or an unparsable model answer means the job does nothing and says so. It never takes a default action.
- The maintainers list is the one in `tools/guard/guard/gate.py` (`MAINTAINERS`). Import it or read it, don't copy it.
- Labels: `notchling:queued`, `notchling:ready`, `notchling:needs-you`, `notchling:stuck`, `notchling:review`. Triage never adds any `notchling:*` label.

## Task A: Triage

**Files:** `tools/agents/triage.py`, `tools/agents/triage_labels.json` (the allowlist), `tools/agents/tests/test_triage.py`, `.github/workflows/triage.yml`.

- `triage.py prompt` reads the issue's title and body from the environment and prints the model request. The issue text is fenced as data. It asks for JSON only: `type` (bug, feature, question, other), `missing` (a list from a fixed set), `duplicate_of` (an int or null), `security` (a bool).
- `triage.py ask` calls the Anthropic Messages API with `urllib` (model from env, default `claude-haiku-5-5`). It runs with no GitHub token in its environment and prints the raw answer.
- `triage.py parse` validates the answer strictly against that shape and maps it to an action plan:
  - labels, only from the allowlist;
  - a comment template id, from fixed templates in the script that never include issue text;
  - `security: true` means a template asking the reporter to use private vulnerability reporting, plus no type label.
- `triage.py apply` posts the labels and comment through the REST API with `GITHUB_TOKEN`. It refuses any label outside the allowlist or starting with `notchling:`, and it never closes an issue.
- Workflow, on `issues: [opened]`:
  - job `classify` (`permissions: {}`, env has only `ANTHROPIC_API_KEY`) outputs the validated plan JSON through job outputs;
  - job `label` (`permissions: issues: write`) runs `apply` on that output.
  - Without the secret, `classify` exits 0 with an empty plan and a notice.
- Tests:
  - prompt fencing, including an issue body that tries to close the fence;
  - the parse rejects extra fields, wrong types, unknown labels and `notchling:` labels;
  - the security path;
  - apply against a fake API server, asserting the exact requests, with no close call ever made;
  - the comment never contains issue text.

## Task B: Worker pre-start checks and the push wrapper

**Files:** `tools/agents/preflight.py`, `tools/agents/push_wrapper.py`, `tools/agents/tests/test_preflight.py`, `tools/agents/tests/test_push_wrapper.py`, `docs/agents/worker-setup.md`.

- `preflight.py <owner/repo> <issue>` reads the issue's timeline through the API.
  - It passes only if the latest `labeled` event for `notchling:queued` has an actor in `MAINTAINERS`, the label is still on, and the body wasn't edited after that event. The edit check uses the `edited` timeline events, and `updated_at` is not enough.
  - It also needs the issue to be open, not a pull request, and not labeled `notchling:stuck`.
  - Exit codes: 0 ok, 1 refused (with the reason), 2 API error, which counts as refused.
- `push_wrapper.py`, run by the wrapper user and never by the agent:
  - Inputs: the agent's worktree path, the issue number, the PR title, and a body file.
  - It checks the branch is `agent/<issue>-<slug>` (lowercase slug) and that the worktree is clean.
  - It mints an App JWT (RS256) with the `openssl` command line, since the stdlib has no RSA. The App id and key path come from config; the key path must be mode 0600 and owned by the running user.
  - It exchanges the JWT for an installation token narrowed to this one repository with `contents: write, pull_requests: write`, valid for one hour, and never writes the token to disk.
  - It pushes only `refs/heads/agent/*`, using `git -c http.extraHeader` with the token on stdin or in the environment and never in argv. It refuses any other ref, a force push, or a tag.
  - It opens the PR as the App, with a body that includes `Agent-Model:` when given.
- Tests:
  - the timeline fixtures for preflight: wrong actor, label removed and re-added, body edited after the label, a PR rather than an issue, an API error;
  - JWT shape, using a test RSA key generated in setUp with `openssl genrsa` (skip when no `openssl`) and checked with `openssl dgst -verify`;
  - refused refs;
  - a push to a local bare repo, done through a fake that replaces the network URL;
  - the token appears in no argv (inspect the subprocess calls through a seam) and in no file.
- `docs/agents/worker-setup.md` covers the steps the maintainer runs on each worker, with exact commands, marked untried:
  - create the agent user with no GitHub credential;
  - create the wrapper user;
  - store the App key in the Secret Vault;
  - set the sudoers line that lets sd run the wrapper;
  - a network allowlist note;
  - how the Machines "Push is refused" step changes later.

## Task C: Review agent (recommend-only)

**Files:** `tools/agents/review.py`, `tools/agents/tests/test_review.py`, `.github/workflows/review-run.yml`, `.github/workflows/review-post.yml`.

- `review-run.yml`, on `pull_request`, with `permissions: {}` and no secrets: checks out the PR, runs nothing beyond what the other workflows run (it only records the PR number and head SHA as an artifact-free job output), and is the event `review-post` waits on.
- `review-post.yml` (`workflow_run` of review-run, completed; `permissions: pull-requests: write, statuses: write, contents: read`) checks out the default branch only and runs `review.py`. It runs only when one of these holds:
  - the PR is from the App (login from a repo variable `AGENT_APP_LOGIN`);
  - the PR has the `notchling:review` label.
- `review.py`:
  - Reads the diff and check results through the API, never files a fork wrote.
  - Picks the reviewer model by rotation over a fixed list keyed by PR number. `Agent-Model:` is informational only.
  - Builds a prompt with the goals, the rules and ARCHITECTURE.md as context, and the diff fenced as data.
  - Asks for one verdict JSON: `verdict` (`merge`, `changes`, `not_right`), `reasons`, and `pointers` (file:line). It validates that strictly.
  - Posts a commit status `review-agent` on the head SHA plus one comment. The status is `success` only for `merge`, which is a recommendation: the agent never merges or approves.
  - Missing key, API error or an invalid answer give the status `failure` with "review missing". It fails closed.
  - After two `changes` verdicts on one PR (counted from its own earlier statuses or comments), it posts `needs-you` instead and stops.
- Tests:
  - rotation is deterministic and ignores `Agent-Model:`;
  - diff fencing, including a diff that tries to close the fence;
  - verdict validation;
  - fail-closed paths;
  - the two-strikes rule;
  - statuses and comments against a fake API;
  - the workflow-shape tests for both files (permissions, no secrets in review-run, default-branch checkout in review-post, env-only event fields), in a new test file.

## Task D: Docs and checks

- `GOVERNANCE.md`, `docs/policies/automation.md` and `CONTRIBUTING.md` say what now exists and what's still off, and why.
- `docs/feature-map.md` gets rows.
- `.github/workflows/guard.yml` runs the tools/agents tests.
- `docs/rules.md` gets a row for any new rule.
- Final review on a fresh model, then the fix pass, then the merge into `agent-run-oss`.

# Phase 3: Operating Manual Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** The repository explains itself in layers, so another developer or an agent can work correctly from what it says, and every rule it lists is enforced by a check.

**Architecture:** A short root `AGENTS.md` that points to the layers, plus a one-line `CLAUDE.md` (`@AGENTS.md`). Today's `CLAUDE.md` notes about the Swift app move to `Sources/AGENTS.md`, which the root imports for Claude Code. There are documents for goals, architecture and decisions, rules, flow, and governance and policies. `guard rules` proves that every listed rule has a check that fails on its own bad example. A dev profile, the first repo skills and a feature map complete the manual.

**Tech Stack:** Markdown, Python 3.9+ stdlib (tools/guard), bash.

**Spec:** `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md` (revision 2): "The operating manual", "Quality", "The line", "The flow", "GitHub setup", "Releases and updates", "Architecture", "Phases" item 3.

## Global Constraints

- Agent instructions live in `AGENTS.md`. The root `CLAUDE.md` is exactly `@AGENTS.md` plus a newline. CI fails on any other tracked `CLAUDE*.md`.
- Every rule in `docs/rules.md` has an id, a why, and a check. The check's failure names the rule id, and a test proves the check fails on its own bad example. CI fails when a listed rule has no such check, or when a check's rule isn't listed.
- Nothing the Swift app relies on is lost. Every fact in today's `CLAUDE.md` lands in `Sources/AGENTS.md`, the root `AGENTS.md`, or `ARCHITECTURE.md`. Claude Code still loads the Swift notes by default through `@` imports, which work recursively.
- User-facing text is short and plain. Write the way the existing docs read: short sentences, no marketing.
- Python stays stdlib-only and 3.9-compatible. Tests: `python3 -m unittest discover -s tools/guard/tests -t tools/guard`.
- The manual's layers are protected (spec): `docs/goals.md`, `docs/rules.md`, `ARCHITECTURE.md`, `docs/decisions/**`, `GOVERNANCE.md`, `docs/policies/**`, every `AGENTS.md`, `CLAUDE.md` and `.claude/`.

## Review Focus

- **A rule listed with no real check, or a check that passes its own bad example.** `guard rules` runs each check on its bad example. It is tested with a fake registry entry whose check never fires.
- **Swift notes lost in the move.** Test: every backticked identifier and file path in the old `CLAUDE.md` appears somewhere in the new `AGENTS.md`/`Sources/AGENTS.md`/`ARCHITECTURE.md`.
- **A stray `CLAUDE.md`** (for example `Sources/CLAUDE.md`) that turns into a second, unreviewed instruction source. `GUARD-CLAUDE-MD` handles it.
- **A dev profile that still writes into the real `~/.claude/`.** The script refuses to run without a sandbox home, and its docs list what it can't isolate: Keychain, and UserDefaults beyond the dev bundle id.
- **Skills presented as trusted before anyone tried them.** Each skill carries a `Tried:` line, and it stays "not yet" until it has been tried.

---

### Task A: Agent instructions (AGENTS.md layers)

**Files:**
- Create: `AGENTS.md`, `Sources/AGENTS.md`
- Modify: `CLAUDE.md` (becomes `@AGENTS.md`), `mobile/AGENTS.md` (only if a fact must move)
- Test: `tools/guard/tests/test_manual_move.py`

- [ ] Write a test that extracts every backticked token (identifier or path) from the old `CLAUDE.md` with `git show agent-run-oss:CLAUDE.md`. It asserts each token appears in the union of `AGENTS.md`, `Sources/AGENTS.md` and `ARCHITECTURE.md`; for `ARCHITECTURE.md`, skip it if the file doesn't exist yet. Run it and see it fail.
- [ ] Root `AGENTS.md`, under about 80 lines:
  - what Notchling is;
  - the layers table, with links;
  - build, test and check commands (Swift, guard, phone);
  - the line: what may auto-merge versus what waits (point to `tools/guard/zones.json`);
  - how work flows;
  - the style rules: short plain text, Title Case menus, match the surrounding code, guard early, no force-unwraps;
  - the safety rules that apply to any change: tests never touch Keychain, logins, ssh or `gh`; never edit the user's real `~/.claude/settings.json` except our own hook entries.
  - It ends with `@Sources/AGENTS.md`, so Claude Code loads the Swift notes, plus a link to `mobile/AGENTS.md`.
- [ ] `Sources/AGENTS.md` is today's `CLAUDE.md` content about the Swift app, nearly verbatim: build and install, Phone, Checking UI changes, Architecture in one pass, Things that bit before, Style. It may be reorganised but must not be shortened in substance.
- [ ] `CLAUDE.md` is exactly `@AGENTS.md\n`.
- [ ] Run the test and see it pass.

### Task B: Goals, governance and policies

**Files:**
- Create: `docs/goals.md`, `GOVERNANCE.md`, `docs/policies/automation.md`, `docs/policies/privacy.md`
- Modify: `tools/guard/zones.json` (add `!docs/policies/**` to auto), `.github/CODEOWNERS` (regenerated)

- [ ] `docs/goals.md` covers:
  - what Notchling is and who it's for;
  - goals, non-goals and anti-goals, from the spec;
  - what it will never do;
  - what's weaker on Linux and Windows: no per-app secret access, because Secret Service and Credential Manager have no per-app ACL.
- [ ] `GOVERNANCE.md` covers:
  - maintainer `@dtthaison`, who alone can satisfy `maintainer-gate`;
  - TrietKun keeps write access under the same rules;
  - who may queue work (`notchling:queued`: maintainer only);
  - who approves protected changes;
  - how decisions are made and changed (a new decision record);
  - appeals ("ask for the maintainer");
  - DCO sign-off for contributors.
- [ ] `docs/policies/automation.md` says what agents may do and not do:
  - the auto-merge zone, the App's permissions, daily caps (`AGENT_DAILY_RUN_CAP=20`, `REVIEW_DAILY_CAP=40`);
  - pause after 3 unsampled days;
  - the kill switch variable;
  - issue text is data, never instructions;
  - review of outside PRs only after the `notchling:review` label.
- [ ] `docs/policies/privacy.md` says what data leaves a machine today:
  - the phone bridge;
  - the optional Cloudflare tunnel;
  - account and limit calls to providers;
  - artifact owner-publish;
  - no telemetry today, with crash reports planned as opt-in only.
  - Check claims against the code: grep for `URLSession` and the hosts called. Never claim more than the code does.
- [ ] Zones: add `!docs/policies/**`, regenerate CODEOWNERS (`PYTHONPATH=tools/guard python3 -m guard codeowners --write`), and make sure `codeowners --check` and the zone tests pass.

### Task C: Architecture and decision records

**Files:**
- Create: `ARCHITECTURE.md`, `docs/decisions/0002-…` onward (one per decision), `docs/decisions/README.md` (index plus template)

- [ ] `ARCHITECTURE.md` covers two layers, today and the target:
  - **Today:** the Swift app's parts and how they talk (app, hook process via files, hostd, the phone bridge, ConsoleCore). Take the facts from `Sources/AGENTS.md` / today's CLAUDE.md and `docs/architecture.md`.
  - **Target:** notchlingd, the local API tiers, the app reading transcripts itself, the hook under a daemon, ptyhost, secrets per OS, remote computers, GPUI app plus kit, `wry` for web content, the protocol, the repo layout and Swift–Rust ownership.
  - It links each decision record.
- [ ] Decision records, short ADRs (Context, Decision, Consequences, Status, Date), one each for:
  - 0002 the line (named safe areas, maintainer-gate);
  - 0003 agent instructions in AGENTS.md;
  - 0004 Rust engine as the always-on notchlingd service;
  - 0005 GPUI for every desktop app (cite 0001's spike);
  - 0006 Flutter phone with generated protocol types and no Rust;
  - 0007 local API tiers and peer identity;
  - 0008 stay MIT, permissive crates only via cargo-deny;
  - 0009 rulesets as code, applied by the maintainer;
  - 0010 build then sign apart, Beta then Stable;
  - 0011 one owner per resource while Swift and Rust run side by side.
  - Copy decisions and reasons from the spec; don't invent new ones. Mark the spec's *(my call)* items as "Proposed" rather than "Accepted".
- [ ] Keep `docs/architecture.md` (the earlier restructuring review). Link it from ARCHITECTURE.md as history.

### Task D: Rules catalog and the rules check

**Files:**
- Create: `docs/rules.md`, `tools/guard/guard/rules.py`, `tools/guard/tests/test_rules.py`
- Modify: `tools/guard/guard/__main__.py` (subcommand `rules`), `.github/workflows/guard.yml` (step "Rules"), and `tools/guard/guard/claudemd.py` (new, for rule `GUARD-CLAUDE-MD`)

- [ ] `rules.py` holds `REGISTRY: Dict[str, Callable[[], bool]]`, mapping each rule id to a probe. The probe runs that rule's real check on the rule's bad example and returns True when the check fires with that rule id. Cover every rule id the guard emits:
  - LINT-* (from `lint.RULES` bad examples);
  - CAP-* (`capability.RULES`, plus CAP-BINARY, CAP-DYNAMIC, CAP-OBSCURE);
  - WEAK-TEST-EDIT, WEAK-SKIP, WEAK-UNICODE;
  - ZONE and CASE-COLLISION (gate);
  - GUARD-CODEOWNERS, GUARD-PINS, GUARD-RULESETS, GUARD-BASELINE, GUARD-CLAUDE-MD, GUARD-RULES.
  - Read the modules to find the real ids; don't invent ones the code doesn't emit.
- [ ] `docs/rules.md` has one table row per rule: `| ID | Rule | Why | Check |`. The Check column names the command, e.g. `python3 -m guard lint`.
- [ ] `python3 -m guard rules`:
  - parses the ids in `docs/rules.md`'s table;
  - fails `GUARD-RULES` when an id is listed without a registry entry, has a registry entry but isn't listed, or its probe returns False;
  - prints each problem.
- [ ] `claudemd.py`, rule `GUARD-CLAUDE-MD`: among tracked files (`git ls-files`), the only `CLAUDE*.md` (case-insensitive, any folder) is the root `CLAUDE.md`, and its content is exactly `@AGENTS.md\n`. Wire it into `guard rules` or as its own subcommand `claudemd`; either way, guard.yml runs it.
- [ ] Tests:
  - the real docs pass;
  - a temp rules.md with an unknown id fails;
  - a registry entry missing from the docs fails;
  - a probe that returns False fails (monkeypatched);
  - GUARD-CLAUDE-MD fails on a temp repo with `Sources/CLAUDE.md`, or with a root CLAUDE.md holding other text.

### Task E: Flow, feature map and first skills

**Files:**
- Modify: `CONTRIBUTING.md`, `.github/PULL_REQUEST_TEMPLATE.md`
- Create: `docs/feature-map.md`, `.claude/skills/port-swift-part/SKILL.md`, `.claude/skills/add-protocol-field/SKILL.md`, `.claude/skills/add-provider/SKILL.md`, `.claude/skills/fix-bug-reproduction-first/SKILL.md`

- [ ] `CONTRIBUTING.md` is rewritten for people and agents alike:
  - issue → `notchling:queued` (maintainer) → branch `agent/<issue>-<slug>` or a fork → PR → checks (list them by name) → merge;
  - Beta, then Stable (coming in Phase 6);
  - how to build and run the tests (Swift, guard, phone);
  - what's in the auto-merge zone;
  - DCO sign-off.
  - It no longer says "no test suite yet". Keep the useful parts of today's CONTRIBUTING (bug report details, scope, security).
- [ ] The PR template checklist covers: tests added, checks run locally (`swift test`, `python3 -m guard lint`, …), changelog fragment, no secrets in the diff, and whether protected areas were touched.
- [ ] `docs/feature-map.md` has one row per feature area: area, folders and files, tests that cover it, zone status (auto or protected, from zones.json), and the spec part ids (E*/U*/P*). It is the map for porting.
- [ ] Skills use frontmatter `name` and `description`; the description says when to use it. Each has a body with steps, the checks to run, and a final line `Tried: not yet`. The skills:
  - `fix-bug-reproduction-first`: reproduce with a failing test on main, then fix, and record the rule id if a check could have caught it.
  - `add-provider`: follow docs/architecture.md "Adding a provider".
  - `port-swift-part`: port one E*/U* part from the Swift app to Rust: tests first from fixtures, behavior fixtures match, ownership table.
  - `add-protocol-field`: additive only, schema first, regenerate types, conformance fixture. Mark it as needing Phase 4's protocol folder.

### Task F: Dev profile

**Files:**
- Create: `tools/dev-profile.sh`, `docs/dev-profile.md`
- Modify: `build.sh` (a `--dev` flag)

- [ ] `build.sh --dev` builds `build/Notchling Dev.app` with bundle id `local.notchling.dev` and name "Notchling Dev", so its UserDefaults are separate. Default builds are byte-for-byte unchanged in behavior: diff the commands, and check `./build.sh` still produces the same bundle id.
- [ ] `tools/dev-profile.sh`:
  - creates `~/.notchling-dev/home`, with a `.claude` folder seeded only from what the user chooses (`--copy-login` is off by default);
  - runs a command with `HOME` and `CLAUDE_CONFIG_DIR` pointed into it: `tools/dev-profile.sh claude …` and `tools/dev-profile.sh app`, which launches the dev build's binary directly, not through `open`, which would drop the environment;
  - refuses to run when the target would be the real home.
- [ ] `docs/dev-profile.md` says what is isolated:
  - `~/.claude`, hooks and skills, Application Support;
  - UserDefaults, through the dev bundle id.
- [ ] It also says, plainly, what isn't isolated:
  - the login Keychain, so account switching and the vault in a dev build touch real items: don't use them there;
  - ports 47821/47822, so turn the phone bridge off in the dev build.
- [ ] Verify by running `tools/dev-profile.sh claude --version`, and with a probe binary that prints `NSHomeDirectory()` and the Application Support path under the profile.

### Task G: Integrate and review

- [ ] Run the guard suite on both Pythons, then `guard lint`, `pins`, `codeowners --check`, `rulesets` and `rules`, and `swift test` if build.sh changed.
- [ ] Final whole-branch review on a fresh model, then the fix pass. Merge into `agent-run-oss` with `--no-ff`.

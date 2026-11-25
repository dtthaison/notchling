# XXXX: Skills follow the app, data follows the source

## Context

Notchling installs skills into Claude Code's, Codex's and Antigravity's skills folders: `notchling-delegate`, `notchling-pages`, `notchling-vault`, `notchling-settings` and more. `crates/engine/src/install/skills.rs` writes them. A folder is Notchling's when it holds the mark file `.notchling-skill`. Each install is Write, Update, Unchanged or Remove, and a tool that isn't installed is skipped. Decision [0019](0019-agent-instructions-per-tool.md) says which files Notchling owns in each tool.

Provider model data changes every few weeks. Antigravity removes models on 2026-11-18. `gemini-4-hydrogen` came out on 2026-11-12. The delegate skill had model names in its text, so it went stale without a release.

Claude Code can't talk to Codex or Antigravity itself. Knowing which of their models to use is Notchling's job, not something each user writes into their own rules.

The model data comes from the catalog of decision [0016](0016-provider-interface-in-the-engine.md). The reference implementation of catalog roles and `--for` is the branch `agent/delegate-skill-models`.

## Decision

Four layers, each changing at its own pace.

1. **Skill text says how to do the job.** It holds the steps and the safety rules, never data that changes: no model names, prices or lists. Notchling writes it from code at app launch and rewrites it when the built text differs. The mark file decides ownership: delete it to keep your own edit. The text moves with the app version.
2. **The commands a skill calls are the stable contract.** `notchling delegate-models --for <role>`, `notchling secrets`, `notchling pages`, `notchling settings`. Flags are only added, never renamed or removed, so an older skill works with a newer app.
3. **The data behind the commands follows its source.** In order:
   - the compiled catalog;
   - the override `catalog.json` beside the daemon socket, which now also takes `roles`, `not_for` and `use` per model;
   - each tool's live model list;
   - a marked model that is gone is replaced by the provider's own hint, else by the newest live model of the same family (same codename, highest numeric version). The output says which model replaced which.

   A provider change or a user's preference needs no release.
4. **Not now: a signed online catalog** for default roles between releases. It costs network use and a trust chain.

One exception to layer 1: a skill's one-line description may carry live trigger words, such as the vault skill's saved secret names. Notchling rewrites the description when that data changes, not only at launch.

Each machine's Notchling installs its own skills, matching its own version. Versions across machines stay in step through [version sync](XXXX-tool-versions-follow-the-mac.md).

Settings shows all of this to the user, in a Skills & Models section built separately.

## Consequences

- A new provider model needs nothing: the live list brings it in, and the family rule or the provider's hint picks it.
- A user's preference goes in the override file.
- A new command needs a new app version.
- A release changes only the shipped defaults.
- The family rule may pick a model the user wouldn't. The output states the replacement, and the override file changes it.
- The override file can be written by an agent, like every file the user owns. This is the class of risk decision [0007](0007-local-api-tiers.md) accepts. The file can only name a model; it never widens what may start (0016's allowlist still applies).

## Status

Accepted. The maintainer approved the design on 2026-11-15.

## Date

2026-11-15

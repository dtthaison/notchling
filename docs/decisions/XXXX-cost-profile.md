# XXXX: The Cost Profile owns one model table and sets each tool up from it

## Context

Which model and effort each kind of work gets lives today in files nobody generates: `~/.claude/rules/model-choice.md`, `pstack-models.md`, `orchestrator.md`, and what the user remembers for Codex and Antigravity. They drift from each other and from the models that exist. A new Claude model appears and no file names it; a hand-written table keeps sending reviews to a model that is gone.

Notchling already knows the models ([the catalog](0016-provider-interface-in-the-engine.md)), runs the delegation and writes agent instructions per tool ([0019](0019-agent-instructions-per-tool.md)). It is the one place that can keep one table and write every tool's files from it.

Facts that bound the change:

- [0019](0019-agent-instructions-per-tool.md) lets Notchling write only its own `notchling-*` files, and [Goals](../goals.md) says it never changes a file it didn't write. A Cost Profile that stops at those files can't replace `model-choice.md` or manage `orchestrator.md`, which is where the table lives for most people.
- The user's rule files hold lines that are not the table (habits, reminders). Replacing the file must not lose them.
- Claude Code, Codex and Antigravity rewrite their own config files. Ownership of a whole file is the wrong unit there; a key is the right one.
- [0020](0020-agents-change-settings-by-class.md): guard levels and free text imported from a person's files must not be something an agent can write.

## Decision

- **One table.** Seven roles (read, code, fix, judge, hardest, review, image) by three tools (Claude Code, Codex, Antigravity); each cell is a model and an effort, or none (image has no Claude cell). Three presets, Saver, Balanced (the default) and Quality, are data in `crates/engine/data/profiles.json`. Floors hold in every preset and for every pick: judge is at least opus high, hardest at least opus xhigh, fix at least sonnet high.
- **Models come from the catalog, never from code.** A Claude cell names a family (`sonnet`, `opus`); its id is the newest current id of that family in the catalog and the live list. A family the catalog lacks moves to the nearest one, higher first, and the floors apply after. Codex and Antigravity cells take the newest model of the cell's family from each tool's own list, and clamp an effort the model doesn't offer. No `claude-*` id is written in code.
- **Set Up is a plan, then an apply.** The plan is pure: a list of file actions, each with a unified diff. Apply writes them, makes a backup of every file it changes (`<file>.notchling-backup-<stamp>`, a name Claude Code doesn't load as a rule) and records a manifest at `daemon/cost-setup.json`: path, hash written, backup path, and per key the value before and after.
- **What Set Up writes, by tool, and who agrees.**
  - Notchling's own files need no extra yes: `~/.claude/rules/notchling-cost.md` (the table, generated, with the habits), the agents `notchling-builder`, `notchling-reader` and `notchling-reviewer`, Codex's fenced block in `~/.codex/AGENTS.md` (the file is created when missing), and the rule file in Antigravity's `notchling` plugin folder.
  - Keys in files Notchling doesn't own are written one by one: `bashOutputMaxChars` and `modelSettings.<id>.autoCompactWindow` in `settings.json`; `[agents]` keys in Codex's `config.toml`, each only after Codex itself accepts it (`codex features list -c key=value`), only when the file exists.
  - Files the person owns change only after the person agrees to that part: `pstack-models.md` (only when pstack's plugin is installed; it is never created without the plugin and never removed), a hand-written model rule replaced by `notchling-cost.md`, an existing `orchestrator.md` managed in place, pointers in other files that still name the old rule. A new `notchling-orchestrator` is an offer, off by default. Each consent is a name; without it the action is shown as an Offer with its diff and nothing is written.
- **Import before replace.** A hand-written rule file is read line by line. A line that is a table row or a role line becomes a pick in the profile; the table's own header and separator are regenerated; every other line is kept, in a section of `notchling-cost.md`. The report says, for each line, where it went. The file is replaced only after that.
- **Undo is per tool and never overwrites.** It reverses the manifest: a created file is deleted, a changed file is restored from its backup, keys are put back one by one, a Codex block is cut out. If a file, key or block changed since Apply, it is a conflict: reported, left as it is. Applying again keeps the first backup, so Undo reaches the original.
- **Check reports drift** on every managed part: in step, outdated, edited, missing, changed by the tool, returned. For an edited rule the person picks Keep mine (the edit is imported into the profile and the file is regenerated from it) or Restore. Other files: Keep mine accepts them as they are; Restore rewrites them.
- **Settings classes.** The profile is stored in Settings: `costPreset`, `costPicks`, `costClaude`, `costCodex` and `costAgy` are Ask (what runs changes). `costProfileApp` (guard levels, habit edits, imported lines) is Never: an agent never writes or reads it. The agent tier reads the table and plans, and sees the changed keys of `settings.json` and `config.toml` but not their text. Applying, undoing and resolving drift are app tier and behind the `install` switch, like hooks and skills.
- **The ops** are `cost.profile.get`, `cost.profile.set`, `cost.setup.plan`, `cost.setup.apply`, `cost.setup.undo` and `cost.setup.check`; the command line is `notchling cost setup [--apply]`, `notchling cost check` and `notchling cost undo <tool>`. Setup is a dry run unless `--apply` is given.

This amends [Goals](../goals.md)'s line that Notchling "never changes or removes a file it didn't write": it may, for the Cost Profile only, with the person's yes to that file, a backup and an Undo that stops at anything changed since.

## Consequences

- One edit to the table changes every tool's instructions; a new model reaches them at the next Set Up (or Check), not by hand.
- A person with no pstack, no `model-choice.md` and no orchestrator gets only Notchling's own files and two settings keys.
- The command line's `--apply` is refused for an agent: only the app writes. Agents plan and check; the person applies.
- `notchling wait` and the guard hook are not part of this: the rule's lines for them are off until they ship (`WAIT_SHIPS`).
- Codex's `[agents]` keys depend on Codex accepting them; a Codex that rejects one gets nothing for that key and says so.
- The Antigravity model list is not read from `agy` yet, so its cells are written as the profile names them and marked unchecked.
- Done in `crates/engine/src/cost_profile/` (plan, apply, undo, check, import, resolve), `crates/engine/data/profiles.json`, `crates/notchlingd/src/cost_profile_ops.rs`, `cost_profile_json.rs` and `cost_setup_cli.rs`.

## Status

Proposed. Amends [Goals](../goals.md) and builds on [0019](0019-agent-instructions-per-tool.md) and [0020](0020-agents-change-settings-by-class.md). The maintainer settles it by merging the change that brings it.

## Date

2026-11-20

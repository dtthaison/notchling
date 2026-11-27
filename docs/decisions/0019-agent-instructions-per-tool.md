# 0019: The skills, rules and hooks Notchling installs in each agent tool

## Context

Notchling already writes into the agent tools' own folders: five hook entries in `~/.claude/settings.json` (`install::hooks::SLOTS`) and three skills, `notchling-delegate`, `notchling-pages` and `notchling-vault`, into Claude Code's, Codex's and Antigravity's skills folders (`install::skills`, through `install.apply` behind the daemon's `install` switch). The goals named only the hook entries. Settings lists the hooks but not the skills, and nothing can be turned off one at a time.

The maintainer wants two more texts in every tool Notchling supports: a rule that keeps the main chat for planning and hands the work to subagents, and a rule that says which model and effort each kind of work gets. Each should be visible and switchable in Settings.

Found in the installed tools on 2026-11-13 (Claude Code 2.1.323, Codex 0.166.0, Antigravity CLI):

- **Claude Code** loads every `~/.claude/rules/*.md` in every project, skills from `~/.claude/skills/` and agents from `~/.claude/agents/`, and runs the main chat as an agent when `settings.json` has an `agent` key. Its Agent tool takes `model` and `effort` per call.
- **Codex** has one global instructions file, `~/.codex/AGENTS.md` (`AGENTS.override.md` replaces it), and no rules folder; skills are in `~/.codex/skills/`. It spawns subagents only when instructions ask, and only with multi-agent on in `config.toml`.
- **Antigravity** loads a plugin's `rules/*.md` marked `trigger: always_on` from `~/.gemini/config/plugins/<name>/` (24 KB a file, 20,000 tokens in all) and skills from `~/.gemini/config/skills/`. A subagent can't be given a model; swarm workers can. Its docs warn that writes bypassing its language server can clobber `config.json`.
- Users may already have their own: a model-choice rule such as `~/.claude/rules/model-choice.md`, or a plan-only agent they set up by hand.

The design, with the instruction registry, the model presets, the daemon ops, the Settings layout and the draft texts, is [the agent instructions spec](../superpowers/specs/2026-11-13-agent-instructions-design.md). This record holds what is decided; where the two differ, this record wins.

## Decision

1. **One model, agent instructions,** covers everything Notchling puts into an agent tool: skills, rules and agents. Each has one text in the repo (`crates/engine/data/instructions/`), rendered per tool, and a toggle per tool. Every toggle is on by default except Plan-Only Main Chat.
2. **Settings > Agents** has one page per tool: Claude Code, Codex, Antigravity, and any later tool whose client is found. Each page has five parts: **Client** (where it was found and its version, or Install Help), **Instructions** (a row per instruction: toggle, one-line explanation, status, its full text and path), **Hooks** (each hook and what it does), **Models** (the tool's model table) and **Status and Repair**. The skills (vault, pages, delegate) are rows like the rules.
3. **Only Notchling's files.** Notchling writes only the files in the table below, plus its hook entries and, only while Plan-Only Main Chat is on, the `agent` key in `~/.claude/settings.json`. Every root is a parameter in code; tests use temp roots only.
4. **Ownership by marker and hash** (below). Notchling never overwrites or removes a file it didn't write, or one someone edited unless the user chooses Use Notchling's.
5. **The user's own file wins** (below). When the user has their own text for the same purpose, Notchling writes nothing for it in that tool.
6. **Off removes.** Turning a row off removes Notchling's file (or its section of `AGENTS.md`). No disabled copy is kept: the tools load any folder under `skills/`, and the text can always be rendered again.
7. **One model table per tool,** and the only place a model is named. Rows: Read, Code, Fix, Judge, Hardest. Each cell is a model and an effort picked from the catalog (decision [0016](0016-provider-interface-in-the-engine.md)), never free text, so nothing typed in Settings reaches an agent's prompt. Presets: Save, Tiered (the default), Best; any edit makes it Custom. The delegation text never names a model: one line in it follows the model-choice state (Notchling's table, the user's own file, or off), and both texts are rendered in one pass so they never disagree.
8. **Hooks.** Claude Code's five hooks stay as `install::hooks::SLOTS` writes them, in two groups the user can turn off, `prompt_hooks` and `page_hooks`, each shown with its event, matcher, timeout, command and what it does for the user. Notchling adds no hooks to Codex or Antigravity: the sessions it drives get their approvals over Codex's protocol and ACP.
9. **Every write goes through the daemon,** from app-tier ops behind the `install` switch (off at every start), under the lock `InstallOps` already takes. The app passes the wanted state in each call; the daemon never acts on the settings store alone, since an agent with a shell can write it. A forged value can only turn a Notchling file on or off, or pick another catalog model.
10. **Updates refresh, never add.** After an app update, with the `install` switch on, the app repairs each tool it finds: Notchling's unedited files are brought up to date, nothing that is off is written, and an edited file or the user's own is never touched.

### Files per tool

`<claude>` is `~/.claude` (a lane's `CLAUDE_CONFIG_DIR` links the parts named in `lanes::SHARED`), `<codex>` is `CODEX_HOME` or `~/.codex`, `<gemini>` is `~/.gemini/config`.

| Instruction | Kind | Default | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- | --- | --- |
| Delegation, `notchling-delegation` | rule | on | `<claude>/rules/notchling-delegation.md` | a fenced section of `<codex>/AGENTS.md` when Notchling owns that file, else `<codex>/skills/notchling-delegation/` | `<gemini>/plugins/notchling/rules/notchling-delegation.md` |
| Model choice, `notchling-model-choice` | rule | on | `<claude>/rules/notchling-model-choice.md` | as delegation | `<gemini>/plugins/notchling/rules/notchling-model-choice.md` |
| Delegate, `notchling-delegate` | skill | on | `<claude>/skills/notchling-delegate/` | `<codex>/skills/notchling-delegate/` | `<gemini>/skills/notchling-delegate/` |
| Vault, `notchling-vault` | skill | on | `<claude>/skills/notchling-vault/` | `<codex>/skills/notchling-vault/` | `<gemini>/skills/notchling-vault/` |
| Pages, `notchling-pages` | skill | on | `<claude>/skills/notchling-pages/` | none | none |
| Plan-Only Main Chat, `notchling-orchestrator` | agent | off | `<claude>/agents/notchling-orchestrator.md` and `"agent": "notchling-orchestrator"` in `<claude>/settings.json` | none | none |
| Prompt Hooks, Page Hooks | hooks | on | Notchling's entries in `<claude>/settings.json`, as today | none | none |

- Skill folders keep today's place and `.notchling-skill` mark, so folders today's build wrote stay Notchling's.
- Notchling owns `<codex>/AGENTS.md` only if it created it, when neither `AGENTS.md` nor `AGENTS.override.md` existed. Each of its sections is fenced by `<!-- notchling:<id> -->` and `<!-- /notchling:<id> -->`, and the file goes when its last section goes.
- The Antigravity plugin folder holds `plugin.json` and a `.notchling-plugin` mark, and goes when its last rule goes. Its rules carry `trigger: always_on`. Skills stay in `<gemini>/skills/`: inside the plugin they would be renamed.
- Never written: `~/.claude/CLAUDE.md`, any other key of `settings.json`, Codex's `config.toml` (read only, to see whether multi-agent is on), Antigravity's `config.json`, `~/.gemini/GEMINI.md`.
- A tool whose own folder is missing gets nothing, as today (`plan_skills` never creates `~/.codex` or `~/.gemini/config`).

### Ownership

Every Notchling file starts with one marker line, after the front matter where there is one. It also tells someone who deleted the app how to remove the file:

```
<!-- notchling-delegation v1. Written by Notchling. Turn it off in Notchling > Settings > Agents. If you edit this file, Notchling stops changing it. -->
```

The daemon keeps a manifest, `<data>/daemon/instructions.json`, written with `write_private_no_follow`: for each path, the id, tool, version and SHA-256 of the bytes Notchling last wrote. A plan compares the file on disk with it:

| On disk | Wanted on | Wanted off |
| --- | --- | --- |
| missing | write | nothing |
| Notchling's, hash matches | update if the render differs | remove |
| Notchling's, hash differs (edited) | leave; "Edited by you" | leave; "Off, but your edited copy is still there" |
| Notchling's mark, no manifest entry (today's build) | taken as unedited once, then recorded | remove |
| written by the Swift app (`name: notchling-…`, no mark) | leave; "Written by the old app" | nothing |
| not Notchling's, same name | leave; "A file of this name isn't Notchling's" | nothing |
| a link, or not a plain file or folder | leave | nothing |

One action replaces an edited or old-app file: **Use Notchling's** (`instructions.restore`), which first moves the user's copy to `<file>.notchling-backup-<ms>`, the suffix `settings.json` backups use. The `agent` key follows the same rule: Notchling sets it only when the key is absent, and removes it only while it still says `notchling-orchestrator`.

### The user's own file

A file is the user's own when it isn't Notchling's and is in effect for the same purpose:

| Purpose | Counts as the user's own |
| --- | --- |
| Model choice, Claude Code | a `<claude>/rules/*.md` whose name contains `model` |
| Delegation, Claude Code | a `<claude>/rules/*.md` whose name contains `delegat` or `orchestrat`, or an `agent` key in `settings.json` naming any agent but Notchling's. An agent file alone isn't in effect: the row only notes it. |
| Either, Codex | a heading in the user's own `<codex>/AGENTS.md` that contains "model choice", "choosing a model" or "delegat" |
| Either, Antigravity | a rule of that name pattern in another plugin under `<gemini>/plugins/`, or a heading of that kind in `~/.gemini/GEMINI.md` |

Notchling then writes nothing for that instruction in that tool. The row says **Your own rules are used**, with the file's path and, behind Show Text, the file's text. The toggle stays on, so Notchling's text comes back at the next repair if the user's file goes. For model choice, the Models part shows the user's text in place of the table. Detection reads file names and headings only; Settings reads a file's text when the user opens it, over an app-tier op, and the text goes into no message, log or file Notchling writes.

### The vault skill

Its description lists the saved secrets' names (with the host each is for), never values, so its text changes when a secret is saved or removed. After a vault change the daemon renders it again, after 2 s of quiet, behind the `install` switch, only in the tools where it is on, Notchling's and unedited. While the vault switch is off it is left as it is. Turning it off in a tool stops agents there from offering the vault; `notchling secret` still works.

## The maintainer's answers

The design left three questions open. The maintainer answered them on 2026-11-13:

1. **Plan-only main chat in Claude Code: offered, off by default.** The Claude Code page has a Plan-Only Main Chat toggle. Turned on, it writes Notchling's orchestrator agent, `<claude>/agents/notchling-orchestrator.md` (a short plan-only prompt whose tools leave out Edit, Write and NotebookEdit; the delegation rule keeps the how), and sets `"agent": "notchling-orchestrator"` in `~/.claude/settings.json`, with a backup, touching no other key. Turned off, it removes both. The delegation rule is on by default either way. The maintainer's words named the agent `orchestrator`; it is `notchling-orchestrator` because a user may already have an agent of that name (the maintainer does), two agents of one name leave Claude Code to pick one, and Notchling's files are `notchling-*`.
2. **Codex with the user's own `AGENTS.md`: skills.** When `<codex>/AGENTS.md` or `AGENTS.override.md` exists and isn't Notchling's, the delegation and model-choice texts go in as `<codex>/skills/notchling-delegation/` and `<codex>/skills/notchling-model-choice/`, and the user's file is never touched. The row says "Installed as a skill: your AGENTS.md is yours".
3. **The user's own model rules are kept.** When a tool has the user's own model-choice rules (such as `~/.claude/rules/model-choice.md` or `pstack-models.md`), Notchling writes no model-choice rule for that tool, and Settings says **Your own rules are used** and shows their text. The delegation text then points to the user's file.

## Consequences

- The goals change with this record ([goals.md](../goals.md)): Notchling may write the files above and, only while Plan-Only Main Chat is on, the `agent` key. Before, the goals named only the hook entries, though the skills were already written. [privacy.md](../policies/privacy.md) says what the texts send: the tools put them into each session's prompt, so they go to the tool's provider, and the vault skill carries the saved secrets' names.
- What a tool can't do is said on its page, not hidden:
  - Antigravity can't give a subagent a model. Its table names models only for swarm workers and `notchling delegate`.
  - Codex loads a skill only when a task matches its description, so the texts installed as skills apply less often than always-on text.
  - When Codex's `config.toml` has multi-agent off, its page says the delegation text does nothing until it's turned on in Codex.
  - Plan-Only Main Chat takes the file-editing tools from the main chat, but Bash stays, so it is a strong nudge, not a sandbox.
- A lane session gets no rules until `rules` joins `lanes::SHARED` (phase 4): until then it has neither Notchling's rules nor the user's own.
- A changed text needs a new version: a test fails when a text's hash changes and its version doesn't. The texts steer users' agents, so they live in `crates/engine/`, outside the auto-merge zone.
- The local API grows additively: `instructions.status`, `instructions.text`, `instructions.apply`, `instructions.repair`, `instructions.restore`, `instructions.remove` and the `instructions.changed` event. `install.*` keep working and forward their skill names. New settings keys: `agentInstructions`, `modelChoice`, `agentSetupVersion`.
- Welcome's Install Hook becomes Set Up Your Agents: one line per tool found, and one button that applies the defaults.
- Nothing of this is built yet. Today: the hooks and the three skills as `install.apply` writes them, and a read-only Hooks list in Settings (`sheets/hook_rows.rs`).

## Known problems in today's code

Each is fixed in the phase named, not by this record. Items 1 to 5 come from the design's research; 6 was found while writing this record.

1. **An edited skill is overwritten.** `install::skills::plan_one` plans `Update` for a marked folder whose `SKILL.md` differs from the render, so the next apply replaces the user's edit, and a remove deletes it. Phase 3: the manifest's hash decides.
2. **The vault skill loses its names while the vault is off.** `VaultOps::skill_names` is empty with the vault switch off, so an apply rewrites the skill as "saved now: none yet". Phase 3: left as it is while the vault is off.
3. **Lanes miss rules.** `lanes::SHARED` links `skills`, `agents` and `plugins` but not `rules`, so a lane session loads no `~/.claude/rules/` file. Phase 4.
4. **Two settings do nothing where they seem to.** "Keep local copies of artifacts" (`keepPages`) is read only by Settings: the page hooks save every publish whatever it says. "Answer prompts from the notch" (`answerFromNotch`) is read only by the notch, never by the hook or the daemon. Phase 5: the Page Hooks toggle on the Claude Code page takes `keepPages`'s place; `answerFromNotch` stays the notch's own.
5. **The tool enum is written three times:** `delegate::skill::SkillTool`, `vault::skill::SkillTool` and `install::skills::tool_name`. Phase 2: one `catalog::Tool`.
6. **The pages skill and privacy.md promise a publish the Rust hook never makes.** Both say the hook publishes to a page another saved account owns, with that account's login; `notchling hook` always passes no owner to `pages::hook::pre_tool_use` (`pages::owner` isn't wired, crates/AGENTS.md). Phase 2 corrects the skill's text as it moves to data; privacy.md's Artifact section is corrected with phase 5.

## Phases

Each phase is one pull request into `agent-run-oss`. No test reads or writes the real `~/.claude`, `~/.codex` or `~/.gemini`. Zone: **M** waits for the maintainer (protected files, or a capability check such as `CAP-FILEWRITE`); **A** is the auto-merge zone, though today the maintainer merges every pull request.

| # | What | Tests | Zone |
| --- | --- | --- | --- |
| 0 | This record, the spec, and the goals and privacy changes | `python3 -m guard rules`, `python3 -m guard lint` | M |
| 1 | The Settings > Agents shell and the Claude Code Hooks part over today's `install.plan` (event, matcher, timeout, command, what it does), with today's Hooks list moved into it; kit `switch` and `segmented` | a pure model over a recorded `install.plan` reply; a render | A |
| 2 | Engine `instructions` core, pure: the registry, the texts as data (delegation, model choice and the orchestrator agent new; the three skill texts moved, their wrappers kept), the renderer, the Claude Code `Target`, the model table and presets over a `Catalog`, own-file detection, the ownership plan; problems 5 and 6 (the skill) | render snapshots per tool; every placeholder filled; the version-hash test; plan cases over temp roots (missing, ours, edited, today's mark, the Swift app's, taken, link, own file); the delegation text's model line in its three states | M |
| 3 | Apply, remove and restore with the manifest; skills go through it (`install.*` forward); the `agent` key through `install::file`'s backed-up writer; problems 1 and 2 | apply, edit, apply keeps the edit; off removes only unedited files; restore backs up first; a planted link is refused; today's marked folders are adopted once; the `agent` key is written only when absent with every other byte of `settings.json` kept, and removed only while Notchling's; today's `skills_tests.rs` unchanged | M, `CAP-FILEWRITE` |
| 4 | The daemon ops, the vault refresh, the settings keys, `docs/local-api-ops.md`; `rules` in `lanes::SHARED` (problem 3) | `instructions_ops_tests.rs`: tiers, switch off returns the plan, the lock, a vault refresh only behind the switch, the user's own text only to the app tier; a lanes test | M |
| 5 | App: the Claude Code page complete (Client, Instructions with Plan-Only Main Chat, Models, Status and Repair), the hook group toggles (problem 4), Welcome's Set Up Your Agents, repair after an update; privacy.md's Artifact section (problem 6) | pure models over fixture replies (`yours`, `edited`, `off_edited`, switch off); the `agentSetupVersion` flow over a fake transport; renders, light and dark | A for the app (`CAP-PROCESS` if Install Help adds a URL opener), M for privacy.md |
| 6 | Codex: its `Target` (`AGENTS.md` when free, skills otherwise), multi-agent read from `config.toml`, the Codex page | temp `CODEX_HOME`s with and without the user's `AGENTS.md` or `AGENTS.override.md`; a section added and removed; a `config.toml` with multi-agent off | M for the engine, A for the app |
| 7 | Antigravity: the plugin `Target`, rules with `trigger: always_on`, the Antigravity page with its model note | a temp `~/.gemini/config`; the plugin folder made, and removed with its last rule; another plugin's rule seen as the user's own | M for the engine, A for the app |

Phases 1 and 2 can run in parallel; 6 and 7 can too, after 5.

## Rejected alternatives

- **A Notchling section inside the user's own `~/.codex/AGENTS.md`.** It would always apply, but it edits a file the user owns (answer 2).
- **Plan-only main chat on by default.** It writes a key of the user's config beyond Notchling's hooks and changes how every session behaves (answer 1).
- **Notchling's model table beside the user's own** (the design's Add Notchling's Too). Two tables would disagree, and answer 3 keeps the user's alone.
- **A disabled copy kept in the tool's folder.** The tools load any folder under `skills/`, so it would still load.
- **Writing Codex's `config.toml` or Antigravity's `config.json`** to turn on multi-agent or set subagent models. They are the tools' own files, and Antigravity's docs warn that a write that bypasses its language server can clobber its settings.

## Status

Proposed. The maintainer asked for the feature and answered the design's three open questions on 2026-11-13; merging this record with its goals and privacy changes settles it. Each phase is its own pull request.

## Date

2026-11-13

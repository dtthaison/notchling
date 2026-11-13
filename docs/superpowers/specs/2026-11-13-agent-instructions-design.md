# Agent instructions: the skills, rules, hooks and model choice Notchling installs

Date: 2026-11-13. Status: decided in [0019](../../decisions/0019-agent-instructions-per-tool.md), which holds the files per tool, ownership, the user's own files, the maintainer's three answers, the known problems in today's code and the phases. Where the two differ, the record wins. This page holds the rest of the design: what each tool supports, the engine's shape, model choice, the hooks, the ops, the Settings page and the draft texts.

## 1. What each tool supports

Found in the installed tools (Claude Code 2.1.323, Codex 0.166.0, Antigravity CLI), not taken from memory. Paths assume the real home and are parameters in code.

| Need | Claude Code | Codex | Antigravity CLI |
| --- | --- | --- | --- |
| Always-on text | `~/.claude/rules/*.md`, loaded in every project. Also `~/.claude/CLAUDE.md`, the user's own. | One global file, `~/.codex/AGENTS.md` (or `AGENTS.override.md`, which replaces it). No rules folder. | A plugin's `rules/*.md` with front matter `trigger: always_on`, under `~/.gemini/config/plugins/<name>/`; a new plugin there loads by itself. Global rules: 24 KB a file, a 20,000-token budget. |
| Skills | `~/.claude/skills/<name>/SKILL.md` | `~/.codex/skills/<name>/SKILL.md` | `~/.gemini/config/skills/<name>/SKILL.md` |
| Subagents | The Agent tool, with `model` and `effort` per call; agent files in `~/.claude/agents/`; `--agent` (and the `agent` setting) runs the main chat as an agent, "its system prompt, tool restrictions, and model". | `spawn_agent` (multi-agent v2, `features.multi_agent_v2` in `config.toml`, with `default_subagent_model` and `default_subagent_reasoning_effort`); role files in `~/.codex/agents/`. Codex's own prompt: "Do not spawn sub-agents unless the user or applicable AGENTS.md/skill instructions explicitly ask." | Plugin `agents/<name>.md` with `subagent: true`. Its migration skill drops a subagent's `model`, so a subagent runs on the session's model. The built-in swarm gives each worker a model (`swarm{action:"model"}`). |
| Model per subagent | Yes, per call. | Yes: `spawn_agent` lists "Available models". | Only for swarm workers. |
| Hooks | `settings.json` `hooks` (Notchling's five today) | `[hooks]` in `config.toml` | A plugin's `hooks.json` |

## 2. One engine module for skills, rules and agents

A new module, `notchling_engine::instructions`, plans purely and writes through the existing private-write helpers. It replaces `install::skills` and keeps its behavior, apart from the fixes in 0019's known problems 1 and 2.

```rust
pub enum Kind { Skill, Rule, Agent }

pub struct Instruction {
    pub id: &'static str,
    pub kind: Kind,
    pub title: &'static str,
    pub explain: &'static str,
    pub version: u32,
    pub source: &'static str,
    pub tools: &'static [Tool],
    pub default_on: bool,
    pub same_purpose: Purpose,
    pub inputs: Inputs,
}

pub trait Target {
    fn tool(&self) -> Tool;
    fn client(&self, env: &Env) -> Client;
    fn place(&self, i: &Instruction, s: &Scan) -> Place;
    fn wrap(&self, i: &Instruction, body: &str) -> String;
    fn own_files(&self, i: &Instruction, s: &Scan) -> Vec<PathBuf>;
}
```

`id` is the file and folder stem; `source` is the `include_str!` of its data file; `tools` are `catalog::Tool`s; `same_purpose` is how to find the user's own file (0019, "The user's own file"); `inputs` are what the render needs. A new tool is one `Target`.

The registry, in this order:

| id | Kind | Tools | Default | Inputs |
| --- | --- | --- | --- | --- |
| `notchling-delegation` | Rule | Claude, Codex, Antigravity | on | the model-choice state (section 3.3) |
| `notchling-model-choice` | Rule | Claude, Codex, Antigravity | on | the tool's model table |
| `notchling-delegate` | Skill | Claude, Codex, Antigravity | on | the binary, Codex's first model |
| `notchling-pages` | Skill | Claude | on | the binary |
| `notchling-vault` | Skill | Claude, Codex, Antigravity | on | the binary, the saved secrets' names |
| `notchling-orchestrator` | Agent | Claude | off | none |

What changes in the skill installer:

- `SkillSpec` becomes an `Instruction` of kind `Skill`; `plan_skills` and `apply_skills` become `instructions::plan` and `apply`. The `.notchling-skill` mark stays.
- The three copies of the tool enum become `catalog::Tool`.
- The three `TEMPLATE` consts move into data files (section 6). `delegate::skill::content`, `vault::skill::content` and `pages::skill::content` stay as thin wrappers, so their tests stay as they are.
- `install.plan`, `install.apply` and `install.remove` keep taking skill names in `what` and forward them.

## 3. Model choice

### 3.1 Roles

The rows are the same in every tool. The model-choice text is rendered from them, and nothing else names a model.

| Role | What it covers |
| --- | --- |
| Read | bulk reading, search, listing, logs, running a check and reporting it |
| Code | routine code: specified edits, tests, small features, mechanical refactors |
| Fix | a bug fix |
| Judge | design, review, root cause, prose, synthesis of other agents' reports |
| Hardest | cross-cutting design, subtle concurrency or algorithms, a bug two fixes failed to kill |

Fixed lines under the table: pass model and effort on every call; after two failures go one step up and say so; a second opinion uses a different model; judge cost per finished task.

### 3.2 Defaults and presets

Models and efforts come from `catalog.get` (data file, live list, cache: decision 0016). A cell is a pick from that list.

| Role | Claude Code (Tiered) | Codex (Tiered) | Antigravity (Tiered) |
| --- | --- | --- | --- |
| Read | haiku · medium | Codex's default model · low | the session model (swarm workers: the cheapest listed) |
| Code | sonnet · high | default · medium | default · medium |
| Fix | sonnet · xhigh | default · high | default · high |
| Judge | opus · xhigh | default · xhigh | default · high |
| Hardest | fable · xhigh | default · xhigh | default · high |

- Claude's column uses family aliases (`haiku`, `sonnet`, `opus`, `fable`), which the Agent tool takes and which follow a new model in a family (0016's borrowing rule) with no edit.
- Codex and Antigravity ship as one model with rising effort. The catalog knows their model lists but not which model is the cheap one, so a guessed ladder would be wrong; the user picks models per row.
- Presets: **Save** (each row one step down the tool's ladder and one effort down), **Tiered** (the table above), **Best** (one step up). Claude's ladder is haiku, sonnet, opus, fable; for the other two a preset moves only effort. Any edit makes it **Custom**.
- A stored model the catalog no longer lists is rendered as the preset's default, and the row says "Not in Codex's list now; the default is used."
- When the tool has the user's own model rules, no table is rendered and the Models part shows the user's text instead (0019, answer 3).

### 3.3 Staying in step with delegation

Both texts are rendered in one pass from one state. The delegation text never names a model; one line in it follows the model-choice state:

| Model-choice state for the tool | The line in the delegation text |
| --- | --- |
| Notchling's installed | Pick each agent's model and effort by notchling-model-choice, and say which in one line. |
| The user's own file | Pick each agent's model and effort by your rules in `<their path>`, and say which in one line. |
| Off | Give reading and checks a smaller model than judgment, and say which in one line. |

A change to the table or to either toggle renders both files again in one apply.

## 4. Hooks

### 4.1 Claude Code: the five hooks, as `install::hooks::SLOTS` writes them

The command is the same for all: `'<Notchling.app>/Contents/MacOS/notchling' hook` (`install::hook_command`; Settings shows `install::summary`, which drops anything around the program).

| Group (`what`) | Event · matcher | Timeout | What it does for you |
| --- | --- | --- | --- |
| Prompt Hooks (`prompt_hooks`) | `PreToolUse` · `AskUserQuestion` | 300 s | When Claude asks you a question with choices, it shows in the notch, Chat and the phone, and your pick goes back to Claude. |
| Prompt Hooks | `PermissionRequest` · `*` | 300 s | When Claude asks permission to use a tool, you answer Allow or Deny from the notch, Chat or the phone. Notchling never answers for you. |
| Page Hooks (`page_hooks`) | `PreToolUse` · `Artifact` | 60 s | A read of a page this Mac published is answered from Notchling's local copy, so a page from another account doesn't fail. |
| Page Hooks | `PostToolUse` · `Artifact` | 10 s | After a publish, saves a local copy under the account that published it. |
| Page Hooks | `PostToolUseFailure` · `Artifact` | 10 s | When an Artifact read fails (a 404, another account), gives Claude the local copy's path. |

A prompt hook steps aside, and Claude asks in its own terminal, when there's no daemon or no app attached, Act on Sessions is off, the session is `claude -p`, no answer comes in 290 s, the event is over 1 MiB, or the hook's process doesn't descend from the session. The page hooks run in the hook process itself, need no daemon, and step aside after 8 s or on any error.

Each group's toggle maps to `install.apply` or `install.remove` with `what`. Each change keeps a backup of `settings.json` and the user's other hooks, as today. The rows also show a hook the Swift app left (tag "Swift app") and a `notchling hook` entry outside Notchling's places (tag "Not managed"), as `hook_rows.rs` does now.

### 4.2 Codex and Antigravity

Both support hooks. Notchling adds none: the sessions it drives get their approvals over Codex's own protocol and ACP. Their Hooks part says "Notchling adds no hooks to Codex" with that reason.

## 5. Settings, ops and the UI

### 5.1 Settings keys (`settings/keys.rs`)

| Key | Kind | Default | Holds |
| --- | --- | --- | --- |
| `agentInstructions` | Dict | `{}` | per tool, per id (instructions and hook groups): `true` or `false`. A missing entry is the instruction's default (on, except `notchling-orchestrator`). |
| `modelChoice` | Json | none (Tiered) | per tool: `{"preset": "save" \| "tiered" \| "best" \| "custom", "rows": {"read": {"model", "effort"}, …}}` |
| `agentSetupVersion` | String | none | the instruction set's version last applied (section 5.4) |

The app reads them and passes the wanted state in each call's arguments. The daemon never acts on the store alone.

### 5.2 Daemon ops

Additive; names in `wire.rs`; served by `instructions_ops.rs`, an `OpGroup` beside `InstallOps`, under its writing lock.

| Op | Tier | Switch | Args | Reply |
| --- | --- | --- | --- | --- |
| `instructions.status` | app | none, reads only | `{tool?}` | per tool: `client {found, path, version, install_url}`, `items [{id, kind, title, explain, path, form, state, own_path?, version, installed_version?}]`, `hooks` (Claude: `install.plan`'s `present` and changes), `models {preset, rows, choices}`, `health {ok, needs_repair}` |
| `instructions.text` | app | none | `{tool, id, own?}` | `{path, version, text}`: Notchling's render, never the file on disk. With `own: true`, for a `yours` row: the user's file, read at the call, at most 256 KB, never in a log, the audit or an error message. |
| `instructions.apply` | app | `install` | `{tool, want: {id: true \| false}, models?}` | the install envelope, extended with `items` |
| `instructions.repair` | app | `install` | `{tool, want, models?}` | as apply: writes what's missing, updates what's outdated and Notchling's, adds back the hooks of the groups that are on |
| `instructions.restore` | app | `install` | `{tool, id}` | replaces an edited or old-app file after backing it up |
| `instructions.remove` | app | `install` | `{tool?}` | removes every unedited Notchling file and, for Claude Code, calls `install.remove` |
| event `instructions.changed` | topic `install` | | `{tool}` | after any write, a vault refresh included |

`state` is one of `installed`, `outdated`, `missing`, `edited`, `old_app`, `yours`, `taken`, `off`, `off_edited`, `no_tool`, `waiting` (the vault is off). With the switch off, apply returns the plan and the off message, as `install.apply` does.

### 5.3 The Settings page

Settings gets two pages, **General** (today's sections) and **Agents**, switched by a segmented row at the top. Agents has a tab per tool whose client is found, plus any tool set up before and now missing, so its files can be removed. Each tool page, top to bottom:

1. **Client.** Mark, name, "Found at `~/.local/bin/claude`, version 2.1.323", or "Not found" with an **Install Help** button that opens the tool's install page (a URL from the catalog data). Versions come from files only: Claude Code's `versions/<v>` link, Codex's `codex-package.json`; Antigravity shows "Version unknown". No process starts to read one.
2. **Instructions.** A row per instruction: title, toggle, one-line explanation, status pill, and **Show Text**, which opens the full rendered text (mono, scrolling inside a fixed height) with its path and a Copy button. A "Your own rules are used" row shows the user's path, and Show Text shows the user's file. An edited row has **Use Notchling's**. Claude Code's page ends with **Plan-Only Main Chat**, off, explained as "The main chat plans and hands every change to a subagent. Edit and Write are taken from it; Bash stays."
3. **Hooks.** Claude Code: the two group toggles, each with its event rows (event · matcher, timeout, command, what it does, tag). The others: one line saying none, and why.
4. **Models.** The preset pick, then the role table with a model and an effort pick per row. Antigravity's note above it: a subagent runs on the session's model, so the table names models only for swarm workers and `notchling delegate`. With the user's own rules, their text in place of the table.
5. **Status and Repair.** "Everything matches" or "2 need repair: …" with **Repair**, and **Remove Everything Notchling Added** (it asks first). Both are disabled, with the `install` switch's off text, while it is off, as Welcome does now.

Kit: `card`, `label` (`TypeRole::Section`, `Body`, `Meta`, `Mono` for the full text), `scrollable`, `button_with` (`ButtonKind::Secondary` for Repair and Show Text, `Ghost` for Copy), `icon_button` with `Icon::Copy`, `Icon::ChevronRight` and `ChevronDown`, `status_pill` (`Status::Done` installed, `Needs` repair or edited, `Working` while applying), `tool_icon`, and `menu_panel` with `MenuItem` for the model, effort and preset picks. Two kit additions: the toggle (`switch()` in `settings_view.rs`) moves to `kit::ui::switch`, and `kit::ui::segmented` for the page and tool tabs. Each part is tagged `.part("Agents …")`.

App code: pure models in `sheets/agents.rs` (rows from a status reply, tested everywhere), views in `sheets/agents_view.rs`, `agents_models_view.rs` and `agents_hooks_view.rs` (each under 600 lines), calls in `sheets/agents_wire.rs` made off the UI thread, renders `settings-agents-claude`, `-codex` and `-antigravity` in `sheets/renders.rs`. Today's `hook_rows.rs` and `settings_hooks_view.rs` fold into the Hooks part, and the "Keep local copies of artifacts" row becomes the Page Hooks toggle.

### 5.4 Welcome, updates, uninstall

- **Welcome.** Install Hook becomes **Set Up Your Agents**: one line per tool found ("Claude Code: 5 hooks, 3 skills, 1 rule; your model rules are kept"), a **Set Up** button that applies the defaults to every tool found, and **Choose in Settings**. With the switch off it shows what it would do, as now.
- **After an update.** The instruction set's version is the highest `version` among the texts. When the app finds the `install` switch on and `agentSetupVersion` behind, it calls `instructions.repair` per tool found and stores the new version. An edited file says "Edited by you; Notchling has a newer text (v3)".
- **Uninstall.** Remove Everything Notchling Added per tool, and once in About for every tool. Each marker line tells someone who deleted the app how to remove the file by hand.

## 6. Where the texts live

- One file per instruction, `crates/engine/data/instructions/<name>.md`, compiled in with `include_str!`. A short header gives `id`, `kind`, `version`, `title`, `explain`, then the body.
- Per-tool parts are marked blocks: `{{#claude}}…{{/claude}}`, `{{#codex}}…{{/codex}}`, `{{#antigravity}}…{{/antigravity}}`. Values: `{{binary}}`, `{{model_table}}`, `{{model_line}}`, `{{vault_names}}`, `{{codex_model}}`. The renderer is pure; an unknown placeholder or an unclosed block fails a test, not a user.
- Each `Target::wrap` adds the tool's front matter (a skill's `name` and `description`, Antigravity's `trigger: always_on`, an agent's `name`, `description` and `tools`) and the marker line.
- A changed text bumps its `version`. A test fails when a text's hash changes and its version doesn't (`instructions/versions.json` holds the hashes).

## Appendix: draft texts (Claude Code renders)

`~/.claude/rules/notchling-delegation.md`:

```markdown
<!-- notchling-delegation v1. Written by Notchling. Turn it off in Notchling > Settings > Agents. If you edit this file, Notchling stops changing it. -->
# Keep this chat for planning; hand the work to subagents

You are the user's main chat. Talk, read, decide and plan here, and hand the work to subagents so this conversation's context stays small.

- Hand off: reading more than a few files, any edit, running tests or builds, commits. Use the Agent tool; give a large change `isolation: "worktree"`.
- Do yourself: answering from what you already know, a lookup in one known file, reading an agent's report, talking with the user.
- Brief each agent fully: the goal, what is ruled out, the files to read, the checks to run, and whether it may commit or push. It knows nothing else.
- Pick each agent's model and effort by notchling-model-choice, and say which in one line.
- Run independent pieces in parallel, in one message.
- When an agent reports, check its key claim, tell the user what matters in a few lines, and offer the next step.
- Work for Codex or Antigravity goes through the notchling-delegate skill.
```

`~/.claude/rules/notchling-model-choice.md` (Tiered):

```markdown
<!-- notchling-model-choice v1. Written by Notchling. Change the table in Notchling > Settings > Agents > Claude Code > Models. If you edit this file, Notchling stops changing it. -->
# Choosing a subagent's model

| Work | Model | Effort |
| --- | --- | --- |
| Reading many files, search, logs, running a check and reporting it | haiku | medium |
| Routine code: specified edits, tests, small features, mechanical refactors | sonnet | high |
| A bug fix | sonnet | xhigh |
| Judgment: design, review, root cause, prose, combining agents' reports | opus | xhigh |
| The hardest work: cross-cutting design, subtle concurrency, a bug two fixes failed to kill | fable | xhigh |

- Pass `model` and `effort` on every Agent call.
- When an agent fails the same task twice, go one step up (haiku, sonnet, opus, fable) and say so.
- For a second opinion, use a different model from the first.
- Judge cost per finished task, not per call: a cheap agent that needs a retry costs more.
```

`~/.claude/agents/notchling-orchestrator.md` (only while Plan-Only Main Chat is on). It names no `model`, so the main chat keeps the one the user picked. Phase 2 checks against the installed Claude Code how much of the default system prompt a main-chat agent's prompt replaces, and whether a deny list (rather than this allow list) is supported, so that tools added later aren't lost.

```markdown
---
name: notchling-orchestrator
description: Main chat that plans and delegates. It reads and answers, and hands every change to a subagent; it never edits files itself.
tools: Agent, SendMessage, Read, Grep, Glob, Bash, WebSearch, WebFetch, Skill, AskUserQuestion, TaskCreate, TaskUpdate, TaskList, TaskGet, TaskStop
---
<!-- notchling-orchestrator v1. Written by Notchling. Turn it off in Notchling > Settings > Agents > Claude Code > Plan-Only Main Chat. If you edit this file, Notchling stops changing it. -->
You are the user's main chat, and you plan. You can't edit files: hand every change, test run and commit to a subagent with the Agent tool, as the delegation rule says. Don't use Bash to change files either.
```

Codex renders the delegation and model-choice texts with `spawn_agent` in place of the Agent tool, and its own models and efforts. Antigravity renders them for subagents and swarm workers, with the note that a subagent runs on the session's model.

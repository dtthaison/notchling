use super::model::{Habits, Profile, Role};
use super::resolve::{resolve, Available, Resolved};
use crate::catalog::Tool;

pub const RULE_NAME: &str = "notchling-cost.md";
pub const VERSION_TAG: &str = "notchling-cost v1";
pub const BUILDER: &str = "notchling-builder";
pub const READER: &str = "notchling-reader";
pub const REVIEWER: &str = "notchling-reviewer";
pub const ORCHESTRATOR: &str = "notchling-orchestrator";
pub const FENCE_ID: &str = "notchling-cost";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Features {
    pub wait: bool,
}

impl Default for Features {
    fn default() -> Features {
        Features { wait: WAIT_SHIPS }
    }
}

pub const WAIT_SHIPS: bool = false;

pub fn preset_title(preset: &str) -> String {
    let mut chars = preset.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub fn marker(profile: &Profile) -> String {
    format!(
        "<!-- {VERSION_TAG} ({}). Written by Notchling from your Cost Profile (Notchling > Settings > Models & Costs). If you edit this file, Notchling reports it and asks before changing it. -->",
        preset_label(profile)
    )
}

pub fn preset_label(profile: &Profile) -> String {
    if profile.is_custom() {
        format!("{}, edited", preset_title(&profile.preset))
    } else {
        preset_title(&profile.preset)
    }
}

struct Row {
    role: Role,
    uses: &'static str,
    agent: &'static str,
}

const CLAUDE_ROWS: [Row; 5] = [
    Row {
        role: Role::Read,
        uses: "reading many files, search, logs, running a check",
        agent: READER,
    },
    Row {
        role: Role::Code,
        uses: "builders, lanes, UI tasks, tests, refactors, production runs",
        agent: BUILDER,
    },
    Row {
        role: Role::Fix,
        uses: "a bug fix",
        agent: BUILDER,
    },
    Row {
        role: Role::Judge,
        uses: "design, root cause, synthesis, prose",
        agent: "general-purpose",
    },
    Row {
        role: Role::Hardest,
        uses: "cross-cutting design, subtle concurrency, a bug two fixes failed to kill",
        agent: "general-purpose",
    },
];

fn claude(profile: &Profile, role: Role, avail: &Available) -> Option<Resolved> {
    resolve(profile, role, Tool::Claude, avail)
}

fn kept_section(profile: &Profile) -> String {
    let mut out = String::new();
    for imported in &profile.imports {
        let lines: Vec<&str> = imported.kept.iter().map(String::as_str).collect();
        if lines.iter().all(|l| l.trim().is_empty()) {
            continue;
        }
        out.push_str(&format!(
            "\n## Your own rules, kept from {}\n\n{}\n",
            imported.source,
            lines.join("\n").trim_matches('\n')
        ));
    }
    out
}

const PGREP_LINE: &str = "- Never wait with a `pgrep -f` or `sleep` loop: it matches its own command line and never ends. Use the background command's notification, `caffeinate -w <pid>`, or `notchling shells` to see and stop a stuck one.";

fn wait_line(habits: &Habits, features: Features) -> String {
    let first = if features.wait {
        format!(
            "- To wait, use `notchling wait` (CI, a shell, a file, a port, a session); never poll. A builder waits at most {} s; for longer, it hands back.",
            habits.wait_secs
        )
    } else {
        format!(
            "- Never poll with `sleep` loops: run long commands in the background and wait for their notification. A builder waits at most {} s; for longer, it hands back.",
            habits.wait_secs
        )
    };
    format!("{first}\n{PGREP_LINE}")
}

pub fn claude_rule(profile: &Profile, avail: &Available, features: Features) -> String {
    let habits = profile.habits();
    let mut out = format!("{}\n# Models and work habits\n\n", marker(profile));
    out.push_str("Start each Agent description with its role, like `[code] add the search field`, and pass `model` and `effort`. To go above the table, say why: `[code up: two fixes failed]`.\n\n");
    out.push_str("| Role | For | Agent type | Model | Effort |\n|---|---|---|---|---|\n");
    for row in &CLAUDE_ROWS {
        if let Some(r) = claude(profile, row.role, avail) {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                row.role.name(),
                row.uses,
                row.agent,
                r.model.as_deref().unwrap_or(""),
                r.effort
            ));
        }
    }
    out.push('\n');
    out.push_str("- Reviews: `notchling delegate-models --for review` names the tool, model and effort; hand the diff to `notchling delegate`. Once per branch, at the end; re-review only the fix diff. No review subagents; if delegation fails, use one notchling-reviewer.\n");
    out.push_str(&format!(
        "- One task per builder. Near {} tool calls or {}k context, write the handoff (goal, state, decisions, next step, files, checks) and hand back. The hard stop is {} turns.\n",
        habits.hand_back_turns,
        habits.builder_context / 1000,
        habits.max_turns
    ));
    out.push_str(&format!(
        "- At most {} builders at once; queue the rest.\n",
        habits.parallel_cap
    ));
    out.push_str(&format!(
        "- Read narrowly: `grep -n` or `rg -n` first, then about {} lines at a time (`sed -n 'a,bp'` or Read with offset and limit). Never `cat` a whole file over {} lines.\n",
        habits.read_lines, habits.cat_lines
    ));
    out.push_str(&wait_line(&habits, features));
    out.push('\n');
    out.push_str("- After two failures on the same task, go one step up and say so.\n");
    out.push_str(&kept_section(profile));
    out
}

fn agent_head(name: &str, description: &str, lines: &[String]) -> String {
    let mut out = format!("---\nname: {name}\ndescription: {description}\n");
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("---\n");
    out
}

fn model_lines(r: &Resolved) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(model) = &r.model {
        lines.push(format!("model: {model}"));
    }
    lines.push(format!("effort: {}", r.effort));
    lines
}

pub fn builder_agent(profile: &Profile, avail: &Available) -> Option<String> {
    let r = claude(profile, Role::Code, avail)?;
    let habits = profile.habits();
    let mut lines = model_lines(&r);
    lines.push(format!("maxTurns: {}", habits.max_turns));
    let mut out = agent_head(
        BUILDER,
        "Builds one scoped task: reads narrowly, makes the change, runs the checks and writes the handoff. Use for code, fix and production work.",
        &lines,
    );
    out.push_str(&format!("{}\n", marker(profile)));
    out.push_str(&format!(
        "\nYou build one task and hand back.\n\n- Read only what the task needs: search first, then about {} lines at a time.\n- Make the change, run the checks the brief names, and commit as the brief says.\n- Near {} tool calls or {}k context, stop, write the handoff (goal, state, decisions, next step, files, checks) and hand back. Your turn limit is {}; the handoff must be written before it.\n- Never wait longer than {} s for a command; hand back instead.\n- End with what you did, what you checked, and what is left.\n",
        habits.read_lines,
        habits.hand_back_turns,
        habits.builder_context / 1000,
        habits.max_turns,
        habits.wait_secs
    ));
    Some(out)
}

pub fn reader_agent(profile: &Profile, avail: &Available) -> Option<String> {
    let r = claude(profile, Role::Read, avail)?;
    let habits = profile.habits();
    let mut lines = model_lines(&r);
    lines.push("maxTurns: 40".to_string());
    lines.push("disallowedTools: Edit, Write, NotebookEdit".to_string());
    let mut out = agent_head(
        READER,
        "Reads files, searches, scans logs and runs a check, then reports what it found. Never edits.",
        &lines,
    );
    out.push_str(&format!("{}\n", marker(profile)));
    out.push_str(&format!(
        "\nYou read and report; you change nothing.\n\n- Search first, then read about {} lines at a time.\n- Report the answer with file paths and line numbers, in a few lines. Do not paste whole files.\n",
        habits.read_lines
    ));
    Some(out)
}

pub fn reviewer_agent(profile: &Profile, avail: &Available) -> Option<String> {
    let r = claude(profile, Role::Review, avail)?;
    let mut lines = model_lines(&r);
    lines.push("maxTurns: 40".to_string());
    lines.push("disallowedTools: Edit, Write, NotebookEdit".to_string());
    let mut out = agent_head(
        REVIEWER,
        "Reviews a diff when delegation to another tool failed. Read-only.",
        &lines,
    );
    out.push_str(&format!("{}\n", marker(profile)));
    out.push_str("\nReview the diff you are given for correctness bugs and broken project rules. You change nothing.\n\n- Read the diff and only the files it touches.\n- Report each finding with a file, a line and why it is wrong, most serious first. Say plainly when you found nothing.\n");
    Some(out)
}

pub fn orchestrator_agent(profile: &Profile, features: Features) -> String {
    let mut out = agent_head(
        ORCHESTRATOR,
        "Main chat session that plans and delegates. It reads and answers, and hands every change to a subagent; it never edits files itself.",
        &["tools: Agent, SendMessage, Read, Grep, Glob, Bash, WebSearch, WebFetch, Skill, AskUserQuestion, TaskCreate, TaskUpdate, TaskList, TaskGet, TaskStop".to_string()],
    );
    out.push_str(&format!("{}\n", marker(profile)));
    out.push_str("\nYou are the user's main chat session. Talk, read, investigate and plan here; make no changes yourself.\n\n");
    out.push_str("- Every code or file change goes to a subagent (Agent tool), or for long work the user will want to follow, a new background session.\n");
    out.push_str("- Pick each subagent's model and effort from ~/.claude/rules/notchling-cost.md, and say which in one line.\n");
    out.push_str("- Brief each agent fully: goal, what is ruled out, files to read, checks to run, commit and push rules. It knows nothing else.\n");
    out.push_str("- Never call EnterWorktree in this session; it would bind every running subagent to that worktree.\n");
    out.push_str("- Bash is for reading and checking, not for editing files.\n");
    out.push_str(&format!("- {}\n", REVIEW_BULLET));
    if features.wait {
        out.push_str(&format!("- {}\n", WAIT_BULLET));
    }
    out.push_str("- When an agent reports, verify the key claim, relay what matters in a few lines, and offer the next step.\n");
    out
}

pub const REVIEW_BULLET: &str = "Reviews go through `notchling delegate`, not a review subagent: `notchling delegate-models --for review` names the tool, model and effort.";
pub const WAIT_BULLET: &str = "Wait with `notchling wait`; never poll.";
pub const RULE_POINTER: &str = "rules/notchling-cost.md";

pub fn orchestrator_in_place(existing: &str, features: Features) -> String {
    let mut text = existing.to_string();
    for old in ["rules/model-choice.md", "rules/pstack-models.md"] {
        text = text.replace(old, RULE_POINTER);
    }
    let has = |t: &str, needle: &str| t.lines().any(|l| l.contains(needle));
    let mut add: Vec<String> = Vec::new();
    if !text
        .lines()
        .any(|l| l.contains("notchling delegate") && l.to_lowercase().contains("review"))
    {
        add.push(format!("- {REVIEW_BULLET}"));
    }
    if features.wait && !has(&text, "notchling wait") {
        add.push(format!("- {WAIT_BULLET}"));
    }
    if add.is_empty() {
        return text;
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    for line in add {
        text.push_str(&line);
        text.push('\n');
    }
    text
}

fn table_for(profile: &Profile, tool: Tool, avail: &Available) -> String {
    let mut out = String::from("| Role | Model | Effort |\n|---|---|---|\n");
    for role in Role::ALL {
        if let Some(r) = resolve(profile, role, tool, avail) {
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                role.name(),
                r.model.as_deref().unwrap_or("tool default"),
                r.effort
            ));
        }
    }
    out
}

pub fn codex_block(profile: &Profile, avail: &Available, features: Features) -> String {
    let habits = profile.habits();
    let mut out = format!("{}\n# Models and work habits\n\n", marker(profile));
    out.push_str("When you start a subagent, pick its model and reasoning effort from this table by the kind of work.\n\n");
    out.push_str(&table_for(profile, Tool::Codex, avail));
    out.push_str(&format!(
        "\n- At most {} subagents at once.\n- Read narrowly: search first, then about {} lines at a time; never print a whole file over {} lines.\n- Reviews and images go through `notchling delegate`, not a subagent.\n",
        habits.parallel_cap, habits.read_lines, habits.cat_lines
    ));
    out.push_str(&wait_line(&habits, features));
    out.push('\n');
    out
}

pub fn agy_rule(profile: &Profile, avail: &Available, features: Features) -> String {
    let habits = profile.habits();
    let mut out = format!(
        "---\ntrigger: always_on\n---\n{}\n# Models and work habits\n\n",
        marker(profile)
    );
    out.push_str("A subagent runs on the session's model. Swarm workers and sessions Notchling starts use this table.\n\n");
    out.push_str(&table_for(profile, Tool::Antigravity, avail));
    out.push_str(&format!(
        "\n- Read narrowly: search first, then about {} lines at a time; never read a whole file over {} lines.\n- Reviews and images go through `notchling delegate`.\n",
        habits.read_lines, habits.cat_lines
    ));
    out.push_str(&wait_line(&habits, features));
    out.push('\n');
    out
}

pub fn fence_open(id: &str) -> String {
    format!("<!-- notchling:{id} -->")
}

pub fn fence_close(id: &str) -> String {
    format!("<!-- /notchling:{id} -->")
}

pub fn fenced(id: &str, inner: &str) -> String {
    format!(
        "{}\n{}\n{}\n",
        fence_open(id),
        inner.trim_end(),
        fence_close(id)
    )
}

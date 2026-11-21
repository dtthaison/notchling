use serde_json::{json, Value};

pub struct OpUse {
    pub op: &'static str,
    pub tier: &'static str,
    pub acts: bool,
    pub switch: Option<&'static str>,
}

const fn read(op: &'static str) -> OpUse {
    OpUse {
        op,
        tier: "agent",
        acts: false,
        switch: None,
    }
}

const fn acts(op: &'static str) -> OpUse {
    OpUse {
        op,
        tier: "agent",
        acts: true,
        switch: None,
    }
}

const fn with_switch(op: OpUse, switch: &'static str) -> OpUse {
    OpUse {
        switch: Some(switch),
        ..op
    }
}

const fn app(op: &'static str) -> OpUse {
    OpUse {
        op,
        tier: "app",
        acts: true,
        switch: None,
    }
}

pub struct Row {
    pub command: &'static str,
    pub usage: &'static str,
    pub summary: &'static str,
    pub ops: &'static [OpUse],
    pub consent: &'static str,
    pub json: bool,
    pub waits: bool,
    pub idempotent: bool,
    pub exits: &'static [i32],
    pub skill: &'static str,
}

const READ_EXITS: &[i32] = &[0, 1, 2, 3, 4];
const LOCAL_EXITS: &[i32] = &[0, 1, 2];
const ACT_EXITS: &[i32] = &[0, 1, 2, 3, 4, 5, 6, 7, 124];

#[allow(clippy::too_many_arguments)]
const fn row(
    command: &'static str,
    usage: &'static str,
    summary: &'static str,
    ops: &'static [OpUse],
    consent: &'static str,
    json: bool,
    exits: &'static [i32],
    skill: &'static str,
) -> Row {
    Row {
        command,
        usage,
        summary,
        ops,
        consent,
        json,
        waits: false,
        idempotent: false,
        exits,
        skill,
    }
}

const fn waits(row: Row) -> Row {
    Row { waits: true, ..row }
}

const fn idempotent(row: Row) -> Row {
    Row {
        idempotent: true,
        ..row
    }
}

pub const COMMANDS: &[Row] = &[
    row(
        "status",
        "notchling status [--json] [--socket <path>]",
        "the daemon's version, pid, uptime and clients",
        &[read("daemon.status")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "sessions",
        "notchling sessions [--json] [--socket <path>]",
        "the sessions the daemon sees",
        &[read("sessions.list")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "providers",
        "notchling providers [--json]",
        "what Notchling can do for each tool: native, fallback, not applicable, gap",
        &[],
        "none",
        true,
        LOCAL_EXITS,
        "",
    ),
    row(
        "pages",
        "notchling pages [words] [--json] [--socket <path>]",
        "the saved Artifact pages (id, title, account), newest first",
        &[read("pages.list")],
        "none",
        true,
        READ_EXITS,
        "notchling-pages",
    ),
    row(
        "page",
        "notchling page <link|id|words> [--json] [--socket <path>]",
        "where a saved page is, and who can republish it",
        &[read("pages.find")],
        "none",
        true,
        READ_EXITS,
        "notchling-pages",
    ),
    row(
        "delegate",
        "notchling delegate <codex|agy> [--model --effort --folder --out --write --session --timeout --json] -- <prompt>",
        "hand a job to another tool and wait (read-only unless --write)",
        &[],
        "none",
        true,
        &[0, 1, 2, 3, 4, 124],
        "notchling-delegate",
    ),
    row(
        "delegate-models",
        "notchling delegate-models [codex|agy] [--for <role>] [--json]",
        "the models and efforts each tool offers, and the pick for a role",
        &[],
        "none",
        true,
        &[0, 1, 2],
        "notchling-delegate",
    ),
    row(
        "models",
        "notchling models [check|update] [--json] [--no-skills] [--force]",
        "read each tool's models and efforts; update writes Notchling's copy",
        &[read("models.check"), acts("models.update")],
        "none",
        true,
        READ_EXITS,
        "notchling-models",
    ),
    row(
        "cost report",
        "notchling cost report [--days N] [--plan api|subscription|auto] [--json]",
        "what your agents spent over the last N days and what to change",
        &[read("cost.report")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "cost setup",
        "notchling cost setup [--apply] [--consent a,b] [--json]",
        "plan the Cost Profile's files and show every diff; --apply is the app's",
        &[read("cost.setup.plan"), app("cost.setup.apply")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "cost check",
        "notchling cost check [--json]",
        "which of those files drifted since Notchling wrote them",
        &[read("cost.setup.check")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "cost undo",
        "notchling cost undo <claude|codex|agy> [--json]",
        "put one tool's files back from their backups (the app's)",
        &[app("cost.setup.undo")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "secrets",
        "notchling secrets [words] [--json] [--socket <path>]",
        "the saved secrets that fit a URL, host or words (never values)",
        &[with_switch(read("vault.find"), "vault")],
        "none",
        true,
        READ_EXITS,
        "notchling-vault",
    ),
    row(
        "secret",
        "notchling secret [--json] <name>[,<name>…] -- <command> [args…]",
        "run a command with secrets, its output hidden; exits 125 when it stops before the command, else with the command's own code (126 can't run it, 127 not found)",
        &[with_switch(acts("vault.use"), "vault")],
        "by_entry",
        true,
        &[0, 125, 126, 127],
        "notchling-vault",
    ),
    idempotent(waits(row(
        "secret-request",
        "notchling secret-request <name> [--kind token|login --url --note --how --expires YYYY-MM-DD] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]",
        "ask the user to add or update a secret; the request is saved even with the Vault off, and waits for the user",
        &[acts("vault.request")],
        "request",
        true,
        ACT_EXITS,
        "notchling-vault",
    ))),
    row(
        "settings list",
        "notchling settings list [--json]",
        "every setting: its key, value, and whether an agent may change it",
        &[read("settings.list")],
        "none",
        true,
        READ_EXITS,
        "notchling-settings",
    ),
    row(
        "settings get",
        "notchling settings get <key> [--json]",
        "one setting",
        &[read("settings.get")],
        "none",
        true,
        READ_EXITS,
        "notchling-settings",
    ),
    idempotent(waits(row(
        "settings set",
        "notchling settings set <key> <value> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]",
        "change a setting; an Ask key waits for the user's yes in the Inbox",
        &[acts("settings.set"), acts("settings.asked")],
        "by_key_class",
        true,
        ACT_EXITS,
        "notchling-settings",
    ))),
    row(
        "consent",
        "notchling consent <id> [--wait] [--timeout <seconds>] [--json]",
        "one request for the user's yes: its state, and its result once it ends",
        &[read("consent.get")],
        "none",
        true,
        &[0, 1, 2, 3, 4, 5, 6, 124],
        "",
    ),
    row(
        "consents",
        "notchling consents [--json]",
        "this session's requests for the user's yes, newest first",
        &[read("consent.mine")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "consent cancel",
        "notchling consent cancel <id> [--json]",
        "withdraw a request this session opened",
        &[acts("consent.cancel")],
        "none",
        true,
        &[0, 1, 2, 3, 4],
        "",
    ),
    row(
        "shells",
        "notchling shells [--json] [--socket <path>]",
        "this session's background shells: id, state, how long each has run, and whether it looks stuck",
        &[read("shells.mine")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "shells stop",
        "notchling shells stop <id> [--json] [--socket <path>]",
        "end the processes writing one of this session's background shells (SIGTERM, then SIGKILL after 3 s)",
        &[acts("shells.stop")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "session reload",
        "notchling session reload <id|self> [--after-turn] [--json]",
        "restart a session's process with the same conversation, to pick up new MCP servers, settings, skills and hooks",
        &[with_switch(acts("session.reload"), "acting")],
        "own_free",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "cluster status",
        "notchling cluster status [--json]",
        "this machine's cluster and its members",
        &[read("cluster.status")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "cluster join",
        "notchling cluster join --token-stdin [--json]",
        "join the cluster whose token is on stdin",
        &[acts("cluster.join")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
    row(
        "cluster reach",
        "notchling cluster reach <addr> <port>",
        "check that a Tailscale address answers on a port",
        &[],
        "none",
        false,
        LOCAL_EXITS,
        "",
    ),
    waits(row(
        "ask",
        "notchling ask [--choice <text>]... [--main-may-answer] [--wait] [--timeout <seconds>] [--json] -- <question>",
        "ask the person a question and wait for the answer",
        &[
            with_switch(acts("ask.open"), "acting"),
            with_switch(read("ask.wait"), "acting"),
        ],
        "none",
        true,
        &[0, 1, 2, 3, 4, 5, 124],
        "notchling-ask",
    )),
    row(
        "asks",
        "notchling asks [--wait] [--json]",
        "the open questions you may answer; --wait blocks until there is one",
        &[with_switch(read("ask.mine"), "acting")],
        "none",
        true,
        &[0, 1, 2, 3, 4, 124],
        "notchling-ask",
    ),
    row(
        "answer",
        "notchling answer <id> (--choice <n> | -- <text>) [--json]",
        "answer a question you may answer",
        &[with_switch(acts("ask.answer"), "acting")],
        "own_free",
        true,
        READ_EXITS,
        "notchling-ask",
    ),
    row(
        "help",
        "notchling help [--json]",
        "every command; --json for agents",
        &[],
        "none",
        true,
        &[0, 2],
        "",
    ),
    row(
        "capabilities",
        "notchling capabilities [--json] [--socket <path>]",
        "what this caller may do now, and why not",
        &[read("daemon.capabilities")],
        "none",
        true,
        READ_EXITS,
        "",
    ),
];

pub struct Planned {
    pub op: &'static str,
    pub feature: &'static str,
    pub phase: u8,
}

const fn planned(op: &'static str, feature: &'static str, phase: u8) -> Planned {
    Planned { op, feature, phase }
}

pub const PLANNED: &[Planned] = &[
    planned("events.subscribe", "Follow events", 2),
    planned("search.query", "Search pages and sessions", 2),
    planned("inbox.list", "Read the Inbox", 2),
    planned("accounts.list", "Read accounts", 2),
    planned("providers.list", "Read providers", 2),
    planned("providers.usage", "Read provider usage", 2),
    planned("limits.status", "Read limits", 2),
    planned("limits.usage", "Read limits", 2),
    planned("limits.set_mode", "Change the limit mode", 2),
    planned("cost.summary", "Read the cost summary", 2),
    planned("organizations.list", "Read the organization", 2),
    planned("settings.switches", "Read the switches", 2),
    planned("settings.get_acting", "Read the switches", 2),
    planned("awake.state", "Read Keep Awake", 2),
    planned("versions.get", "Read tool versions", 2),
    planned("versions.latest", "Read the newest versions", 2),
    planned("versions.workers", "Read worker versions", 2),
    planned("versions.check", "Check tool versions", 2),
    planned("outbox.list", "Read queued messages", 2),
    planned("activity.list", "Read the session's activity", 2),
    planned("prompts.list", "Read pending prompts", 2),
    planned("catalog.roles", "Read the role table", 2),
    planned("catalog.get", "Read the model catalog", 2),
    planned("log.verify", "Verify the log chain", 2),
    planned("settings.set_acting", "Switch Act on Sessions on", 3),
    planned("bridge.set", "Switch the Phone Bridge on", 3),
    planned("settings.set_switch", "Switch a daemon switch on", 3),
    planned("session.new", "Start a session", 4),
    planned("folder.trust", "Trust a folder for a session", 4),
    planned("session.reply", "Reply to a session", 4),
    planned("session.send", "Queue a reply to a session", 4),
    planned("session.answer", "Answer a session's question", 4),
    planned("session.interrupt", "Interrupt a session", 4),
    planned("session.stop", "Stop a session", 4),
    planned("session.remove", "Remove a session", 4),
    planned("session.command", "Run a slash command in a session", 4),
    planned("session.dialog", "Read a session's dialog", 4),
    planned("session.dialog_pick", "Pick in a session's dialog", 4),
    planned("session.dialog_dismiss", "Dismiss a session's dialog", 4),
    planned("session.chrome", "Switch a session's Chrome", 4),
    planned("session.shell_output", "Read a session's shell output", 4),
    planned("session.reload.state", "Read a reload's state", 4),
    planned("session.held.discard", "Discard a held message", 4),
    planned("codex.send", "Send to a Codex session", 4),
    planned("codex.stop", "Stop a Codex session", 4),
    planned("codex.delete", "Delete a Codex session", 4),
    planned("codex.children", "Read a Codex session's children", 4),
    planned("agy.sessions", "List Antigravity sessions", 4),
    planned("agy.state", "Read an Antigravity session", 4),
    planned("agy.conversation", "Read an Antigravity conversation", 4),
    planned("agy.open", "Open an Antigravity session", 4),
    planned("agy.send", "Send to an Antigravity session", 4),
    planned("agy.cancel", "Cancel an Antigravity turn", 4),
    planned("agy.set_option", "Set an Antigravity option", 4),
    planned("cluster.dispatch", "Dispatch a job to a machine", 5),
    planned("cluster.follow", "Follow a job", 5),
    planned("cluster.return", "Return a job's result", 5),
    planned("shell.stop", "Stop a shell job", 5),
    planned("cluster.create", "Create a cluster", 6),
    planned("cluster.join.token", "Make a join token", 6),
    planned("cluster.invite", "Invite a machine", 6),
    planned("cluster.member.set", "Change a member", 6),
    planned("cluster.remove", "Remove a member", 6),
    planned("cluster.main.set", "Choose the primary", 6),
    planned("cluster.move_home", "Move the home", 6),
    planned("cluster.move.now", "Move the home now", 6),
    planned("cluster.failover.set", "Set failover", 6),
    planned("cluster.allow.set", "Allow secrets for the cluster", 6),
    planned("accounts.switch", "Switch an account", 7),
    planned("accounts.add.begin", "Add an account", 7),
    planned("accounts.remove", "Remove an account", 7),
    planned("accounts.move_all", "Move every session to a lane", 7),
    planned("lanes.list", "Read lanes", 7),
    planned("lanes.create", "Create a lane", 7),
    planned("lanes.remove", "Remove a lane", 7),
    planned("lanes.switch", "Switch a lane", 7),
    planned("providers.switch", "Switch a provider", 7),
    planned("providers.add.begin", "Add a provider", 7),
    planned("providers.remove", "Remove a provider", 7),
    planned("session.move", "Move a session to another account", 7),
    planned("install.plan", "Plan Set Up", 8),
    planned("install.apply", "Apply Set Up", 8),
    planned("tailscale.switch", "Switch the tailnet", 8),
    planned("versions.update_local", "Update a tool", 8),
    planned("cost.profile.get", "Read the Cost Profile", 8),
    planned("cost.profile.set", "Change the Cost Profile", 8),
    planned("agy.sidecar.setup", "Set up the Antigravity sidecar", 8),
];

pub struct UserOnly {
    pub op: &'static str,
    pub feature: &'static str,
    pub why: &'static str,
}

const fn only(op: &'static str, feature: &'static str, why: &'static str) -> UserOnly {
    UserOnly { op, feature, why }
}

const PROMPT: &str = "A prompt is the user's own check on an agent.";
const TRAIL: &str = "An agent would erase its own trail.";
const PHONE: &str = "A paired phone answers cards: an agent could approve itself.";
const VALUES: &str = "Values never reach an agent.";
const DRIVES: &str = "It changes which machine drives the user's sessions.";
const CARD: &str = "The card is the user's yes.";
const SCOPE: &str = "It changes what an agent may see.";

pub const USER_ONLY: &[UserOnly] = &[
    only("session.permission", "Answer a permission prompt", PROMPT),
    only("agy.permission", "Answer a permission prompt", PROMPT),
    only(
        "install.remove",
        "Remove hooks and skills",
        "An agent would remove the check on itself.",
    ),
    only("log.audit.delete", "Delete the Audit", TRAIL),
    only("log.chain.start", "Start a new log chain", TRAIL),
    only("bridge.pair.begin", "Pair a phone", PHONE),
    only("bridge.pair.answer", "Pair a phone", PHONE),
    only("bridge.revoke", "Revoke a phone", PHONE),
    only("inbox.answer", "Answer an Inbox item", CARD),
    only("consent.answer", "Answer a consent card", CARD),
    only("ask.dismiss", "Dismiss a question", CARD),
    only("vault.approval.answer", "Answer a secret approval", CARD),
    only("vault.list", "List the Vault", VALUES),
    only("vault.save", "Save a secret", VALUES),
    only("vault.admin", "Use a secret on another machine", VALUES),
    only("vault.remove", "Remove a secret", VALUES),
    only("cluster.promote", "Promote a machine", DRIVES),
    only("cluster.take_back", "Take a machine back", DRIVES),
    only("cluster.continue_here", "Continue a session here", DRIVES),
    only("session.take_back", "Take a session back", DRIVES),
    only("organizations.create", "Create an organization", SCOPE),
    only("organizations.rename", "Rename an organization", SCOPE),
    only("organizations.color.set", "Colour an organization", SCOPE),
    only(
        "organizations.folders.set",
        "Set an organization's folders",
        SCOPE,
    ),
    only("organizations.delete", "Delete an organization", SCOPE),
];

pub struct Internal {
    pub op: &'static str,
    pub why: &'static str,
}

const fn internal(op: &'static str, why: &'static str) -> Internal {
    Internal { op, why }
}

pub const INTERNAL_WHYS: &[&str] = &["app_ui", "hook", "node", "protocol"];

pub const INTERNAL: &[Internal] = &[
    internal("hello", "protocol"),
    internal("ask.list", "app_ui"),
    internal("consent.list", "app_ui"),
    internal("consent.opened", "app_ui"),
    internal("hook.request", "hook"),
    internal("session.question", "hook"),
    internal("agy.sidecar.take", "hook"),
    internal("agy.sidecar.report", "hook"),
    internal("agy.sidecar.status", "hook"),
    internal("agy.hook", "hook"),
    internal("vault.request.cancel", "app_ui"),
    internal("vault.pending", "app_ui"),
    internal("inbox.seen", "app_ui"),
    internal("bridge.status", "app_ui"),
    internal("bridge.devices", "app_ui"),
    internal("bridge.tunnel.status", "app_ui"),
    internal("accounts.add.finish", "app_ui"),
    internal("accounts.result", "app_ui"),
    internal("providers.add.cancel", "app_ui"),
    internal("git.status", "app_ui"),
    internal("git.diff", "app_ui"),
    internal("git.branch_diff_stat", "app_ui"),
    internal("git.branch_diff_file", "app_ui"),
    internal("git.branches", "app_ui"),
    internal("git.worktrees", "app_ui"),
    internal("git.branch_list", "app_ui"),
    internal("outbox.edit", "app_ui"),
    internal("outbox.remove", "app_ui"),
    internal("outbox.hold", "app_ui"),
    internal("outbox.resend", "app_ui"),
    internal("outbox.discard", "app_ui"),
    internal("cluster.side_copy.hide", "app_ui"),
    internal("organizations.folders.preview", "app_ui"),
];

pub fn all_ops() -> Vec<&'static str> {
    let mut all: Vec<&'static str> = COMMANDS
        .iter()
        .flat_map(|r| r.ops.iter().map(|o| o.op))
        .collect();
    all.extend(PLANNED.iter().map(|p| p.op));
    all.extend(USER_ONLY.iter().map(|u| u.op));
    all.extend(INTERNAL.iter().map(|i| i.op));
    all
}

pub fn command_of(op: &str) -> Option<(&'static Row, &'static OpUse)> {
    COMMANDS
        .iter()
        .find_map(|r| r.ops.iter().find(|o| o.op == op).map(|o| (r, o)))
}

pub fn why_not(op: &str) -> Option<(String, Value)> {
    if let Some(p) = PLANNED.iter().find(|p| p.op == op) {
        return Some((
            "This has no command yet; ask the user in words.".to_string(),
            json!({
                "reason": "not_yet",
                "phase": p.phase,
                "ask": {"command": "notchling ask -- …", "kind": "ask"},
            }),
        ));
    }
    if USER_ONLY.iter().any(|u| u.op == op) {
        return Some((
            "Only you can do this, in Notchling.".to_string(),
            json!({"reason": "never", "never": true}),
        ));
    }
    match INTERNAL.iter().find(|i| i.op == op) {
        Some(i) if i.why == "app_ui" => Some((
            "only the Notchling app may do this".to_string(),
            json!({"reason": "app_only"}),
        )),
        Some(_) => Some((
            "Notchling's own processes use this.".to_string(),
            json!({"reason": "internal_op"}),
        )),
        None => None,
    }
}

pub fn usage_lines() -> Vec<String> {
    COMMANDS
        .iter()
        .map(|r| format!("  {}\n      {}", r.usage, r.summary))
        .collect()
}

fn op_value(o: &OpUse) -> Value {
    json!({"op": o.op, "tier": o.tier, "acts": o.acts})
}

fn row_value(r: &Row) -> Value {
    json!({
        "command": r.command,
        "usage": r.usage,
        "summary": r.summary,
        "ops": r.ops.iter().map(op_value).collect::<Vec<_>>(),
        "consent": r.consent,
        "json": r.json,
        "waits": r.waits,
        "idempotent": r.idempotent,
        "exits": r.exits,
        "skill": if r.skill.is_empty() { Value::Null } else { json!(r.skill) },
    })
}

pub fn registry_value() -> Value {
    json!({
        "exitCodes": crate::contract::EXIT_CODES
            .iter()
            .map(|(code, name, meaning)| json!({"code": code, "name": name, "meaning": meaning}))
            .collect::<Vec<_>>(),
        "reasons": crate::contract::REASONS,
        "commands": COMMANDS.iter().map(row_value).collect::<Vec<_>>(),
        "planned": PLANNED
            .iter()
            .map(|p| json!({"op": p.op, "feature": p.feature, "phase": p.phase}))
            .collect::<Vec<_>>(),
        "userOnly": USER_ONLY
            .iter()
            .map(|u| json!({"op": u.op, "feature": u.feature, "why": u.why}))
            .collect::<Vec<_>>(),
        "internal": INTERNAL
            .iter()
            .map(|i| json!({"op": i.op, "why": i.why}))
            .collect::<Vec<_>>(),
    })
}

pub fn help_value(version: &str) -> Value {
    let mut value = registry_value();
    if let Some(map) = value.as_object_mut() {
        map.remove("internal");
    }
    value["notchling"] = json!(version);
    value["api"] = json!(notchling_local_api::API_VERSION);
    value
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;

use serde_json::{json, Value};

pub struct OpUse {
    pub op: &'static str,
    pub tier: &'static str,
    pub acts: bool,
    pub switch: Option<&'static str>,
}

pub(crate) const fn read(op: &'static str) -> OpUse {
    OpUse {
        op,
        tier: "agent",
        acts: false,
        switch: None,
    }
}

pub(crate) const fn acts(op: &'static str) -> OpUse {
    OpUse {
        op,
        tier: "agent",
        acts: true,
        switch: None,
    }
}

pub(crate) const fn with_switch(op: OpUse, switch: &'static str) -> OpUse {
    OpUse {
        switch: Some(switch),
        ..op
    }
}

pub(crate) const fn app(op: &'static str) -> OpUse {
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

pub(crate) const READ_EXITS: &[i32] = &[0, 1, 2, 3, 4];
pub(crate) const LOCAL_EXITS: &[i32] = &[0, 1, 2];
pub(crate) const ACT_EXITS: &[i32] = &[0, 1, 2, 3, 4, 5, 6, 7, 124];

#[allow(clippy::too_many_arguments)]
pub(crate) const fn row(
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

pub(crate) const fn waits(row: Row) -> Row {
    Row { waits: true, ..row }
}

pub(crate) const fn idempotent(row: Row) -> Row {
    Row {
        idempotent: true,
        ..row
    }
}

pub const BASE: &[Row] = &[
    row(
        "status",
        "notchling status [--json] [--socket <path>]",
        "the daemon's version, pid, uptime and clients",
        &[read("daemon.status")],
        "none",
        true,
        READ_EXITS,
        "notchling",
    ),
    row(
        "sessions",
        "notchling sessions [--json] [--socket <path>]",
        "the sessions the daemon sees",
        &[read("sessions.list")],
        "none",
        true,
        READ_EXITS,
        "notchling-sessions",
    ),
    row(
        "sessions history",
        "notchling sessions history [--limit <n>] [--before <cursor>] [--json] [--socket <path>]",
        "the sessions Notchling no longer lists, newest first: the ones that ended in the last day, then older Claude Code sessions of this session's organization from the last 14 days (title masked); --before takes the next cursor of an earlier reply",
        &[read("sessions.history")],
        "none",
        true,
        READ_EXITS,
        "notchling-sessions",
    ),
    row(
        "providers",
        "notchling providers [--json]",
        "what Notchling can do for each tool: native, fallback, not applicable, gap",
        &[],
        "none",
        true,
        LOCAL_EXITS,
        "notchling",
    ),
    row(
        "pages",
        "notchling pages [words] [--json] [--socket <path>]",
        "the saved Artifact pages of this session's organization (id, title, account), newest first",
        &[read("pages.list")],
        "none",
        true,
        READ_EXITS,
        "notchling-pages",
    ),
    row(
        "page",
        "notchling page <link|id|words> [--json] [--socket <path>]",
        "where a saved page of this session's organization is, and who can republish it",
        &[read("pages.find")],
        "none",
        true,
        READ_EXITS,
        "notchling-pages",
    ),
    row(
        "delegate",
        "notchling delegate <codex|agy> [--model --effort --folder --out --write --session --timeout --json] -- <prompt>",
        "hand a job to another tool and wait (read-only unless --write); inside a session the daemon lists it with your jobs",
        &[acts("delegate.begin"), acts("delegate.end")],
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
        "notchling",
    ),
    row(
        "cost setup",
        "notchling cost setup [--apply] [--consent a,b] [--json]",
        "plan the Cost Profile's files and show every diff; --apply is the app's",
        &[read("cost.setup.plan"), app("cost.setup.apply")],
        "none",
        true,
        READ_EXITS,
        "notchling",
    ),
    row(
        "cost check",
        "notchling cost check [--json]",
        "which of those files drifted since Notchling wrote them",
        &[read("cost.setup.check")],
        "none",
        true,
        READ_EXITS,
        "notchling",
    ),
    row(
        "cost undo",
        "notchling cost undo <claude|codex|agy> [--json]",
        "put one tool's files back from their backups (the app's)",
        &[app("cost.setup.undo")],
        "none",
        true,
        READ_EXITS,
        "notchling",
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
        "notchling secret-request <name> [--kind token|login --url --note --how --expires YYYY-MM-DD] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "ask the user to add or update a secret, named <service or host>-<env>-<what> in lowercase with dashes (emqx-dashboard-dev-admin), never a generic word like dashboards, api or login; the request is saved even with the Vault off, and waits for the user",
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
        "notchling settings set <key> <value> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "change a setting (attentionLevel takes calm, normal or urgent, free); an Ask key waits for the user's yes in the Inbox",
        &[acts("settings.set"), acts("settings.asked")],
        "by_key_class",
        true,
        ACT_EXITS,
        "notchling-settings",
    ))),
    idempotent(waits(row(
        "switch on",
        "notchling switch on <acting|bridge|accounts|vault|install|cluster|machines> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "ask the user to turn a switch on for this run; only the Mac's app turns it on, when the user allows",
        &[acts("switch.request")],
        "request",
        true,
        ACT_EXITS,
        "notchling",
    ))),
    row(
        "switch off",
        "notchling switch off <acting|bridge|accounts|vault|install|cluster|machines> [--json]",
        "turn a switch off; no yes needed, and the user sees a notice with Turn On",
        &[acts("switch.off")],
        "none",
        true,
        READ_EXITS,
        "notchling",
    ),
];

const BASE_REST: &[Row] = &[
    row(
        "shells",
        "notchling shells [--stuck] [--json] [--socket <path>]",
        "this session's background shells: id, state, how long each has run, and whether it looks stuck; --stuck lists only those that do, with the reason (no_progress: no CPU and no new output for 10 minutes)",
        &[read("shells.mine"), read("shells.stuck")],
        "none",
        true,
        READ_EXITS,
        "notchling-sessions",
    ),
    row(
        "accounts",
        "notchling accounts [--json] [--socket <path>]",
        "the accounts of this session's organization per tool, with a masked email",
        &[read("accounts.summary")],
        "none",
        true,
        READ_EXITS,
        "notchling-accounts",
    ),
    row(
        "limits",
        "notchling limits [--tool <claude|codex|agy> [--pool <name>]] [--json] [--socket <path>]",
        "whether each tool can run now, its pools and windows and how old the reading is; with --tool exits 0 when usable and 1 when not (4 for other_organization), the reason in the error (credits_out, pool_full, signed_out, stale, limit_reached, other_organization); a tool or pool that is not there or matches twice is a usage error, exit 2 (unknown_tool, unknown_pool, ambiguous_pool), and the message lists the choices",
        &[read("limits.summary")],
        "none",
        true,
        READ_EXITS,
        "notchling-delegate",
    ),
    row(
        "orgs",
        "notchling orgs [--json] [--socket <path>]",
        "this session's organization and how many others exist",
        &[read("organizations.mine")],
        "none",
        true,
        READ_EXITS,
        "notchling",
    ),
    row(
        "search",
        "notchling search <words> [--kind pages|sessions] [--since <30m|24h|7d|date|seconds>] [--tool claude|codex|agy] [--project <folder>] [--account <value>] [--owner <value>] [--section <name>] [--state <name>] [--when today|<n>d|YYYY-MM] [--mode text|hybrid|semantic] [--limit <n>] [--json] [--socket <path>]",
        "pages and sessions of this session's organization that match: the last 14 days of sessions, secrets and emails masked; a flag may repeat to match any of its values, different flags all must match; each hit lists its facets; --mode hybrid or semantic also finds by meaning when Semantic Search is on, and each hit says how it matched",
        &[read("search.query")],
        "none",
        true,
        READ_EXITS,
        "notchling-pages",
    ),
    row(
        "switches",
        "notchling switches [--json] [--socket <path>]",
        "the seven daemon switches: acting, bridge, accounts, vault, install, cluster, machines",
        &[read("settings.switches")],
        "none",
        true,
        READ_EXITS,
        "notchling",
    ),
    row(
        "awake",
        "notchling awake [on|off] [--json] [--socket <path>]",
        "whether the Mac is held awake; on and off change the free mode key",
        &[read("awake.state")],
        "none",
        true,
        READ_EXITS,
        "notchling-settings",
    ),
    row(
        "versions",
        "notchling versions [--latest] [--workers] [--json] [--socket <path>]",
        "the tool versions found here, the newest upstream and each worker's",
        &[read("versions.get"), read("versions.latest"), read("versions.workers")],
        "none",
        true,
        READ_EXITS,
        "notchling",
    ),
    row(
        "roles",
        "notchling roles [--json] [--socket <path>]",
        "the role table: which model each role uses",
        &[read("catalog.roles")],
        "none",
        true,
        READ_EXITS,
        "notchling",
    ),
    row(
        "chat open-file",
        "notchling chat open-file <path> [--session <id>] [--json] [--socket <path>]",
        "open a regular file on this Mac in the file panel of Notchling's Chat window (code with line numbers, JSON pretty-printed, JSONL as one row per line, images, Markdown, text); --session is accepted, but the file opens in the window's one file panel, whichever session is selected; a pipe, device or folder is refused and another machine can't ask for it",
        &[acts("chat.open_file")],
        "none",
        true,
        READ_EXITS,
        "notchling",
    ),
    row(
        "shells stop",
        "notchling shells stop <id> [--json] [--socket <path>]",
        "end the processes writing one of this session's background shells (SIGTERM, then SIGKILL after 3 s)",
        &[acts("shells.stop")],
        "none",
        true,
        READ_EXITS,
        "notchling-sessions",
    ),
    row(
        "session reload",
        "notchling session reload <id|self> [--after-turn] [--json]",
        "restart a session's process with the same conversation, to pick up new MCP servers, settings, skills and hooks",
        &[with_switch(acts("session.reload"), "acting")],
        "own_free",
        true,
        READ_EXITS,
        "notchling-sessions",
    ),
    waits(row(
        "session new",
        "notchling session new <folder> [--model <name>] [--message <text>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "start a Claude Code session in a folder; the user says yes on a card, and the session becomes yours",
        &[with_switch(acts("session.start"), "acting")],
        "request",
        true,
        ACT_EXITS,
        "notchling-sessions",
    )),
    waits(row(
        "session reply",
        "notchling session reply <id> <text> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "send a message to a session; free to one you started (30 an hour), a card for any other",
        &[with_switch(acts("session.say"), "acting")],
        "own_free",
        true,
        ACT_EXITS,
        "notchling-sessions",
    )),
    waits(row(
        "session stop",
        "notchling session stop|interrupt <id> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "stop a session, or interrupt its turn; free for yourself and sessions you started, a card for any other",
        &[with_switch(acts("session.halt"), "acting")],
        "own_free",
        true,
        ACT_EXITS,
        "notchling-sessions",
    )),
    waits(row(
        "session remove",
        "notchling session remove <id> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "remove a stopped session you started; a card for any other",
        &[with_switch(acts("session.discard"), "acting")],
        "own_free",
        true,
        ACT_EXITS,
        "notchling-sessions",
    )),
    waits(row(
        "session answer",
        "notchling session answer <id> --answers <json> [--question <json>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "answer the question a session is asking; the user says yes on a card that shows the question and the answers",
        &[with_switch(acts("session.choose"), "acting")],
        "request",
        true,
        ACT_EXITS,
        "notchling-sessions",
    )),
    waits(row(
        "session command",
        "notchling session command <id> <command> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "run a slash command in a Claude Code session; the user says yes on a card, and a command that loosens permissions is refused",
        &[with_switch(acts("session.run"), "acting")],
        "request",
        true,
        ACT_EXITS,
        "notchling-sessions",
    )),
    waits(row(
        "session dialog",
        "notchling session dialog <id> --pick <1-9>|--dismiss [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "pick an option in the dialog a session shows, or dismiss it; the user says yes on a card that shows the dialog",
        &[with_switch(acts("session.pick"), "acting")],
        "request",
        true,
        ACT_EXITS,
        "notchling-sessions",
    )),
    waits(row(
        "session chrome",
        "notchling session chrome <id> on|off [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "turn Chrome on or off for a Claude Code session; the user says yes on a card",
        &[with_switch(acts("session.browse"), "acting")],
        "request",
        true,
        ACT_EXITS,
        "notchling-sessions",
    )),
    waits(row(
        "session options",
        "notchling session options <id> <option> <value> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "set an option of an Antigravity session; the user says yes on a card",
        &[with_switch(acts("session.tune"), "acting")],
        "request",
        true,
        ACT_EXITS,
        "notchling-sessions",
    )),
    row(
        "cluster status",
        "notchling cluster status [--json]",
        "this machine's cluster and its members",
        &[read("cluster.status")],
        "none",
        true,
        READ_EXITS,
        "notchling-cluster",
    ),
    row(
        "cluster join",
        "notchling cluster join --token-stdin [--json]",
        "join the cluster whose token is on stdin",
        &[acts("cluster.join")],
        "none",
        true,
        READ_EXITS,
        "notchling-cluster",
    ),
    row(
        "cluster reach",
        "notchling cluster reach <addr> <port>",
        "check that a Tailscale address answers on a port",
        &[],
        "none",
        false,
        LOCAL_EXITS,
        "notchling-cluster",
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
        "notchling",
    ),
    row(
        "capabilities",
        "notchling capabilities [--json] [--socket <path>]",
        "what this caller may do now, and why not",
        &[read("daemon.capabilities")],
        "none",
        true,
        READ_EXITS,
        "notchling",
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
    planned("inbox.list", "Read the Inbox", 2),
    planned("accounts.list", "Read accounts", 2),
    planned("providers.list", "Read providers", 2),
    planned("providers.usage", "Read provider usage", 2),
    planned("limits.status", "Read limits", 2),
    planned("limits.usage", "Read limits", 2),
    planned("limits.set_mode", "Change the limit mode", 2),
    planned("cost.summary", "Read the cost summary", 2),
    planned("organizations.list", "Read the organization", 2),
    planned("settings.get_acting", "Read the switches", 2),
    planned("versions.check", "Check tool versions", 2),
    planned("catalog.get", "Read the model catalog", 2),
    planned("log.verify", "Verify the log chain", 2),
    planned("session.shell_output", "Read a session's shell output", 4),
    planned("session.reload.state", "Read a reload's state", 4),
    planned("session.held.discard", "Discard a held message", 4),
    planned("codex.children", "Read a Codex session's children", 4),
    planned("agy.sessions", "List Antigravity sessions", 4),
    planned("agy.state", "Read an Antigravity session", 4),
    planned("agy.conversation", "Read an Antigravity conversation", 4),
    planned("agy.open", "Open an Antigravity session", 4),
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
    internal("settings.set_acting", "app_ui"),
    internal("bridge.set", "app_ui"),
    internal("settings.set_switch", "app_ui"),
    internal("session.new", "app_ui"),
    internal("session.answer", "app_ui"),
    internal("session.command", "app_ui"),
    internal("session.dialog", "app_ui"),
    internal("session.dialog_pick", "app_ui"),
    internal("session.dialog_dismiss", "app_ui"),
    internal("session.chrome", "app_ui"),
    internal("embed.refresh", "app_ui"),
    internal("embed.settings", "app_ui"),
    internal("embed.remove", "app_ui"),
    internal("embed.fetch", "app_ui"),
    internal("embed.fetch.cancel", "app_ui"),
    internal("agy.set_option", "app_ui"),
    internal("folder.trust", "app_ui"),
    internal("session.reply", "app_ui"),
    internal("session.send", "app_ui"),
    internal("session.interrupt", "app_ui"),
    internal("session.stop", "app_ui"),
    internal("session.remove", "app_ui"),
    internal("codex.send", "app_ui"),
    internal("codex.stop", "app_ui"),
    internal("codex.delete", "app_ui"),
    internal("agy.send", "app_ui"),
    internal("agy.cancel", "app_ui"),
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
    internal("outbox.list", "app_ui"),
    internal("activity.list", "app_ui"),
    internal("prompts.list", "app_ui"),
    internal("outbox.edit", "app_ui"),
    internal("outbox.remove", "app_ui"),
    internal("outbox.hold", "app_ui"),
    internal("outbox.resend", "app_ui"),
    internal("outbox.discard", "app_ui"),
    internal("cluster.side_copy.hide", "app_ui"),
    internal("cluster.create", "app_ui"),
    internal("cluster.join.token", "app_ui"),
    internal("cluster.invite", "app_ui"),
    internal("cluster.member.set", "app_ui"),
    internal("cluster.remove", "app_ui"),
    internal("cluster.main.set", "app_ui"),
    internal("cluster.move_home", "app_ui"),
    internal("cluster.move.now", "app_ui"),
    internal("cluster.failover.set", "app_ui"),
    internal("cluster.allow.set", "app_ui"),
    internal("cluster.follow", "app_ui"),
    internal("cluster.return", "app_ui"),
    internal("accounts.switch", "app_ui"),
    internal("accounts.add.begin", "app_ui"),
    internal("accounts.remove", "app_ui"),
    internal("accounts.move_all", "app_ui"),
    internal("lanes.list", "app_ui"),
    internal("lanes.create", "app_ui"),
    internal("lanes.remove", "app_ui"),
    internal("lanes.switch", "app_ui"),
    internal("providers.switch", "app_ui"),
    internal("providers.add.begin", "app_ui"),
    internal("providers.remove", "app_ui"),
    internal("session.move", "app_ui"),
    internal("shell.stop", "app_ui"),
    internal("machines.apply", "app_ui"),
    internal("install.apply", "app_ui"),
    internal("cost.profile.set", "app_ui"),
    internal("versions.update_local", "app_ui"),
    internal("tailscale.switch", "app_ui"),
    internal("agy.sidecar.setup", "app_ui"),
    internal("organizations.folders.preview", "app_ui"),
];

pub fn rows() -> impl Iterator<Item = &'static Row> {
    BASE.iter()
        .chain(crate::commands_inbox::ROWS.iter())
        .chain(BASE_REST.iter())
        .chain(crate::commands_jobs::ROWS.iter())
        .chain(crate::commands_cluster::ROWS.iter())
        .chain(crate::commands_accounts::ROWS.iter())
        .chain(crate::commands_search::ROWS.iter())
        .chain(crate::commands_machines::ROWS.iter())
        .chain(crate::commands_setup::ROWS.iter())
}

pub fn all_ops() -> Vec<&'static str> {
    let mut all: Vec<&'static str> = rows().flat_map(|r| r.ops.iter().map(|o| o.op)).collect();
    all.extend(PLANNED.iter().map(|p| p.op));
    all.extend(USER_ONLY.iter().map(|u| u.op));
    all.extend(INTERNAL.iter().map(|i| i.op));
    all
}

pub fn command_of(op: &str) -> Option<(&'static Row, &'static OpUse)> {
    rows().find_map(|r| r.ops.iter().find(|o| o.op == op).map(|o| (r, o)))
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
    rows()
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
        "commands": rows().map(row_value).collect::<Vec<_>>(),
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

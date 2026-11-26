use crate::commands::{acts, read, row, waits, with_switch, Row, ACT_EXITS, READ_EXITS};

pub const ROWS: &[Row] = &[
    waits(row(
        "accounts switch",
        "notchling accounts switch <account> | notchling accounts add claude|codex|agy | notchling accounts remove <account> | notchling accounts move-all <account>|main | notchling accounts move <session> <lane>|main | notchling lanes create <account> | notchling lanes remove <lane> | notchling lanes switch <lane> <account>; each takes [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "change the accounts and lanes of your organization on this Mac; the user says yes on a card that names the tool, the account by a masked email, the organization and the change; an account of another organization is never changed",
        &[with_switch(acts("accounts.request"), "accounts")],
        "request",
        true,
        ACT_EXITS,
        "notchling-accounts",
    )),
    row(
        "lanes list",
        "notchling lanes list [--json] [--socket <path>]",
        "the Claude Code lanes of this session's organization, each with an opaque id, its account's masked email and how many sessions run in it",
        &[read("lanes.summary")],
        "none",
        true,
        READ_EXITS,
        "notchling-accounts",
    ),
];

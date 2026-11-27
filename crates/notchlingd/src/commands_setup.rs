use crate::commands::{
    acts, idempotent, read, row, waits, with_switch, Row, ACT_EXITS, READ_EXITS,
};

pub const ROWS: &[Row] = &[
    row(
        "install plan",
        "notchling install plan [--what <a,b>] [--json] [--socket <path>]",
        "what Set Up would change in each tool's folders, file by file, with no change made; works with the install switch off",
        &[read("install.plan")],
        "none",
        true,
        READ_EXITS,
        "notchling",
    ),
    waits(row(
        "install request",
        "notchling install request [--part <id>]... [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "ask to set Notchling up in a tool's folders; the user says yes on a card on the Mac that lists each part, and may keep only some of them",
        &[with_switch(acts("install.request"), "install")],
        "request",
        true,
        ACT_EXITS,
        "notchling",
    )),
    waits(row(
        "cost profile",
        "notchling cost profile set|setup|undo [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "ask to change the Cost Profile or its generated files; the user says yes on a card on the Mac that lists each file, and may keep only some of them; reading the profile needs no yes",
        &[
            with_switch(acts("cost.request"), "install"),
            read("cost.profile.get"),
        ],
        "request",
        true,
        ACT_EXITS,
        "notchling",
    )),
    waits(row(
        "versions update",
        "notchling versions update <tool> [--to <version>] [--worker <machine>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "ask to update a tool on this Mac, which needs the install switch, or on a machine of your organization with --worker, which needs the machines switch; the user says yes on a card on the Mac that names the tool, the version and where",
        &[with_switch(acts("versions.request"), "install")],
        "request",
        true,
        ACT_EXITS,
        "notchling",
    )),
    idempotent(waits(row(
        "check-updates",
        "notchling check-updates [on|off] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]",
        "whether Notchling checks for new versions; on or off asks to change the checkForUpdates setting, an Ask key that waits for the user's yes (the same ops as settings get and settings set)",
        &[],
        "by_key_class",
        true,
        ACT_EXITS,
        "notchling-settings",
    ))),
];

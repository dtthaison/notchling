use crate::commands::{acts, row, waits, with_switch, Row, ACT_EXITS};

pub const ROWS: &[Row] = &[waits(row(
    "cluster create",
    "notchling cluster create <name> | notchling cluster token | notchling cluster invite --node <id> --name <name> [--role master|worker] [--acting on|off] | notchling cluster member <node> [--role master|worker] [--delegation on|off] | notchling cluster failover on|off|30m|1h | notchling cluster main <session> [on|off] | notchling cluster allow <node> <secret>... | notchling cluster remove <node> | notchling cluster move-home <node> | notchling cluster move-now; each takes [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]",
    "change the cluster of your organization; the user says yes on a card on the Mac that names the machine and the change; token and invite give the join token once, to this session, with notchling consent <id>",
    &[with_switch(acts("cluster.admin.request"), "cluster")],
    "request",
    true,
    ACT_EXITS,
    "notchling-cluster",
))];

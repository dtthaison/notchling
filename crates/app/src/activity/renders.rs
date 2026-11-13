use serde_json::{json, Value};

use super::model::{ActivityModel, Entry, Kind};
use crate::views::ViewSpec;

pub const NOW_MS: i64 = 1_794_096_000_000;

pub const UTC: i64 = 0;

pub const VIEWS: [ViewSpec; 2] = [
    ViewSpec {
        name: "activity",
        size: (620.0, 760.0),
        about: "the Activity page: every kind of row, newest first (made-up data)",
    },
    ViewSpec {
        name: "activity-filtered",
        size: (620.0, 460.0),
        about: "the Activity page with the Refused and Failed chips on and a search (made-up data)",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    All,
    Filtered,
}

pub fn shown(name: &str) -> Option<Shown> {
    match name {
        "activity" => Some(Shown::All),
        "activity-filtered" => Some(Shown::Filtered),
        _ => None,
    }
}

fn row(
    ago_s: i64,
    source: &str,
    kind: &str,
    op: &str,
    who: &str,
    result: &str,
    detail: Option<&str>,
) -> Value {
    let mut v = json!({
        "time_ms": NOW_MS - ago_s * 1000,
        "source": source,
        "kind": kind,
        "op": op,
        "who": who,
        "result": result,
    });
    if let Some(detail) = detail {
        v["detail"] = json!(detail);
    }
    v
}

fn entries() -> Vec<Entry> {
    let rows = [
        row(40, "audit", "acted", "session.reply", "app", "ok", None),
        row(
            410,
            "audit",
            "failed",
            "session.move",
            "app",
            "failed",
            Some("the target account is not signed in"),
        ),
        row(
            900,
            "audit",
            "refused",
            "vault.pending",
            "curl",
            "forbidden",
            None,
        ),
        row(
            1_300,
            "phone",
            "inbox",
            "inbox.answer",
            "Pixel 9",
            "allow_once",
            None,
        ),
        row(
            1_900,
            "audit",
            "setting",
            "settings.set_switch accounts",
            "app",
            "on",
            None,
        ),
        row(
            3_100,
            "phone",
            "refused",
            "inbox.answer",
            "Pixel 9",
            "refused: acting_off",
            None,
        ),
        row(4_800, "phone", "acted", "message", "Pixel 9", "ok", None),
        row(
            7_200,
            "account",
            "account_switch",
            "account.active",
            "app",
            "switched",
            Some("6f1c2d3e"),
        ),
        row(
            9_000,
            "audit",
            "refused",
            "connect",
            "unknown",
            "peer_unknown",
            None,
        ),
        row(
            12_000,
            "audit",
            "refused",
            "session.stop",
            "node",
            "forbidden",
            None,
        ),
        row(30_000, "audit", "acted", "vault.use", "claude", "ok", None),
        row(
            95_000,
            "audit",
            "inbox",
            "vault.approval.answer",
            "app",
            "ok",
            None,
        ),
        row(200_000, "audit", "acted", "session.new", "app", "ok", None),
    ];
    rows.iter().filter_map(Entry::parse).collect()
}

pub fn model(shown: Shown) -> ActivityModel {
    let mut model = ActivityModel::loaded(entries());
    model.entries.sort_by_key(|e| std::cmp::Reverse(e.time_ms));
    if shown == Shown::Filtered {
        model.toggle(Kind::Refused);
        model.toggle(Kind::Failed);
        model.search = "session".to_owned();
    }
    model
}

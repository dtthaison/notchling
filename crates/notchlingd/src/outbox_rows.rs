use notchling_engine::outbox::{Row, State, Why};
use notchling_events::kinds::agents::ReplyVia;
use serde_json::{json, Value};

pub(crate) fn wire(row: &Row, position: usize, note: Option<&str>, shown: Option<Why>) -> Value {
    let mut out = json!({
        "id": row.id,
        "session": row.session,
        "tool": row.tool.as_str(),
        "kind": row.kind.as_str(),
        "text": row.text,
        "state": row.state.as_str(),
        "position": position,
        "queuedMs": row.queued_ms,
        "updatedMs": row.updated_ms,
        "via": row.via.as_str(),
    });
    let why = match (row.state, row.why) {
        (State::Waiting, None) => shown,
        (_, stored) => stored,
    };
    if let Some(why) = why {
        out["why"] = json!(why.as_str());
    }
    if let (State::Unsure, Some(attempt)) = (row.state, row.attempt_ms) {
        out["attemptMs"] = json!(attempt);
    }
    if row.state == State::Unsure {
        match (row.aside_ms, row.aside_at_ms) {
            (Some(aside), _) => out["asideMs"] = json!(aside),
            (None, Some(at)) => out["asideAtMs"] = json!(at),
            (None, None) => {}
        }
    }
    if let Some(until) = row.hold_ms {
        out["heldUntilMs"] = json!(until);
    }
    if row.joined > 1 {
        out["joined"] = json!(row.joined);
    }
    if let Some(note) = note {
        if row.state == State::Box {
            out["box"] = json!(note);
        } else if shows_error(row) {
            out["error"] = json!(note);
        }
    }
    out
}

pub(crate) fn receipt(row: &Row) -> Value {
    let mut out = json!({
        "id": row.id,
        "state": row.state.as_str(),
        "tries": row.tries,
        "queuedMs": row.queued_ms,
        "updatedMs": row.updated_ms,
        "via": row.via.as_str(),
    });
    if let Some(why) = row.why {
        out["why"] = json!(why.as_str());
    }
    out
}

pub(crate) fn keeps_note(row: &Row) -> bool {
    row.state == State::Box || shows_error(row)
}

fn shows_error(row: &Row) -> bool {
    row.state == State::Waiting && row.why == Some(Why::Retry)
}

pub(crate) fn positions(rows: &[Row]) -> Vec<usize> {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    rows.iter()
        .map(
            |row| match seen.iter_mut().find(|(s, _)| *s == row.session) {
                Some((_, n)) => {
                    *n += 1;
                    *n
                }
                None => {
                    seen.push((&row.session, 1));
                    1
                }
            },
        )
        .collect()
}

pub(crate) fn position_of(rows: &[Row], id: &str) -> usize {
    rows.iter().position(|r| r.id == id).map_or(1, |i| i + 1)
}

pub(crate) fn reply_via(via: notchling_engine::outbox::Via) -> ReplyVia {
    use notchling_engine::outbox::Via;
    match via {
        Via::Chat => ReplyVia::Chat,
        Via::Notch => ReplyVia::Notch,
        Via::Phone => ReplyVia::Phone,
        Via::Agent => ReplyVia::Agent,
    }
}

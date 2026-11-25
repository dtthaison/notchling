use crate::outbox_courier::Courier;
use notchling_engine::outbox::{Gate, Kind, Row, Why, LOGIN_RECHECK_MS};

pub(crate) fn hold(verdict: Gate, row: &Row, courier: &dyn Courier, now_ms: i64) -> Gate {
    let plain = row.kind == Kind::Prompt && row.why != Some(Why::Answer);
    match verdict {
        Gate::Go { .. } if plain && courier.signed_out(row) => Gate::Wait {
            why: Why::LoginNeeded,
            until_ms: now_ms + LOGIN_RECHECK_MS,
        },
        other => other,
    }
}

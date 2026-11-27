use super::*;
use notchling_engine::consent::{Consent, State as Step};
use notchling_events::scope::{self, ConsentNote};

struct Held {
    row: Consent,
    before: Step,
}

pub(super) struct Answering<'a> {
    consent: &'a Arc<ConsentOps>,
    asked: Consent,
    held: Vec<Held>,
}

fn snapshot_of(row: &Consent) -> (Option<bool>, Value) {
    (
        row.change.get("existing").and_then(Value::as_bool),
        row.change.get("revision").cloned().unwrap_or(Value::Null),
    )
}

fn same_snapshot(a: &Consent, b: &Consent) -> bool {
    let (existing, revision) = snapshot_of(a);
    existing.is_some() && (existing, revision) == snapshot_of(b)
}

impl Answering<'_> {
    pub(super) fn primary(&self) -> &Consent {
        &self.held[0].row
    }

    pub(super) fn form(&self) -> &Consent {
        &self.asked
    }

    pub(super) fn live(&self) -> bool {
        self.consent.now() < self.primary().expires_ms
    }

    pub(super) fn renames(&self, entry: &VaultEntry) -> bool {
        entry.id() != self.asked.target.to_lowercase()
    }

    pub(super) fn join_twins(&mut self, caller: &Caller) {
        let twins: Vec<Consent> = self
            .consent
            .live_twins(self.primary())
            .into_iter()
            .filter(|twin| same_snapshot(twin, &self.asked))
            .collect();
        for twin in twins {
            if let Ok(row) = self
                .consent
                .take_for_app(caller, &twin.id, SECRET_REQUEST, None)
            {
                self.held.push(Held {
                    row,
                    before: twin.state,
                });
            }
        }
        scope::note_consent(&self.held[0].row.id, ConsentNote::Used);
    }

    pub(super) fn withdraw(&self) {
        for held in &self.held {
            self.consent.withdraw_use(&held.row);
        }
    }

    pub(super) fn release(&self) {
        for held in &self.held {
            let _ = self.consent.store().reopen(&held.row.id, held.before);
        }
        scope::take_consent();
    }

    pub(super) fn finish(&self, outcome: Result<Value, String>) {
        for held in &self.held {
            self.consent.finish_use(&held.row, outcome.clone());
        }
    }
}

pub(super) fn name_taken(name: &str) -> ApiError {
    failed(format!("There's already a secret named {name}.")).with_reason("name_taken")
}

impl VaultOps {
    pub(super) fn begin_answer<'a>(
        &self,
        consent: &'a Arc<ConsentOps>,
        caller: &Caller,
        id: &str,
    ) -> Result<Answering<'a>, ApiError> {
        ConsentOps::app_only(caller)?;
        let asked = consent.get_row(id)?;
        let live = asked.state.open() && asked.expires_ms > consent.now();
        let mut chosen = asked.clone();
        if asked.kind == SECRET_REQUEST && !live {
            let twins = consent.live_twins(&asked);
            if let Some(newer) = twins.into_iter().find(|t| same_snapshot(t, &asked)) {
                chosen = newer;
            }
        }
        let row = consent.take_for_app(caller, &chosen.id, SECRET_REQUEST, None)?;
        Ok(Answering {
            consent,
            asked,
            held: vec![Held {
                row,
                before: chosen.state,
            }],
        })
    }
}

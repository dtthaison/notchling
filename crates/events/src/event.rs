use crate::spec::{KindSpec, Outcome};
use crate::value::Fields;

pub const MAX_EVENT_BYTES: usize = 4096;
pub const ENVELOPE_BYTES: usize = 512;

pub trait Kind {
    const SPEC: &'static KindSpec;
    fn fields(&self) -> Fields;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub spec: &'static KindSpec,
    pub outcome: Outcome,
    pub ts: i64,
    pub fields: Fields,
}

impl Event {
    pub fn new<K: Kind>(kind: &K, outcome: Outcome, ts: i64) -> Event {
        let mut fields = kind.fields();
        fields.fit(MAX_EVENT_BYTES - ENVELOPE_BYTES);
        Event {
            spec: K::SPEC,
            outcome,
            ts,
            fields,
        }
    }

    pub fn json_len(&self) -> usize {
        ENVELOPE_BYTES + self.fields.json_len()
    }
}

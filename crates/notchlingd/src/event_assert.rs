use notchling_events::field::FieldValue;
use notchling_events::{Event, MemorySink};

pub(crate) fn text(event: &Event, field: &str) -> Option<String> {
    match event.fields.get(field)? {
        FieldValue::Name(name) => Some((*name).to_owned()),
        FieldValue::Token(token) => Some(token.to_string()),
        FieldValue::Ref { id, .. } => Some(id.to_string()),
        FieldValue::Bool(flag) => Some(flag.to_string()),
        FieldValue::Int(n) => Some(n.to_string()),
        FieldValue::UInt(n) => Some(n.to_string()),
        FieldValue::Null | FieldValue::List(_) => None,
    }
}

pub(crate) fn of(sink: &MemorySink, kind: &str) -> Vec<Event> {
    sink.events()
        .into_iter()
        .filter(|e| e.spec.name == kind)
        .collect()
}

pub(crate) fn results(sink: &MemorySink, kind: &str) -> Vec<String> {
    of(sink, kind)
        .iter()
        .filter_map(|e| text(e, "result"))
        .collect()
}

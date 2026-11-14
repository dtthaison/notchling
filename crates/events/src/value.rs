use crate::field::{FieldValue, LIST_MAX};
use crate::field_ref::{hash_ref, RefMode};

const LIST_REF_CUT: usize = 64;
const TOP_REF_CUT: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fields(Vec<(&'static str, FieldValue)>);

impl Fields {
    pub fn new(entries: Vec<(&'static str, FieldValue)>) -> Fields {
        Fields(entries)
    }

    pub fn get(&self, name: &str) -> Option<&FieldValue> {
        self.0.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(|(n, _)| *n).collect()
    }

    pub fn write_json(&self, out: &mut String, mode: &RefMode) {
        let mut order: Vec<&(&'static str, FieldValue)> = self
            .0
            .iter()
            .filter(|(_, v)| *v != FieldValue::Null)
            .collect();
        order.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        out.push('{');
        for (i, (name, value)) in order.into_iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write_str(out, name);
            out.push(':');
            write_value(out, value, mode);
        }
        out.push('}');
    }

    pub fn json_len(&self) -> usize {
        let mut out = String::new();
        self.write_json(&mut out, &RefMode::Plain(crate::field_ref::Salt([0; 32])));
        out.len()
    }

    pub(crate) fn fit(&mut self, budget: usize) {
        if self.json_len() <= budget {
            return;
        }
        for (_, value) in self.0.iter_mut() {
            if let FieldValue::List(items) = value {
                items.truncate(LIST_MAX);
                for item in items.iter_mut() {
                    cut_ref(item, LIST_REF_CUT);
                }
            }
        }
        while self.json_len() > budget {
            let longest = self
                .0
                .iter_mut()
                .filter_map(|(_, v)| match v {
                    FieldValue::List(items) if !items.is_empty() => Some(items),
                    _ => None,
                })
                .max_by_key(|items| items.len());
            match longest {
                Some(items) => {
                    items.pop();
                }
                None => break,
            }
        }
        if self.json_len() > budget {
            for (_, value) in self.0.iter_mut() {
                cut_ref(value, TOP_REF_CUT);
            }
        }
    }
}

fn cut_ref(value: &mut FieldValue, max: usize) {
    if let FieldValue::Ref { id, .. } = value {
        if id.chars().count() > max {
            *id = id.chars().take(max).collect::<String>().into();
        }
    }
}

pub(crate) fn write_str(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub(crate) fn write_value(out: &mut String, value: &FieldValue, mode: &RefMode) {
    match value {
        FieldValue::Null => out.push_str("null"),
        FieldValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        FieldValue::Int(n) => out.push_str(&n.to_string()),
        FieldValue::UInt(n) => out.push_str(&n.to_string()),
        FieldValue::Name(name) => write_str(out, name),
        FieldValue::Token(token) => write_str(out, token),
        FieldValue::Ref {
            kind,
            id,
            always_hashed,
        } => {
            if mode.hashes(*always_hashed) {
                write_str(out, &hash_ref(mode.salt(), kind, id));
            } else {
                write_str(out, id);
            }
        }
        FieldValue::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item, mode);
            }
            out.push(']');
        }
    }
}

use super::MAX_TEXT_CHARACTERS;
use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::{Map, Number, Value};
use std::fmt;

pub const MAX_PARSE_BYTES: usize = 1_000_000;
const TYPE_BYTES: usize = 32;
const NAME_CHARACTERS: usize = 256;
const REPLY_CHARACTERS: usize = MAX_TEXT_CHARACTERS + 1;
const INPUT_CHARACTERS: usize = 20_000;
const TREE_DEPTH: u8 = 4;
const TREE_ITEMS: usize = 1_000;
const MAX_BLOCKS: usize = 2_000;

#[derive(Clone, Copy)]
enum Shape {
    Tree(usize),
    Fields(&'static [(&'static str, Shape)]),
    Content,
}

const BLOCK: &[(&str, Shape)] = &[
    ("type", Shape::Tree(NAME_CHARACTERS)),
    ("text", Shape::Tree(REPLY_CHARACTERS)),
    ("name", Shape::Tree(NAME_CHARACTERS)),
    ("id", Shape::Tree(NAME_CHARACTERS)),
    ("tool_use_id", Shape::Tree(NAME_CHARACTERS)),
    ("input", Shape::Tree(INPUT_CHARACTERS)),
    ("content", Shape::Tree(INPUT_CHARACTERS)),
    ("source", Shape::Tree(INPUT_CHARACTERS)),
];

const MESSAGE: &[(&str, Shape)] = &[
    ("model", Shape::Tree(NAME_CHARACTERS)),
    ("content", Shape::Content),
];

const LINE: &[(&str, Shape)] = &[
    ("type", Shape::Tree(NAME_CHARACTERS)),
    ("timestamp", Shape::Tree(NAME_CHARACTERS)),
    ("uuid", Shape::Tree(NAME_CHARACTERS)),
    ("isSidechain", Shape::Tree(NAME_CHARACTERS)),
    ("isMeta", Shape::Tree(NAME_CHARACTERS)),
    ("isCompactSummary", Shape::Tree(NAME_CHARACTERS)),
    ("message", Shape::Fields(MESSAGE)),
];

#[derive(Deserialize)]
struct Head<'a> {
    #[serde(borrow, rename = "type")]
    kind: Option<&'a str>,
}

pub(super) fn is_assistant(line: &str) -> bool {
    serde_json::from_str::<Head>(line)
        .ok()
        .and_then(|head| head.kind)
        .is_some_and(|kind| kind.len() <= TYPE_BYTES && kind == "assistant")
}

pub(super) fn parse_object(line: &[u8]) -> Option<Map<String, Value>> {
    let mut reader = serde_json::Deserializer::from_slice(line);
    let value = Pick {
        shape: Shape::Fields(LINE),
        depth: TREE_DEPTH,
    }
    .deserialize(&mut reader)
    .ok()?;
    reader.end().ok()?;
    match value {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

fn cut(text: &str, characters: usize) -> String {
    let end = text
        .char_indices()
        .nth(characters)
        .map_or(text.len(), |(at, _)| at);
    text[..end].to_string()
}

fn is_kept_block(block: &Value) -> bool {
    matches!(
        block.get("type").and_then(Value::as_str),
        Some("text" | "tool_use" | "tool_result" | "image")
    )
}

#[derive(Clone, Copy)]
struct Pick {
    shape: Shape,
    depth: u8,
}

impl Pick {
    fn characters(self) -> usize {
        match self.shape {
            Shape::Tree(characters) => characters,
            Shape::Content => REPLY_CHARACTERS,
            Shape::Fields(_) => NAME_CHARACTERS,
        }
    }

    fn deeper(self) -> Self {
        Self {
            shape: self.shape,
            depth: self.depth.saturating_sub(1),
        }
    }
}

impl<'de> DeserializeSeed<'de> for Pick {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Pick {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(Number::from_f64(value).map_or(Value::Null, Value::Number))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(cut(value, self.characters())))
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        match self.shape {
            Shape::Content => {
                let block = Pick {
                    shape: Shape::Fields(BLOCK),
                    depth: TREE_DEPTH,
                };
                while items.len() < MAX_BLOCKS {
                    match seq.next_element_seed(block)? {
                        Some(found) if is_kept_block(&found) => items.push(found),
                        Some(_) => {}
                        None => return Ok(Value::Array(items)),
                    }
                }
                while seq.next_element::<IgnoredAny>()?.is_some() {}
            }
            Shape::Tree(_) if self.depth > 0 => {
                while items.len() < TREE_ITEMS {
                    match seq.next_element_seed(self.deeper())? {
                        Some(item) => items.push(item),
                        None => return Ok(Value::Array(items)),
                    }
                }
                while seq.next_element::<IgnoredAny>()?.is_some() {}
            }
            _ => {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                return Ok(Value::Null);
            }
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        match self.shape {
            Shape::Fields(fields) => {
                let mut out = Map::new();
                while let Some(slot) = map.next_key_seed(Slot(fields))? {
                    match slot {
                        Some(at) => {
                            let (name, shape) = fields[at];
                            let value = map.next_value_seed(Pick {
                                shape,
                                depth: TREE_DEPTH,
                            })?;
                            out.insert(name.to_string(), value);
                        }
                        None => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(Value::Object(out))
            }
            Shape::Tree(_) if self.depth > 0 => {
                let mut out = Map::new();
                while out.len() < TREE_ITEMS {
                    let Some(key) = map.next_key_seed(Key)? else {
                        return Ok(Value::Object(out));
                    };
                    out.insert(key, map.next_value_seed(self.deeper())?);
                }
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Value::Object(out))
            }
            _ => {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Value::Null)
            }
        }
    }
}

struct Slot(&'static [(&'static str, Shape)]);

impl<'de> DeserializeSeed<'de> for Slot {
    type Value = Option<usize>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for Slot {
    type Value = Option<usize>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a field name")
    }

    fn visit_str<E>(self, name: &str) -> Result<Self::Value, E> {
        Ok(self.0.iter().position(|(known, _)| *known == name))
    }
}

struct Key;

impl<'de> DeserializeSeed<'de> for Key {
    type Value = String;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for Key {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a field name")
    }

    fn visit_str<E>(self, name: &str) -> Result<String, E> {
        Ok(cut(name, NAME_CHARACTERS))
    }
}

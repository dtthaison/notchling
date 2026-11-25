use kitoken::Kitoken;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

pub struct Tokens {
    kit: Kitoken,
    prefix: Vec<u32>,
    suffix: Vec<u32>,
    pad: u32,
}

#[derive(Deserialize)]
struct Head {
    #[serde(default)]
    post_processor: Option<Value>,
    #[serde(default)]
    padding: Option<Padding>,
    #[serde(default)]
    added_tokens: Vec<Added>,
}

#[derive(Deserialize)]
struct Padding {
    pad_id: u32,
}

#[derive(Deserialize)]
struct Added {
    id: u32,
    content: String,
}

const NATIVE_MAGIC: &[u8] = b"notchling-tokens 1\n";

#[derive(Serialize, Deserialize)]
struct NativeHead {
    prefix: Vec<u32>,
    suffix: Vec<u32>,
    pad: u32,
}

const PAD_NAMES: [&str; 3] = ["[PAD]", "<pad>", "<|pad|>"];

fn special_ids(name: &str, specials: Option<&Value>) -> Vec<u32> {
    specials
        .and_then(|s| s.get(name))
        .and_then(|t| t.get("ids"))
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(|i| i.as_u64().map(|i| i as u32))
                .collect()
        })
        .unwrap_or_default()
}

fn pair_id(v: Option<&Value>) -> Option<u32> {
    v.and_then(|p| p.get(1))
        .and_then(Value::as_u64)
        .map(|i| i as u32)
}

fn template(processor: &Value) -> Option<(Vec<u32>, Vec<u32>)> {
    match processor.get("type").and_then(Value::as_str)? {
        "TemplateProcessing" => {
            let specials = processor.get("special_tokens");
            let mut prefix = Vec::new();
            let mut suffix = Vec::new();
            let mut past = false;
            for piece in processor.get("single")?.as_array()? {
                if piece.get("Sequence").is_some() {
                    past = true;
                    continue;
                }
                let Some(name) = piece
                    .get("SpecialToken")
                    .and_then(|t| t.get("id"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                let ids = special_ids(name, specials);
                if past {
                    suffix.extend(ids);
                } else {
                    prefix.extend(ids);
                }
            }
            Some((prefix, suffix))
        }
        "BertProcessing" | "RobertaProcessing" => {
            let cls = pair_id(processor.get("cls"))?;
            let sep = pair_id(processor.get("sep"))?;
            Some((vec![cls], vec![sep]))
        }
        "Sequence" => processor
            .get("processors")?
            .as_array()?
            .iter()
            .find_map(template),
        _ => None,
    }
}

impl Tokens {
    pub fn from_tokenizer_json(path: &Path) -> Result<Tokens, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_tokenizer_slice(&bytes)
    }

    pub fn from_tokenizer_slice(bytes: &[u8]) -> Result<Tokens, String> {
        let kit = Kitoken::from_tokenizers_slice(bytes).map_err(|e| e.to_string())?;
        let head: Head = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        let (prefix, suffix) = head
            .post_processor
            .as_ref()
            .and_then(template)
            .unwrap_or_default();
        let pad = head
            .padding
            .map(|p| p.pad_id)
            .or_else(|| {
                head.added_tokens
                    .iter()
                    .find(|t| PAD_NAMES.contains(&t.content.as_str()))
                    .map(|t| t.id)
            })
            .unwrap_or(0);
        Ok(Tokens {
            kit,
            prefix,
            suffix,
            pad,
        })
    }

    pub fn from_native_file(path: &Path) -> Result<Tokens, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_native_slice(&bytes)
    }

    pub fn from_native_slice(bytes: &[u8]) -> Result<Tokens, String> {
        let rest = bytes
            .strip_prefix(NATIVE_MAGIC)
            .ok_or("not a notchling tokenizer file")?;
        let end = rest
            .iter()
            .position(|&b| b == b'\n')
            .ok_or("the tokenizer file has no header")?;
        let head: NativeHead = serde_json::from_slice(&rest[..end]).map_err(|e| e.to_string())?;
        let kit = Kitoken::from_slice(&rest[end + 1..]).map_err(|e| e.to_string())?;
        Ok(Tokens {
            kit,
            prefix: head.prefix,
            suffix: head.suffix,
            pad: head.pad,
        })
    }

    pub fn to_native_bytes(&self) -> Vec<u8> {
        let head = NativeHead {
            prefix: self.prefix.clone(),
            suffix: self.suffix.clone(),
            pad: self.pad,
        };
        let mut out = NATIVE_MAGIC.to_vec();
        out.extend(serde_json::to_vec(&head).unwrap_or_default());
        out.push(b'\n');
        out.extend(self.kit.to_vec());
        out
    }

    pub fn write_native_file(&self, path: &Path) -> Result<(), String> {
        crate::part::write_atomic(path, &self.to_native_bytes())
    }

    pub fn encode(&self, text: &str, max: usize) -> Vec<u32> {
        let room = max.saturating_sub(self.prefix.len() + self.suffix.len());
        let mut body = self.kit.encode(text, true).unwrap_or_default();
        body.truncate(room);
        let mut out = Vec::with_capacity(self.prefix.len() + body.len() + self.suffix.len());
        out.extend_from_slice(&self.prefix);
        out.extend(body);
        out.extend_from_slice(&self.suffix);
        out.truncate(max);
        out
    }

    pub fn cls_id(&self) -> u32 {
        self.prefix.first().copied().unwrap_or(self.pad)
    }

    pub fn pad_id(&self) -> u32 {
        self.pad
    }
}

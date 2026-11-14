use crate::envelope::{write_body, Envelope};
use crate::event::Event;
use crate::field::Seal;
use crate::field_ref::RefMode;
use crate::value::write_str;
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const SEAL_KEY: &str = ",\"seal\":";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsealedWhy {
    KeyMissing,
    KeyDenied,
    KeyWaiting,
}

impl UnsealedWhy {
    pub fn name(self) -> &'static str {
        match self {
            UnsealedWhy::KeyMissing => "key_missing",
            UnsealedWhy::KeyDenied => "key_denied",
            UnsealedWhy::KeyWaiting => "key_waiting",
        }
    }

    pub fn from_name(name: &str) -> Option<UnsealedWhy> {
        match name {
            "key_missing" => Some(UnsealedWhy::KeyMissing),
            "key_denied" => Some(UnsealedWhy::KeyDenied),
            "key_waiting" => Some(UnsealedWhy::KeyWaiting),
            _ => None,
        }
    }
}

pub fn seal_of(key: &[u8; 32], bytes: &[u8]) -> Seal {
    let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(key) else {
        return Seal::from_bytes([0u8; 32]);
    };
    mac.update(bytes);
    let out: [u8; 32] = mac.finalize().into_bytes().into();
    Seal::from_bytes(out)
}

fn head(n: u64, env: &Envelope, event: &Event, mode: &RefMode) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("{\"n\":");
    out.push_str(&n.to_string());
    out.push(',');
    write_body(env, event, mode, &mut out);
    out.push_str(",\"prev\":");
    out
}

pub fn sealed_line(
    key: &[u8; 32],
    n: u64,
    prev: Option<&Seal>,
    env: &Envelope,
    event: &Event,
    mode: &RefMode,
) -> (String, Seal) {
    let mut line = head(n, env, event, mode);
    match prev {
        Some(prev) => write_str(&mut line, prev.as_str()),
        None => line.push_str("null"),
    }
    let seal = seal_of(key, line.as_bytes());
    line.push_str(SEAL_KEY);
    write_str(&mut line, seal.as_str());
    line.push('}');
    (line, seal)
}

pub fn unsealed_line(
    n: u64,
    why: UnsealedWhy,
    env: &Envelope,
    event: &Event,
    mode: &RefMode,
) -> String {
    let mut line = head(n, env, event, mode);
    line.push_str("null,\"unsealed\":");
    write_str(&mut line, why.name());
    line.push_str(SEAL_KEY);
    line.push_str("null}");
    line
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineHead {
    pub n: u64,
    pub kind: String,
    pub prev: Option<Seal>,
    pub seal: Option<Seal>,
    pub unsealed: Option<UnsealedWhy>,
}

pub fn read_head(line: &str) -> Option<LineHead> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let object = value.as_object()?;
    let seal_field = |name: &str| -> Option<Option<Seal>> {
        match object.get(name)? {
            serde_json::Value::Null => Some(None),
            serde_json::Value::String(text) => Seal::parse(text).map(Some),
            _ => None,
        }
    };
    Some(LineHead {
        n: object.get("n")?.as_u64()?,
        kind: object.get("kind")?.as_str()?.to_string(),
        prev: seal_field("prev")?,
        seal: seal_field("seal")?,
        unsealed: object
            .get("unsealed")
            .and_then(|v| v.as_str())
            .and_then(UnsealedWhy::from_name),
    })
}

pub fn signed_part(line: &str) -> Option<&str> {
    line.rfind(SEAL_KEY).map(|at| &line[..at])
}

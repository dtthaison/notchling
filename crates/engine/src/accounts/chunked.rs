use super::generic_password::Keychain;
use super::keychain::{add_line, KeychainError};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use sha2::{Digest, Sha256};

pub const HEADER: &str = "notchling-chunks:v1:";
pub const OWN_SERVICES: [&str; 3] = [
    "Notchling Accounts",
    "Notchling Codex",
    "Notchling Antigravity",
];
pub const PART_BYTES: usize = 1050;
pub const MAX_PARTS: usize = 64;

pub fn is_own(service: &str) -> bool {
    OWN_SERVICES.contains(&service)
}

pub fn part_name(account: &str, i: usize) -> String {
    format!("{account}#{i}")
}

pub trait Items {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeychainError>;
    fn put(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError>;
    fn remove(&self, service: &str, account: &str) -> Result<bool, KeychainError>;
}

pub struct Plain<'a>(pub &'a dyn Keychain);

impl Items for Plain<'_> {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeychainError> {
        Ok(self.0.read(service, account))
    }

    fn put(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError> {
        self.0.write(service, account, value)
    }

    fn remove(&self, service: &str, account: &str) -> Result<bool, KeychainError> {
        self.0.delete(service, account);
        Ok(true)
    }
}

fn sha_hex(bytes: &[u8]) -> String {
    super::keychain::hex(&Sha256::digest(bytes))
}

fn parse_header(rest: &str) -> Option<(usize, &str)> {
    let (n, sha) = rest.split_once(':')?;
    let n: usize = n.parse().ok()?;
    let ok = (1..=MAX_PARTS).contains(&n)
        && sha.len() == 64
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    ok.then_some((n, sha))
}

fn parts_now(items: &dyn Items, service: &str, account: &str) -> Result<usize, KeychainError> {
    Ok(items
        .get(service, account)?
        .as_deref()
        .and_then(|t| t.strip_prefix(HEADER))
        .and_then(parse_header)
        .map_or(0, |(n, _)| n))
}

pub fn read(
    items: &dyn Items,
    service: &str,
    account: &str,
) -> Result<Option<String>, KeychainError> {
    let Some(text) = items.get(service, account)? else {
        return Ok(None);
    };
    if !is_own(service) {
        return Ok(Some(text));
    }
    let Some(rest) = text.strip_prefix(HEADER) else {
        return Ok(Some(text));
    };
    let (n, sha) = parse_header(rest).ok_or(KeychainError::Damaged)?;
    let mut bytes = Vec::new();
    for i in 0..n {
        let part = items
            .get(service, &part_name(account, i))?
            .ok_or(KeychainError::Damaged)?;
        let data = STANDARD
            .decode(part.trim())
            .map_err(|_| KeychainError::Damaged)?;
        bytes.extend_from_slice(&data);
    }
    if sha_hex(&bytes) != sha {
        return Err(KeychainError::Damaged);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| KeychainError::Damaged)
}

pub fn write(
    items: &dyn Items,
    service: &str,
    account: &str,
    value: &str,
) -> Result<(), KeychainError> {
    if !is_own(service) {
        return items.put(service, account, value);
    }
    let before = parts_now(items, service, account)?;
    let one_item = !value.starts_with(HEADER) && add_line(service, account, value).is_ok();
    let parts = if one_item {
        items.put(service, account, value)?;
        0
    } else {
        let chunks: Vec<&[u8]> = value.as_bytes().chunks(PART_BYTES).collect();
        if chunks.len() > MAX_PARTS {
            return Err(KeychainError::TooLong);
        }
        for (i, chunk) in chunks.iter().enumerate() {
            items.put(service, &part_name(account, i), &STANDARD.encode(chunk))?;
        }
        let header = format!("{HEADER}{}:{}", chunks.len(), sha_hex(value.as_bytes()));
        items.put(service, account, &header)?;
        chunks.len()
    };
    if read(items, service, account)?.as_deref() != Some(value) {
        return Err(KeychainError::NotStored);
    }
    remove_parts(items, service, account, parts, before);
    Ok(())
}

pub fn delete(items: &dyn Items, service: &str, account: &str) -> Result<bool, KeychainError> {
    if !is_own(service) {
        return items.remove(service, account);
    }
    let before = parts_now(items, service, account)?;
    let existed = items.remove(service, account)?;
    remove_parts(items, service, account, 0, before);
    Ok(existed)
}

fn remove_parts(items: &dyn Items, service: &str, account: &str, from: usize, named: usize) {
    for i in from..MAX_PARTS {
        let name = part_name(account, i);
        if i >= named && !matches!(items.get(service, &name), Ok(Some(_))) {
            break;
        }
        let _ = items.remove(service, &name);
    }
}

#[cfg(test)]
#[path = "chunked_tests.rs"]
mod tests;

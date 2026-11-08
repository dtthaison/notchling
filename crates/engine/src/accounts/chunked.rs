//! Long values in Notchling's own Keychain items, split over several items.
//!
//! `security -i` reads a command into a 4 KB line and the value goes in as hex, so a value over
//! about 1.9 KB can't be written in one item without putting it on argv (which Notchling never
//! does). A saved Codex `auth.json` (3.5–5 KB) or a saved Claude Code login with a long profile
//! doesn't fit. So a value too long for one line is kept like this:
//!
//! - account `<id>` holds a header, `notchling-chunks:v1:<n>:<sha256>`: the number of parts and the
//!   SHA-256 of the whole value, lowercase hex;
//! - accounts `<id>#0` … `<id>#<n-1>` (same service) hold the parts: consecutive slices of the
//!   value's bytes, at most [`PART_BYTES`] each, each stored as base64 (printable, so `security -w`
//!   prints it as is and never as hex), each line well under the cap.
//!
//! A write puts the parts first and the header last, reads the whole value back, and only then
//! deletes parts a longer earlier value left behind. A read checks the SHA-256, so a half-written
//! or damaged value is an error ([`KeychainError::Damaged`]), never a wrong login. A value that
//! fits one line is still written as one item, the format the Swift app wrote and reads; a read
//! accepts both.
//!
//! Only Notchling's own saved-copy services ([`OWN_SERVICES`]) are ever split. Items another tool
//! reads (Claude Code's `Claude Code-credentials*`, Antigravity's `gemini`/`antigravity`) keep
//! their one-item shape; a value too long for them is refused ([`KeychainError::TooLong`]).

use super::generic_password::Keychain;
use super::keychain::{add_line, KeychainError};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use sha2::{Digest, Sha256};

/// What a split value's header starts with.
pub const HEADER: &str = "notchling-chunks:v1:";
/// The services whose items only Notchling reads: Claude Code saved logins, Codex and Antigravity
/// saved copies.
pub const OWN_SERVICES: [&str; 3] = [
    "Notchling Accounts",
    "Notchling Codex",
    "Notchling Antigravity",
];
/// Bytes of the value per part: 1400 base64 characters, 2800 hex on the line.
pub const PART_BYTES: usize = 1050;
/// The most parts a value may have (about 67 KB).
pub const MAX_PARTS: usize = 64;

/// True for a service only Notchling reads, whose long values may be split.
pub fn is_own(service: &str) -> bool {
    OWN_SERVICES.contains(&service)
}

/// The item that holds part `i` of `account`'s value.
pub fn part_name(account: &str, i: usize) -> String {
    format!("{account}#{i}")
}

/// One item at a time: what [`read`], [`write`] and [`delete`] are built on.
pub trait Items {
    /// The item's text as `security -w` printed it, trimmed; `None` when there is none.
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeychainError>;
    /// Adds or updates one item; a value too long for the line is refused.
    fn put(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError>;
    /// Removes one item; true when there was one.
    fn remove(&self, service: &str, account: &str) -> Result<bool, KeychainError>;
}

/// [`Items`] over the plain [`Keychain`] the E8b stores use.
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

/// The part count and checksum in a header's text after [`HEADER`].
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

/// The number of parts the item's current header names (0 for none or a one-item value).
fn parts_now(items: &dyn Items, service: &str, account: &str) -> Result<usize, KeychainError> {
    Ok(items
        .get(service, account)?
        .as_deref()
        .and_then(|t| t.strip_prefix(HEADER))
        .and_then(parse_header)
        .map_or(0, |(n, _)| n))
}

/// The value: one item's text, or a split value put back together and checked.
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

/// Stores `value`: one item when it fits the line (or the service isn't Notchling's own), else
/// parts and a header. Read back whole before it counts as stored.
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

/// Removes the value and any parts; true when there was an item.
pub fn delete(items: &dyn Items, service: &str, account: &str) -> Result<bool, KeychainError> {
    if !is_own(service) {
        return items.remove(service, account);
    }
    let before = parts_now(items, service, account)?;
    let existed = items.remove(service, account)?;
    remove_parts(items, service, account, 0, before);
    Ok(existed)
}

/// Removes parts from `from` on: every one an earlier header named, then any found after them (a
/// write that stopped before its header). Best effort: a part no header names is never read.
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

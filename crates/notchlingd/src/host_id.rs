use notchling_engine::bridge::net::identity::BridgeIdentity;
use notchling_events::ids::new_op_id;
use notchling_events::Hex;
use std::fs;
use std::io;
use std::path::Path;

const CERTIFICATE_ID_LEN: usize = 16;

fn stored(file: &Path) -> Option<Hex> {
    let text = fs::read_to_string(file).ok()?;
    Hex::new(text.trim())
}

fn certificate_id(bridge_dir: &Path) -> Option<Hex> {
    if !BridgeIdentity::exists(bridge_dir) {
        return None;
    }
    let identity = BridgeIdentity::load(bridge_dir).ok()?;
    let prefix: String = identity
        .fingerprint
        .chars()
        .take(CERTIFICATE_ID_LEN)
        .collect();
    Hex::new(&prefix)
}

fn save(file: &Path, id: &Hex) -> io::Result<()> {
    if let Some(parent) = file.parent() {
        notchling_platform::files::ensure_private_dir(parent).map_err(io::Error::other)?;
    }
    notchling_platform::files::write_private_no_follow(file, id.as_str().as_bytes())
        .map_err(io::Error::other)
}

pub fn resolve(bridge_dir: &Path, file: &Path) -> io::Result<(Hex, Option<(Hex, Hex)>)> {
    let old = stored(file);
    if let Some(id) = certificate_id(bridge_dir) {
        if old.as_ref() == Some(&id) {
            return Ok((id, None));
        }
        save(file, &id)?;
        let changed = old.map(|old| (old, id.clone()));
        return Ok((id, changed));
    }
    if let Some(id) = old {
        return Ok((id, None));
    }
    let id = new_op_id();
    save(file, &id)?;
    Ok((id, None))
}

#[cfg(test)]
#[path = "host_id_tests.rs"]
mod tests;

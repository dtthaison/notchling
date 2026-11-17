use crate::cluster::build_id;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DistFile {
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DistManifest {
    pub build: String,
    pub files: BTreeMap<String, DistFile>,
}

pub fn read(dist: &Path) -> Option<DistManifest> {
    let bytes = std::fs::read(dist.join("manifest.json")).ok()?;
    let manifest: DistManifest = serde_json::from_slice(&bytes).ok()?;
    build_id::accepted(&manifest.build)?;
    let sound = manifest
        .files
        .iter()
        .all(|(name, file)| known_name(name) && is_sha256(&file.sha256));
    sound.then_some(manifest)
}

fn known_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("notchling-") else {
        return false;
    };
    matches!(
        rest.split_once('-'),
        Some(("linux" | "macos", "x86_64" | "aarch64"))
    )
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

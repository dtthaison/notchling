use super::AuditDir;
use notchling_events::Hex;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const NAMES: [&str; 2] = ["audit.jsonl", "audit.jsonl.1"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyFile {
    pub name: String,
    pub sha256: Hex,
    pub lines: u64,
}

pub fn has_legacy(audit_dir: &AuditDir) -> bool {
    fs::read_dir(audit_dir.legacy_dir())
        .map(|mut entries| entries.any(|e| e.is_ok()))
        .unwrap_or(false)
}

fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    (1u32..)
        .map(|n| dir.join(format!("{name}.{n}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

fn read_only(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o400))?;
    }
    #[cfg(not(unix))]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

pub fn move_legacy(data_dir: &Path, audit_dir: &AuditDir) -> io::Result<Vec<LegacyFile>> {
    let mut moved = Vec::new();
    for name in NAMES {
        let from = data_dir.join(name);
        if !matches!(fs::symlink_metadata(&from), Ok(meta) if meta.is_file()) {
            continue;
        }
        audit_dir.ensure()?;
        let legacy = audit_dir.legacy_dir();
        fs::create_dir_all(&legacy)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&legacy, fs::Permissions::from_mode(0o700))?;
        }
        let bytes = fs::read(&from)?;
        let to = free_name(&legacy, name);
        fs::rename(&from, &to)?;
        read_only(&to)?;
        let lines = bytes
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .count() as u64;
        moved.push(LegacyFile {
            name: to
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| name.to_owned()),
            sha256: Hex::from_bytes(&Sha256::digest(&bytes)),
            lines,
        });
    }
    Ok(moved)
}

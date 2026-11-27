use notchling_events::audit_check::{check, CheckInput, CheckOutcome};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn state(root: &Path) -> PathBuf {
    root.join("notchling")
}

pub fn audit_dir(root: &Path) -> PathBuf {
    state(root).join("audit")
}

pub fn key_path(root: &Path) -> PathBuf {
    state(root).join("daemon/audit-key")
}

pub fn month_files(root: &Path) -> Vec<(u32, PathBuf)> {
    let mut files: Vec<(u32, PathBuf)> = std::fs::read_dir(audit_dir(root))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let digits: String = name
                .strip_suffix(".jsonl")?
                .chars()
                .filter(char::is_ascii_digit)
                .collect();
            let month = digits.parse().ok()?;
            Some((month, entry.path()))
        })
        .collect();
    files.sort();
    files
}

pub fn texts(root: &Path) -> Vec<(u32, String)> {
    month_files(root)
        .into_iter()
        .map(|(month, path)| (month, std::fs::read_to_string(path).unwrap_or_default()))
        .collect()
}

pub fn raw_lines(root: &Path) -> Vec<String> {
    texts(root)
        .into_iter()
        .flat_map(|(_, text)| text.lines().map(str::to_owned).collect::<Vec<_>>())
        .collect()
}

pub fn lines(root: &Path) -> Vec<Value> {
    raw_lines(root)
        .iter()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

pub fn kinds(root: &Path) -> Vec<String> {
    lines(root)
        .iter()
        .filter_map(|line| line["kind"].as_str().map(str::to_owned))
        .collect()
}

pub fn of_kind(root: &Path, kind: &str) -> Vec<Value> {
    lines(root)
        .into_iter()
        .filter(|line| line["kind"] == kind)
        .collect()
}

pub fn key(root: &Path) -> Option<[u8; 32]> {
    let bytes = std::fs::read(key_path(root)).ok()?;
    <[u8; 32]>::try_from(bytes.as_slice()).ok()
}

pub fn verdict(root: &Path) -> CheckOutcome {
    let key = key(root);
    let owned = texts(root);
    let months: Vec<(u32, Option<&str>)> = owned
        .iter()
        .map(|(month, text)| (*month, Some(text.as_str())))
        .collect();
    check(&CheckInput {
        key: key.as_ref(),
        months: &months,
        anchor: None,
        checkpoint: None,
        from_n: None,
    })
}

pub fn wait_until(root: &Path, ready: impl Fn(&[Value]) -> bool) -> Vec<Value> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let all = lines(root);
        if ready(&all) || std::time::Instant::now() > deadline {
            return all;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

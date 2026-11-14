use crate::HostError;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(unix)]
pub fn is_process_alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
pub fn is_process_alive(_pid: i32) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn parent_pid(pid: i32) -> Option<i32> {
    // SAFETY: an all-zero proc_bsdinfo is a valid out buffer of the size passed.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: info is valid for `size` bytes.
    let got = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if got != size {
        return None;
    }
    let parent = i32::try_from(info.pbi_ppid).ok()?;
    (parent > 1).then_some(parent)
}

#[cfg(not(target_os = "macos"))]
pub fn parent_pid(pid: i32) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = &stat[stat.rfind(')')? + 1..];
    let parent: i32 = after.split_whitespace().nth(1)?.parse().ok()?;
    (parent > 1).then_some(parent)
}

pub fn descends(pid: i32, root: i32, parent_of: &dyn Fn(i32) -> Option<i32>) -> bool {
    let mut current = pid;
    for _ in 0..8 {
        if current == root {
            return true;
        }
        match parent_of(current) {
            Some(parent) => current = parent,
            None => return false,
        }
    }
    false
}

pub fn session_id_for_root_pid(
    root_pid: i32,
    sessions_dir: &Path,
    parent_of: &dyn Fn(i32) -> Option<i32>,
) -> Option<String> {
    let entries = std::fs::read_dir(sessions_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let (Some(pid), Some(session)) = (value["pid"].as_i64(), value["sessionId"].as_str())
        else {
            continue;
        };
        let Ok(pid) = i32::try_from(pid) else {
            continue;
        };
        if descends(pid, root_pid, parent_of) {
            return Some(session.to_string());
        }
    }
    None
}

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

pub fn scrubbed(environment: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut result: BTreeMap<String, String> = environment
        .into_iter()
        .filter(|(key, _)| !key.starts_with("CLAUDE_CODE_") && key != "CLAUDECODE")
        .collect();
    result.insert("TERM".into(), "xterm-256color".into());
    result.insert("COLORTERM".into(), "truecolor".into());
    result.into_iter().collect()
}

pub fn login_environment(timeout: Duration) -> Vec<(String, String)> {
    let own = || std::env::vars().collect::<Vec<_>>();
    let shell = if cfg!(target_os = "macos") {
        "/bin/zsh"
    } else {
        "/bin/sh"
    };
    let child = std::process::Command::new(shell)
        .args(["-l", "-c", "/usr/bin/env -0"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return own();
    };
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return own();
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut data);
        let _ = tx.send(data);
    });
    let data = rx.recv_timeout(timeout);
    if data.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let parsed = data.map(|d| parse_env(&d)).unwrap_or_default();
    if parsed.is_empty() {
        own()
    } else {
        parsed
    }
}

pub fn parse_env(data: &[u8]) -> Vec<(String, String)> {
    data.split(|b| *b == 0)
        .filter_map(|entry| {
            let text = std::str::from_utf8(entry).ok()?;
            let (key, value) = text.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

pub fn claude_candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ]
}

pub fn resolve_claude(home: &Path) -> Result<PathBuf, HostError> {
    claude_candidates(home)
        .into_iter()
        .find(|p| is_executable(p))
        .ok_or_else(|| {
            HostError::Refused(
                "The claude command was not found (~/.local/bin/claude).".to_string(),
            )
        })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubbing_drops_claude_markers_and_sets_the_terminal() {
        let env = scrubbed(vec![
            ("CLAUDE_CODE_ENTRYPOINT".into(), "cli".into()),
            ("CLAUDECODE".into(), "1".into()),
            ("CLAUDE_CONFIG_DIR".into(), "/x".into()),
            ("TERM".into(), "dumb".into()),
        ]);
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["CLAUDE_CONFIG_DIR", "COLORTERM", "TERM"]);
        assert!(env.contains(&("TERM".into(), "xterm-256color".into())));
    }

    #[test]
    fn env_dash_zero_parses() {
        assert_eq!(
            parse_env(b"A=1\0B=x=y\0broken\0"),
            [("A".into(), "1".into()), ("B".into(), "x=y".into())]
        );
    }

    #[test]
    fn a_session_is_found_through_its_ancestors() {
        let dir = std::env::temp_dir().join(format!("ph-map-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("300.json"), r#"{"pid":300,"sessionId":"s-300"}"#).expect("w");
        std::fs::write(dir.join("bad.json"), "nope").expect("w");
        let parents = |pid: i32| match pid {
            300 => Some(200),
            200 => Some(100),
            _ => None,
        };
        assert_eq!(
            session_id_for_root_pid(100, &dir, &parents).as_deref(),
            Some("s-300")
        );
        assert_eq!(session_id_for_root_pid(999, &dir, &parents), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn this_process_is_alive_and_has_a_parent() {
        let me = std::process::id() as i32;
        assert!(is_process_alive(me));
        assert!(parent_pid(me).is_some());
    }
}

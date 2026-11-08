//! What the daemon can check about a process that asks to use secrets marked ask-first, for the
//! approval prompt. `vault.use`'s `folder` and `command` are what the caller says; beside them
//! the `vault.approval.pending` event carries `verified {exe, pid, cwd, parents, folder_matches}`:
//!
//! - `exe` and `pid` from the socket's peer credentials ([`Caller`]), never from the request;
//! - `cwd`, the process's real working folder (`proc_pidinfo(PROC_PIDVNODEPATHINFO)` on macOS,
//!   `/proc/<pid>/cwd` on Linux; `null` elsewhere or when the kernel won't say);
//! - `parents`, the names of up to [`PARENTS_DEPTH`] processes above it, nearest first
//!   (`notchling` ← `zsh` ← `claude` gives `["zsh", "claude"]`);
//! - `folder_matches`, whether the claimed folder is that working folder: compared as written
//!   once `.`, `..`, repeated and trailing slashes are folded (no link is followed), or else as
//!   the file system resolves the claimed one. `null` when the working folder is unknown.
//!
//! The OS lookups sit behind [`Processes`], so tests never read a real process.

use notchling_local_api::Caller;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

/// How many processes above the caller are named.
pub const PARENTS_DEPTH: usize = 4;

/// What the OS says about a process.
pub trait Processes: Send + Sync {
    fn parent(&self, pid: u32) -> Option<u32>;
    fn name(&self, pid: u32) -> Option<String>;
    fn cwd(&self, pid: u32) -> Option<PathBuf>;
    /// A folder with every link resolved (the claimed folder, to compare with the kernel's).
    fn resolve(&self, path: &Path) -> Option<PathBuf>;
}

/// The kernel's answers.
pub struct SystemProcesses;

impl Processes for SystemProcesses {
    fn parent(&self, pid: u32) -> Option<u32> {
        crate::hook::parent_pid(pid)
    }
    fn name(&self, pid: u32) -> Option<String> {
        process_name(pid)
    }
    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        process_cwd(pid)
    }
    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok()
    }
}

/// The names of the processes above `pid`, nearest first: at most [`PARENTS_DEPTH`], ending at
/// launchd/init, an unknown parent or a pid already seen.
pub fn parent_names(processes: &dyn Processes, pid: u32) -> Vec<String> {
    let mut seen = HashSet::from([pid]);
    let mut names = Vec::new();
    let mut next = processes.parent(pid);
    while names.len() < PARENTS_DEPTH {
        let Some(p) = next.filter(|p| *p > 1 && seen.insert(*p)) else {
            break;
        };
        names.push(processes.name(p).unwrap_or_else(|| "?".to_string()));
        next = processes.parent(p);
    }
    names
}

/// An absolute path with `.`, `..`, repeated and trailing slashes folded, following no link;
/// `None` for an empty or relative one.
pub fn standardize(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return None;
    }
    let mut out = PathBuf::from("/");
    for part in path.components() {
        match part {
            Component::Normal(p) => out.push(p),
            Component::ParentDir => {
                out.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    Some(out)
}

/// Whether the claimed folder is the real working folder; `None` when that is unknown.
pub fn folder_matches(
    processes: &dyn Processes,
    claimed: &str,
    cwd: Option<&Path>,
) -> Option<bool> {
    let cwd = standardize(&cwd?.to_string_lossy())?;
    let Some(claimed) = standardize(claimed) else {
        return Some(false);
    };
    if claimed == cwd {
        return Some(true);
    }
    let real = |p: &Path| processes.resolve(p);
    Some(matches!((real(&claimed), real(&cwd)), (Some(a), Some(b)) if a == b))
}

/// The event's `verified` object for this caller and the folder it claims.
pub fn verified(processes: &dyn Processes, caller: &Caller, claimed: &str) -> Value {
    let cwd = caller.pid.and_then(|pid| processes.cwd(pid));
    let parents = caller
        .pid
        .map(|pid| parent_names(processes, pid))
        .unwrap_or_default();
    json!({
        "exe": caller.peer.as_ref().map(|p| p.to_string_lossy()),
        "pid": caller.pid,
        "cwd": cwd.as_ref().map(|p| p.to_string_lossy()),
        "parents": parents,
        "folder_matches": folder_matches(processes, claimed, cwd.as_deref()),
    })
}

#[cfg(target_os = "macos")]
fn process_name(pid: u32) -> Option<String> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is writable and its size is passed.
    let n = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let n = usize::try_from(n)
        .ok()
        .filter(|n| *n > 0 && *n <= buf.len())?;
    Some(String::from_utf8_lossy(&buf[..n]).into_owned())
}

#[cfg(target_os = "linux")]
fn process_name(pid: u32) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(comm.trim_end().to_string()).filter(|n| !n.is_empty())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_name(_pid: u32) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn process_cwd(pid: u32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let pid = libc::c_int::try_from(pid).ok()?;
    // SAFETY: proc_vnodepathinfo is plain data; all zeroes is a valid value.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    // SAFETY: the buffer is a proc_vnodepathinfo of the size passed.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut info as *mut libc::proc_vnodepathinfo).cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    // vip_path is MAXPATHLEN C chars, laid out as 32 rows of 32.
    let raw: Vec<u8> = info
        .pvi_cdir
        .vip_path
        .iter()
        .flatten()
        .map(|c| *c as u8)
        .take_while(|b| *b != 0)
        .collect();
    (!raw.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(&raw)))
}

#[cfg(target_os = "linux")]
fn process_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_cwd(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_local_api::{ClientKind, Tier};
    use std::collections::HashMap;

    /// A process table: pid → (parent, name), working folders, and links.
    #[derive(Default)]
    pub(crate) struct FakeProcesses {
        pub table: HashMap<u32, (u32, &'static str)>,
        pub cwds: HashMap<u32, &'static str>,
        pub links: HashMap<&'static str, &'static str>,
    }

    impl Processes for FakeProcesses {
        fn parent(&self, pid: u32) -> Option<u32> {
            self.table.get(&pid).map(|e| e.0)
        }
        fn name(&self, pid: u32) -> Option<String> {
            self.table.get(&pid).map(|e| e.1.to_string())
        }
        fn cwd(&self, pid: u32) -> Option<PathBuf> {
            self.cwds.get(&pid).map(PathBuf::from)
        }
        fn resolve(&self, path: &Path) -> Option<PathBuf> {
            let path = path.to_str()?;
            Some(PathBuf::from(self.links.get(path).copied().unwrap_or(path)))
        }
    }

    fn chain() -> FakeProcesses {
        FakeProcesses {
            table: HashMap::from([
                (500, (400, "notchling")),
                (400, (300, "zsh")),
                (300, (200, "claude")),
                (200, (100, "tmux")),
                (100, (50, "login")),
                (50, (1, "sshd")),
            ]),
            ..FakeProcesses::default()
        }
    }

    #[test]
    fn parents_are_named_nearest_first_four_at_most() {
        let p = chain();
        assert_eq!(parent_names(&p, 500), ["zsh", "claude", "tmux", "login"]);
        assert_eq!(parent_names(&p, 100), ["sshd"], "stops above launchd");
        assert!(parent_names(&p, 7).is_empty(), "unknown pid");
    }

    #[test]
    fn a_cycle_in_the_table_ends_the_walk() {
        let p = FakeProcesses {
            table: HashMap::from([(10, (11, "a")), (11, (12, "b")), (12, (10, "c"))]),
            ..FakeProcesses::default()
        };
        assert_eq!(parent_names(&p, 10), ["b", "c"]);
        let own = FakeProcesses {
            table: HashMap::from([(10, (10, "self"))]),
            ..FakeProcesses::default()
        };
        assert!(parent_names(&own, 10).is_empty());
    }

    #[test]
    fn folders_compare_once_standardized() {
        let p = FakeProcesses::default();
        let cwd = Some(Path::new("/w/app"));
        for same in ["/w/app", "/w/app/", "/w//app", "/w/./app", "/w/x/../app"] {
            assert_eq!(folder_matches(&p, same, cwd), Some(true), "{same}");
        }
        for other in ["/w/app2", "/w", "", "w/app", "./app"] {
            assert_eq!(folder_matches(&p, other, cwd), Some(false), "{other}");
        }
        assert_eq!(folder_matches(&p, "/w/app", None), None, "cwd unknown");
        assert_eq!(standardize("/a/b/"), Some(PathBuf::from("/a/b")));
        assert_eq!(standardize("/../a"), Some(PathBuf::from("/a")));
        assert_eq!(standardize("rel"), None);
    }

    #[test]
    fn a_link_to_the_same_folder_matches_through_the_file_system() {
        let p = FakeProcesses {
            links: HashMap::from([("/tmp/w", "/private/tmp/w")]),
            ..FakeProcesses::default()
        };
        let cwd = Some(Path::new("/private/tmp/w"));
        assert_eq!(folder_matches(&p, "/tmp/w/", cwd), Some(true));
        assert_eq!(folder_matches(&p, "/tmp/v", cwd), Some(false));
    }

    #[test]
    fn the_verified_object() {
        let mut p = chain();
        p.cwds.insert(500, "/w/app");
        let caller = Caller {
            tier: Tier::Agent,
            client: ClientKind::Cli,
            peer: Some("/opt/notchling/bin/notchling".into()),
            pid: Some(500),
        };
        assert_eq!(
            verified(&p, &caller, "/elsewhere"),
            json!({"exe": "/opt/notchling/bin/notchling", "pid": 500, "cwd": "/w/app",
                   "parents": ["zsh", "claude", "tmux", "login"], "folder_matches": false})
        );
        assert_eq!(verified(&p, &caller, "/w/app/")["folder_matches"], true);
        let unknown = Caller {
            peer: None,
            pid: None,
            ..caller
        };
        assert_eq!(
            verified(&p, &unknown, "/w/app"),
            json!({"exe": null, "pid": null, "cwd": null, "parents": [], "folder_matches": null})
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_kernel_knows_this_test_s_own_folder_and_parent() {
        let me = std::process::id();
        let cwd = SystemProcesses.cwd(me).expect("the kernel says");
        let here = std::env::current_dir().unwrap();
        assert_eq!(
            std::fs::canonicalize(cwd).unwrap(),
            std::fs::canonicalize(here).unwrap()
        );
        let parent = SystemProcesses.parent(me).expect("a parent");
        assert!(SystemProcesses.name(parent).is_some_and(|n| !n.is_empty()));
    }
}

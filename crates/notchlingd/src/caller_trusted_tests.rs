use crate::caller_session::trusted_in;
use crate::vault_caller::Processes;
use notchling_engine::sessions::Roots;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Table {
    exes: HashMap<u32, PathBuf>,
    links: HashMap<PathBuf, PathBuf>,
}

impl Processes for Table {
    fn parent(&self, pid: u32) -> Option<u32> {
        HashMap::from([(900, 800), (800, 700), (700, 2)])
            .get(&pid)
            .copied()
    }
    fn name(&self, _: u32) -> Option<String> {
        Some("claude".into())
    }
    fn cwd(&self, _: u32) -> Option<PathBuf> {
        None
    }
    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        Some(self.links.get(path).cloned().unwrap_or(path.to_path_buf()))
    }
    fn exe(&self, pid: u32) -> Option<PathBuf> {
        self.exes.get(&pid).cloned()
    }
}

fn home(tag: &str, files: &[(u32, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-trusted-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".claude/sessions")).unwrap();
    for (pid, session) in files {
        std::fs::write(
            dir.join(format!(".claude/sessions/{pid}.json")),
            format!("{{\"sessionId\": \"{session}\"}}"),
        )
        .unwrap();
    }
    dir
}

fn find(dir: &Path, exe: Option<&str>, links: &[(PathBuf, &str)]) -> Option<String> {
    let procs = Table {
        exes: exe.map(|e| (800, PathBuf::from(e))).into_iter().collect(),
        links: links
            .iter()
            .map(|(a, b)| (a.clone(), PathBuf::from(b)))
            .collect(),
    };
    trusted_in(Roots::new(dir, &dir.join("data")), Arc::new(procs))(900)
}

#[test]
fn a_session_file_naming_a_process_running_the_installed_claude_is_trusted() {
    let dir = home("real", &[(800, "sess-a")]);
    let versions = dir.join(".local/share/claude/versions/2.1.0/claude");
    assert_eq!(
        find(&dir, Some(versions.to_str().unwrap()), &[]).as_deref(),
        Some("sess-a")
    );
    let bin = dir.join(".local/bin/claude");
    assert_eq!(
        find(&dir, Some(bin.to_str().unwrap()), &[]).as_deref(),
        Some("sess-a")
    );
    assert_eq!(
        find(&dir, Some("/usr/local/bin/claude"), &[]).as_deref(),
        Some("sess-a")
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_process_named_claude_that_runs_another_program_is_not_trusted() {
    let dir = home("renamed", &[(800, "sess-victim")]);
    assert_eq!(find(&dir, Some("/bin/zsh"), &[]), None, "exec -a claude");
    let fake = dir.join("bin/claude");
    assert_eq!(
        find(
            &dir,
            Some(fake.to_str().unwrap()),
            &[(fake.clone(), "/bin/sleep")]
        ),
        None,
        "a symlink named claude to another program"
    );
    assert_eq!(
        find(&dir, Some("/tmp/claude"), &[]),
        None,
        "a copy elsewhere"
    );
    assert_eq!(find(&dir, None, &[]), None, "no executable path known");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_session_file_on_the_lineage_means_no_session() {
    let dir = home("none", &[]);
    assert_eq!(find(&dir, Some("/usr/local/bin/claude"), &[]), None);
    std::fs::remove_dir_all(&dir).ok();
}

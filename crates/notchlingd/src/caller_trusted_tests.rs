use crate::caller_session::trusted_in;
use crate::vault_caller::Processes;
use notchling_engine::sessions::Roots;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

struct Table {
    parents: HashMap<u32, u32>,
    names: HashMap<u32, &'static str>,
}

impl Processes for Table {
    fn parent(&self, pid: u32) -> Option<u32> {
        self.parents.get(&pid).copied()
    }
    fn name(&self, pid: u32) -> Option<String> {
        self.names.get(&pid).map(|n| (*n).to_owned())
    }
    fn cwd(&self, _: u32) -> Option<PathBuf> {
        None
    }
    fn resolve(&self, _: &Path) -> Option<PathBuf> {
        None
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

fn table(names: &[(u32, &'static str)]) -> Arc<Table> {
    Arc::new(Table {
        parents: HashMap::from([(900, 800), (800, 700), (700, 2)]),
        names: names.iter().copied().collect(),
    })
}

#[test]
fn a_session_file_naming_a_claude_ancestor_is_trusted() {
    let dir = home("claude", &[(800, "sess-a")]);
    let find = trusted_in(
        Roots::new(&dir, &dir.join("data")),
        table(&[(800, "claude"), (900, "notchling")]),
    );
    assert_eq!(find(900).as_deref(), Some("sess-a"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_session_file_planted_for_a_shell_ancestor_is_not_trusted() {
    let dir = home("shell", &[(800, "sess-victim")]);
    let find = trusted_in(
        Roots::new(&dir, &dir.join("data")),
        table(&[(800, "zsh"), (700, "claude"), (900, "notchling")]),
    );
    assert_eq!(find(900), None, "the process the file names isn't Claude");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_session_file_on_the_lineage_means_no_session() {
    let dir = home("none", &[]);
    let find = trusted_in(
        Roots::new(&dir, &dir.join("data")),
        table(&[(800, "claude")]),
    );
    assert_eq!(find(900), None);
    std::fs::remove_dir_all(&dir).ok();
}

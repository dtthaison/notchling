use crate::identity::{Identifier, Launches};
use crate::late_emit::LateSalt;
use crate::vault_caller::Processes;
use notchling_engine::organizations::Organizations;
use notchling_engine::sessions::Roots;
use notchling_events::kinds::choices::AgentTool;
use notchling_events::Salt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Default, Clone)]
struct Table {
    parents: HashMap<u32, u32>,
    exes: HashMap<u32, PathBuf>,
    names: HashMap<u32, String>,
    argvs: HashMap<u32, Vec<String>>,
    starts: HashMap<u32, u64>,
    links: HashMap<PathBuf, PathBuf>,
}

impl Processes for Table {
    fn parent(&self, pid: u32) -> Option<u32> {
        self.parents.get(&pid).copied()
    }
    fn name(&self, pid: u32) -> Option<String> {
        self.names.get(&pid).cloned()
    }
    fn cwd(&self, _: u32) -> Option<PathBuf> {
        None
    }
    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        Some(
            self.links
                .get(path)
                .cloned()
                .unwrap_or_else(|| path.to_path_buf()),
        )
    }
    fn exe(&self, pid: u32) -> Option<PathBuf> {
        self.exes.get(&pid).cloned()
    }
    fn argv(&self, pid: u32) -> Option<Vec<String>> {
        self.argvs.get(&pid).cloned()
    }
    fn start_time(&self, pid: u32) -> Option<u64> {
        self.starts.get(&pid).copied()
    }
}

struct Home(PathBuf);

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn home(tag: &str, sessions: &[(u32, &str)]) -> Home {
    let dir = std::env::temp_dir().join(format!("nl-ident-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".claude/sessions")).unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    for (pid, session) in sessions {
        std::fs::write(
            dir.join(format!(".claude/sessions/{pid}.json")),
            format!("{{\"sessionId\": \"{session}\"}}"),
        )
        .unwrap();
    }
    Home(dir)
}

fn salt() -> LateSalt {
    let cell = OnceLock::new();
    let _ = cell.set(Salt([7; 32]));
    Arc::new(cell)
}

fn rows(session: &str, cwd: &str) -> Arc<Mutex<Value>> {
    Arc::new(Mutex::new(json!([{"id": session, "cwd": cwd}])))
}

fn identify(
    home: &Home,
    table: Table,
    sessions: Arc<Mutex<Value>>,
    launches: Launches,
    pid: u32,
) -> notchling_local_api::AgentIdentity {
    Identifier::new(
        Roots::new(&home.0, &home.0.join("data")),
        Arc::new(table),
        sessions,
        salt(),
        launches,
    )
    .with_own_exe(PathBuf::from("/Applications/Notchling.app/notchling"))
    .with_organizations(Organizations::new(&home.0.join("data")).with_home(&home.0))
    .identify(pid)
}

fn lineage(claude_exe: &Path) -> Table {
    let mut t = Table {
        parents: HashMap::from([(900, 800), (800, 700), (700, 600), (600, 1)]),
        ..Table::default()
    };
    t.exes.insert(800, claude_exe.to_path_buf());
    t.names.insert(800, "claude".into());
    t.names.insert(600, "Terminal".into());
    t.exes.insert(
        600,
        PathBuf::from("/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal"),
    );
    t.starts.insert(600, 42);
    t.argvs.insert(
        900,
        vec!["notchling".into(), "settings".into(), "set".into()],
    );
    t
}

#[test]
fn an_install_under_the_install_folders_is_verified() {
    let h = home("ok", &[(800, "sess-a")]);
    let exe = h.0.join(".local/share/claude/versions/2.1.0/claude");
    let who = identify(
        &h,
        lineage(&exe),
        rows("sess-a", "/work/app"),
        Launches::default(),
        900,
    );
    assert_eq!(who.tool, AgentTool::Claude);
    assert!(who.verified);
    assert_eq!(who.session.as_deref(), Some("sess-a"));
    assert_eq!(who.folder.as_deref(), Some("/work/app"));
    assert_eq!(
        who.root.as_deref(),
        Some("/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal:42")
    );
    assert!(who.cmd_hash.is_some());
    assert_eq!(who.cmd_hash.as_deref().map(str::len), Some(32));
    assert!(who.out_hash.is_none());
}

#[test]
fn a_repointed_link_a_copy_a_name_and_npm_are_not_verified() {
    let h = home("no", &[(800, "sess-v")]);
    for exe in [
        "/bin/zsh".to_string(),
        "/tmp/claude".to_string(),
        h.0.join(".local/bin/claude").to_string_lossy().into_owned(),
        "/usr/local/lib/node_modules/.bin/node".to_string(),
    ] {
        let who = identify(
            &h,
            lineage(Path::new(&exe)),
            rows("sess-v", "/work"),
            Launches::default(),
            900,
        );
        assert!(!who.verified, "{exe}");
        assert!(who.organization.is_none(), "{exe}");
    }
}

#[test]
fn a_planted_session_file_for_a_shell_is_not_verified() {
    let h = home("planted", &[(700, "sess-planted")]);
    let mut t = lineage(Path::new("/bin/zsh"));
    t.exes.insert(700, PathBuf::from("/bin/zsh"));
    let who = identify(
        &h,
        t,
        rows("sess-planted", "/work"),
        Launches::default(),
        900,
    );
    assert!(!who.verified);
    assert!(who.organization.is_none());
    assert!(who.session.is_none());
    assert!(who.folder.is_none());
}

#[test]
fn a_planted_file_for_a_shell_never_lends_its_session_or_folder_to_a_delegate_or_a_name() {
    let h = home("planted-more", &[(700, "sess-planted")]);
    let mut t = lineage(Path::new("/bin/zsh"));
    t.exes.insert(700, PathBuf::from("/bin/zsh"));
    t.names.remove(&800);
    t.exes
        .insert(800, PathBuf::from("/Applications/Notchling.app/notchling"));
    t.argvs.insert(
        800,
        vec!["notchling".into(), "delegate".into(), "agy".into()],
    );
    let who = identify(
        &h,
        t,
        rows("sess-planted", "/work"),
        Launches::default(),
        900,
    );
    assert_eq!(who.tool, AgentTool::Antigravity);
    assert!(!who.verified);
    assert!(who.session.is_none());
    assert!(who.folder.is_none());
    assert!(who.organization.is_none());
}

#[test]
fn a_planted_file_on_the_way_up_does_not_hide_the_real_session_above_it() {
    let h = home(
        "planted-above",
        &[(800, "sess-planted"), (700, "sess-real")],
    );
    let exe = h.0.join(".local/share/claude/versions/2.1.0/claude");
    let mut t = lineage(Path::new("/bin/zsh"));
    t.names.remove(&800);
    t.exes.insert(700, exe);
    let who = identify(
        &h,
        t,
        rows("sess-real", "/work/real"),
        Launches::default(),
        900,
    );
    assert!(who.verified);
    assert_eq!(who.tool, AgentTool::Claude);
    assert_eq!(who.session.as_deref(), Some("sess-real"));
    assert_eq!(who.folder.as_deref(), Some("/work/real"));
}

#[test]
fn a_shell_is_a_shell_with_no_session_and_its_root_app() {
    let h = home("shell", &[]);
    let mut t = lineage(Path::new("/bin/zsh"));
    t.names.remove(&800);
    let who = identify(
        &h,
        t,
        Arc::new(Mutex::new(json!([]))),
        Launches::default(),
        900,
    );
    assert_eq!(who.tool, AgentTool::Shell);
    assert!(who.session.is_none() && !who.verified);
    assert_eq!(
        who.root.as_deref(),
        Some("/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal:42")
    );
}

#[test]
fn a_delegate_child_is_that_tool_with_the_delegating_session() {
    let h = home("deleg", &[(800, "sess-d")]);
    let exe = h.0.join(".local/share/claude/versions/2.1.0/claude");
    let mut t = lineage(&exe);
    t.exes
        .insert(700, PathBuf::from("/Applications/Notchling.app/notchling"));
    t.argvs.insert(
        700,
        vec!["notchling".into(), "delegate".into(), "codex".into()],
    );
    let who = identify(&h, t, rows("sess-d", "/work"), Launches::default(), 900);
    assert_eq!(who.tool, AgentTool::Codex);
    assert_eq!(who.session.as_deref(), Some("sess-d"));
    assert!(who.verified);
}

#[test]
fn a_process_under_a_session_the_daemon_started_is_that_tool_and_verified() {
    let h = home("launched", &[]);
    let mut t = lineage(Path::new("/bin/zsh"));
    t.names.remove(&800);
    t.starts.insert(700, 5_000);
    let launches = Launches::default();
    launches.register(700, 5_000, AgentTool::Codex, "codex-sess");
    let who = identify(
        &h,
        t.clone(),
        rows("codex-sess", "/work"),
        launches.clone(),
        900,
    );
    assert_eq!(who.tool, AgentTool::Codex);
    assert_eq!(who.session.as_deref(), Some("codex-sess"));
    assert!(who.verified);
    t.starts.insert(700, 5_001);
    let reused = identify(&h, t, rows("codex-sess", "/work"), launches, 900);
    assert_eq!(reused.tool, AgentTool::Shell);
    assert!(!reused.verified);
}

#[test]
fn the_organization_comes_from_the_session_folder_and_only_for_a_verified_session() {
    let h = home("org", &[(800, "sess-o")]);
    let work = h.0.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let book = Organizations::new(&h.0.join("data")).with_home(&h.0);
    let id = book.create("Acme", None, None).unwrap();
    book.set_folders(&id, &[work.to_string_lossy().into_owned()])
        .unwrap();
    let exe = h.0.join(".local/share/claude/versions/2.1.0/claude");
    let inside = work.join("app").to_string_lossy().into_owned();
    let who = identify(
        &h,
        lineage(&exe),
        rows("sess-o", &inside),
        Launches::default(),
        900,
    );
    assert_eq!(who.organization.as_deref(), Some(id.as_str()));
    let elsewhere = identify(
        &h,
        lineage(&exe),
        rows("sess-o", "/somewhere/else"),
        Launches::default(),
        900,
    );
    assert_eq!(elsewhere.organization.as_deref(), Some("personal"));
    let unverified = identify(
        &h,
        lineage(Path::new("/bin/zsh")),
        rows("sess-o", &inside),
        Launches::default(),
        900,
    );
    assert!(unverified.organization.is_none());
}

#[test]
fn a_delegate_under_a_session_the_daemon_started_is_the_delegated_tool_with_that_session() {
    let h = home("deleg-launched", &[]);
    let mut t = lineage(Path::new("/bin/zsh"));
    t.names.remove(&800);
    t.starts.insert(600, 42);
    t.starts.insert(700, 5_000);
    t.exes
        .insert(800, PathBuf::from("/Applications/Notchling.app/notchling"));
    t.argvs.insert(
        800,
        vec!["notchling".into(), "delegate".into(), "agy".into()],
    );
    let launches = Launches::default();
    launches.register(700, 5_000, AgentTool::Codex, "codex-sess");
    let who = identify(&h, t, rows("codex-sess", "/work"), launches, 900);
    assert_eq!(who.tool, AgentTool::Antigravity);
    assert_eq!(who.session.as_deref(), Some("codex-sess"));
    assert!(who.verified);
}

#[test]
fn a_delegate_run_through_a_link_to_this_binary_is_that_tool() {
    let h = home("deleg-link", &[(700, "sess-l")]);
    let exe = h.0.join(".local/share/claude/versions/2.1.0/claude");
    let mut t = lineage(Path::new("/bin/zsh"));
    t.names.remove(&800);
    t.exes.insert(700, exe);
    t.exes
        .insert(800, PathBuf::from("/usr/local/bin/notchling"));
    t.links.insert(
        PathBuf::from("/usr/local/bin/notchling"),
        PathBuf::from("/Applications/Notchling.app/notchling"),
    );
    t.argvs.insert(
        800,
        vec!["notchling".into(), "delegate".into(), "codex".into()],
    );
    let who = identify(&h, t, rows("sess-l", "/work"), Launches::default(), 900);
    assert_eq!(who.tool, AgentTool::Codex);
    assert_eq!(who.session.as_deref(), Some("sess-l"));
    assert!(who.verified);
}

#[test]
fn the_root_app_is_found_however_deep_the_tree_is_and_is_named_by_its_path() {
    let h = home("deep", &[]);
    let mut t = Table::default();
    let top = 50u32;
    t.parents.insert(top, 1);
    t.exes.insert(
        top,
        PathBuf::from("/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal"),
    );
    t.starts.insert(top, 42);
    for pid in 51..=90u32 {
        t.parents.insert(pid, pid - 1);
    }
    let who = identify(
        &h,
        t.clone(),
        Arc::new(Mutex::new(json!([]))),
        Launches::default(),
        90,
    );
    assert_eq!(
        who.root.as_deref(),
        Some("/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal:42")
    );
    let mut other = t;
    other.exes.insert(
        top,
        PathBuf::from("/tmp/Terminal.app/Contents/MacOS/Terminal"),
    );
    let who = identify(
        &h,
        other,
        Arc::new(Mutex::new(json!([]))),
        Launches::default(),
        90,
    );
    assert_eq!(
        who.root.as_deref(),
        Some("/tmp/Terminal.app/Contents/MacOS/Terminal:42")
    );
}

#[test]
fn a_tree_that_loops_or_never_ends_shares_one_root() {
    let h = home("loop", &[]);
    let mut t = Table::default();
    t.parents.insert(10, 11);
    t.parents.insert(11, 12);
    t.parents.insert(12, 11);
    let who = identify(
        &h,
        t,
        Arc::new(Mutex::new(json!([]))),
        Launches::default(),
        10,
    );
    assert_eq!(who.root.as_deref(), Some("unrooted"));
}

#[test]
fn home_is_the_one_the_engine_names_even_when_the_claude_folder_has_no_parent() {
    let h = home("home", &[]);
    let roots = Roots {
        main_dir: PathBuf::from("/"),
        lanes_file: h.0.join("data/lanes.json"),
    };
    let id = Identifier::new(
        roots,
        Arc::new(Table::default()),
        Arc::new(Mutex::new(json!([]))),
        salt(),
        Launches::default(),
    );
    assert_eq!(id.home(), Path::new("/"));
}

#[test]
fn a_root_with_no_start_time_exe_or_name_shares_the_unrooted_bucket() {
    let h = home("noroot", &[]);
    let rows = || Arc::new(Mutex::new(json!([])));
    let mut no_start = Table::default();
    no_start.parents.insert(10, 11);
    no_start.exes.insert(
        11,
        PathBuf::from("/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal"),
    );
    let who = identify(&h, no_start, rows(), Launches::default(), 10);
    assert_eq!(who.root.as_deref(), Some("unrooted"));
    let mut no_name = Table::default();
    no_name.parents.insert(10, 11);
    no_name.starts.insert(11, 42);
    let who = identify(&h, no_name, rows(), Launches::default(), 10);
    assert_eq!(who.root.as_deref(), Some("unrooted"));
}

#[test]
fn a_tree_exactly_as_deep_as_the_limit_is_rooted_and_one_more_is_not() {
    let h = home("limit", &[]);
    let chain = |hops: u32| {
        let mut t = Table::default();
        for pid in 0..hops {
            t.parents.insert(10 + pid, 11 + pid);
        }
        t.parents.insert(10 + hops, 1);
        t.exes.insert(
            10 + hops,
            PathBuf::from("/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal"),
        );
        t.starts.insert(10 + hops, 42);
        t
    };
    let rows = || Arc::new(Mutex::new(json!([])));
    let at_limit = identify(&h, chain(4096), rows(), Launches::default(), 10);
    assert_eq!(
        at_limit.root.as_deref(),
        Some("/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal:42")
    );
    let past = identify(&h, chain(4097), rows(), Launches::default(), 10);
    assert_eq!(past.root.as_deref(), Some("unrooted"));
}

#[test]
fn a_root_that_is_no_app_bundle_shares_the_unrooted_bucket_however_often_it_is_new() {
    let h = home("rotating", &[]);
    for (start, exe) in [(42, "/bin/sh"), (43, "/bin/sh"), (44, "/tmp/x/nohup")] {
        let mut t = Table::default();
        t.parents.insert(70, 1);
        t.exes.insert(70, PathBuf::from(exe));
        t.starts.insert(70, start);
        t.parents.insert(71, 70);
        let who = identify(
            &h,
            t,
            Arc::new(Mutex::new(json!([]))),
            Launches::default(),
            71,
        );
        assert_eq!(who.root.as_deref(), Some("unrooted"), "{exe} {start}");
    }
}
